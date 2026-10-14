//! Straight RGBA images. Decode and encode go through the pure-Rust `png` crate.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

/// Largest edge forge will decode. Generated sheets are a few thousand pixels.
pub const MAX_EDGE: u32 = 8192;

/// Alpha at or above this counts as ink. Matches the low-alpha clamp.
pub const OPAQUE: u8 = 24;

/// One 8-bit straight RGBA image, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Inclusive-exclusive box of opaque pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BBox {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl RgbaImage {
    pub fn new(width: u32, height: u32) -> Self {
        let n = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        Self {
            width,
            height,
            rgba: vec![0; n],
        }
    }

    pub fn solid(width: u32, height: u32, px: [u8; 4]) -> Self {
        let mut image = Self::new(width, height);
        for chunk in image.rgba.chunks_exact_mut(4) {
            chunk.copy_from_slice(&px);
        }
        image
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.rgba.is_empty()
    }

    fn offset(&self, x: u32, y: u32) -> usize {
        ((y as usize) * (self.width as usize) + (x as usize)) * 4
    }

    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let i = self.offset(x, y);
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    pub fn set(&mut self, x: u32, y: u32, px: [u8; 4]) {
        let i = self.offset(x, y);
        self.rgba[i..i + 4].copy_from_slice(&px);
    }

    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, px: [u8; 4]) {
        let x1 = x.saturating_add(w).min(self.width);
        let y1 = y.saturating_add(h).min(self.height);
        for row in y..y1 {
            for col in x..x1 {
                self.set(col, row, px);
            }
        }
    }

    pub fn opaque_count(&self) -> usize {
        self.rgba.chunks_exact(4).filter(|p| p[3] >= OPAQUE).count()
    }

    pub fn bbox(&self) -> Option<BBox> {
        if self.is_empty() {
            return None;
        }
        let mut min_x = self.width;
        let mut min_y = self.height;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        let mut any = false;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get(x, y)[3] >= OPAQUE {
                    any = true;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        if !any {
            return None;
        }
        Some(BBox {
            x: min_x,
            y: min_y,
            w: max_x - min_x + 1,
            h: max_y - min_y + 1,
        })
    }

    /// Exclusive bottom of the opaque pixels (the feet line when they rest on it).
    pub fn feet_y(&self) -> Option<i32> {
        self.bbox().map(|b| (b.y + b.h) as i32)
    }

    pub fn top_y(&self) -> Option<i32> {
        self.bbox().map(|b| b.y as i32)
    }

    pub fn crop(&self, box_: BBox) -> Self {
        let mut out = Self::new(box_.w, box_.h);
        for y in 0..box_.h {
            for x in 0..box_.w {
                let sx = box_.x + x;
                let sy = box_.y + y;
                if sx < self.width && sy < self.height {
                    out.set(x, y, self.get(sx, sy));
                }
            }
        }
        out
    }

    pub fn region(&self, x: u32, y: u32, w: u32, h: u32) -> Self {
        self.crop(BBox { x, y, w, h })
    }

    /// Nearest-neighbour scale. Integer sizes stay deterministic.
    pub fn scale_nn(&self, factor: f32) -> Self {
        if self.is_empty() || factor <= 0.0 {
            return Self::empty();
        }
        let nw = ((self.width as f32) * factor).round().max(1.0) as u32;
        let nh = ((self.height as f32) * factor).round().max(1.0) as u32;
        let mut out = Self::new(nw, nh);
        for y in 0..nh {
            let sy = (u64::from(y) * u64::from(self.height) / u64::from(nh)) as u32;
            let sy = sy.min(self.height - 1);
            for x in 0..nw {
                let sx = (u64::from(x) * u64::from(self.width) / u64::from(nw)) as u32;
                let sx = sx.min(self.width - 1);
                out.set(x, y, self.get(sx, sy));
            }
        }
        out
    }

    pub fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            rgba: Vec::new(),
        }
    }

    pub fn blit(&mut self, src: &RgbaImage, dest_x: i32, dest_y: i32) {
        if src.is_empty() || self.is_empty() {
            return;
        }
        for y in 0..src.height {
            for x in 0..src.width {
                let dx = dest_x + x as i32;
                let dy = dest_y + y as i32;
                if dx < 0 || dy < 0 {
                    continue;
                }
                let dx = dx as u32;
                let dy = dy as u32;
                if dx >= self.width || dy >= self.height {
                    continue;
                }
                let px = src.get(x, y);
                if px[3] == 0 {
                    continue;
                }
                self.set(dx, dy, px);
            }
        }
    }
}

/// Drop connected components smaller than 1% of the largest one.
/// Eight-connected, so a limb that still touches the body stays.
pub fn despeckle(image: &RgbaImage) -> RgbaImage {
    if image.is_empty() {
        return image.clone();
    }
    let w = image.width as usize;
    let h = image.height as usize;
    let n = w * h;
    let mut opaque = vec![false; n];
    for (i, slot) in opaque.iter_mut().enumerate() {
        if image.rgba[i * 4 + 3] >= OPAQUE {
            *slot = true;
        }
    }
    let mut seen = vec![0u32; n];
    let mut sizes: Vec<u32> = vec![0];
    let mut stack = Vec::new();
    for start in 0..n {
        if !opaque[start] || seen[start] != 0 {
            continue;
        }
        let id = sizes.len() as u32;
        sizes.push(0);
        stack.push(start);
        seen[start] = id;
        while let Some(cur) = stack.pop() {
            sizes[id as usize] += 1;
            let cx = cur % w;
            let cy = cur / w;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = cx as i32 + dx;
                    let ny = cy as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let ni = ny as usize * w + nx as usize;
                    if opaque[ni] && seen[ni] == 0 {
                        seen[ni] = id;
                        stack.push(ni);
                    }
                }
            }
        }
    }
    let largest = sizes.iter().copied().max().unwrap_or(0);
    let keep_at = (largest as f32) * 0.01;
    let mut out = image.clone();
    if largest == 0 {
        return out;
    }
    for (i, seen_id) in seen.iter().enumerate() {
        let id = *seen_id as usize;
        if id == 0 {
            continue;
        }
        if (sizes[id] as f32) < keep_at {
            let o = i * 4;
            out.rgba[o] = 0;
            out.rgba[o + 1] = 0;
            out.rgba[o + 2] = 0;
            out.rgba[o + 3] = 0;
        }
    }
    out
}

pub fn color_dist(px: [u8; 4], key: [u8; 3]) -> f32 {
    let dr = px[0] as f32 - key[0] as f32;
    let dg = px[1] as f32 - key[1] as f32;
    let db = px[2] as f32 - key[2] as f32;
    (dr * dr + dg * dg + db * db).sqrt()
}

/// Share of edge pixels whose colour still leans toward the key. Under 1% passes.
pub fn fringe_ratio(image: &RgbaImage, key: [u8; 3]) -> f32 {
    if image.is_empty() {
        return 0.0;
    }
    let mut edge = 0u32;
    let mut leaning = 0u32;
    for y in 0..image.height {
        for x in 0..image.width {
            let px = image.get(x, y);
            if px[3] < 128 {
                continue;
            }
            if !transparent_neighbor(image, x, y) {
                continue;
            }
            edge += 1;
            if leans_toward_key(px, key) {
                leaning += 1;
            }
        }
    }
    if edge == 0 {
        0.0
    } else {
        leaning as f32 / edge as f32
    }
}

fn transparent_neighbor(image: &RgbaImage, x: u32, y: u32) -> bool {
    for (dx, dy) in [(0i32, -1), (0, 1), (-1, 0), (1, 0)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= image.width as i32 || ny >= image.height as i32 {
            return true;
        }
        if image.get(nx as u32, ny as u32)[3] < OPAQUE {
            return true;
        }
    }
    false
}

pub fn leans_toward_key(px: [u8; 4], key: [u8; 3]) -> bool {
    if color_dist(px, key) < 36.0 {
        return true;
    }
    if key[1] > key[0].saturating_add(40) && key[1] > key[2].saturating_add(40) {
        return px[1] > px[0].max(px[2]).saturating_add(16);
    }
    false
}

/// Opaque pixels near a chroma-key candidate, as a share of all opaque pixels.
pub fn residue_ratio(image: &RgbaImage, key: [u8; 3]) -> f32 {
    if image.is_empty() {
        return 0.0;
    }
    let candidates = [
        key,
        [0, 255, 0],
        [0x10, 0xF0, 0x10],
        [0, 0, 255],
        [255, 0, 255],
    ];
    let mut opaque = 0u32;
    let mut near = 0u32;
    for chunk in image.rgba.chunks_exact(4) {
        if chunk[3] < OPAQUE {
            continue;
        }
        opaque += 1;
        let px = [chunk[0], chunk[1], chunk[2], chunk[3]];
        if candidates.iter().any(|c| color_dist(px, *c) < 28.0) {
            near += 1;
        }
    }
    if opaque == 0 {
        0.0
    } else {
        near as f32 / opaque as f32
    }
}

/// Distance from the top of the opaque box to the eye line, when it can be seen.
///
/// Red eyes (the custom-character signal) win. Otherwise the widest row in the
/// upper part of the figure is the eye line, and only when it is wider than the
/// crown. A rectangle has no such row, so the caller falls back to height.
pub fn head_to_eye(image: &RgbaImage) -> Option<u32> {
    let box_ = image.bbox()?;
    if let Some(y) = red_eye_y(image, box_) {
        return Some(y.saturating_sub(box_.y));
    }
    let crown = row_width(image, box_.y);
    let limit = box_.y + (box_.h * 60) / 100;
    let mut best_w = 0u32;
    let mut best_y = box_.y;
    for y in box_.y..=limit.min(image.height.saturating_sub(1)) {
        let w = row_width(image, y);
        if w > best_w {
            best_w = w;
            best_y = y;
        }
    }
    if best_y > box_.y && best_w >= crown.saturating_add(2) {
        Some(best_y - box_.y)
    } else {
        None
    }
}

fn row_width(image: &RgbaImage, y: u32) -> u32 {
    let mut n = 0u32;
    for x in 0..image.width {
        if image.get(x, y)[3] >= OPAQUE {
            n += 1;
        }
    }
    n
}

fn red_eye_y(image: &RgbaImage, box_: BBox) -> Option<u32> {
    let y_cut = box_.y + (box_.h * 60) / 100;
    let w = image.width as usize;
    let h = image.height as usize;
    let n = w * h;
    let mut mask = vec![false; n];
    for y in box_.y..y_cut.min(image.height) {
        for x in 0..image.width {
            let px = image.get(x, y);
            if px[0] > 150 && px[1] < 80 && px[2] < 80 && px[3] > 200 {
                mask[y as usize * w + x as usize] = true;
            }
        }
    }
    let mut seen = vec![false; n];
    let mut comps: Vec<(u32, f32, f32)> = Vec::new();
    let mut stack = Vec::new();
    for start in 0..n {
        if !mask[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut count = 0u32;
        let mut sx = 0f32;
        let mut sy = 0f32;
        while let Some(cur) = stack.pop() {
            count += 1;
            let cx = (cur % w) as f32;
            let cy = (cur / w) as f32;
            sx += cx;
            sy += cy;
            let ix = cur % w;
            let iy = cur / w;
            for (dx, dy) in [(0i32, -1), (0, 1), (-1, 0), (1, 0)] {
                let nx = ix as i32 + dx;
                let ny = iy as i32 + dy;
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let ni = ny as usize * w + nx as usize;
                if mask[ni] && !seen[ni] {
                    seen[ni] = true;
                    stack.push(ni);
                }
            }
        }
        if count >= 4 {
            comps.push((count, sx / count as f32, sy / count as f32));
        }
    }
    let min_dx = (box_.w as f32 * 0.08).max(4.0);
    let max_dx = box_.w as f32 * 0.7;
    let mut best: Option<f32> = None;
    for i in 0..comps.len() {
        for j in (i + 1)..comps.len() {
            let a = comps[i];
            let b = comps[j];
            let dx = (a.1 - b.1).abs();
            let dy = (a.2 - b.2).abs();
            let ratio = (a.0.max(b.0) as f32) / (a.0.min(b.0) as f32);
            if dx > min_dx && dx < max_dx && dy < 15.0 && ratio < 2.2 {
                let y = (a.2 + b.2) / 2.0;
                if best.map(|cur| y < cur).unwrap_or(true) {
                    best = Some(y);
                }
            }
        }
    }
    best.map(|y| y.round() as u32)
}

/// Mean x of the face band, measured up from the feet so a raised arm does not shift it.
pub fn face_x(image: &RgbaImage) -> Option<f32> {
    let box_ = image.bbox()?;
    let feet = box_.y + box_.h;
    let y0 = feet.saturating_sub((box_.h * 80) / 100);
    let y1 = feet.saturating_sub((box_.h * 58) / 100).max(y0 + 1);
    let mut n = 0u32;
    let mut sum = 0f32;
    for y in y0..y1.min(image.height) {
        for x in 0..image.width {
            if image.get(x, y)[3] >= OPAQUE {
                n += 1;
                sum += x as f32;
            }
        }
    }
    if n == 0 {
        let cx = box_.x as f32 + (box_.w as f32 - 1.0) / 2.0;
        Some(cx)
    } else {
        Some(sum / n as f32)
    }
}

pub fn decode_png(path: &Path) -> Result<RgbaImage, String> {
    let file = File::open(path).map_err(|err| err.to_string())?;
    let decoder = png::Decoder::new(BufReader::new(file));
    decode_reader(decoder)
}

fn decode_reader<R: std::io::BufRead + std::io::Seek>(
    mut decoder: png::Decoder<R>,
) -> Result<RgbaImage, String> {
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|err| err.to_string())?;
    let info = reader.info();
    if info.width == 0 || info.height == 0 || info.width > MAX_EDGE || info.height > MAX_EDGE {
        return Err(format!("PNG is {}x{}", info.width, info.height));
    }
    let width = info.width;
    let height = info.height;
    let need = reader
        .output_buffer_size()
        .ok_or_else(|| "PNG has no output size".to_string())?;
    let mut buffer = vec![0u8; need];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|err| err.to_string())?;
    buffer.truncate(frame.buffer_size());
    let rgba = to_rgba(&buffer, frame.color_type, width, height)?;
    Ok(RgbaImage {
        width,
        height,
        rgba,
    })
}

fn to_rgba(data: &[u8], color: png::ColorType, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let pixels = (width as usize).saturating_mul(height as usize);
    let mut rgba = Vec::with_capacity(pixels.saturating_mul(4));
    match color {
        png::ColorType::Rgba => {
            if data.len() < pixels * 4 {
                return Err("PNG RGBA buffer is short".to_string());
            }
            rgba.extend_from_slice(&data[..pixels * 4]);
        }
        png::ColorType::Rgb => {
            if data.len() < pixels * 3 {
                return Err("PNG RGB buffer is short".to_string());
            }
            for chunk in data[..pixels * 3].chunks_exact(3) {
                rgba.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            if data.len() < pixels {
                return Err("PNG gray buffer is short".to_string());
            }
            for byte in &data[..pixels] {
                rgba.extend_from_slice(&[*byte, *byte, *byte, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            if data.len() < pixels * 2 {
                return Err("PNG gray-alpha buffer is short".to_string());
            }
            for chunk in data[..pixels * 2].chunks_exact(2) {
                rgba.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
        }
        other => return Err(format!("PNG color type {other:?} did not expand")),
    }
    Ok(rgba)
}

pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    if image.width == 0 || image.height == 0 {
        return Err("cannot encode an empty image".to_string());
    }
    let expect = (image.width as usize)
        .saturating_mul(image.height as usize)
        .saturating_mul(4);
    if image.rgba.len() < expect {
        return Err("RGBA buffer is short".to_string());
    }
    let mut encoded = Vec::new();
    let mut encoder = png::Encoder::new(&mut encoded, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
    writer
        .write_image_data(&image.rgba[..expect])
        .map_err(|err| err.to_string())?;
    drop(writer);
    Ok(encoded)
}

pub fn save_png(path: &Path, image: &RgbaImage) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let bytes = encode_png(image)?;
    let mut file = BufWriter::new(File::create(path).map_err(|err| err.to_string())?);
    std::io::Write::write_all(&mut file, &bytes).map_err(|err| err.to_string())?;
    Ok(())
}

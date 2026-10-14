//! Key, despill, equal-column slice, low-alpha clamp, despeckle.
//!
//! The background colour comes from the corners, not from a hardcoded `#00FF00`.
//! Columns are equal widths of the sheet: two poses that touch stay in their
//! own columns instead of becoming one blob.

use crate::raster::{self, OPAQUE, RgbaImage};

/// One cell of the equal grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A cropped sprite and where it sat on the sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedFrame {
    pub slot: usize,
    pub image: RgbaImage,
    /// Left of the crop, in sheet pixels.
    pub origin_x: u32,
    /// Top of the crop, in sheet pixels.
    pub origin_y: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// Corners were already transparent.
    NativeAlpha,
    /// Flat colour sampled from the corners.
    Chroma([u8; 3]),
}

/// Keyed sheet, sliced into `rows * cols` cells (row-major).
#[derive(Debug, Clone)]
pub struct Extracted {
    pub key: KeyKind,
    pub cells: Vec<Cell>,
    pub frames: Vec<ExtractedFrame>,
    /// Fringe ratio across the non-empty crops.
    pub fringe_ratio: f32,
}

impl Extracted {
    pub fn key_rgb(&self) -> [u8; 3] {
        match self.key {
            KeyKind::NativeAlpha => [0, 255, 0],
            KeyKind::Chroma(rgb) => rgb,
        }
    }
}

pub fn extract(image: &RgbaImage, cols: usize) -> Result<Extracted, String> {
    extract_grid(image, 1, cols)
}

pub fn extract_grid(image: &RgbaImage, rows: usize, cols: usize) -> Result<Extracted, String> {
    if rows == 0 || cols == 0 {
        return Err("slot count is zero".to_string());
    }
    if image.width == 0 || image.height == 0 {
        return Err("sheet is empty".to_string());
    }
    let key = detect_key(image);
    let keyed = apply_key(image, key);
    let mut cells = Vec::with_capacity(rows * cols);
    let mut frames = Vec::with_capacity(rows * cols);
    for row in 0..rows {
        let y0 = (row as u32) * image.height / (rows as u32);
        let y1 = (row as u32 + 1) * image.height / (rows as u32);
        for col in 0..cols {
            let x0 = (col as u32) * image.width / (cols as u32);
            let x1 = (col as u32 + 1) * image.width / (cols as u32);
            let cell = Cell {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
            };
            let slot = frames.len();
            let region = keyed.region(x0, y0, cell.width, cell.height);
            let cleaned = raster::despeckle(&region);
            let (origin_x, origin_y, image) = crop_opaque(&cleaned, x0, y0);
            cells.push(cell);
            frames.push(ExtractedFrame {
                slot,
                image,
                origin_x,
                origin_y,
            });
        }
    }
    let key_rgb = match key {
        KeyKind::NativeAlpha => [0, 255, 0],
        KeyKind::Chroma(rgb) => rgb,
    };
    let fringe_ratio = combined_fringe(&frames, key_rgb);
    Ok(Extracted {
        key,
        cells,
        frames,
        fringe_ratio,
    })
}

fn crop_opaque(image: &RgbaImage, cell_x: u32, cell_y: u32) -> (u32, u32, RgbaImage) {
    match image.bbox() {
        Some(box_) => (cell_x + box_.x, cell_y + box_.y, image.crop(box_)),
        None => (cell_x, cell_y, RgbaImage::empty()),
    }
}

fn combined_fringe(frames: &[ExtractedFrame], key: [u8; 3]) -> f32 {
    let mut edge = 0.0f32;
    let mut weighted = 0.0f32;
    for frame in frames {
        if frame.image.opaque_count() == 0 {
            continue;
        }
        let ratio = raster::fringe_ratio(&frame.image, key);
        let edges = edge_count(&frame.image) as f32;
        edge += edges;
        weighted += ratio * edges;
    }
    if edge == 0.0 { 0.0 } else { weighted / edge }
}

fn edge_count(image: &RgbaImage) -> u32 {
    // Recomputed inside fringe_ratio; a second pass is fine for the weight.
    // Use opaque count as a stable weight when the image is small.
    image.opaque_count() as u32
}

/// Median colour of the four corners. Transparent corners mean native alpha.
pub fn detect_key(image: &RgbaImage) -> KeyKind {
    let n = 4u32.min(image.width).min(image.height).max(1);
    let origins = [
        (0, 0),
        (image.width.saturating_sub(n), 0),
        (0, image.height.saturating_sub(n)),
        (
            image.width.saturating_sub(n),
            image.height.saturating_sub(n),
        ),
    ];
    let mut samples: Vec<[u8; 4]> = Vec::new();
    for (ox, oy) in origins {
        for y in oy..(oy + n).min(image.height) {
            for x in ox..(ox + n).min(image.width) {
                samples.push(image.get(x, y));
            }
        }
    }
    if samples.is_empty() {
        return KeyKind::Chroma([0, 255, 0]);
    }
    let transparent = samples.iter().filter(|p| p[3] < 16).count();
    if transparent * 2 >= samples.len() {
        return KeyKind::NativeAlpha;
    }
    let mut rs: Vec<u8> = samples.iter().map(|p| p[0]).collect();
    let mut gs: Vec<u8> = samples.iter().map(|p| p[1]).collect();
    let mut bs: Vec<u8> = samples.iter().map(|p| p[2]).collect();
    rs.sort_unstable();
    gs.sort_unstable();
    bs.sort_unstable();
    let mid = samples.len() / 2;
    KeyKind::Chroma([rs[mid], gs[mid], bs[mid]])
}

pub fn apply_key(image: &RgbaImage, key: KeyKind) -> RgbaImage {
    let mut out = image.clone();
    match key {
        KeyKind::NativeAlpha => {
            for chunk in out.rgba.chunks_exact_mut(4) {
                if chunk[3] < OPAQUE {
                    chunk[0] = 0;
                    chunk[1] = 0;
                    chunk[2] = 0;
                    chunk[3] = 0;
                }
            }
        }
        KeyKind::Chroma(key) => {
            for chunk in out.rgba.chunks_exact_mut(4) {
                let (rgb, alpha) = key_pixel([chunk[0], chunk[1], chunk[2]], key);
                chunk[0] = rgb[0];
                chunk[1] = rgb[1];
                chunk[2] = rgb[2];
                chunk[3] = alpha;
            }
        }
    }
    out
}

/// Soft key plus despill. Green-dominant keys use the green-excess ramp from
/// the hand pipeline (opaque at excess ≤ 40, gone by excess ≥ 100).
fn key_pixel(rgb: [u8; 3], key: [u8; 3]) -> ([u8; 3], u8) {
    let green_key =
        key[1] >= 180 && key[1] > key[0].saturating_add(40) && key[1] > key[2].saturating_add(40);
    let alpha_f = if green_key {
        let excess = rgb[1] as i32 - (rgb[0] as i32).max(rgb[2] as i32);
        if excess <= 40 {
            1.0
        } else {
            (1.0 - (excess - 40) as f32 / 60.0).clamp(0.0, 1.0)
        }
    } else {
        let dist = raster::color_dist([rgb[0], rgb[1], rgb[2], 255], key);
        if dist <= 40.0 {
            0.0
        } else if dist >= 100.0 {
            1.0
        } else {
            (dist - 40.0) / 60.0
        }
    };
    let alpha = (alpha_f * 255.0).round() as u8;
    if alpha < OPAQUE {
        return ([0, 0, 0], 0);
    }
    let rgb = despill(rgb, key, green_key, alpha_f);
    (rgb, alpha)
}

fn despill(rgb: [u8; 3], key: [u8; 3], green_key: bool, alpha_f: f32) -> [u8; 3] {
    if green_key {
        let cap = rgb[0].max(rgb[2]).saturating_add(8);
        [rgb[0], rgb[1].min(cap), rgb[2]]
    } else {
        let spill = (1.0 - alpha_f).clamp(0.0, 1.0);
        let r = (rgb[0] as f32 - spill * key[0] as f32).clamp(0.0, 255.0) as u8;
        let g = (rgb[1] as f32 - spill * key[1] as f32).clamp(0.0, 255.0) as u8;
        let b = (rgb[2] as f32 - spill * key[2] as f32).clamp(0.0, 255.0) as u8;
        [r, g, b]
    }
}

/// True when every crop sits inside its cell. Empty crops count as inside.
pub fn frames_inside_cells(extracted: &Extracted) -> bool {
    extracted.frames.iter().all(|frame| {
        let cell = &extracted.cells[frame.slot];
        if frame.image.is_empty() || frame.image.opaque_count() == 0 {
            return true;
        }
        let right = frame.origin_x + frame.image.width;
        let bottom = frame.origin_y + frame.image.height;
        frame.origin_x >= cell.x
            && frame.origin_y >= cell.y
            && right <= cell.x + cell.width
            && bottom <= cell.y + cell.height
    })
}

pub fn frame_contains_color(image: &RgbaImage, pred: impl Fn([u8; 4]) -> bool) -> bool {
    image
        .rgba
        .chunks_exact(4)
        .any(|p| p[3] >= OPAQUE && pred([p[0], p[1], p[2], p[3]]))
}

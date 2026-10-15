//! Contact sheet and one GIF per animation.
//!
//! Pose lengths come from the pack XML through [`shimeji_packs::load_dir`].
//! The engine tick is 40 ms (`shimeji_app::runtime::TICK_MS`). GIF delays are
//! in centiseconds, so a pose of N ticks is N × 4. This crate does not depend
//! on shimeji-core; the model types are the re-export on shimeji-packs.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use shimeji_packs::model::{Action, Character, Pose};

use crate::raster::{self, RgbaImage};

/// Engine tick, in milliseconds. GIF delay units are 10 ms.
const TICK_MS: u32 = 40;
const GIF_UNIT_MS: u32 = 10;
/// NeuQuant speed. Delays do not depend on it.
const GIF_SPEED: i32 = 10;

const COLUMNS: u32 = 8;
const ROWS: u32 = 6;
const POSE_COUNT: u16 = 46;
const MIN_CELL_W: u32 = 24;
const MIN_CELL_H: u32 = 16;
const SCALE: u32 = 2;
const LABEL_X: u32 = 2;
const LABEL_Y: u32 = 2;

/// 3×5 glyphs. The leftmost column is bit 2 (`0b100`).
const GLYPHS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// Files written by [`review`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewOutput {
    pub sheet: PathBuf,
    pub gifs: Vec<PathBuf>,
}

/// Contact sheet plus one GIF per animation that has a pose.
pub fn review(pack: &Path, out: &Path) -> Result<ReviewOutput, String> {
    fs::create_dir_all(out).map_err(|err| err.to_string())?;
    let sheet_image = contact_sheet(pack)?;
    let sheet = out.join("contact-sheet.png");
    raster::save_png(&sheet, &sheet_image)?;
    let loaded = shimeji_packs::load_dir(pack).map_err(|err| err.to_string())?;
    let mut gifs = Vec::new();
    for character in &loaded.pack.characters {
        gifs.extend(write_gifs(pack, character, out)?);
    }
    Ok(ReviewOutput { sheet, gifs })
}

fn contact_sheet(pack: &Path) -> Result<RgbaImage, String> {
    let mut frames = Vec::with_capacity(POSE_COUNT as usize);
    let mut cell_w = MIN_CELL_W;
    let mut cell_h = MIN_CELL_H;
    for index in 1..=POSE_COUNT {
        let image = load_shime(pack, index);
        if let Some(image) = &image {
            cell_w = cell_w.max(image.width);
            cell_h = cell_h.max(image.height);
        }
        frames.push(image);
    }
    let mut sheet = RgbaImage::new(COLUMNS * cell_w, ROWS * cell_h);
    for (offset, image) in frames.into_iter().enumerate() {
        let index = (offset as u16) + 1;
        let col = (index as u32 - 1) % COLUMNS;
        let row = (index as u32 - 1) / COLUMNS;
        let origin_x = col * cell_w;
        let origin_y = row * cell_h;
        if let Some(image) = image {
            sheet.blit(&image, origin_x as i32, origin_y as i32);
        }
        draw_label(&mut sheet, origin_x, origin_y, index);
    }
    Ok(sheet)
}

fn load_shime(pack: &Path, index: u16) -> Option<RgbaImage> {
    for path in [
        pack.join("img").join(format!("shime{index}.png")),
        pack.join(format!("shime{index}.png")),
    ] {
        if path.is_file() {
            return raster::decode_png(&path).ok();
        }
    }
    None
}

fn draw_label(image: &mut RgbaImage, origin_x: u32, origin_y: u32, id: u16) {
    let width = 3 * SCALE + SCALE + 3 * SCALE;
    let height = 5 * SCALE;
    image.fill_rect(
        origin_x + LABEL_X,
        origin_y + LABEL_Y,
        width,
        height,
        [255, 255, 255, 255],
    );
    let tens = id / 10;
    let ones = id % 10;
    if id >= 10 {
        draw_glyph(image, origin_x + LABEL_X, origin_y + LABEL_Y, tens as u8);
        draw_glyph(
            image,
            origin_x + LABEL_X + 3 * SCALE + SCALE,
            origin_y + LABEL_Y,
            ones as u8,
        );
    } else {
        draw_glyph(image, origin_x + LABEL_X, origin_y + LABEL_Y, ones as u8);
    }
}

fn draw_glyph(image: &mut RgbaImage, x: u32, y: u32, digit: u8) {
    let rows = GLYPHS[digit as usize];
    for (row, bits) in rows.iter().enumerate() {
        for col in 0..3u32 {
            if bits & (1 << (2 - col)) == 0 {
                continue;
            }
            image.fill_rect(
                x + col * SCALE,
                y + row as u32 * SCALE,
                SCALE,
                SCALE,
                [0, 0, 0, 255],
            );
        }
    }
}

fn is_black(px: [u8; 4]) -> bool {
    px[0] < 16 && px[1] < 16 && px[2] < 16 && px[3] > 200
}

fn read_digit(image: &RgbaImage, x: u32, y: u32) -> Option<u8> {
    if x + 3 * SCALE > image.width || y + 5 * SCALE > image.height {
        return None;
    }
    let mut rows = [0u8; 5];
    let mut any = false;
    for row in 0..5u32 {
        let mut bits = 0u8;
        for col in 0..3u32 {
            if is_black(image.get(x + col * SCALE, y + row * SCALE)) {
                bits |= 1 << (2 - col);
                any = true;
            }
        }
        rows[row as usize] = bits;
    }
    if !any {
        return None;
    }
    GLYPHS
        .iter()
        .position(|glyph| *glyph == rows)
        .map(|i| i as u8)
}

/// The id drawn in cell `index` (1-based, row-major). `None` when the cell has no digit.
pub fn read_label(sheet: &RgbaImage, index: u16) -> Option<u16> {
    if index == 0 || index > POSE_COUNT {
        return None;
    }
    if sheet.width % COLUMNS != 0 || sheet.height % ROWS != 0 {
        return None;
    }
    let cell_w = sheet.width / COLUMNS;
    let cell_h = sheet.height / ROWS;
    let zero = u32::from(index - 1);
    let origin_x = (zero % COLUMNS) * cell_w;
    let origin_y = (zero / COLUMNS) * cell_h;
    let first = read_digit(sheet, origin_x + LABEL_X, origin_y + LABEL_Y)?;
    let second = read_digit(
        sheet,
        origin_x + LABEL_X + 3 * SCALE + SCALE,
        origin_y + LABEL_Y,
    );
    Some(match second {
        Some(digit) => u16::from(first) * 10 + u16::from(digit),
        None => u16::from(first),
    })
}

/// Labels of cells 1 through 46, skipping a cell that has no readable digit.
pub fn read_all_labels(sheet: &RgbaImage) -> Vec<u16> {
    (1..=POSE_COUNT)
        .filter_map(|index| read_label(sheet, index))
        .collect()
}

fn write_gifs(pack: &Path, character: &Character, out: &Path) -> Result<Vec<PathBuf>, String> {
    let mut written = Vec::new();
    let mut used = HashSet::new();
    for (name, action) in character.actions.iter() {
        let mut found = Vec::new();
        action.visit(&mut |action: &Action| {
            let label = action.name.clone().unwrap_or_else(|| name.to_string());
            for animation in &action.animations {
                if animation.poses.is_empty() {
                    continue;
                }
                found.push((label.clone(), animation.poses.clone()));
            }
        });
        let several = found.len() > 1;
        for (index, (label, poses)) in found.into_iter().enumerate() {
            let base = if several {
                format!("{}-{}", sanitize(&label), index + 1)
            } else {
                sanitize(&label)
            };
            let file = unique_name(&mut used, &base);
            let path = out.join(file);
            write_gif(&path, pack, character, &poses)?;
            written.push(path);
        }
    }
    Ok(written)
}

fn unique_name(used: &mut HashSet<String>, base: &str) -> String {
    if used.insert(base.to_string()) {
        return format!("{base}.gif");
    }
    let mut index = 2u32;
    loop {
        let candidate = format!("{base}-{index}");
        if used.insert(candidate.clone()) {
            return format!("{candidate}.gif");
        }
        index += 1;
    }
}

fn sanitize(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_control() || ch == '/' || ch == '\\' || ch == ':' {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let trimmed = out.trim_matches('.').to_string();
    if trimmed.is_empty() {
        "animation".to_string()
    } else {
        trimmed
    }
}

/// Ticks to GIF centiseconds. Non-positive lengths become one centisecond.
pub fn delay_cs(ticks: i32) -> u16 {
    if ticks <= 0 {
        return 1;
    }
    let cs = ticks.saturating_mul((TICK_MS / GIF_UNIT_MS) as i32);
    u16::try_from(cs).unwrap_or(u16::MAX)
}

fn write_gif(
    path: &Path,
    pack: &Path,
    character: &Character,
    poses: &[Pose],
) -> Result<(), String> {
    let mut loaded = Vec::with_capacity(poses.len());
    let mut width = 1u32;
    let mut height = 1u32;
    for pose in poses {
        let image = pose_file(pack, character, pose)
            .and_then(|file| raster::decode_png(&file).ok())
            .unwrap_or_else(|| RgbaImage::new(1, 1));
        width = width.max(image.width.max(1));
        height = height.max(image.height.max(1));
        loaded.push((image, delay_cs(pose.duration_ticks)));
    }
    let width = u16::try_from(width).map_err(|_| "frame width does not fit a GIF".to_string())?;
    let height =
        u16::try_from(height).map_err(|_| "frame height does not fit a GIF".to_string())?;
    let mut bytes = Vec::new();
    let mut encoder =
        gif::Encoder::new(&mut bytes, width, height, &[]).map_err(|err| err.to_string())?;
    encoder
        .set_repeat(gif::Repeat::Infinite)
        .map_err(|err| err.to_string())?;
    for (image, delay) in &mut loaded {
        let mut canvas = RgbaImage::new(u32::from(width), u32::from(height));
        canvas.blit(image, 0, 0);
        let mut frame = gif::Frame::from_rgba_speed(width, height, &mut canvas.rgba, GIF_SPEED);
        frame.delay = *delay;
        encoder.write_frame(&frame).map_err(|err| err.to_string())?;
    }
    drop(encoder);
    fs::write(path, bytes).map_err(|err| err.to_string())
}

fn pose_file(pack: &Path, character: &Character, pose: &Pose) -> Option<PathBuf> {
    let image = pose.image.as_ref()?;
    // Loaded names are NFC. Open the spelling on disk, which can be NFD.
    let rooted = format!("{}/{}", character.image_root, image.relative);
    if let Some(rooted) = shimeji_packs::stored_path(pack, &rooted).filter(|path| path.is_file()) {
        return Some(rooted);
    }
    shimeji_packs::stored_path(pack, &image.relative).filter(|path| path.is_file())
}

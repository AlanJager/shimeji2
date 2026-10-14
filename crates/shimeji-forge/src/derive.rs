//! Split frames 42–46 and the carry clip on 34–36.
//!
//! The split distances are the horizontal gaps measured on the bundled shime:
//! two copies of frame 1, ±4, ±8, ±12, ±16, and ±20. The clip is the window
//! rectangle above and to the left of the grab point (64, 64) on a 128 cell.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::raster::{self, RgbaImage};

/// Classic frame and the ± offset, in pixels of a 128-wide cell.
pub const SPLIT_OFFSETS: [(u16, i32); 5] = [(42, 4), (43, 8), (44, 12), (45, 16), (46, 20)];

/// Carry frames. Pixels in the window rectangle are cleared.
pub const CLIP_FRAMES: [u16; 3] = [34, 35, 36];

/// One composited split frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitOffset {
    pub frame: u16,
    pub dx: i32,
}

/// The clip line applied to one carry frame. Pixels with `x < self.x` and
/// `y < self.y` are cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipLine {
    pub frame: u16,
    pub x: u32,
    pub y: u32,
}

/// What `forge derive` wrote into the pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeriveReport {
    pub offsets: Vec<SplitOffset>,
    pub clips: Vec<ClipLine>,
}

/// Round-half scale of a 128-cell offset onto `width`.
pub fn scaled_offset(offset: i32, width: u32) -> i32 {
    offset.saturating_mul(width as i32).saturating_add(64) / 128
}

/// Two copies of `source`, shifted left and right by [`scaled_offset`].
pub fn split_frame(source: &RgbaImage, offset: i32) -> RgbaImage {
    let dx = scaled_offset(offset, source.width);
    let mut out = RgbaImage::new(source.width, source.height);
    out.blit(source, -dx, 0);
    out.blit(source, dx, 0);
    out
}

/// Clip origin for a cell of this size. 128×128 stays at (64, 64).
pub fn clip_lines(width: u32, height: u32) -> (u32, u32) {
    (64 * width / 128, 64 * height / 128)
}

/// Clear every pixel in the window rectangle. Returns the clip origin.
pub fn clip_frame(image: &mut RgbaImage) -> (u32, u32) {
    let (clip_x, clip_y) = clip_lines(image.width, image.height);
    let x_end = clip_x.min(image.width);
    let y_end = clip_y.min(image.height);
    for y in 0..y_end {
        for x in 0..x_end {
            image.set(x, y, [0, 0, 0, 0]);
        }
    }
    (clip_x, clip_y)
}

fn frame_file(pack: &Path, index: u16) -> Option<PathBuf> {
    let in_img = pack.join("img").join(format!("shime{index}.png"));
    if in_img.is_file() {
        return Some(in_img);
    }
    let bare = pack.join(format!("shime{index}.png"));
    if bare.is_file() {
        return Some(bare);
    }
    None
}

/// Replace shime42–46 with composites of shime1, and clip shime34–36 in place.
pub fn derive_pack(pack: &Path) -> Result<DeriveReport, String> {
    let source_path =
        frame_file(pack, 1).ok_or_else(|| format!("missing shime1.png in {}", pack.display()))?;
    let dir = source_path
        .parent()
        .ok_or_else(|| "shime1.png has no directory".to_string())?
        .to_path_buf();
    let source = raster::decode_png(&source_path)?;
    let mut offsets = Vec::new();
    for (frame, offset) in SPLIT_OFFSETS {
        let image = split_frame(&source, offset);
        let dx = scaled_offset(offset, source.width);
        raster::save_png(&dir.join(format!("shime{frame}.png")), &image)?;
        offsets.push(SplitOffset { frame, dx });
    }
    let mut clips = Vec::new();
    for frame in CLIP_FRAMES {
        let path = dir.join(format!("shime{frame}.png"));
        if !path.is_file() {
            return Err(format!("missing {}", path.display()));
        }
        let mut image = raster::decode_png(&path)?;
        let (x, y) = clip_frame(&mut image);
        raster::save_png(&path, &image)?;
        clips.push(ClipLine { frame, x, y });
    }
    let body = json!({
        "offsets": offsets.iter().map(|row| json!({"frame": row.frame, "dx": row.dx})).collect::<Vec<_>>(),
        "clips": clips.iter().map(|row| json!({"frame": row.frame, "x": row.x, "y": row.y})).collect::<Vec<_>>(),
    });
    fs::write(
        pack.join("derive.json"),
        serde_json::to_string_pretty(&body).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    Ok(DeriveReport { offsets, clips })
}

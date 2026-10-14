//! Slice and assemble a Standard Atlas.

use std::path::Path;

use crate::import::fs::{remove_file_nofollow, write_bytes};
use crate::standard_atlas::{self, CELL};

pub(crate) fn read_rgba(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    crate::images::decode_png_file(path)
}

pub(crate) fn crop(atlas: &[u8], atlas_w: u32, origin_x: u32, origin_y: u32, edge: u32) -> Vec<u8> {
    let mut out = vec![0u8; (edge * edge * 4) as usize];
    for y in 0..edge {
        let src = (((origin_y + y) * atlas_w + origin_x) * 4) as usize;
        let dst = (y * edge * 4) as usize;
        let n = (edge * 4) as usize;
        out[dst..dst + n].copy_from_slice(&atlas[src..src + n]);
    }
    out
}

pub(crate) fn blit(
    atlas: &mut [u8],
    atlas_w: u32,
    image: &[u8],
    image_w: u32,
    origin_x: u32,
    origin_y: u32,
    edge: u32,
) {
    let copy_w = image_w.min(edge);
    for y in 0..edge {
        let src = ((y * image_w) * 4) as usize;
        let dst = (((origin_y + y) * atlas_w + origin_x) * 4) as usize;
        let n = (copy_w * 4) as usize;
        atlas[dst..dst + n].copy_from_slice(&image[src..src + n]);
    }
}

pub(crate) fn is_empty(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).all(|pixel| pixel[3] == 0)
}

/// Write `img/shime{n}.png` for non-empty cells. Empty cells are removed.
/// The returned set is the image attribute text that should stay unresolved.
pub(crate) fn write_cells(dir: &Path, atlas: &[u8], atlas_w: u32) -> std::io::Result<Vec<String>> {
    let img = dir.join("img");
    crate::import::fs::ensure_dir(&img)?;
    let mut quiet = Vec::new();
    for index in 1..=standard_atlas::POSE_COUNT {
        let (origin_x, origin_y) = standard_atlas::cell_origin(index).expect("pose cell");
        let cell = crop(atlas, atlas_w, origin_x, origin_y, CELL);
        let path = img.join(format!("shime{index}.png"));
        let source = format!("/shime{index}.png");
        if is_empty(&cell) {
            remove_file_nofollow(&path)?;
            quiet.push(source);
            continue;
        }
        let encoded =
            crate::images::encode_rgba(CELL, CELL, &cell).map_err(std::io::Error::other)?;
        write_bytes(&path, &encoded)?;
    }
    Ok(quiet)
}

//! Native pack → Codex v2 sheet.
//!
//! The sheet is always 1536×2288 and `pet.json` always says
//! `spriteVersionNumber` 2. Rows come from research note 3. An optional
//! second atlas, layout `shimeji-ext-v1` (8×6 of 128×128), fills the rows
//! classic art cannot: idle, waving, and the twelve off-axis look directions.
//! Fail and work use that atlas only when every cell of the group has ink.

use std::io;
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use crate::import::fs::{ensure_dir, write_bytes};
use crate::native::{atlas, manifest};
use crate::standard_atlas::{self, CELL};

const CELL_W: u32 = 192;
const CELL_H: u32 = 208;
const SHEET_W: u32 = 1536;
const SHEET_H: u32 = 2288;
/// Exclusive bottom of the placed sprite, inside the 208-tall cell.
const BOTTOM: i32 = 203;

const LOOK_DEGREES: [&str; 16] = [
    "000", "022.5", "045", "067.5", "090", "112.5", "135", "157.5", "180", "202.5", "225", "247.5",
    "270", "292.5", "315", "337.5",
];

/// One row the classic frames could not fill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingRow {
    pub row: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
}

/// What [`export_codex`] wrote, including an empty missing list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexExport {
    pub missing: Vec<MissingRow>,
}

struct Sheet {
    rgba: Vec<u8>,
}

struct Source<'a> {
    atlas: &'a [u8],
    width: u32,
    pixel_art: bool,
}

struct Extension {
    width: u32,
    rgba: Vec<u8>,
}

impl Sheet {
    fn new() -> Self {
        Self {
            rgba: vec![0; (SHEET_W * SHEET_H * 4) as usize],
        }
    }

    fn paint(&mut self, row: u32, col: u32, src: &[u8], mirror: bool, pixel_art: bool, dy: i32) {
        let (dst_w, dst_h, local_x, local_y) = placement(pixel_art);
        let top = local_y + dy;
        for y in 0..dst_h {
            let dest_y = top + y as i32;
            if dest_y < 0 || dest_y >= CELL_H as i32 {
                continue;
            }
            for x in 0..dst_w {
                let dest_x = local_x + x as i32;
                if dest_x < 0 || dest_x >= CELL_W as i32 {
                    continue;
                }
                let px = sample(src, x, y, dst_w, dst_h, mirror);
                if px[3] == 0 {
                    continue;
                }
                let sx = col * CELL_W + dest_x as u32;
                let sy = row * CELL_H + dest_y as u32;
                let index = ((sy * SHEET_W + sx) * 4) as usize;
                self.rgba[index..index + 4].copy_from_slice(&px);
            }
        }
    }
}

/// 1× pixel art sits in the cell with its left at x=32. Smoothed art is 1.5×
/// and fills the cell width, top at y=11, so the bottom is y=203.
fn placement(pixel_art: bool) -> (u32, u32, i32, i32) {
    if pixel_art {
        (CELL, CELL, (CELL_W - CELL) as i32 / 2, BOTTOM - CELL as i32)
    } else {
        let scaled = CELL * 3 / 2;
        (scaled, scaled, 0, BOTTOM - scaled as i32)
    }
}

fn sample(src: &[u8], x: u32, y: u32, dst_w: u32, dst_h: u32, mirror: bool) -> [u8; 4] {
    let mut sx = x * CELL / dst_w;
    let sy = y * CELL / dst_h;
    if mirror {
        sx = CELL - 1 - sx;
    }
    let index = ((sy * CELL + sx) * 4) as usize;
    [src[index], src[index + 1], src[index + 2], src[index + 3]]
}

fn classic_cell(atlas_px: &[u8], atlas_w: u32, frame: u16) -> Vec<u8> {
    let (x, y) = standard_atlas::cell_origin(frame).expect("classic frame");
    atlas::crop(atlas_px, atlas_w, x, y, CELL)
}

fn ext_cell(ext: &[u8], ext_w: u32, index: u16) -> Option<Vec<u8>> {
    let (x, y) = standard_atlas::cell_origin(index + 1)?;
    Some(atlas::crop(ext, ext_w, x, y, CELL))
}

fn ext_ready(ext: Option<&Extension>, cells: &[u16]) -> bool {
    ext.is_some_and(|atlas| group_ready(&atlas.rgba, atlas.width, cells.iter().copied()))
}

fn group_ready(ext: &[u8], ext_w: u32, cells: impl IntoIterator<Item = u16>) -> bool {
    cells
        .into_iter()
        .all(|index| ext_cell(ext, ext_w, index).is_some_and(|cell| !atlas::is_empty(&cell)))
}

fn paint_frames(
    sheet: &mut Sheet,
    source: &Source<'_>,
    row: u32,
    frames: &[u16],
    mirror: bool,
    dys: &[i32],
) {
    for (col, frame) in frames.iter().copied().enumerate() {
        let cell = classic_cell(source.atlas, source.width, frame);
        let dy = dys.get(col).copied().unwrap_or(0);
        sheet.paint(row, col as u32, &cell, mirror, source.pixel_art, dy);
    }
}

fn paint_ext(sheet: &mut Sheet, ext: &[u8], ext_w: u32, cells: &[u16], row: u32, pixel_art: bool) {
    for (col, index) in cells.iter().copied().enumerate() {
        let Some(cell) = ext_cell(ext, ext_w, index) else {
            continue;
        };
        sheet.paint(row, col as u32, &cell, false, pixel_art, 0);
    }
}

fn missing(row: &str, direction: Option<&str>) -> MissingRow {
    MissingRow {
        row: row.to_string(),
        direction: direction.map(str::to_string),
    }
}

/// Write `spritesheet.png`, `pet.json`, and `missing-rows.json` under `dest`.
pub fn export_codex(native_dir: &Path, dest: &Path) -> io::Result<CodexExport> {
    let pack = native_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("pack");
    let parsed = manifest::read_manifest(native_dir, pack).map_err(io::Error::other)?;
    let (atlas_w, atlas_h, atlas_px) = atlas::read_rgba(&parsed.atlas).map_err(io::Error::other)?;
    let expect = standard_atlas::atlas_size(1);
    if (atlas_w, atlas_h) != expect {
        return Err(io::Error::other(format!(
            "atlas is {atlas_w}x{atlas_h}; shimeji-standard-v1 is {}x{}",
            expect.0, expect.1
        )));
    }
    let (pixel_art, ext) = pack_extras(native_dir)?;
    let mut sheet = Sheet::new();
    let mut gaps = Vec::new();
    let source = Source {
        atlas: &atlas_px,
        width: atlas_w,
        pixel_art,
    };
    fill(&mut sheet, &source, ext.as_ref(), &mut gaps);

    ensure_dir(dest)?;
    let encoded =
        crate::images::encode_rgba(SHEET_W, SHEET_H, &sheet.rgba).map_err(io::Error::other)?;
    write_bytes(&dest.join("spritesheet.png"), &encoded)?;
    let pet = json!({
        "id": parsed.id,
        "displayName": parsed.display_name,
        "spritesheetPath": "spritesheet.png",
        "spriteVersionNumber": 2,
    });
    let pet_bytes = serde_json::to_vec_pretty(&pet).map_err(io::Error::other)?;
    write_bytes(&dest.join("pet.json"), &pet_bytes)?;
    let report = json!({ "missing": &gaps });
    let report_bytes = serde_json::to_vec_pretty(&report).map_err(io::Error::other)?;
    write_bytes(&dest.join("missing-rows.json"), &report_bytes)?;
    Ok(CodexExport { missing: gaps })
}

fn fill(
    sheet: &mut Sheet,
    source: &Source<'_>,
    ext: Option<&Extension>,
    gaps: &mut Vec<MissingRow>,
) {
    let idle: Vec<u16> = (0..6).collect();
    let wave: Vec<u16> = (8..12).collect();
    let fail: Vec<u16> = (16..24).collect();
    let work: Vec<u16> = (24..30).collect();
    let look: Vec<u16> = (32..48).collect();
    let idle_ready = ext_ready(ext, &idle);
    let wave_ready = ext_ready(ext, &wave);
    let fail_ready = ext_ready(ext, &fail);
    let work_ready = ext_ready(ext, &work);
    let look_ready = ext_ready(ext, &look);

    if let (true, Some(atlas)) = (idle_ready, ext) {
        paint_ext(sheet, &atlas.rgba, atlas.width, &idle, 0, source.pixel_art);
    } else {
        gaps.push(missing("idle", None));
    }

    let walk = [1, 2, 1, 3, 1, 2, 1, 3];
    paint_frames(sheet, source, 1, &walk, true, &[]);
    paint_frames(sheet, source, 2, &walk, false, &[]);

    if let (true, Some(atlas)) = (wave_ready, ext) {
        paint_ext(sheet, &atlas.rgba, atlas.width, &wave, 3, source.pixel_art);
    } else {
        gaps.push(missing("waving", None));
    }

    paint_frames(
        sheet,
        source,
        4,
        &[11, 22, 22, 4, 1],
        false,
        &[0, -6, -10, -6, 0],
    );

    if let (true, Some(atlas)) = (fail_ready, ext) {
        paint_ext(sheet, &atlas.rgba, atlas.width, &fail, 5, source.pixel_art);
    } else {
        paint_frames(
            sheet,
            source,
            5,
            &[18, 19, 20, 21, 21, 21, 21, 21],
            false,
            &[],
        );
    }

    paint_frames(sheet, source, 6, &[31, 32, 33, 32, 31, 26], false, &[]);

    if let (true, Some(atlas)) = (work_ready, ext) {
        paint_ext(sheet, &atlas.rgba, atlas.width, &work, 7, source.pixel_art);
    } else {
        paint_frames(sheet, source, 7, &[15, 16, 17, 15, 16, 17], false, &[]);
    }

    paint_frames(sheet, source, 8, &[26, 15, 27, 16, 28, 17], false, &[]);

    if let (true, Some(atlas)) = (look_ready, ext) {
        paint_ext(
            sheet,
            &atlas.rgba,
            atlas.width,
            &look[..8],
            9,
            source.pixel_art,
        );
        paint_ext(
            sheet,
            &atlas.rgba,
            atlas.width,
            &look[8..],
            10,
            source.pixel_art,
        );
    } else {
        paint_cardinal_looks(sheet, source, gaps);
    }
}

fn paint_cardinal_looks(sheet: &mut Sheet, source: &Source<'_>, gaps: &mut Vec<MissingRow>) {
    // 000 up, 090 right (mirrored stand), 180 down, 270 left. The other
    // twelve directions have no classic frame.
    let filled = [
        (0usize, 26u16, false),
        (4, 1, true),
        (8, 11, false),
        (12, 1, false),
    ];
    for (index, direction) in LOOK_DEGREES.iter().enumerate() {
        if let Some((_, frame, mirror)) = filled.iter().find(|(slot, _, _)| *slot == index) {
            let cell = classic_cell(source.atlas, source.width, *frame);
            let row = 9 + (index / 8) as u32;
            let col = (index % 8) as u32;
            sheet.paint(row, col, &cell, *mirror, source.pixel_art, 0);
        } else {
            gaps.push(missing("look", Some(direction)));
        }
    }
}

fn pack_extras(dir: &Path) -> io::Result<(bool, Option<Extension>)> {
    let bytes = crate::import::fs::read_regular(&dir.join("pack.json"))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    let pixel_art = value
        .pointer("/art/pixelArt")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let Some(atlases) = value.get("atlases").and_then(Value::as_array) else {
        return Ok((pixel_art, None));
    };
    let Some(ext) = atlases
        .iter()
        .find(|atlas| atlas.get("layout").and_then(Value::as_str) == Some("shimeji-ext-v1"))
    else {
        return Ok((pixel_art, None));
    };
    let image = ext
        .get("image")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("extension atlas is missing image"))?;
    let path = manifest::safe_join(dir, image)
        .ok_or_else(|| io::Error::other("extension atlas image escapes the pack"))?;
    let (width, height, rgba) = atlas::read_rgba(&path).map_err(io::Error::other)?;
    let expect = standard_atlas::atlas_size(1);
    if (width, height) != expect {
        return Err(io::Error::other(format!(
            "extension atlas is {width}x{height}; shimeji-ext-v1 is {}x{}",
            expect.0, expect.1
        )));
    }
    Ok((pixel_art, Some(Extension { width, rgba })))
}

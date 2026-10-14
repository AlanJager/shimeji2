//! Codex pet import.
//!
//! The sheet size is the version: 1536×1872 is v1 and 1536×2288 is v2.
//! `spriteVersionNumber` is only a hint. Cells are 192×208 on an 8-column
//! grid. Each row shares one anchor, x = 96 and y = the lower median of the
//! alpha-box bottoms. Behaviors are the embedded English profile.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use shimeji_core::diagnostics::{Diagnostic, DiagnosticKind, Severity};
use shimeji_core::model::{LayoutKind, Pack, Pose, ResolvedImage};

use crate::capabilities::{self, file_stem};
use crate::import::fs::{ensure_dir, remove_file_nofollow, write_bytes};
use crate::{LoadOutcome, PackOrigin};

/// Renderer scale for a Codex pet. 128/208 is about 0.615; physics stays in pixels.
pub const CODEX_DISPLAY_SCALE: f64 = 0.62;

const CELL_W: u32 = 192;
const CELL_H: u32 = 208;
const COLUMNS: u32 = 8;
const V1: (u32, u32) = (1536, 1872);
const V2: (u32, u32) = (1536, 2288);
const PROFILE: &str = include_str!("../assets/codex-profile.xml");
const PET_CAP: u64 = 1024 * 1024;

struct Row {
    state: &'static str,
    names: &'static [&'static str],
    /// Tick length of each stock frame. Extras use the first value.
    ticks: &'static [i32],
    action: &'static str,
    velocity_x: i32,
}

const ROWS: &[Row] = &[
    Row {
        state: "idle",
        names: &["stand", "idle-2", "idle-3", "idle-4", "idle-5", "idle-6"],
        ticks: &[7, 3, 3, 4, 4, 8],
        action: "Idle",
        velocity_x: 0,
    },
    Row {
        state: "running-right",
        names: &[
            "pinched-a",
            "pinched-b",
            "pinched-c",
            "pinched-d",
            "pinched-e",
            "pinched-f",
            "pinched-g",
            "pinched-h",
        ],
        ticks: &[3, 3, 3, 3, 3, 3, 3, 6],
        action: "Dragged",
        velocity_x: 0,
    },
    Row {
        state: "running-left",
        names: &[
            "walk-a", "walk-b", "walk-3", "walk-4", "walk-5", "walk-6", "walk-7", "walk-8",
        ],
        ticks: &[3, 3, 3, 3, 3, 3, 3, 6],
        action: "WalkLeft",
        velocity_x: -2,
    },
    Row {
        state: "waving",
        names: &["wave-1", "wave-2", "wave-3", "wave-4"],
        ticks: &[4, 4, 4, 7],
        action: "Wave",
        velocity_x: 0,
    },
    Row {
        state: "jumping",
        names: &["jump", "jump-2", "jump-3", "fall", "jump-5"],
        ticks: &[4, 4, 4, 4, 7],
        action: "Jump",
        velocity_x: 0,
    },
    Row {
        state: "failed",
        names: &[
            "resist-a", "resist-b", "bounce-a", "bounce-b", "crawl-a", "crawl-b", "failed-7", "lie",
        ],
        ticks: &[4, 4, 4, 4, 4, 4, 4, 6],
        action: "Failed",
        velocity_x: 0,
    },
    Row {
        state: "waiting",
        names: &[
            "sit",
            "waiting-2",
            "waiting-3",
            "waiting-4",
            "waiting-5",
            "waiting-6",
        ],
        ticks: &[4, 4, 4, 4, 4, 7],
        action: "Waiting",
        velocity_x: 0,
    },
    Row {
        state: "working",
        names: &[
            "dangle",
            "working-2",
            "working-3",
            "working-4",
            "working-5",
            "working-6",
        ],
        ticks: &[3, 3, 3, 3, 3, 6],
        action: "Working",
        velocity_x: 0,
    },
    Row {
        state: "review",
        names: &[
            "spin", "review-2", "review-3", "review-4", "review-5", "review-6",
        ],
        ticks: &[4, 4, 4, 4, 4, 7],
        action: "Review",
        velocity_x: 0,
    },
    Row {
        state: "look-a",
        names: &[
            "look-0", "look-1", "look-2", "look-3", "look-4", "look-5", "look-6", "look-7",
        ],
        ticks: &[4, 4, 4, 4, 4, 4, 4, 4],
        action: "CursorFollow",
        velocity_x: 0,
    },
    Row {
        state: "look-b",
        names: &[
            "look-8", "look-9", "look-10", "look-11", "look-12", "look-13", "look-14", "look-15",
        ],
        ticks: &[4, 4, 4, 4, 4, 4, 4, 4],
        action: "CursorFollow",
        velocity_x: 0,
    },
];

struct Extra {
    action: &'static str,
    name: String,
    anchor_y: i32,
    velocity_x: i32,
    ticks: i32,
}

pub(crate) fn is_codex_dir(path: &Path) -> bool {
    is_regular_file(&path.join("pet.json"))
}

pub(crate) fn origin(path: &Path) -> Option<PackOrigin> {
    if !is_codex_dir(path) {
        return None;
    }
    let bytes = read_capped(&path.join("pet.json"), PET_CAP).ok()?;
    let pet = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
    let mut ignored = Vec::new();
    let sheet = find_sheet(path, &pet, &mut ignored, "")?;
    let (width, height) = crate::images::png_pixel_size(&sheet).ok()?;
    match (width, height) {
        V1 => Some(PackOrigin::CodexV1),
        V2 => Some(PackOrigin::CodexV2),
        _ => None,
    }
}

pub(crate) fn load_codex(path: &Path) -> io::Result<LoadOutcome> {
    let pack_name = dir_name(path);
    let mut diagnostics = Vec::new();
    let pet_bytes = match read_capped(&path.join("pet.json"), PET_CAP) {
        Ok(bytes) => bytes,
        Err(error) => {
            diagnostics.push(error_diag(
                &pack_name,
                format!("could not read pet.json: {error}"),
            ));
            return Ok(empty(&pack_name, diagnostics, None));
        }
    };
    let pet: Value = match serde_json::from_slice(&pet_bytes) {
        Ok(value) => value,
        Err(error) => {
            diagnostics.push(error_diag(
                &pack_name,
                format!("pet.json is not JSON: {error}"),
            ));
            return Ok(empty(&pack_name, diagnostics, None));
        }
    };
    let Some(sheet) = find_sheet(path, &pet, &mut diagnostics, &pack_name) else {
        return Ok(empty(&pack_name, diagnostics, None));
    };
    let (width, height, rgba) = match crate::images::decode_png_file(&sheet) {
        Ok(decoded) => decoded,
        Err(message) => {
            diagnostics.push(error_diag(
                &pack_name,
                format!("sheet is not a Codex PNG: {message}"),
            ));
            return Ok(empty(&pack_name, diagnostics, None));
        }
    };
    let Some(version) = version_of(width, height) else {
        diagnostics.push(error_diag(
            &pack_name,
            format!("sheet is {width}x{height}; Codex sheets are 1536x1872 (v1) or 1536x2288 (v2)"),
        ));
        return Ok(empty(&pack_name, diagnostics, None));
    };
    if let Some(declared) = pet.get("spriteVersionNumber").and_then(version_number) {
        let pixel_version = if version == V2 { 2 } else { 1 };
        if declared != pixel_version {
            diagnostics.push(Diagnostic::new(
                Severity::Info,
                DiagnosticKind::Other,
                &pack_name,
                "pet.json",
                None,
                format!(
                    "pet.json says sprite version {declared} but the sheet is {width}x{height}, so this is Codex v{pixel_version}"
                ),
            ));
        }
    }

    let rows = if version == V2 { 11 } else { 9 };
    let (written, extras) = materialize(path, &rgba, width, rows)?;

    let (doc, more) =
        crate::xml::parse_document(PROFILE.as_bytes(), &pack_name, "codex-profile.xml");
    diagnostics.extend(more);
    let Some(mut doc) = doc else {
        diagnostics.push(error_diag(&pack_name, "codex profile did not parse"));
        return Ok(empty(&pack_name, diagnostics, Some(CODEX_DISPLAY_SCALE)));
    };
    bind_frames(&mut doc.actions, &written, &extras);

    let mut info = doc.info;
    let character_name = text_field(&pet, "displayName")
        .or_else(|| text_field(&pet, "id"))
        .unwrap_or_else(|| pack_name.clone());
    info.display_name = Some(character_name.clone());

    let mut character = shimeji_core::model::Character {
        name: character_name,
        schema: doc.schema,
        behavior_schema: doc.schema,
        image_root: "img".to_string(),
        actions_file: "codex-profile.xml".to_string(),
        behaviors_file: "codex-profile.xml".to_string(),
        info_file: None,
        constants: doc.constants,
        actions: doc.actions,
        behaviors: doc.behaviors,
        behavior_tree: doc.behavior_tree,
        info,
    };
    let names = capabilities::collect_pose_names(&character);
    let missing = capabilities::missing_core(&names);
    if !missing.is_empty() {
        diagnostics.push(error_diag(
            &pack_name,
            format!("pack rejected: missing core poses {}", missing.join(", ")),
        ));
        return Ok(empty(&pack_name, diagnostics, Some(CODEX_DISPLAY_SCALE)));
    }
    let pruned = capabilities::prune_character(&mut character);
    capabilities::note_pruned(
        &mut diagnostics,
        &pack_name,
        &character.behaviors_file,
        &pruned,
    );
    crate::layout::validate_refs(&character, &mut diagnostics, &pack_name);
    Ok(LoadOutcome {
        pack: Pack {
            name: pack_name,
            layout: LayoutKind::ClassicSingle,
            characters: vec![character],
        },
        diagnostics,
        display_scale: Some(CODEX_DISPLAY_SCALE),
    })
}

fn materialize(
    dir: &Path,
    rgba: &[u8],
    width: u32,
    rows: usize,
) -> io::Result<(HashMap<String, i32>, Vec<Extra>)> {
    let img = dir.join("img");
    ensure_dir(&img)?;
    let mut written = HashMap::new();
    let mut extras = Vec::new();
    for (row_index, row) in ROWS.iter().enumerate().take(rows) {
        let mut cells = Vec::new();
        for col in 0..COLUMNS {
            let crop = crop_cell(rgba, width, row_index as u32, col);
            let name = cell_name(row, col);
            let path = img.join(format!("{name}.png"));
            let Some(bottom) = alpha_bottom(&crop, CELL_W, CELL_H) else {
                remove_file_nofollow(&path)?;
                continue;
            };
            cells.push((name, bottom, col >= row.names.len() as u32, crop));
        }
        if cells.is_empty() {
            continue;
        }
        let mut bottoms: Vec<i32> = cells.iter().map(|(_, bottom, _, _)| *bottom).collect();
        let anchor_y = lower_median(&mut bottoms);
        for (name, _, is_extra, crop) in cells {
            let encoded =
                crate::images::encode_rgba(CELL_W, CELL_H, &crop).map_err(io::Error::other)?;
            write_bytes(&img.join(format!("{name}.png")), &encoded)?;
            written.insert(name.clone(), anchor_y);
            if is_extra {
                extras.push(Extra {
                    action: row.action,
                    name,
                    anchor_y,
                    velocity_x: row.velocity_x,
                    ticks: row.ticks[0],
                });
            }
        }
    }
    for row in ROWS.iter().skip(rows) {
        for name in row.names {
            remove_file_nofollow(&img.join(format!("{name}.png")))?;
        }
    }
    Ok((written, extras))
}

fn cell_name(row: &Row, col: u32) -> String {
    if (col as usize) < row.names.len() {
        row.names[col as usize].to_string()
    } else {
        format!("{}-extra-{}", row.state, col + 1)
    }
}

fn bind_frames(
    actions: &mut shimeji_core::model::OrderedMap<shimeji_core::model::Action>,
    written: &HashMap<String, i32>,
    extras: &[Extra],
) {
    actions.for_each_value_mut(|action| {
        for animation in &mut action.animations {
            animation
                .poses
                .retain(|pose| match pose.image_source.as_deref() {
                    Some(source) => {
                        file_stem(source).is_some_and(|stem| written.contains_key(&stem))
                    }
                    None => true,
                });
            for pose in &mut animation.poses {
                let Some(stem) = pose.image_source.as_deref().and_then(file_stem) else {
                    continue;
                };
                let Some(anchor_y) = written.get(&stem) else {
                    continue;
                };
                pose.anchor_x = 96;
                pose.anchor_y = *anchor_y;
                pose.image = Some(resolved(&stem));
                match pose.image_right_source.as_deref().and_then(file_stem) {
                    Some(right) if written.contains_key(&right) => {
                        pose.image_right = Some(resolved(&right));
                    }
                    _ => {
                        pose.image_right_source = None;
                        pose.image_right = None;
                    }
                }
            }
        }
        let Some(name) = action.name.clone() else {
            return;
        };
        let Some(animation) = action.animations.first_mut() else {
            return;
        };
        for extra in extras.iter().filter(|extra| extra.action == name) {
            animation.poses.push(extra_pose(extra));
        }
    });
}

fn extra_pose(extra: &Extra) -> Pose {
    Pose {
        image_source: Some(format!("/{}.png", extra.name)),
        image: Some(resolved(&extra.name)),
        image_right_source: None,
        image_right: None,
        right_sibling: None,
        anchor_x: 96,
        anchor_y: extra.anchor_y,
        velocity_x: extra.velocity_x,
        velocity_y: 0,
        duration_ticks: extra.ticks,
        sound: None,
        volume: None,
        line: 0,
    }
}

fn resolved(stem: &str) -> ResolvedImage {
    ResolvedImage {
        source: format!("/{stem}.png"),
        relative: format!("{stem}.png"),
        width: CELL_W as i32,
        height: CELL_H as i32,
    }
}

fn crop_cell(sheet: &[u8], sheet_w: u32, row: u32, col: u32) -> Vec<u8> {
    let mut out = vec![0u8; (CELL_W * CELL_H * 4) as usize];
    for y in 0..CELL_H {
        let src_x = col * CELL_W;
        let src_y = row * CELL_H + y;
        let src = ((src_y * sheet_w + src_x) * 4) as usize;
        let dst = (y * CELL_W * 4) as usize;
        let n = (CELL_W * 4) as usize;
        out[dst..dst + n].copy_from_slice(&sheet[src..src + n]);
    }
    out
}

fn alpha_bottom(rgba: &[u8], width: u32, height: u32) -> Option<i32> {
    let width = width as usize;
    let mut found = None;
    for y in 0..height as usize {
        let row = &rgba[y * width * 4..(y + 1) * width * 4];
        if row.chunks_exact(4).any(|pixel| pixel[3] > 0) {
            found = Some(y);
        }
    }
    found.map(|y| y as i32 + 1)
}

fn lower_median(values: &mut [i32]) -> i32 {
    values.sort_unstable();
    values[(values.len() - 1) / 2]
}

fn version_of(width: u32, height: u32) -> Option<(u32, u32)> {
    match (width, height) {
        V1 => Some(V1),
        V2 => Some(V2),
        _ => None,
    }
}

fn find_sheet(dir: &Path, pet: &Value, diags: &mut Vec<Diagnostic>, pack: &str) -> Option<PathBuf> {
    if let Some(raw) = pet.get("spritesheetPath").and_then(Value::as_str) {
        if !raw.is_empty() {
            let Some(relative) = safe_relative(raw) else {
                diags.push(error_diag(pack, "spritesheetPath escapes the pet folder"));
                return None;
            };
            let full = dir.join(relative);
            return match fs::symlink_metadata(&full) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    diags.push(error_diag(pack, "spritesheet is a symlink"));
                    None
                }
                Ok(meta) if meta.is_file() => Some(full),
                _ => {
                    diags.push(error_diag(
                        pack,
                        format!("spritesheetPath {raw} was not found"),
                    ));
                    None
                }
            };
        }
    }
    for name in ["spritesheet.png", "sheet.png", "pet.png"] {
        let full = dir.join(name);
        if is_regular_file(&full) {
            return Some(full);
        }
    }
    diags.push(error_diag(
        pack,
        "no Codex spritesheet (spritesheet.png, sheet.png, or pet.png)",
    ));
    None
}

fn safe_relative(raw: &str) -> Option<PathBuf> {
    if raw.is_empty() || raw.contains('\0') {
        return None;
    }
    let raw = raw.replace('\\', "/");
    let path = Path::new(&raw);
    if path.is_absolute() {
        return None;
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return None;
        }
    }
    Some(PathBuf::from(raw))
}

fn version_number(value: &Value) -> Option<u32> {
    match value {
        Value::Number(number) => number.as_u64().and_then(|n| u32::try_from(n).ok()),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn text_field(pet: &Value, key: &str) -> Option<String> {
    pet.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn read_capped(path: &Path, cap: u64) -> io::Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    if meta.len() > cap {
        return Err(io::Error::other(format!(
            "{} is larger than {cap} bytes",
            path.display()
        )));
    }
    let mut file = crate::import::fs::open_nofollow(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.is_file())
        .unwrap_or(false)
}

fn dir_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("pack")
        .to_string()
}

fn empty(name: &str, diagnostics: Vec<Diagnostic>, display_scale: Option<f64>) -> LoadOutcome {
    LoadOutcome {
        pack: Pack {
            name: name.to_string(),
            layout: LayoutKind::ClassicSingle,
            characters: Vec::new(),
        },
        diagnostics,
        display_scale,
    }
}

fn error_diag(pack: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(
        Severity::Error,
        DiagnosticKind::Other,
        pack,
        "",
        None,
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_parses_and_names_match_the_rows() {
        let (doc, diags) =
            crate::xml::parse_document(PROFILE.as_bytes(), "codex", "codex-profile.xml");
        let errors: Vec<_> = diags
            .iter()
            .filter(|diag| diag.severity == Severity::Error)
            .map(|diag| diag.message.clone())
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        let doc = doc.expect("profile");
        for row in ROWS {
            assert!(doc.actions.contains(row.action), "{}", row.action);
        }
        assert!(doc.actions.contains("WalkRight"));
        assert!(doc.actions.contains("Fall"));
        assert!(doc.behaviors.contains("CursorFollow"));
        assert!(doc.behaviors.contains("ChaseMouse"));
        let mut expected = std::collections::BTreeSet::new();
        for row in ROWS {
            for name in row.names {
                expected.insert((*name).to_string());
            }
        }
        let mut saw = std::collections::BTreeSet::new();
        for action in doc.actions.values() {
            for animation in &action.animations {
                for pose in &animation.poses {
                    if let Some(stem) = pose.image_source.as_deref().and_then(file_stem) {
                        saw.insert(stem);
                    }
                    if let Some(stem) = pose.image_right_source.as_deref().and_then(file_stem) {
                        saw.insert(stem);
                    }
                }
            }
        }
        assert_eq!(saw, expected);
    }
}

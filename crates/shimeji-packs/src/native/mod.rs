//! Native `pack.json` packs.
//!
//! Standard behaviors are the bundled shime XML. A manifest anchor is a delta
//! from that pose's atlas default, so a per-use anchor keeps its gap.

pub(crate) mod atlas;
pub(crate) mod manifest;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use serde_json::json;
use shimeji_core::diagnostics::{Diagnostic, DiagnosticKind, Severity};
use shimeji_core::model::{Action, ActionNode, LayoutKind, Pack, Schema};

use crate::LoadOutcome;
use crate::capabilities::{self, file_stem};
use crate::import::fs::{ensure_dir, read_regular, write_bytes};
use crate::standard_atlas::{self, CELL};
use crate::xml::LoadedDoc;

pub(crate) const ACTIONS_XML: &[u8] =
    include_bytes!("../../../../assets/packs/shime/conf/動作.xml");
pub(crate) const BEHAVIORS_XML: &[u8] =
    include_bytes!("../../../../assets/packs/shime/conf/行動.xml");

pub(crate) fn is_native_dir(path: &Path) -> bool {
    fs::symlink_metadata(path.join("pack.json"))
        .map(|meta| meta.is_file())
        .unwrap_or(false)
}

pub(crate) fn load_native(path: &Path) -> io::Result<LoadOutcome> {
    let pack_name = dir_name(path);
    let mut manifest = match manifest::read_manifest(path, &pack_name) {
        Ok(manifest) => manifest,
        Err(message) => {
            return Ok(empty(
                &pack_name,
                vec![error_diag(&pack_name, message)],
                None,
            ));
        }
    };
    let mut diagnostics = std::mem::take(&mut manifest.diagnostics);
    let (width, height, rgba) = match atlas::read_rgba(&manifest.atlas) {
        Ok(decoded) => decoded,
        Err(message) => {
            diagnostics.push(error_diag(&pack_name, message));
            return Ok(empty(&pack_name, diagnostics, Some(manifest.display_scale)));
        }
    };
    let expect = standard_atlas::atlas_size(1);
    if (width, height) != expect {
        diagnostics.push(error_diag(
            &pack_name,
            format!(
                "atlas is {width}x{height}; shimeji-standard-v1 is {}x{}",
                expect.0, expect.1
            ),
        ));
        return Ok(empty(&pack_name, diagnostics, Some(manifest.display_scale)));
    }
    let quiet = atlas::write_cells(path, &rgba, width)?;
    let mut character = match load_behaviors(path, &pack_name, &manifest, &mut diagnostics) {
        Some(character) => character,
        None => {
            return Ok(empty(&pack_name, diagnostics, Some(manifest.display_scale)));
        }
    };
    apply_overrides(&mut character, &manifest.poses, path, &mut diagnostics)?;
    let quiet: HashSet<String> = quiet.into_iter().collect();
    crate::layout::attach_images(
        &mut character,
        &path.join("img"),
        &mut diagnostics,
        &pack_name,
        &quiet,
    );
    let names = capabilities::collect_pose_names(&character);
    let missing = capabilities::missing_core(&names);
    if !missing.is_empty() {
        diagnostics.push(error_diag(
            &pack_name,
            format!("pack rejected: missing core poses {}", missing.join(", ")),
        ));
        return Ok(empty(&pack_name, diagnostics, Some(manifest.display_scale)));
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
        display_scale: Some(manifest.display_scale),
    })
}

fn load_behaviors(
    dir: &Path,
    pack: &str,
    manifest: &manifest::Manifest,
    diags: &mut Vec<Diagnostic>,
) -> Option<shimeji_core::model::Character> {
    if let Some(path) = &manifest.behaviors_file {
        let bytes = match read_regular(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                diags.push(error_diag(
                    pack,
                    format!("could not read behaviors: {error}"),
                ));
                return None;
            }
        };
        let file = path
            .strip_prefix(dir)
            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| "behaviors.xml".to_string());
        let (doc, more) = crate::xml::parse_document(&bytes, pack, &file);
        diags.extend(more);
        let doc = doc?;
        return Some(character_from(
            manifest,
            doc.schema,
            doc.schema,
            file.clone(),
            file,
            doc,
        ));
    }
    let (actions, more) = crate::xml::parse_document(ACTIONS_XML, pack, "conf/動作.xml");
    diags.extend(more);
    let (behaviors, more) = crate::xml::parse_document(BEHAVIORS_XML, pack, "conf/行動.xml");
    diags.extend(more);
    let (Some(actions), Some(behaviors)) = (actions, behaviors) else {
        diags.push(error_diag(pack, "standard behaviors did not parse"));
        return None;
    };
    Some(merge_standard(manifest, actions, behaviors))
}

fn merge_standard(
    manifest: &manifest::Manifest,
    actions: LoadedDoc,
    behaviors: LoadedDoc,
) -> shimeji_core::model::Character {
    let mut constants = actions.constants;
    for (key, value) in behaviors.constants.into_pairs() {
        constants.insert(key, value);
    }
    let mut action_map = actions.actions;
    for (key, value) in behaviors.actions.into_pairs() {
        action_map.insert(key, value);
    }
    let mut behavior_map = actions.behaviors;
    for (key, value) in behaviors.behaviors.into_pairs() {
        behavior_map.insert(key, value);
    }
    let mut tree = actions.behavior_tree;
    tree.extend(behaviors.behavior_tree);
    let mut info = actions.info;
    if behaviors.info.display_name.is_some() {
        info.display_name = behaviors.info.display_name;
    }
    info.display_name = Some(manifest.display_name.clone());
    shimeji_core::model::Character {
        name: manifest.display_name.clone(),
        schema: actions.schema,
        behavior_schema: behaviors.schema,
        image_root: "img".to_string(),
        actions_file: "conf/動作.xml".to_string(),
        behaviors_file: "conf/行動.xml".to_string(),
        info_file: None,
        constants,
        actions: action_map,
        behaviors: behavior_map,
        behavior_tree: tree,
        info,
    }
}

fn character_from(
    manifest: &manifest::Manifest,
    schema: Schema,
    behavior_schema: Schema,
    actions_file: String,
    behaviors_file: String,
    doc: LoadedDoc,
) -> shimeji_core::model::Character {
    let mut info = doc.info;
    info.display_name = Some(manifest.display_name.clone());
    shimeji_core::model::Character {
        name: manifest.display_name.clone(),
        schema,
        behavior_schema,
        image_root: "img".to_string(),
        actions_file,
        behaviors_file,
        info_file: None,
        constants: doc.constants,
        actions: doc.actions,
        behaviors: doc.behaviors,
        behavior_tree: doc.behavior_tree,
        info,
    }
}

fn apply_overrides(
    character: &mut shimeji_core::model::Character,
    poses: &std::collections::BTreeMap<String, manifest::PoseOverride>,
    dir: &Path,
    diags: &mut Vec<Diagnostic>,
) -> io::Result<()> {
    let mut deltas: HashMap<u16, (i32, i32)> = HashMap::new();
    let mut rights: HashMap<u16, String> = HashMap::new();
    for (name, over) in poses {
        let Some(pose) = standard_atlas::by_name(name) else {
            continue;
        };
        if let Some((x, y)) = over.anchor {
            deltas.insert(pose.index, (x - pose.anchor_x, y - pose.anchor_y));
        }
        if let Some(right) = &over.right {
            match place_right(dir, right) {
                Ok(relative) => {
                    rights.insert(pose.index, relative);
                }
                Err(message) => diags.push(Diagnostic::new(
                    Severity::Warning,
                    DiagnosticKind::MissingImage,
                    &character.name,
                    "pack.json",
                    None,
                    message,
                )),
            }
        }
    }
    character.actions.for_each_value_mut(|action| {
        walk_action_mut(action, &mut |action| {
            for animation in &mut action.animations {
                for pose in &mut animation.poses {
                    let Some(source) = pose.image_source.as_deref() else {
                        continue;
                    };
                    let Some(index) = image_index(source) else {
                        continue;
                    };
                    if let Some((dx, dy)) = deltas.get(&index) {
                        pose.anchor_x += dx;
                        pose.anchor_y += dy;
                    }
                    if let Some(relative) = rights.get(&index) {
                        pose.image_right_source = Some(format!("/{relative}"));
                    }
                }
            }
        });
    });
    Ok(())
}

fn place_right(dir: &Path, raw: &str) -> Result<String, String> {
    let Some(path) = manifest::safe_join(dir, raw) else {
        return Err(format!("right-facing image {raw} escapes the pack"));
    };
    let img = dir.join("img");
    ensure_dir(&img).map_err(|error| error.to_string())?;
    if let Ok(relative) = path.strip_prefix(&img) {
        let text = relative.to_string_lossy().replace('\\', "/");
        if !text.is_empty() && path.is_file() {
            return Ok(text);
        }
    }
    let bytes = read_regular(&path).map_err(|_| format!("right-facing image {raw} is missing"))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("right-facing image {raw} has no file name"))?;
    write_bytes(&img.join(file_name), &bytes).map_err(|error| error.to_string())?;
    Ok(file_name.to_string())
}

fn image_index(source: &str) -> Option<u16> {
    let stem = file_stem(source)?;
    if let Some(rest) = stem.strip_prefix("shime") {
        if !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()) {
            if let Ok(index) = rest.parse::<u16>() {
                if standard_atlas::by_index(index).is_some() {
                    return Some(index);
                }
            }
        }
    }
    standard_atlas::by_name(&stem).map(|pose| pose.index)
}

fn walk_action_mut(action: &mut Action, visit: &mut impl FnMut(&mut Action)) {
    visit(action);
    for child in &mut action.children {
        if let ActionNode::Action(child) = child {
            walk_action_mut(child, visit);
        }
    }
}

/// Build a native pack from classic `shime{N}.png` files.
pub fn write_standard_pack(classic_dir: &Path, dest: &Path) -> io::Result<()> {
    let id = dir_name(classic_dir);
    let (width, height) = standard_atlas::atlas_size(1);
    let mut atlas = vec![0u8; (width * height * 4) as usize];
    for index in 1..=standard_atlas::POSE_COUNT {
        let path = classic_dir.join("img").join(format!("shime{index}.png"));
        let (image_w, image_h, rgba) = atlas::read_rgba(&path).map_err(io::Error::other)?;
        if image_w != CELL || image_h != CELL {
            return Err(io::Error::other(format!(
                "shime{index}.png is {image_w}x{image_h}; the standard cell is {CELL}"
            )));
        }
        let (origin_x, origin_y) = standard_atlas::cell_origin(index).expect("pose cell");
        atlas::blit(&mut atlas, width, &rgba, image_w, origin_x, origin_y, CELL);
    }
    ensure_dir(dest)?;
    let encoded = crate::images::encode_rgba(width, height, &atlas).map_err(io::Error::other)?;
    write_bytes(&dest.join("atlas.png"), &encoded)?;
    let name = if id == "shime" {
        "Shimeji"
    } else {
        id.as_str()
    };
    let mut body = json!({
        "format": "shimeji2-pack",
        "formatVersion": 1,
        "id": id,
        "name": name,
        "license": "Zlib",
        "provenance": {"derivedFrom": "classic:shimeji4mac"},
        "art": {"facing": "left", "pixelArt": false, "displayScale": 1.0},
        "atlases": [{"image": "atlas.png", "layout": "shimeji-standard-v1"}],
        "behaviors": "standard"
    });
    if id == "shime" {
        body["authors"] = json!([{"name": "Yuki Yamada (group finity)"}]);
    }
    let text = serde_json::to_vec_pretty(&body).map_err(io::Error::other)?;
    write_bytes(&dest.join("pack.json"), &text)?;
    Ok(())
}

pub(crate) use ACTIONS_XML as STANDARD_ACTIONS;
pub(crate) use BEHAVIORS_XML as STANDARD_BEHAVIORS;

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
        "pack.json",
        None,
        message,
    )
}

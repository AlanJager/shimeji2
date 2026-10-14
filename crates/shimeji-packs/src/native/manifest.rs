//! `pack.json` for a shimeji2 native pack.
//!
//! Unknown fields are ignored. `behaviors` is `"standard"` or a file name.
//! The only atlas layout this unit loads is `shimeji-standard-v1`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use shimeji_core::diagnostics::{Diagnostic, DiagnosticKind, Severity};

const MANIFEST_CAP: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct PoseOverride {
    pub anchor: Option<(i32, i32)>,
    pub right: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Manifest {
    #[allow(dead_code)]
    pub id: String,
    pub display_name: String,
    pub display_scale: f64,
    pub atlas: PathBuf,
    pub behaviors_file: Option<PathBuf>,
    pub poses: BTreeMap<String, PoseOverride>,
    pub diagnostics: Vec<Diagnostic>,
}

pub(crate) fn read_manifest(dir: &Path, pack: &str) -> Result<Manifest, String> {
    let path = dir.join("pack.json");
    let meta = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("pack.json is not a regular file".to_string());
    }
    if meta.len() > MANIFEST_CAP {
        return Err("pack.json is larger than 1 MiB".to_string());
    }
    let bytes = crate::import::fs::read_regular(&path).map_err(|error| error.to_string())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    parse(dir, pack, &value)
}

fn parse(dir: &Path, pack: &str, value: &Value) -> Result<Manifest, String> {
    let object = value.as_object().ok_or("pack.json is not an object")?;
    let format = object.get("format").and_then(Value::as_str).unwrap_or("");
    if format != "shimeji2-pack" {
        return Err(format!("format {format:?} is not shimeji2-pack"));
    }
    let mut diagnostics = Vec::new();
    if let Some(version) = object.get("formatVersion") {
        let ok = version.as_u64() == Some(1) || version.as_i64() == Some(1);
        if !ok {
            diagnostics.push(warn(pack, format!("formatVersion {version} is not 1")));
        }
    }
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .unwrap_or(pack)
        .to_string();
    let display_name = object
        .get("name")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .unwrap_or(id.as_str())
        .to_string();
    let _ = (
        object.get("description"),
        object.get("authors"),
        object.get("license"),
        object.get("provenance"),
    );
    if let Some(file) = object.get("licenseFile").and_then(Value::as_str) {
        if safe_join(dir, file).is_none_or(|path| !path.is_file()) {
            diagnostics.push(warn(pack, format!("license file {file} is missing")));
        }
    }
    let (display_scale, art_diags) = art_of(object.get("art"), pack);
    diagnostics.extend(art_diags);
    let atlases = object
        .get("atlases")
        .and_then(Value::as_array)
        .ok_or("pack.json has no atlases")?;
    if atlases.is_empty() {
        return Err("pack.json has no atlases".to_string());
    }
    if atlases.len() > 1 {
        diagnostics.push(warn(pack, "only the first atlas is used"));
    }
    let atlas_value = &atlases[0];
    let layout = atlas_value.get("layout").and_then(Value::as_str);
    if layout != Some("shimeji-standard-v1") {
        return Err(
            "only shimeji-standard-v1 atlases load; other layouts are a later unit".to_string(),
        );
    }
    let image = atlas_value
        .get("image")
        .and_then(Value::as_str)
        .ok_or("atlas is missing image")?;
    let atlas = safe_join(dir, image).ok_or("atlas image escapes the pack")?;
    if let Some(retina) = atlas_value.get("image@2x").and_then(Value::as_str) {
        match safe_join(dir, retina) {
            Some(path) if path.is_file() => {
                if let Ok((width, height)) = crate::images::png_pixel_size(&path) {
                    if (width, height) != crate::standard_atlas::atlas_size(2) {
                        diagnostics.push(warn(
                            pack,
                            format!("image@2x is {width}x{height}; expected 2048x1536"),
                        ));
                    }
                }
            }
            _ => diagnostics.push(warn(pack, format!("image@2x {retina} is missing"))),
        }
    }
    let poses = poses_of(object.get("poses"), pack, &mut diagnostics);
    let behaviors_file = match object.get("behaviors") {
        None => None,
        Some(Value::String(text)) if text == "standard" => None,
        Some(Value::String(text)) => {
            let path = safe_join(dir, text).ok_or("behaviors path escapes the pack")?;
            if crate::import::fs::read_regular(&path).is_err() {
                return Err(format!("behaviors file {text} was not found"));
            }
            Some(path)
        }
        Some(other) => {
            return Err(format!(
                "behaviors must be \"standard\" or a file name, got {other}"
            ));
        }
    };
    if let Some(sounds) = object.get("sounds").and_then(Value::as_object) {
        for (name, sound) in sounds {
            let file = sound.get("file").and_then(Value::as_str);
            match file {
                Some(file) => match safe_join(dir, file) {
                    Some(path) if path.is_file() => {}
                    _ => {
                        diagnostics.push(warn(pack, format!("sound {name} file {file} is missing")))
                    }
                },
                None => diagnostics.push(warn(pack, format!("sound {name} has no file"))),
            }
        }
    }
    Ok(Manifest {
        id,
        display_name,
        display_scale,
        atlas,
        behaviors_file,
        poses,
        diagnostics,
    })
}

fn art_of(art: Option<&Value>, pack: &str) -> (f64, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let Some(art) = art else {
        return (1.0, diagnostics);
    };
    if let Some(facing) = art.get("facing").and_then(Value::as_str) {
        if facing != "left" && facing != "right" {
            diagnostics.push(warn(
                pack,
                format!("art.facing {facing} is not left or right"),
            ));
        }
    } else if art.get("facing").is_some() {
        diagnostics.push(warn(pack, "art.facing is not a string"));
    }
    if let Some(pixel_art) = art.get("pixelArt") {
        if !pixel_art.is_boolean() {
            diagnostics.push(warn(pack, "art.pixelArt is not a boolean"));
        }
    }
    let scale = match art.get("displayScale") {
        None => 1.0,
        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.0),
        Some(_) => {
            diagnostics.push(warn(pack, "art.displayScale is not a number"));
            return (1.0, diagnostics);
        }
    };
    if scale.is_finite() && scale > 0.0 {
        (scale, diagnostics)
    } else {
        diagnostics.push(warn(
            pack,
            format!("art.displayScale {scale} is not positive; using 1"),
        ));
        (1.0, diagnostics)
    }
}

fn poses_of(
    poses: Option<&Value>,
    pack: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, PoseOverride> {
    let mut out = BTreeMap::new();
    let Some(object) = poses.and_then(Value::as_object) else {
        return out;
    };
    for (name, value) in object {
        if crate::standard_atlas::by_name(name).is_none() {
            diagnostics.push(warn(
                pack,
                format!("pose {name} is not in the standard atlas"),
            ));
            continue;
        }
        let anchor = match value.get("anchor") {
            None => None,
            Some(Value::Array(pair)) if pair.len() == 2 => {
                match (as_i32(&pair[0]), as_i32(&pair[1])) {
                    (Some(x), Some(y)) => Some((x, y)),
                    _ => {
                        diagnostics.push(warn(pack, format!("pose {name} anchor is not a pair")));
                        None
                    }
                }
            }
            Some(_) => {
                diagnostics.push(warn(pack, format!("pose {name} anchor is not a pair")));
                None
            }
        };
        let right = value
            .get("right")
            .and_then(Value::as_str)
            .map(str::to_string);
        out.insert(name.clone(), PoseOverride { anchor, right });
    }
    out
}

fn as_i32(value: &Value) -> Option<i32> {
    value.as_i64().and_then(|number| i32::try_from(number).ok())
}

pub(crate) fn safe_join(dir: &Path, raw: &str) -> Option<PathBuf> {
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
    Some(dir.join(raw))
}

fn warn(pack: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(
        Severity::Warning,
        DiagnosticKind::Other,
        pack,
        "pack.json",
        None,
        message,
    )
}

//! Classic exporter.
//!
//! Writes `img/shime{N}.png` and the bundled Japanese XML. When every pose
//! anchor delta is zero the XML bytes are the originals, so a shime round trip
//! stays trace-identical. A non-zero delta is added to every `基準座標` of that
//! image, which keeps per-use gaps such as shime9 and shime19.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use crate::import::fs::{ensure_dir, read_regular, write_bytes};
use crate::native::{self, manifest};
use crate::standard_atlas::{self, CELL};
use crate::xml::parse_pair;

pub fn export_classic(native_dir: &Path, dest: &Path) -> io::Result<()> {
    let pack = native_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("pack");
    let manifest = manifest::read_manifest(native_dir, pack).map_err(io::Error::other)?;
    let (width, height, rgba) =
        crate::native::atlas::read_rgba(&manifest.atlas).map_err(io::Error::other)?;
    let expect = standard_atlas::atlas_size(1);
    if (width, height) != expect {
        return Err(io::Error::other(format!(
            "atlas is {width}x{height}; shimeji-standard-v1 is {}x{}",
            expect.0, expect.1
        )));
    }
    let img = dest.join("img");
    let conf = dest.join("conf");
    ensure_dir(&img)?;
    ensure_dir(&conf)?;
    for index in 1..=standard_atlas::POSE_COUNT {
        let (origin_x, origin_y) = standard_atlas::cell_origin(index).expect("pose cell");
        let cell = crate::native::atlas::crop(&rgba, width, origin_x, origin_y, CELL);
        let encoded = crate::images::encode_rgba(CELL, CELL, &cell).map_err(io::Error::other)?;
        write_bytes(&img.join(format!("shime{index}.png")), &encoded)?;
    }
    let deltas = anchor_deltas(&manifest.poses);
    if let Some(path) = &manifest.behaviors_file {
        let bytes = read_regular(path)?;
        let patched = patch_xml(&bytes, &deltas);
        write_bytes(&conf.join("actions.xml"), &patched)?;
        write_bytes(&conf.join("behaviors.xml"), &patched)?;
        return Ok(());
    }
    let actions = if deltas.is_empty() {
        native::STANDARD_ACTIONS.to_vec()
    } else {
        patch_xml(native::STANDARD_ACTIONS, &deltas)
    };
    write_bytes(&conf.join("動作.xml"), &actions)?;
    write_bytes(&conf.join("行動.xml"), native::STANDARD_BEHAVIORS)?;
    Ok(())
}

fn anchor_deltas(
    poses: &std::collections::BTreeMap<String, manifest::PoseOverride>,
) -> HashMap<u16, (i32, i32)> {
    let mut deltas = HashMap::new();
    for (name, over) in poses {
        let Some(pose) = standard_atlas::by_name(name) else {
            continue;
        };
        let Some((x, y)) = over.anchor else {
            continue;
        };
        let dx = x - pose.anchor_x;
        let dy = y - pose.anchor_y;
        if dx != 0 || dy != 0 {
            deltas.insert(pose.index, (dx, dy));
        }
    }
    deltas
}

fn patch_xml(bytes: &[u8], deltas: &HashMap<u16, (i32, i32)>) -> Vec<u8> {
    if deltas.is_empty() {
        return bytes.to_vec();
    }
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        out.push_str(&patch_line(line, deltas));
    }
    out.into_bytes()
}

fn patch_line(line: &str, deltas: &HashMap<u16, (i32, i32)>) -> String {
    let Some(index) = shime_on_line(line) else {
        return line.to_string();
    };
    let Some(&(dx, dy)) = deltas.get(&index) else {
        return line.to_string();
    };
    add_anchor(line, dx, dy)
}

fn shime_on_line(line: &str) -> Option<u16> {
    let start = line.find("/shime")?;
    let rest = &line[start + "/shime".len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    if !rest[digits.len()..].starts_with(".png") {
        return None;
    }
    digits.parse().ok()
}

fn add_anchor(line: &str, dx: i32, dy: i32) -> String {
    for key in ["基準座標=\"", "ImageAnchor=\""] {
        let Some(pos) = line.find(key) else {
            continue;
        };
        let value_start = pos + key.len();
        let Some(end) = line[value_start..].find('"') else {
            continue;
        };
        let value = &line[value_start..value_start + end];
        let Some((x, y)) = parse_pair(value) else {
            continue;
        };
        let replaced = format!("{key}{},{}\"", x + dx, y + dy);
        return format!(
            "{}{replaced}{}",
            &line[..pos],
            &line[value_start + end + 1..]
        );
    }
    line.to_string()
}

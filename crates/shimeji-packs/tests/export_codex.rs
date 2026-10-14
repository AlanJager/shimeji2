//! Native → Codex v2. Both plan scenarios: an extension atlas fills every row,
//! and a pack without one reports idle, waving, and the off-axis looks.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use shimeji_packs::{
    CollisionPolicy, MissingRow, PackOrigin, Severity, directory_origin, export_codex, import_pack,
    write_standard_pack,
};

const CELL_W: u32 = 192;
const CELL_H: u32 = 208;
const SHEET_W: u32 = 1536;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch(label: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "shimeji-u16-{label}-{}-{nanos}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn encode(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(rgba).unwrap();
    drop(writer);
    out
}

fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    assert_eq!(info.color_type, png::ColorType::Rgba);
    (info.width, info.height, buf)
}

fn cell_alpha(rgba: &[u8], col: u32, row: u32, local_x: u32, local_y: u32) -> u8 {
    let x = col * CELL_W + local_x;
    let y = row * CELL_H + local_y;
    rgba[((y * SHEET_W + x) * 4 + 3) as usize]
}

fn cell_has_ink(rgba: &[u8], col: u32, row: u32) -> bool {
    for y in 0..CELL_H {
        for x in 0..CELL_W {
            if cell_alpha(rgba, col, row, x, y) > 0 {
                return true;
            }
        }
    }
    false
}

fn shime_native(root: &Path) -> PathBuf {
    let native = root.join("native");
    write_standard_pack(&repo().join("assets/packs/shime"), &native).unwrap();
    native
}

fn add_opaque_ext(native: &Path) {
    let (width, height) = (1024u32, 768u32);
    let rgba = vec![255u8; (width * height * 4) as usize];
    fs::write(native.join("ext.png"), encode(width, height, &rgba)).unwrap();
    let mut body: serde_json::Value =
        serde_json::from_slice(&fs::read(native.join("pack.json")).unwrap()).unwrap();
    body["atlases"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "image": "ext.png",
            "layout": "shimeji-ext-v1"
        }));
    fs::write(
        native.join("pack.json"),
        serde_json::to_vec_pretty(&body).unwrap(),
    )
    .unwrap();
}

fn labels(rows: &[MissingRow]) -> Vec<String> {
    rows.iter()
        .map(|row| match &row.direction {
            Some(direction) => format!("{}:{direction}", row.row),
            None => row.row.clone(),
        })
        .collect()
}

const OFF_AXIS: [&str; 12] = [
    "022.5", "045", "067.5", "112.5", "135", "157.5", "202.5", "225", "247.5", "292.5", "315",
    "337.5",
];

#[test]
fn export_codex_with_an_extension_atlas_loads_and_has_no_missing_rows() {
    let root = scratch("ext");
    let native = shime_native(&root);
    add_opaque_ext(&native);
    let dest = root.join("codex");
    let exported = export_codex(&native, &dest).unwrap();
    assert!(exported.missing.is_empty(), "{:?}", exported.missing);

    let (width, height, _) = decode(&fs::read(dest.join("spritesheet.png")).unwrap());
    assert_eq!((width, height), (1536, 2288));
    let pet: serde_json::Value =
        serde_json::from_slice(&fs::read(dest.join("pet.json")).unwrap()).unwrap();
    assert_eq!(pet["spriteVersionNumber"], 2);
    assert_eq!(pet["spritesheetPath"], "spritesheet.png");
    assert_eq!(pet["id"], "shime");
    assert_eq!(pet["displayName"], "Shimeji");
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(dest.join("missing-rows.json")).unwrap()).unwrap();
    assert!(report["missing"].as_array().unwrap().is_empty());

    let outcome = import_pack(&dest, &root, CollisionPolicy::Report).expect("import");
    let errors: Vec<_> = outcome
        .diagnostics
        .iter()
        .filter(|diag| diag.severity == Severity::Error)
        .map(|diag| diag.message.clone())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(directory_origin(&outcome.path), Some(PackOrigin::CodexV2));
    let character = &outcome.pack.characters[0];
    for name in ["Idle", "Wave", "CursorFollow", "WalkLeft", "Jump"] {
        assert!(character.behaviors.contains(name), "missing {name}");
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn export_codex_without_an_extension_atlas_reports_idle_waving_and_look_directions() {
    let root = scratch("plain");
    let native = shime_native(&root);
    let dest = root.join("codex");
    let exported = export_codex(&native, &dest).unwrap();
    let mut expect = vec!["idle".to_string(), "waving".to_string()];
    for direction in OFF_AXIS {
        expect.push(format!("look:{direction}"));
    }
    assert_eq!(labels(&exported.missing), expect);
    for cardinal in ["000", "090", "180", "270"] {
        assert!(
            !labels(&exported.missing)
                .iter()
                .any(|label| label.ends_with(cardinal)),
            "{cardinal} is filled from a classic frame"
        );
    }

    let (width, height, rgba) = decode(&fs::read(dest.join("spritesheet.png")).unwrap());
    assert_eq!((width, height), (1536, 2288));
    assert!(!cell_has_ink(&rgba, 0, 0), "idle");
    assert!(!cell_has_ink(&rgba, 0, 3), "waving");
    assert!(cell_has_ink(&rgba, 0, 2), "running-left");
    assert!(cell_has_ink(&rgba, 0, 9), "look 000");
    assert!(!cell_has_ink(&rgba, 1, 9), "look 022.5");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn pixel_art_places_a_128_sprite_at_x32_and_mirror_flips_before_the_scale() {
    let root = scratch("pixel");
    let classic = root.join("frames");
    fs::create_dir_all(classic.join("img")).unwrap();
    for index in 1..=46 {
        let mut rgba = vec![0u8; 128 * 128 * 4];
        if index == 1 {
            rgba[0] = 255;
            rgba[3] = 255;
        }
        fs::write(
            classic.join(format!("img/shime{index}.png")),
            encode(128, 128, &rgba),
        )
        .unwrap();
    }
    let native = root.join("native");
    write_standard_pack(&classic, &native).unwrap();

    let mut body: serde_json::Value =
        serde_json::from_slice(&fs::read(native.join("pack.json")).unwrap()).unwrap();
    body["art"]["pixelArt"] = serde_json::json!(true);
    fs::write(
        native.join("pack.json"),
        serde_json::to_vec_pretty(&body).unwrap(),
    )
    .unwrap();
    let pixel_dest = root.join("pixel-out");
    export_codex(&native, &pixel_dest).unwrap();
    let (_, _, rgba) = decode(&fs::read(pixel_dest.join("spritesheet.png")).unwrap());
    assert_eq!(cell_alpha(&rgba, 0, 2, 32, 75), 255, "1x left edge");
    assert_eq!(cell_alpha(&rgba, 0, 2, 0, 11), 0, "not the 1.5x corner");
    assert_eq!(cell_alpha(&rgba, 0, 1, 159, 75), 255, "mirrored 1x");

    body["art"]["pixelArt"] = serde_json::json!(false);
    fs::write(
        native.join("pack.json"),
        serde_json::to_vec_pretty(&body).unwrap(),
    )
    .unwrap();
    let smooth_dest = root.join("smooth-out");
    export_codex(&native, &smooth_dest).unwrap();
    let (_, _, rgba) = decode(&fs::read(smooth_dest.join("spritesheet.png")).unwrap());
    assert_eq!(cell_alpha(&rgba, 0, 2, 0, 11), 255, "1.5x left edge");
    assert_eq!(cell_alpha(&rgba, 0, 1, 191, 11), 255, "mirrored 1.5x");
    let _ = fs::remove_dir_all(&root);
}

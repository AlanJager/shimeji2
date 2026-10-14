//! Synthetic Codex sheets. No third-party art.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use shimeji_packs::model::Character;
use shimeji_packs::{
    CODEX_DISPLAY_SCALE, CollisionPolicy, PackOrigin, PoseCapability, Severity, directory_origin,
    import_pack, pose_capabilities,
};

const CELL_W: u32 = 192;
const CELL_H: u32 = 208;
const SHEET_W: u32 = 1536;
/// Stock frames per row. Rows 9 and 10 exist only on a v2 sheet.
const STOCK: [u32; 11] = [6, 8, 8, 4, 5, 8, 6, 6, 6, 8, 8];
const HAPPY: &[&str] = &[
    "Idle",
    "WalkLeft",
    "WalkRight",
    "Wave",
    "Jump",
    "Waiting",
    "Review",
];

fn scratch(label: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "shimeji-u13-{label}-{}-{nanos}-{n}",
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

fn paint(rgba: &mut [u8], width: u32, col: u32, row: u32, local_y: u32) {
    let x = col * CELL_W;
    let y = row * CELL_H + local_y;
    let index = ((y * width + x) * 4) as usize;
    rgba[index] = 200;
    rgba[index + 3] = 255;
}

/// One opaque pixel on row 100 of every stock cell. `extra_idle` also fills
/// idle column 6, which is past the six stock idle frames.
fn sheet(rows: u32, extra_idle: bool) -> Vec<u8> {
    let height = rows * CELL_H;
    let mut rgba = vec![0u8; (SHEET_W * height * 4) as usize];
    for row in 0..rows {
        for col in 0..STOCK[row as usize] {
            paint(&mut rgba, SHEET_W, col, row, 100);
        }
    }
    if extra_idle {
        paint(&mut rgba, SHEET_W, 6, 0, 100);
    }
    encode(SHEET_W, height, &rgba)
}

fn write_pet(dir: &Path, version: u32, png_bytes: &[u8]) {
    fs::create_dir_all(dir).unwrap();
    let pet = format!(
        r#"{{"id":"synthetic","displayName":"Synthetic","spriteVersionNumber":{version}}}"#
    );
    fs::write(dir.join("pet.json"), pet).unwrap();
    fs::write(dir.join("spritesheet.png"), png_bytes).unwrap();
}

fn character_of(pack: &shimeji_packs::model::Pack) -> &Character {
    assert_eq!(pack.characters.len(), 1, "expected one character");
    &pack.characters[0]
}

fn errors(diags: &[shimeji_packs::Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter(|diag| diag.severity == Severity::Error)
        .map(|diag| diag.message.clone())
        .collect()
}

#[test]
fn a_v1_sheet_imports_idle_walk_wave_jump_waiting_and_review() {
    let root = scratch("v1");
    let source = root.join("codex-v1");
    write_pet(&source, 1, &sheet(9, false));
    let outcome = import_pack(&source, &root, CollisionPolicy::Report).expect("import");
    let errors = errors(&outcome.diagnostics);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(outcome.display_scale, Some(CODEX_DISPLAY_SCALE));
    assert_eq!(directory_origin(&outcome.path), Some(PackOrigin::CodexV1));
    let character = character_of(&outcome.pack);
    assert_eq!(character.name, "Synthetic");
    for name in HAPPY {
        assert!(character.behaviors.contains(name), "missing {name}");
    }
    assert!(
        !character.behaviors.contains("CursorFollow"),
        "v1 has no look row"
    );
    assert_eq!(
        pose_capabilities(&outcome.pack),
        vec![
            PoseCapability::Core,
            PoseCapability::Drag,
            PoseCapability::Sit,
            PoseCapability::Tumble,
        ]
    );
    let walk = character.actions.get("WalkLeft").unwrap();
    let pose = &walk.animations[0].poses[0];
    assert_eq!(pose.velocity_x, -2);
    assert_eq!((pose.anchor_x, pose.anchor_y), (96, 101));
    let image = pose.image.as_ref().unwrap();
    assert_eq!((image.width, image.height), (192, 208));
    assert!(
        pose.image_right.is_some(),
        "walk uses the running-right row"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_v2_sheet_adds_look_and_cursor_follow() {
    let root = scratch("v2");
    let source = root.join("codex-v2");
    write_pet(&source, 2, &sheet(11, false));
    let outcome = import_pack(&source, &root, CollisionPolicy::Report).expect("import");
    let errors = errors(&outcome.diagnostics);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(directory_origin(&outcome.path), Some(PackOrigin::CodexV2));
    let character = character_of(&outcome.pack);
    for name in HAPPY {
        assert!(character.behaviors.contains(name), "missing {name}");
    }
    assert!(character.behaviors.contains("CursorFollow"));
    let caps = pose_capabilities(&outcome.pack);
    assert!(caps.contains(&PoseCapability::Look));
    assert_eq!(
        caps,
        vec![
            PoseCapability::Core,
            PoseCapability::Drag,
            PoseCapability::Sit,
            PoseCapability::Tumble,
            PoseCapability::Look,
        ]
    );
    let follow = character.actions.get("CursorFollow").unwrap();
    assert_eq!(follow.animations[0].poses.len(), 16);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_manifest_that_claims_v1_on_a_v2_sheet_is_v2() {
    let root = scratch("claim");
    let source = root.join("claimed");
    write_pet(&source, 1, &sheet(11, false));
    let outcome = import_pack(&source, &root, CollisionPolicy::Report).expect("import");
    assert_eq!(directory_origin(&outcome.path), Some(PackOrigin::CodexV2));
    assert!(
        character_of(&outcome.pack)
            .behaviors
            .contains("CursorFollow")
    );
    assert!(pose_capabilities(&outcome.pack).contains(&PoseCapability::Look));
    let notes: Vec<_> = outcome
        .diagnostics
        .iter()
        .filter(|diag| diag.severity == Severity::Info)
        .map(|diag| diag.message.clone())
        .collect();
    assert!(
        notes
            .iter()
            .any(|message| message.contains("sprite version 1") && message.contains("Codex v2")),
        "{notes:?}"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_1500_by_1800_sheet_is_rejected_with_a_diagnostic() {
    let root = scratch("bad-size");
    let source = root.join("bad-size");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("pet.json"),
        r#"{"id":"bad","displayName":"Bad","spriteVersionNumber":1}"#,
    )
    .unwrap();
    let rgba = vec![0u8; 1500 * 1800 * 4];
    fs::write(source.join("spritesheet.png"), encode(1500, 1800, &rgba)).unwrap();
    let outcome = import_pack(&source, &root, CollisionPolicy::Report).expect("import stays Ok");
    assert!(outcome.pack.characters.is_empty());
    let messages: Vec<_> = outcome
        .diagnostics
        .iter()
        .map(|diag| diag.message.as_str())
        .collect();
    assert!(
        messages.contains(&"sheet is 1500x1800; Codex sheets are 1536x1872 (v1) or 1536x2288 (v2)"),
        "{messages:?}"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn cells_past_the_stock_frame_count_are_kept_when_non_empty() {
    let root = scratch("extra");
    let source = root.join("extra");
    write_pet(&source, 1, &sheet(9, true));
    let outcome = import_pack(&source, &root, CollisionPolicy::Report).expect("import");
    let errors = errors(&outcome.diagnostics);
    assert!(errors.is_empty(), "{errors:?}");
    let idle = character_of(&outcome.pack).actions.get("Idle").unwrap();
    let poses = &idle.animations[0].poses;
    assert_eq!(poses.len(), 7);
    let extra = poses.last().unwrap();
    assert_eq!(extra.image_source.as_deref(), Some("/idle-extra-7.png"));
    assert_eq!(extra.duration_ticks, 7);
    assert_eq!((extra.anchor_x, extra.anchor_y), (96, 101));
    assert!(outcome.path.join("img/idle-extra-7.png").is_file());
    assert!(!outcome.path.join("img/idle-extra-8.png").exists());
    let _ = fs::remove_dir_all(&root);
}

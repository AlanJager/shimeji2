//! The installed-pack catalog: format, capabilities, characters, diagnostics.

#[path = "support.rs"]
mod support;

use shimeji_app::import::{self, Outcome};
use shimeji_app::library::{Capability, Library, PackFormat};
use shimeji_core::model::LayoutKind;

#[test]
fn the_minimal_ee_fixture_is_labelled_shimeji_ee_with_two_characters() {
    let root = support::scratch("lib-ee");
    let outcome = import::import(&support::fixture("minimal-ee"), &root, None);
    let Outcome::Success(installed) = outcome else {
        panic!("expected success");
    };
    assert_eq!(installed.characters, 2);
    let mut library = Library::open(&root);
    library.insert(
        &installed.id,
        &installed.path,
        &installed.pack,
        &installed.diagnostics,
    );
    let pack = library.get("minimal-ee").expect("installed");
    assert_eq!(pack.format, PackFormat::ShimejiEe);
    assert_eq!(pack.format_label(), "Shimeji-ee");
    assert_eq!(pack.layout, LayoutKind::ImageSets);
    assert_eq!(
        pack.capabilities,
        vec![Capability::Core, Capability::Drag, Capability::Sit]
    );
    assert_eq!(pack.capability_labels(), vec!["core", "drag", "sit"]);
    assert!(!pack.degraded());
    let names: Vec<&str> = pack
        .characters
        .iter()
        .map(|row| row.name.as_str())
        .collect();
    assert_eq!(names, vec!["A", "B"]);
    assert!(pack.diagnostics.is_empty());
    let scanned = Library::open(&root);
    assert_eq!(scanned.get("minimal-ee").unwrap().characters.len(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_unknown_class_is_degraded_and_lists_a_toggleable_behavior() {
    let root = support::scratch("lib-class");
    let source = root.join("src-weird");
    support::write_unknown_class(&source);
    let outcome = import::import(&source, &root, None);
    let Outcome::Degraded(installed) = outcome else {
        panic!("expected degraded");
    };
    let mut library = Library::open(&root);
    library.insert(
        &installed.id,
        &installed.path,
        &installed.pack,
        &installed.diagnostics,
    );
    let pack = library.get("src-weird").unwrap();
    assert!(pack.degraded());
    assert!(
        pack.diagnostics.iter().any(|diagnostic| {
            diagnostic.file.ends_with("actions.xml") && diagnostic.line.is_some()
        }),
        "{:?}",
        pack.diagnostics
    );
    let wave = pack.characters[0]
        .behaviors
        .iter()
        .find(|row| row.name == "Wave")
        .unwrap();
    assert!(wave.toggleable);
    assert!(!wave.hidden);
    let _ = std::fs::remove_dir_all(&root);
}

fn encode_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(rgba).unwrap();
    drop(writer);
    out
}

/// Stock frames per Codex row. The last two rows are v2 only.
fn codex_sheet(rows: u32) -> Vec<u8> {
    const STOCK: [u32; 11] = [6, 8, 8, 4, 5, 8, 6, 6, 6, 8, 8];
    let width = 1536u32;
    let height = rows * 208;
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for row in 0..rows {
        for col in 0..STOCK[row as usize] {
            let x = col * 192;
            let y = row * 208 + 100;
            let index = ((y * width + x) * 4) as usize;
            rgba[index + 3] = 255;
        }
    }
    encode_rgba(width, height, &rgba)
}

fn write_codex(dir: &std::path::Path, version: u32, rows: u32) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("pet.json"),
        format!(
            r#"{{"id":"synthetic","displayName":"Synthetic","spriteVersionNumber":{version}}}"#
        ),
    )
    .unwrap();
    std::fs::write(dir.join("spritesheet.png"), codex_sheet(rows)).unwrap();
}

#[test]
fn codex_and_native_packs_show_format_and_pose_capabilities() {
    let root = support::scratch("lib-u13");
    let v1 = root.join("src-v1");
    let v2 = root.join("src-v2");
    write_codex(&v1, 1, 9);
    write_codex(&v2, 2, 11);
    shimeji_packs::import_pack(&v1, &root, shimeji_packs::CollisionPolicy::Report).unwrap();
    shimeji_packs::import_pack(&v2, &root, shimeji_packs::CollisionPolicy::Report).unwrap();
    let classic =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime");
    shimeji_packs::write_standard_pack(&classic, &root.join("packs/bundled-shime")).unwrap();

    let library = Library::open(&root);
    let v1 = library.get("src-v1").unwrap();
    assert_eq!(v1.format, PackFormat::CodexV1);
    assert_eq!(v1.format_label(), "Codex v1");
    assert_eq!(
        v1.capabilities,
        vec![
            Capability::Core,
            Capability::Drag,
            Capability::Sit,
            Capability::Tumble,
        ]
    );
    assert!(!v1.degraded());

    let v2 = library.get("src-v2").unwrap();
    assert_eq!(v2.format, PackFormat::CodexV2);
    assert_eq!(v2.format_label(), "Codex v2");
    assert_eq!(
        v2.capabilities,
        vec![
            Capability::Core,
            Capability::Drag,
            Capability::Sit,
            Capability::Tumble,
            Capability::Look,
        ]
    );

    let native = library.get("bundled-shime").unwrap();
    assert_eq!(native.format, PackFormat::Native);
    assert_eq!(native.format_label(), "Native");
    assert_eq!(
        native.capabilities,
        vec![
            Capability::Core,
            Capability::Drag,
            Capability::Sit,
            Capability::Tumble,
            Capability::Wall,
            Capability::Ceiling,
            Capability::Window,
            Capability::Multiply,
        ]
    );
    assert!(!native.capabilities.contains(&Capability::Look));
    let _ = std::fs::remove_dir_all(&root);
}

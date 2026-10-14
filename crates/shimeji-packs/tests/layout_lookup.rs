//! Directory layouts and config lookup order.

#[path = "common/mod.rs"]
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use shimeji_core::diagnostics::{DiagnosticKind, Severity};
use shimeji_core::model::LayoutKind;
use shimeji_packs::load_dir;

fn temp_pack(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shimeji-u5a-{label}-{}", process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn behaviors_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <BehaviorList>
    <Behavior Name="ChaseMouse" Action="Sit" Frequency="0"/>
    <Behavior Name="Fall" Action="Sit" Frequency="0"/>
    <Behavior Name="Dragged" Action="Sit" Frequency="0"/>
    <Behavior Name="Thrown" Action="Sit" Frequency="0"/>
  </BehaviorList>
</Mascot>
"#
}

fn actions_xml(image: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
    <Action Name="Sit" Type="Stay">
      <Animation>
        <Pose Image="{image}" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
  </ActionList>
</Mascot>
"#
    )
}

fn on_disk_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// True when this directory can hold two files whose names differ only by case.
fn case_distinct(dir: &Path) -> bool {
    let lower = dir.join("shimeji-case-a");
    let upper = dir.join("shimeji-case-A");
    let _ = fs::remove_file(&lower);
    let _ = fs::remove_file(&upper);
    if fs::write(&lower, b"lower").is_err() {
        return false;
    }
    if fs::write(&upper, b"upper").is_err() {
        let _ = fs::remove_file(&lower);
        return false;
    }
    let distinct = fs::read(&lower).ok().as_deref() == Some(b"lower")
        && fs::read(&upper).ok().as_deref() == Some(b"upper");
    let listed = on_disk_names(dir);
    let _ = fs::remove_file(&lower);
    let _ = fs::remove_file(&upper);
    distinct
        && listed.iter().any(|name| name == "shimeji-case-a")
        && listed.iter().any(|name| name == "shimeji-case-A")
}

#[test]
fn root_xml_hides_conf_and_uses_img() {
    let outcome = load_dir(common::fixture("layout-root")).unwrap();
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
    assert_eq!(outcome.pack.layout, LayoutKind::RootXml);
    let character = outcome.pack.character("layout-root").unwrap();
    assert_eq!(character.image_root, "img");
    assert!(character.actions.contains("Sit"));
    assert!(!character.actions.contains("HiddenByRoot"));
    assert_eq!(
        character.actions.get("Sit").unwrap().animations[0].poses[0]
            .image
            .as_ref()
            .unwrap()
            .relative,
        "p.png"
    );
}

#[test]
fn dropped_image_set_reads_conf_and_root_pngs() {
    let outcome = load_dir(common::fixture("layout-set")).unwrap();
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
    assert_eq!(outcome.pack.layout, LayoutKind::DroppedImageSet);
    let character = outcome.pack.character("layout-set").unwrap();
    assert_eq!(character.image_root, "");
    assert!(character.behaviors.contains("ChaseMouse"));
    let sit = character.actions.get("Sit").unwrap();
    assert_eq!(
        sit.params.get("BornBehavior").unwrap().source_name,
        "BornBehaviour"
    );
    assert_eq!(
        sit.params.get("ResistanceX").unwrap().source_name,
        "RegistanceX"
    );
    let pose = &sit.animations[0].poses[0];
    assert_eq!(pose.image.as_ref().unwrap().relative, "p.png");
    assert_eq!(pose.anchor_x, 3);
    assert_eq!(pose.anchor_y, 4);
}

#[test]
fn conf_set_directory_beats_shared_conf() {
    let dir = std::env::temp_dir().join(format!("shimeji-u5a-setconf-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    common::write_png(&dir.join("img/A/p.png"));
    fs::create_dir_all(dir.join("conf/A")).unwrap();
    fs::write(
        dir.join("conf/A/actions.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
    <Action Name="OnlySet" Type="Stay">
      <Animation>
        <Pose Image="/p.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
  </ActionList>
</Mascot>
"#,
    )
    .unwrap();
    fs::write(
        dir.join("conf/actions.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
    <Action Name="Shared" Type="Stay">
      <Animation>
        <Pose Image="/missing.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
  </ActionList>
</Mascot>
"#,
    )
    .unwrap();
    fs::write(
        dir.join("conf/behaviors.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <BehaviorList>
    <Behavior Name="ChaseMouse" Action="OnlySet" Frequency="0"/>
    <Behavior Name="Fall" Action="OnlySet" Frequency="0"/>
    <Behavior Name="Dragged" Action="OnlySet" Frequency="0"/>
    <Behavior Name="Thrown" Action="OnlySet" Frequency="0"/>
  </BehaviorList>
</Mascot>
"#,
    )
    .unwrap();
    let outcome = load_dir(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
    let character = outcome.pack.character("A").unwrap();
    assert!(character.actions.contains("OnlySet"));
    assert!(!character.actions.contains("Shared"));
}

#[test]
fn abc_png_resolves_to_the_on_disk_name() {
    let dir = temp_pack("caseimg");
    common::write_png(&dir.join("img/ABC.png"));
    fs::write(dir.join("actions.xml"), actions_xml("/abc.png")).unwrap();
    fs::write(dir.join("behaviors.xml"), behaviors_xml()).unwrap();
    let on_disk = on_disk_names(&dir.join("img"));
    let outcome = load_dir(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);

    assert_eq!(on_disk, vec!["ABC.png".to_string()]);
    let image = outcome.pack.characters[0]
        .actions
        .get("Sit")
        .unwrap()
        .animations[0]
        .poses[0]
        .image
        .as_ref()
        .unwrap();
    assert_eq!(image.source, "/abc.png");
    assert_eq!(image.relative, on_disk[0]);
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diag| diag.severity == Severity::Info),
        "{:#?}",
        outcome.diagnostics
    );
    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|diag| { diag.message.contains("/abc.png") && diag.message.contains("ABC.png") })
    );
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diag| diag.kind != DiagnosticKind::MissingImage)
    );
}

#[test]
fn config_and_directory_names_match_case_insensitively() {
    let dir = temp_pack("casecfg");
    fs::create_dir_all(dir.join("IMG/Miku")).unwrap();
    common::write_png(&dir.join("IMG/Miku/p.png"));
    fs::create_dir_all(dir.join("Conf/miku")).unwrap();
    fs::write(dir.join("Conf/miku/Actions.xml"), actions_xml("/p.png")).unwrap();
    fs::write(dir.join("Conf/miku/Behaviors.xml"), behaviors_xml()).unwrap();
    let outcome = load_dir(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);

    assert_eq!(outcome.pack.layout, LayoutKind::ImageSets);
    let character = outcome.pack.character("Miku").unwrap();
    assert_eq!(character.actions_file, "Conf/miku/Actions.xml");
    assert_eq!(character.behaviors_file, "Conf/miku/Behaviors.xml");
    assert_eq!(character.image_root, "IMG/Miku");
    assert_eq!(
        character.actions.get("Sit").unwrap().animations[0].poses[0]
            .image
            .as_ref()
            .unwrap()
            .relative,
        "p.png"
    );
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diag| diag.severity == Severity::Info),
        "{:#?}",
        outcome.diagnostics
    );
    let messages: Vec<&str> = outcome
        .diagnostics
        .iter()
        .map(|diag| diag.message.as_str())
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("img") && message.contains("IMG"))
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("conf") && message.contains("Conf"))
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("Miku") && message.contains("miku"))
    );
    assert!(
        messages
            .iter()
            .any(|message| { message.contains("actions.xml") && message.contains("Actions.xml") })
    );
    assert!(
        messages.iter().any(|message| {
            message.contains("behaviors.xml") && message.contains("Behaviors.xml")
        })
    );
}

#[test]
fn exact_file_name_wins_over_a_case_insensitive_one() {
    let dir = temp_pack("caseexact");
    let sensitive = case_distinct(&dir);
    let img = dir.join("img");
    fs::create_dir_all(&img).unwrap();
    if sensitive {
        common::write_png(&img.join("abc.png"));
        common::write_png(&img.join("ABC.png"));
        fs::write(dir.join("actions.xml"), actions_xml("/abc.png")).unwrap();
        fs::write(dir.join("behaviors.xml"), behaviors_xml()).unwrap();
        let names = on_disk_names(&img);
        let outcome = load_dir(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert!(names.iter().any(|name| name == "abc.png"));
        assert!(names.iter().any(|name| name == "ABC.png"));
        let relative = &outcome.pack.characters[0]
            .actions
            .get("Sit")
            .unwrap()
            .animations[0]
            .poses[0]
            .image
            .as_ref()
            .unwrap()
            .relative;
        assert_eq!(relative, "abc.png");
        assert!(outcome.diagnostics.iter().all(|diag| {
            diag.kind != DiagnosticKind::MissingImage && diag.severity != Severity::Warning
        }));
        assert!(
            !outcome
                .diagnostics
                .iter()
                .any(|diag| diag.message.contains("matched"))
        );
        return;
    }

    common::write_png(&img.join("ABC.png"));
    fs::write(
        dir.join("actions.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
    <Action Name="Sit" Type="Stay">
      <Animation>
        <Pose Image="/ABC.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
        <Pose Image="/abc.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
  </ActionList>
</Mascot>
"#,
    )
    .unwrap();
    fs::write(dir.join("behaviors.xml"), behaviors_xml()).unwrap();
    let on_disk = on_disk_names(&img);
    let outcome = load_dir(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(on_disk, vec!["ABC.png".to_string()]);
    let poses = &outcome.pack.characters[0]
        .actions
        .get("Sit")
        .unwrap()
        .animations[0]
        .poses;
    assert_eq!(poses[0].image.as_ref().unwrap().relative, on_disk[0]);
    assert_eq!(poses[1].image.as_ref().unwrap().relative, on_disk[0]);
    assert_eq!(poses[0].image.as_ref().unwrap().source, "/ABC.png");
    assert_eq!(poses[1].image.as_ref().unwrap().source, "/abc.png");
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diag| diag.severity == Severity::Info),
        "{:#?}",
        outcome.diagnostics
    );
    assert!(
        !outcome
            .diagnostics
            .iter()
            .any(|diag| { diag.message.contains("/ABC.png") && diag.message.contains("matched") })
    );
    assert!(
        outcome.diagnostics.iter().any(|diag| {
            diag.message.contains("/abc.png") && diag.message.contains(&on_disk[0])
        })
    );
}

#[test]
fn ambiguous_case_names_are_missing() {
    let dir = temp_pack("caseambig");
    if !case_distinct(&dir) {
        let _ = fs::remove_dir_all(&dir);
        return;
    }
    let img = dir.join("img");
    fs::create_dir_all(&img).unwrap();
    common::write_png(&img.join("a.png"));
    common::write_png(&img.join("A.png"));
    fs::write(dir.join("actions.xml"), actions_xml("/a.PNG")).unwrap();
    fs::write(dir.join("behaviors.xml"), behaviors_xml()).unwrap();
    let names = on_disk_names(&img);
    let outcome = load_dir(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert!(names.iter().any(|name| name == "a.png"));
    assert!(names.iter().any(|name| name == "A.png"));
    assert!(
        outcome.pack.characters[0]
            .actions
            .get("Sit")
            .unwrap()
            .animations[0]
            .poses[0]
            .image
            .is_none()
    );
    let missing: Vec<_> = outcome
        .diagnostics
        .iter()
        .filter(|diag| diag.kind == DiagnosticKind::MissingImage)
        .collect();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].severity, Severity::Warning);
    assert!(
        missing[0].message.contains("a.png"),
        "{}",
        missing[0].message
    );
    assert!(
        missing[0].message.contains("A.png"),
        "{}",
        missing[0].message
    );
}

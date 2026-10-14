//! Diagnostics for classes, paths, expressions, and entities.

#[path = "common/mod.rs"]
mod common;

use std::fs;
use std::process;
use std::time::Duration;

use shimeji_core::model::{ActionKind, ScriptKind};
use shimeji_packs::{DiagnosticKind, Severity, load_dir};

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("shimeji-u5a-{label}-{}", process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn unknown_class_becomes_stay_and_the_pack_loads() {
    let dir = temp_dir("class");
    common::write_minimal_en(
        &dir,
        r#"
    <Action Name="Sit" Type="Stay">
      <Animation>
        <Pose Image="/p.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
    <Action Name="Weird" Type="Embedded" Class="com.example.Foo"/>
    <Action Name="NotFall" Type="Embedded" Class="com.example.Fall"/>
"#,
    );
    let outcome = load_dir(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(outcome.pack.characters.len(), 1);
    let warnings: Vec<_> = outcome
        .diagnostics
        .iter()
        .filter(|diag| diag.kind == DiagnosticKind::UnknownClass)
        .collect();
    assert_eq!(warnings.len(), 2);
    assert!(
        warnings
            .iter()
            .all(|diag| { diag.severity == Severity::Warning && diag.is_stay_fallback() })
    );
    let character = &outcome.pack.characters[0];
    let weird = character.actions.get("Weird").unwrap();
    assert!(matches!(weird.kind, ActionKind::Stay));
    assert_eq!(weird.degraded_from.as_deref(), Some("com.example.Foo"));
    let not_fall = character.actions.get("NotFall").unwrap();
    assert!(matches!(not_fall.kind, ActionKind::Stay));
    assert_eq!(not_fall.degraded_from.as_deref(), Some("com.example.Fall"));
    assert!(character.actions.contains("Sit"));
}

#[test]
fn image_path_that_escapes_the_image_set_is_rejected() {
    let dir = temp_dir("escape");
    common::write_png(&dir.join("img/p.png"));
    fs::write(
        dir.join("動作.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<マスコット>
  <動作リスト>
    <動作 名前="立つ" 種類="静止">
      <アニメーション>
        <ポーズ 画像="/p.png" 基準座標="0,0" 移動速度="0,0" 長さ="1"/>
        <ポーズ 画像="/../secret.png" 基準座標="0,0" 移動速度="0,0" 長さ="1"/>
      </アニメーション>
    </動作>
  </動作リスト>
</マスコット>
"#,
    )
    .unwrap();
    fs::write(
        dir.join("行動.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<マスコット>
  <行動リスト>
    <行動 名前="マウスの周りに集まる" 動作="立つ" 頻度="0"/>
    <行動 名前="落下する" 動作="立つ" 頻度="0"/>
    <行動 名前="ドラッグされる" 動作="立つ" 頻度="0"/>
    <行動 名前="投げられる" 動作="立つ" 頻度="0"/>
  </行動リスト>
</マスコット>
"#,
    )
    .unwrap();
    let outcome = load_dir(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(outcome.pack.characters.len(), 1);
    let escapes: Vec<_> = outcome
        .diagnostics
        .iter()
        .filter(|diag| diag.kind == DiagnosticKind::ImageEscape)
        .collect();
    assert_eq!(escapes.len(), 1);
    assert_eq!(escapes[0].severity, Severity::Error);
    let poses = &outcome.pack.characters[0]
        .actions
        .get("立つ")
        .unwrap()
        .animations[0]
        .poses;
    assert_eq!(poses[0].image.as_ref().unwrap().relative, "p.png");
    assert!(poses[1].image.is_none());
}

#[test]
fn expression_syntax_error_names_the_file_and_line() {
    let dir = temp_dir("expr");
    common::write_png(&dir.join("img/p.png"));
    // The bad action is line 9.
    let actions = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<Mascot>\n\
  <ActionList>\n\
    <Action Name=\"Sit\" Type=\"Stay\">\n\
      <Animation>\n\
        <Pose Image=\"/p.png\" ImageAnchor=\"0,0\" Velocity=\"0,0\" Duration=\"1\"/>\n\
      </Animation>\n\
    </Action>\n\
    <Action Name=\"Bad\" Type=\"Stay\" Condition=\"${(}\"/>\n\
  </ActionList>\n\
</Mascot>\n";
    fs::write(dir.join("actions.xml"), actions).unwrap();
    fs::write(
        dir.join("behaviors.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <BehaviorList>
    <Behavior Name="ChaseMouse" Action="Sit" Frequency="0"/>
    <Behavior Name="Fall" Action="Sit" Frequency="0"/>
    <Behavior Name="Dragged" Action="Sit" Frequency="0"/>
    <Behavior Name="Thrown" Action="Sit" Frequency="0"/>
  </BehaviorList>
</Mascot>
"#,
    )
    .unwrap();
    let outcome = load_dir(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(outcome.pack.characters.len(), 1);
    let character = &outcome.pack.characters[0];
    assert!(character.actions.contains("Sit"));
    assert!(character.actions.contains("Bad"));
    let errors: Vec<_> = outcome
        .diagnostics
        .iter()
        .filter(|diag| diag.kind == DiagnosticKind::Expression)
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].severity, Severity::Error);
    assert_eq!(errors[0].file, "actions.xml");
    assert_eq!(errors[0].line, Some(9));
    let bad = character.actions.get("Bad").unwrap();
    let condition = bad.params.get("Condition").unwrap();
    assert_eq!(condition.value.raw, "${(}");
    assert_eq!(condition.value.wrapper, Some(ScriptKind::Once));
    assert!(condition.value.ast.is_none());
    assert!(
        character.actions.get("Sit").unwrap().animations[0].poses[0]
            .image
            .is_some()
    );
}

#[test]
fn external_entity_is_not_read() {
    let dir = temp_dir("xxe");
    common::write_png(&dir.join("img/p.png"));
    fs::write(dir.join("secret.txt"), "SENTINEL_SECRET_CONTENT").unwrap();
    let fifo = dir.join("block.fifo");
    assert!(
        process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    fs::write(
        dir.join("actions.xml"),
        r#"<?xml version="1.0"?>
<!DOCTYPE Mascot [
  <!ENTITY secret SYSTEM "secret.txt">
  <!ENTITY blocker SYSTEM "block.fifo">
]>
<Mascot>
  <ActionList>
    <Action Name="Sit" Type="Stay" Condition="&secret;">
      <Animation>
        <Pose Image="/p.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
  </ActionList>
  <Information>
    <Name>&blocker;</Name>
  </Information>
</Mascot>
"#,
    )
    .unwrap();
    fs::write(
        dir.join("behaviors.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <BehaviorList>
    <Behavior Name="ChaseMouse" Action="Sit" Frequency="0"/>
    <Behavior Name="Fall" Action="Sit" Frequency="0"/>
    <Behavior Name="Dragged" Action="Sit" Frequency="0"/>
    <Behavior Name="Thrown" Action="Sit" Frequency="0"/>
  </BehaviorList>
</Mascot>
"#,
    )
    .unwrap();

    let dir_for_load = dir.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let outcome = load_dir(&dir_for_load);
        let _ = sender.send(outcome);
    });
    let outcome = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("loader blocked while an external entity was declared")
        .unwrap();
    let rendered = format!("{outcome:?}");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        !rendered.contains("SENTINEL_SECRET_CONTENT"),
        "entity target was read"
    );
    assert!(outcome.diagnostics.iter().any(|diag| {
        diag.kind == DiagnosticKind::UnresolvedEntity || diag.kind == DiagnosticKind::Xml
    }));
    assert!(
        outcome.pack.characters.len() == 1
            || outcome
                .diagnostics
                .iter()
                .any(|diag| diag.severity == Severity::Error)
    );
}

#[test]
fn pose_image_uses_ihdr_and_unreadable_headers_stay_zero() {
    let dir = temp_dir("ihdr");
    common::write_minimal_en(
        &dir,
        r#"
    <Action Name="Sit" Type="Stay">
      <Animation>
        <Pose Image="/p.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
        <Pose Image="/bad.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
        <Pose Image="/huge.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
        <Pose Image="/missing.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
"#,
    );
    fs::write(dir.join("img/bad.png"), b"not-a-png").unwrap();
    let mut huge = [0u8; 24];
    huge[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    huge[8..12].copy_from_slice(&13u32.to_be_bytes());
    huge[12..16].copy_from_slice(b"IHDR");
    huge[16..20].copy_from_slice(&4097u32.to_be_bytes());
    huge[20..24].copy_from_slice(&1u32.to_be_bytes());
    fs::write(dir.join("img/huge.png"), huge).unwrap();

    let outcome = load_dir(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    let poses = &outcome.pack.characters[0]
        .actions
        .get("Sit")
        .unwrap()
        .animations[0]
        .poses;
    let good = poses[0].image.as_ref().unwrap();
    assert_eq!((good.width, good.height), (1, 1));
    let bad = poses[1].image.as_ref().unwrap();
    assert_eq!((bad.width, bad.height), (0, 0));
    let over = poses[2].image.as_ref().unwrap();
    assert_eq!((over.width, over.height), (0, 0));
    assert!(poses[3].image.is_none());
    assert!(outcome.diagnostics.iter().any(|diag| {
        diag.kind == DiagnosticKind::MissingImage && diag.message.contains("missing.png")
    }));
    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|diag| { diag.kind == DiagnosticKind::Other && diag.message.contains("bad.png") })
    );
    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|diag| { diag.kind == DiagnosticKind::Other && diag.message.contains("4097") })
    );
}

//! Declared encodings and NFC image names.

#[path = "common/mod.rs"]
mod common;

use std::fs;
use std::process;

use shimeji_packs::{CollisionPolicy, import_pack, load_dir, stored_path};

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("shimeji-u5a-{label}-{}", process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn behaviors() -> &'static str {
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

#[test]
fn nfd_file_name_matches_nfc_reference() {
    let dir = temp_dir("nfd");
    let nfc_name = "caf\u{00e9}.png";
    let nfd_name = "cafe\u{0301}.png";
    assert_ne!(nfc_name, nfd_name);
    common::write_png(&dir.join("img").join(nfd_name));
    let actions = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
    <Action Name="Sit" Type="Stay">
      <Animation>
        <Pose Image="/{nfc_name}" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
  </ActionList>
</Mascot>
"#
    );
    fs::write(dir.join("actions.xml"), actions).unwrap();
    fs::write(dir.join("behaviors.xml"), behaviors()).unwrap();
    let outcome = load_dir(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
    let pose = &outcome.pack.characters[0]
        .actions
        .get("Sit")
        .unwrap()
        .animations[0]
        .poses[0];
    assert_eq!(pose.image.as_ref().unwrap().relative, nfc_name);
}

#[test]
fn stored_path_finds_nfd_names_from_nfc_text() {
    let dir = temp_dir("stored");
    common::write_png(&dir.join("img/Cafe\u{0301}/cafe\u{0301}.png"));
    let found = stored_path(&dir, "img/Caf\u{00e9}/caf\u{00e9}.png");
    let missing = stored_path(&dir, "img/Caf\u{00e9}/tea.png");
    let found_is_file = found.as_ref().is_some_and(|path| path.is_file());
    let _ = fs::remove_dir_all(&dir);
    assert!(found_is_file, "{found:?}");
    assert_eq!(missing, None);
}

#[test]
fn folder_import_keys_decoded_images_like_resolved_poses() {
    let dir = temp_dir("import-nfd");
    let source = dir.join("cafe-pack");
    let nfc_name = "caf\u{00e9}.png";
    common::write_png(&source.join("img").join("cafe\u{0301}.png"));
    let actions = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
    <Action Name="Sit" Type="Stay">
      <Animation>
        <Pose Image="/{nfc_name}" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
  </ActionList>
</Mascot>
"#
    );
    fs::write(source.join("actions.xml"), actions).unwrap();
    fs::write(source.join("behaviors.xml"), behaviors()).unwrap();
    let outcome = import_pack(&source, dir.join("library"), CollisionPolicy::Report);
    let _ = fs::remove_dir_all(&dir);
    let outcome = outcome.unwrap();
    let character = &outcome.pack.characters[0];
    let pose = &character.actions.get("Sit").unwrap().animations[0].poses[0];
    // The shell finds a pose's pixels by this key, so it must be the decoded path.
    let key = format!(
        "{}/{}",
        character.image_root,
        pose.image.as_ref().unwrap().relative
    );
    let decoded: Vec<&str> = outcome
        .images
        .iter()
        .map(|image| image.path.as_str())
        .collect();
    assert_eq!(decoded, [key.as_str()]);
}

#[test]
fn shift_jis_declaration_is_honoured() {
    let dir = temp_dir("sjis");
    common::write_png(&dir.join("img/p.png"));
    let text = r#"<?xml version="1.0" encoding="Shift_JIS"?>
<マスコット>
  <動作リスト>
    <動作 名前="立つ" 種類="静止">
      <アニメーション>
        <ポーズ 画像="/p.png" 基準座標="0,0" 移動速度="0,0" 長さ="4"/>
      </アニメーション>
    </動作>
  </動作リスト>
  <行動リスト>
    <行動 名前="マウスの周りに集まる" 動作="立つ" 頻度="0"/>
    <行動 名前="落下する" 動作="立つ" 頻度="0"/>
    <行動 名前="ドラッグされる" 動作="立つ" 頻度="0"/>
    <行動 名前="投げられる" 動作="立つ" 頻度="0"/>
  </行動リスト>
</マスコット>
"#;
    let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode(text);
    assert!(!bytes.windows(3).any(|window| window == [0xE3, 0x83, 0x9E]));
    fs::write(dir.join("動作.xml"), &bytes).unwrap();
    fs::write(dir.join("行動.xml"), &bytes).unwrap();
    let outcome = load_dir(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
    let character = &outcome.pack.characters[0];
    assert!(character.actions.contains("立つ"));
    assert_eq!(
        character.actions.get("立つ").unwrap().animations[0].poses[0].duration_ticks,
        4
    );
}

#[test]
fn utf8_bom_is_accepted() {
    let dir = temp_dir("bom");
    common::write_png(&dir.join("img/p.png"));
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
    <Action Name="Sit" Type="Stay">
      <Animation>
        <Pose Image="/p.png" ImageAnchor="0,0" Velocity="0,0" Duration="3"/>
      </Animation>
    </Action>
  </ActionList>
</Mascot>
"#;
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(xml.as_bytes());
    fs::write(dir.join("actions.xml"), bytes).unwrap();
    fs::write(dir.join("behaviors.xml"), behaviors()).unwrap();
    let outcome = load_dir(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
    assert_eq!(
        outcome.pack.characters[0]
            .actions
            .get("Sit")
            .unwrap()
            .animations[0]
            .poses[0]
            .duration_ticks,
        3
    );
}

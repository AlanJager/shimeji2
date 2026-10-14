//! Pose names from the bundled shime pack and from packs that lack them.

use std::fs;
use std::path::{Path, PathBuf};

use shimeji_app::agent::{PoseChoice, resolve};
use shimeji_core::model::Schema;
use shimeji_packs::load_dir;
use shimeji_proto::AgentState;

fn scratch(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "shimeji-u14-pose-{label}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn names_of(dir: &Path) -> (Schema, Vec<String>) {
    let outcome = load_dir(dir).unwrap_or_else(|error| panic!("load {}: {error}", dir.display()));
    let character = outcome
        .pack
        .characters
        .first()
        .unwrap_or_else(|| panic!("no character in {}", dir.display()));
    let mut names = Vec::new();
    names.extend(character.actions.keys().map(str::to_string));
    names.extend(character.behaviors.keys().map(str::to_string));
    (character.schema, names)
}

#[test]
fn shime_working_is_its_sit_behavior_and_done_is_its_jump() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime");
    let outcome = load_dir(&dir).unwrap();
    let character = outcome.pack.characters.first().expect("shime");
    assert_eq!(character.schema, Schema::Japanese);
    assert!(character.behaviors.contains("座ってボーっとする"));
    assert!(character.actions.contains("ジャンプ"));
    assert!(!character.behaviors.contains("ジャンプ"));

    let (_schema, names) = names_of(&dir);
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(
        resolve(
            Schema::Japanese,
            AgentState::Working,
            name_refs.iter().copied()
        ),
        PoseChoice::Named("座ってボーっとする".to_string())
    );
    assert_eq!(
        resolve(
            Schema::Japanese,
            AgentState::Done,
            name_refs.iter().copied()
        ),
        PoseChoice::Named("ジャンプ".to_string())
    );
}

#[test]
fn an_ee_pack_with_no_matching_names_is_badge_only() {
    let dir = scratch("ee");
    fs::create_dir_all(dir.join("img")).unwrap();
    fs::write(dir.join("img/p.png"), PNG).unwrap();
    fs::write(
        dir.join("actions.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
    <Action Name="Wander" Type="Stay">
      <Animation>
        <Pose Image="/p.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
    <Action Name="Nap" Type="Stay">
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
        dir.join("behaviors.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <BehaviorList>
    <Behavior Name="ChaseMouse" Action="Wander" Frequency="0"/>
    <Behavior Name="Fall" Action="Wander" Frequency="0"/>
    <Behavior Name="Dragged" Action="Wander" Frequency="0"/>
    <Behavior Name="Thrown" Action="Wander" Frequency="0"/>
    <Behavior Name="LookAround" Action="Nap" Frequency="1"/>
  </BehaviorList>
</Mascot>
"#,
    )
    .unwrap();

    let (schema, names) = names_of(&dir);
    assert_eq!(schema, Schema::English);
    for state in [
        AgentState::Working,
        AgentState::Waiting,
        AgentState::Failed,
        AgentState::Done,
        AgentState::Idle,
    ] {
        assert_eq!(
            resolve(schema, state, names.iter().map(String::as_str)),
            PoseChoice::BadgeOnly,
            "{state:?} names {names:?}"
        );
    }
}

#[test]
fn codex_profile_rows_win_over_the_ee_template() {
    let profile =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../shimeji-packs/assets/codex-profile.xml");
    let text = fs::read_to_string(&profile).unwrap();
    let mut names = xml_names(&text);
    assert!(names.iter().any(|name| name == "Working"));
    assert!(names.iter().any(|name| name == "Jump"));
    names.push("SitDown".to_string());
    names.push("Sit".to_string());
    names.push("Jumping".to_string());
    assert_eq!(
        resolve(
            Schema::English,
            AgentState::Working,
            names.iter().map(String::as_str)
        ),
        PoseChoice::Named("Working".to_string())
    );
    assert_eq!(
        resolve(
            Schema::English,
            AgentState::Done,
            names.iter().map(String::as_str)
        ),
        PoseChoice::Named("Jump".to_string())
    );
}

fn xml_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("Name=\"") {
        let after = &rest[start + 6..];
        let Some(end) = after.find('"') else {
            break;
        };
        names.push(after[..end].to_string());
        rest = &after[end + 1..];
    }
    names
}

const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 96, 0, 2, 0, 0, 5, 0, 1,
    122, 94, 171, 63, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

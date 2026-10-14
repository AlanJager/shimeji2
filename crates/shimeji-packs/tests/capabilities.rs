//! Pose capabilities prune behaviors the pack cannot run.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use shimeji_packs::model::{Action, ActionNode, BehaviorNode, Character};
use shimeji_packs::{PoseCapability, Severity, load_dir, pose_capabilities, write_standard_pack};

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

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
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
    let frame = reader.next_frame(&mut buf).unwrap();
    assert_eq!(frame.color_type, png::ColorType::Rgba);
    buf.truncate(frame.buffer_size());
    (frame.width, frame.height, buf)
}

fn zero_cell(rgba: &mut [u8], width: u32, index: u16) {
    let (origin_x, origin_y) = shimeji_packs::standard_atlas::cell_origin(index).unwrap();
    for row in 0..shimeji_packs::standard_atlas::CELL {
        let start = (((origin_y + row) * width + origin_x) * 4) as usize;
        let n = (shimeji_packs::standard_atlas::CELL * 4) as usize;
        rgba[start..start + n].fill(0);
    }
}

fn paint_cell(rgba: &mut [u8], width: u32, index: u16) {
    let (origin_x, origin_y) = shimeji_packs::standard_atlas::cell_origin(index).unwrap();
    let start = ((origin_y * width + origin_x) * 4) as usize;
    rgba[start] = 255;
    rgba[start + 3] = 255;
}

fn mentioned(character: &Character) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    fn walk(nodes: &[BehaviorNode], names: &mut BTreeSet<String>) {
        for node in nodes {
            match node {
                BehaviorNode::Group { children, .. } => walk(children, names),
                BehaviorNode::Behavior { name, .. } | BehaviorNode::Reference { name, .. } => {
                    names.insert(name.clone());
                }
            }
        }
    }
    walk(&character.behavior_tree, &mut names);
    for behavior in character.behaviors.values() {
        for list in &behavior.next_lists {
            for reference in &list.refs {
                names.insert(reference.name.clone());
            }
            walk(&list.nodes, &mut names);
        }
    }
    names
}

fn action_refs(action: &Action, out: &mut BTreeSet<String>) {
    for child in &action.children {
        match child {
            ActionNode::Reference(reference) => {
                out.insert(reference.name.clone());
            }
            ActionNode::Action(child) => action_refs(child, out),
        }
    }
}

#[test]
fn a_full_atlas_keeps_wall_and_ceiling_behaviors() {
    let root = scratch("full");
    let native = root.join("shime");
    write_standard_pack(&repo().join("assets/packs/shime"), &native).unwrap();
    let loaded = load_dir(&native).unwrap();
    let character = &loaded.pack.characters[0];
    assert!(character.behaviors.contains("壁に掴まってボーっとする"));
    assert!(character.behaviors.contains("天井に掴まってボーっとする"));
    assert!(
        loaded
            .diagnostics
            .iter()
            .all(|diag| !diag.message.starts_with("pruned behaviors"))
    );
    assert_eq!(
        pose_capabilities(&loaded.pack),
        vec![
            PoseCapability::Core,
            PoseCapability::Drag,
            PoseCapability::Sit,
            PoseCapability::Tumble,
            PoseCapability::Wall,
            PoseCapability::Ceiling,
            PoseCapability::Window,
            PoseCapability::Multiply,
        ]
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_pack_without_wall_or_ceiling_poses_prunes_those_behaviors() {
    let root = scratch("prune");
    let native = root.join("shime");
    write_standard_pack(&repo().join("assets/packs/shime"), &native).unwrap();
    let bytes = fs::read(native.join("atlas.png")).unwrap();
    let (width, height, mut rgba) = decode(&bytes);
    assert_eq!((width, height), (1024, 768));
    for index in [12, 13, 14, 23, 24, 25] {
        zero_cell(&mut rgba, width, index);
    }
    fs::write(native.join("atlas.png"), encode(width, height, &rgba)).unwrap();

    let loaded = load_dir(&native).unwrap();
    let character = &loaded.pack.characters[0];
    let gone = [
        "壁に掴まってボーっとする",
        "天井に掴まってボーっとする",
        "ワークエリアの壁を登る",
        "ワークエリアの上辺を伝う",
        "IEの壁を登る",
        "左の壁に飛びつく",
        "右の壁に飛びつく",
    ];
    for name in gone {
        assert!(!character.behaviors.contains(name), "{name} was kept");
    }
    for name in [
        "落下する",
        "投げられる",
        "立ってボーっとする",
        "壁から落ちる",
        "天井から落ちる",
    ] {
        assert!(character.behaviors.contains(name), "{name} was pruned");
    }
    let have: BTreeSet<_> = character.behaviors.keys().map(str::to_string).collect();
    for name in mentioned(character) {
        assert!(have.contains(&name), "something still points at {name}");
    }
    let mut fall_refs = BTreeSet::new();
    action_refs(character.actions.get("落下する").unwrap(), &mut fall_refs);
    assert!(!fall_refs.contains("壁に掴まる"));
    assert!(fall_refs.contains("落ちる"));
    let fallback = character
        .behaviors
        .values()
        .find(|behavior| behavior.name.starts_with("立ち止まる"))
        .expect("empty group falls back to idle");
    assert_eq!(fallback.action, "立ってボーっとする");
    let note = loaded
        .diagnostics
        .iter()
        .find(|diag| diag.message.starts_with("pruned behaviors"))
        .expect("prune note");
    assert_eq!(note.severity, Severity::Info);
    let caps = pose_capabilities(&loaded.pack);
    assert!(!caps.contains(&PoseCapability::Wall));
    assert!(!caps.contains(&PoseCapability::Ceiling));
    assert!(caps.contains(&PoseCapability::Core));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_empty_next_list_falls_back_to_idle() {
    let root = scratch("next");
    let native = root.join("thin");
    fs::create_dir_all(&native).unwrap();
    let (width, height) = shimeji_packs::standard_atlas::atlas_size(1);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for index in 1..=4 {
        paint_cell(&mut rgba, width, index);
    }
    fs::write(native.join("atlas.png"), encode(width, height, &rgba)).unwrap();
    fs::write(
        native.join("behaviors.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
    <Action Name="Idle" Type="Stay" BorderType="Floor">
      <Animation>
        <Pose Image="/shime1.png" ImageAnchor="64,128" Velocity="0,0" Duration="5"/>
      </Animation>
    </Action>
    <Action Name="Walk" Type="Move" BorderType="Floor">
      <Animation>
        <Pose Image="/shime2.png" ImageAnchor="64,128" Velocity="-2,0" Duration="5"/>
        <Pose Image="/shime3.png" ImageAnchor="64,128" Velocity="-2,0" Duration="5"/>
      </Animation>
    </Action>
    <Action Name="Climb" Type="Stay" BorderType="Wall">
      <Animation>
        <Pose Image="/shime13.png" ImageAnchor="64,128" Velocity="0,0" Duration="5"/>
      </Animation>
    </Action>
    <Action Name="ChaseMouse" Type="Stay">
      <Animation>
        <Pose Image="/shime1.png" ImageAnchor="64,128" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
    <Action Name="Fall" Type="Stay">
      <Animation>
        <Pose Image="/shime4.png" ImageAnchor="64,128" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
    <Action Name="Dragged" Type="Stay">
      <Animation>
        <Pose Image="/shime1.png" ImageAnchor="64,128" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
    <Action Name="Thrown" Type="Stay">
      <Animation>
        <Pose Image="/shime4.png" ImageAnchor="64,128" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
  </ActionList>
  <BehaviorList>
    <Behavior Name="ChaseMouse" Action="ChaseMouse" Frequency="0"/>
    <Behavior Name="Fall" Action="Fall" Frequency="0"/>
    <Behavior Name="Dragged" Action="Dragged" Frequency="0"/>
    <Behavior Name="Thrown" Action="Thrown" Frequency="0" Hidden="true"/>
    <Behavior Name="Idle" Action="Idle" Frequency="100">
      <NextBehaviorList Add="true">
        <BehaviorReference Name="Climb" Frequency="1"/>
      </NextBehaviorList>
    </Behavior>
    <Behavior Name="Climb" Action="Climb" Frequency="10"/>
  </BehaviorList>
</Mascot>
"#,
    )
    .unwrap();
    fs::write(
        native.join("pack.json"),
        r#"{
  "format": "shimeji2-pack",
  "formatVersion": 1,
  "id": "thin",
  "name": "Thin",
  "license": "Zlib",
  "art": {"facing": "left", "displayScale": 1.0},
  "atlases": [{"image": "atlas.png", "layout": "shimeji-standard-v1"}],
  "behaviors": "behaviors.xml"
}"#,
    )
    .unwrap();

    let loaded = load_dir(&native).unwrap();
    assert_eq!(loaded.pack.characters.len(), 1, "{:?}", loaded.diagnostics);
    let character = &loaded.pack.characters[0];
    assert!(!character.behaviors.contains("Climb"));
    let idle = character.behaviors.get("Idle").unwrap();
    let refs: Vec<_> = idle
        .next_refs()
        .map(|reference| reference.name.as_str())
        .collect();
    assert_eq!(refs, vec!["Idle"]);
    assert!(!mentioned(character).contains("Climb"));
    let _ = fs::remove_dir_all(&root);
}

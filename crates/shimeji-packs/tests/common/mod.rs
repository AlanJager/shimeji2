//! Shared helpers for the pack loader tests.
//!
//! Each integration test includes this file, and not every test calls every helper.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

pub const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 96, 0, 2, 0, 0, 5, 0, 1,
    122, 94, 171, 63, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

pub fn write_png(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, PNG).unwrap();
}

/// English pack with one Stay action and the four required behaviors.
pub fn write_minimal_en(dir: &Path, actions_body: &str) {
    fs::create_dir_all(dir).unwrap();
    write_png(&dir.join("img/p.png"));
    let actions = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
{actions_body}
  </ActionList>
  <BehaviorList>
    <Behavior Name="ChaseMouse" Action="Sit" Frequency="0"/>
    <Behavior Name="Fall" Action="Sit" Frequency="0"/>
    <Behavior Name="Dragged" Action="Sit" Frequency="0"/>
    <Behavior Name="Thrown" Action="Sit" Frequency="0"/>
  </BehaviorList>
</Mascot>
"#
    );
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
}

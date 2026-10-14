//! Shared helpers for the Unit 10 integration tests.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 96, 0, 2, 0, 0, 5, 0, 1,
    122, 94, 171, 63, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

pub fn scratch(label: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("shimeji-u10-{label}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../shimeji-packs/tests/fixtures")
        .join(name)
}

pub fn packs_entries(root: &Path) -> Vec<String> {
    let dir = root.join("packs");
    if !dir.exists() {
        return Vec::new();
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).unwrap().flatten() {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    names
}

/// English pack with an unknown embedded class and one Toggleable behavior.
pub fn write_unknown_class(dir: &Path) {
    fs::create_dir_all(dir.join("img")).unwrap();
    fs::write(dir.join("img/p.png"), PNG).unwrap();
    fs::write(
        dir.join("actions.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
    <Action Name="Sit" Type="Stay">
      <Animation>
        <Pose Image="/p.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
    <Action Name="Weird" Type="Embedded" Class="com.example.Foo"/>
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
    <Behavior Name="ChaseMouse" Action="Sit" Frequency="0"/>
    <Behavior Name="Fall" Action="Sit" Frequency="0"/>
    <Behavior Name="Dragged" Action="Sit" Frequency="0"/>
    <Behavior Name="Thrown" Action="Sit" Frequency="0"/>
    <Behavior Name="Wave" Action="Sit" Frequency="1" Toggleable="true"/>
  </BehaviorList>
</Mascot>
"#,
    )
    .unwrap();
}

/// A stored zip whose only entry escapes the archive.
pub fn zip_slip(name: &str, payload: &[u8]) -> Vec<u8> {
    stored_zip(name, payload)
}

fn stored_zip(name: &str, payload: &[u8]) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    let crc = crc32(payload);
    let mut out = Vec::new();
    out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name_bytes);
    out.extend_from_slice(payload);
    let local_len = out.len();
    out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(name_bytes);
    let central_len = out.len() - local_len;
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(central_len as u32).to_le_bytes());
    out.extend_from_slice(&(local_len as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

//! Installed packs under `packs_root/packs/<id>/`.
//!
//! A drop or a zip goes through [`crate::import`], which calls `import_pack`.
//! This module reads directories that import already installed. Those copies
//! are not the caller's untrusted path.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use shimeji_core::model::{ActionKind, Builtin, LayoutKind, Pack, Schema};
use shimeji_packs::{
    Diagnostic, DiagnosticKind, PackOrigin, PoseCapability, Severity, directory_origin, load_dir,
    pose_capabilities,
};

/// How the actions documents were recognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PackFormat {
    /// Every character's actions document is Japanese.
    Classic,
    /// Every character's actions document is English.
    ShimejiEe,
    /// The pack mixes the two schemas.
    Mixed,
    /// Nothing readable was found.
    Unknown,
    /// A Codex pet whose sheet is 1536×1872.
    CodexV1,
    /// A Codex pet whose sheet is 1536×2288.
    CodexV2,
    /// A shimeji2 `pack.json`.
    Native,
}

impl PackFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::ShimejiEe => "Shimeji-ee",
            Self::Mixed => "Mixed",
            Self::Unknown => "Unknown",
            Self::CodexV1 => "Codex v1",
            Self::CodexV2 => "Codex v2",
            Self::Native => "Native",
        }
    }
}

/// Coarse labels. Codex and native packs use pose names. Classic and
/// Shimeji-ee packs still use action classes, behavior names, and borders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    Core,
    Drag,
    Sit,
    Tumble,
    Wall,
    Ceiling,
    Window,
    Multiply,
    Look,
}

impl Capability {
    pub fn label(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Drag => "drag",
            Self::Sit => "sit",
            Self::Tumble => "tumble",
            Self::Wall => "wall",
            Self::Ceiling => "ceiling",
            Self::Window => "window",
            Self::Multiply => "multiply",
            Self::Look => "look",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BehaviorRow {
    pub name: String,
    pub toggleable: bool,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterRow {
    pub name: String,
    pub behaviors: Vec<BehaviorRow>,
}

/// One directory under `packs/`.
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledPack {
    pub id: String,
    pub path: PathBuf,
    pub format: PackFormat,
    pub layout: LayoutKind,
    pub capabilities: Vec<Capability>,
    pub characters: Vec<CharacterRow>,
    pub diagnostics: Vec<Diagnostic>,
}

impl InstalledPack {
    pub fn degraded(&self) -> bool {
        self.diagnostics.iter().any(is_degraded)
    }

    pub fn format_label(&self) -> &'static str {
        self.format.label()
    }

    pub fn capability_labels(&self) -> Vec<&'static str> {
        self.capabilities
            .iter()
            .copied()
            .map(Capability::label)
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
    packs: Vec<InstalledPack>,
}

impl Library {
    /// Scan `packs_root/packs`. Each directory was written by `import_pack`.
    pub fn open(packs_root: impl AsRef<Path>) -> Self {
        let root = packs_root.as_ref().to_path_buf();
        let mut library = Self {
            root,
            packs: Vec::new(),
        };
        library.reload();
        library
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn packs(&self) -> &[InstalledPack] {
        &self.packs
    }

    pub fn get(&self, id: &str) -> Option<&InstalledPack> {
        self.packs.iter().find(|pack| pack.id == id)
    }

    /// Re-read installed directories. Does not touch a path the user dropped.
    pub fn reload(&mut self) {
        let packs_dir = self.root.join("packs");
        let mut ids = Vec::new();
        if let Ok(read) = fs::read_dir(&packs_dir) {
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                        ids.push((name.to_string(), path));
                    }
                }
            }
        }
        ids.sort_by(|left, right| left.0.cmp(&right.0));
        self.packs = ids
            .into_iter()
            .map(|(id, path)| record_from_disk(&id, path))
            .collect();
    }

    /// Remember a pack `import_pack` just installed, without reading the user's source.
    pub fn insert(&mut self, id: &str, path: &Path, pack: &Pack, diagnostics: &[Diagnostic]) {
        let row = record(id, path.to_path_buf(), pack, diagnostics.to_vec());
        if let Some(existing) = self.packs.iter_mut().find(|item| item.id == id) {
            *existing = row;
        } else {
            self.packs.push(row);
            self.packs.sort_by(|left, right| left.id.cmp(&right.id));
        }
    }
}

pub fn is_degraded(diagnostic: &Diagnostic) -> bool {
    matches!(diagnostic.severity, Severity::Warning | Severity::Error)
}

fn record_from_disk(id: &str, path: PathBuf) -> InstalledPack {
    match load_dir(&path) {
        Ok(loaded) => record(id, path, &loaded.pack, loaded.diagnostics),
        Err(error) => InstalledPack {
            id: id.to_string(),
            path,
            format: PackFormat::Unknown,
            layout: LayoutKind::RootXml,
            capabilities: Vec::new(),
            characters: Vec::new(),
            diagnostics: vec![Diagnostic::new(
                Severity::Error,
                DiagnosticKind::Other,
                id,
                "",
                None,
                format!("could not read installed pack: {error}"),
            )],
        },
    }
}

fn record(id: &str, path: PathBuf, pack: &Pack, diagnostics: Vec<Diagnostic>) -> InstalledPack {
    let format = format_of(&path, pack);
    let capabilities = capabilities_of(&path, pack);
    InstalledPack {
        id: id.to_string(),
        path,
        format,
        layout: pack.layout,
        capabilities,
        characters: pack
            .characters
            .iter()
            .map(|character| CharacterRow {
                name: character.name.clone(),
                behaviors: character
                    .behaviors
                    .iter()
                    .map(|(name, behavior)| BehaviorRow {
                        name: name.to_string(),
                        toggleable: behavior.toggleable,
                        hidden: behavior.hidden,
                    })
                    .collect(),
            })
            .collect(),
        diagnostics,
    }
}

fn format_of(path: &Path, pack: &Pack) -> PackFormat {
    match directory_origin(path) {
        Some(PackOrigin::CodexV1) => return PackFormat::CodexV1,
        Some(PackOrigin::CodexV2) => return PackFormat::CodexV2,
        Some(PackOrigin::Native) => return PackFormat::Native,
        None => {}
    }
    let mut japanese = false;
    let mut english = false;
    for character in &pack.characters {
        match character.schema {
            Schema::Japanese => japanese = true,
            Schema::English => english = true,
        }
    }
    match (japanese, english) {
        (true, false) => PackFormat::Classic,
        (false, true) => PackFormat::ShimejiEe,
        (true, true) => PackFormat::Mixed,
        (false, false) => PackFormat::Unknown,
    }
}

fn capabilities_of(path: &Path, pack: &Pack) -> Vec<Capability> {
    match directory_origin(path) {
        Some(PackOrigin::CodexV1 | PackOrigin::CodexV2 | PackOrigin::Native) => {
            return pose_capabilities(pack)
                .into_iter()
                .map(Capability::from_pose)
                .collect();
        }
        None => {}
    }
    let mut set = BTreeSet::new();
    if !pack.characters.is_empty() {
        set.insert(Capability::Core);
    }
    for character in &pack.characters {
        for (_, behavior) in character.behaviors.iter() {
            if is_drag_name(&behavior.name) {
                set.insert(Capability::Drag);
            }
            if is_sit_name(&behavior.name) {
                set.insert(Capability::Sit);
            }
        }
        for action in character.actions.values() {
            action.visit(&mut |action| {
                match action.kind {
                    ActionKind::Builtin(Builtin::Dragged) => {
                        set.insert(Capability::Drag);
                    }
                    ActionKind::Builtin(Builtin::Breed) => {
                        set.insert(Capability::Multiply);
                    }
                    ActionKind::Builtin(Builtin::Look) => {
                        set.insert(Capability::Look);
                    }
                    ActionKind::Builtin(Builtin::Jump) => {
                        set.insert(Capability::Tumble);
                    }
                    ActionKind::Builtin(
                        Builtin::WalkWithIe | Builtin::FallWithIe | Builtin::ThrowIe,
                    ) => {
                        set.insert(Capability::Window);
                    }
                    ActionKind::Stay => {
                        if action.name.as_deref().is_some_and(is_sit_name) {
                            set.insert(Capability::Sit);
                        }
                    }
                    _ => {}
                }
                if let Some(border) = action.params.get("BorderType") {
                    match border.value.raw.trim() {
                        "Wall" | "壁" => {
                            set.insert(Capability::Wall);
                        }
                        "Ceiling" | "天井" => {
                            set.insert(Capability::Ceiling);
                        }
                        _ => {}
                    }
                }
            });
        }
    }
    set.into_iter().collect()
}

impl Capability {
    fn from_pose(capability: PoseCapability) -> Self {
        match capability {
            PoseCapability::Core => Self::Core,
            PoseCapability::Drag => Self::Drag,
            PoseCapability::Sit => Self::Sit,
            PoseCapability::Tumble => Self::Tumble,
            PoseCapability::Wall => Self::Wall,
            PoseCapability::Ceiling => Self::Ceiling,
            PoseCapability::Window => Self::Window,
            PoseCapability::Multiply => Self::Multiply,
            PoseCapability::Look => Self::Look,
        }
    }
}

fn is_drag_name(name: &str) -> bool {
    name == "Dragged" || name == "ドラッグされる"
}

fn is_sit_name(name: &str) -> bool {
    name == "Sit" || name == "座る"
}

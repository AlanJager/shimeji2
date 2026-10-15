//! How a trusted pack directory is recognised, and where each character's
//! files and images live.
//!
//! Detection, in order:
//!
//! 1. **Image sets** ([`LayoutKind::ImageSets`]). `img/` contains at least one
//!    real subdirectory whose name is not `unused` (any case) and does not
//!    start with `.`. Symlinks are not directories here. Each subdirectory is
//!    a character. This is the Wispy-style application layout (`img/<Set>/`
//!    plus a shared `conf/`). `img/unused/` is not a character. Loose files in
//!    `img/` (for example `icon.png`) are not characters either.
//! 2. **Root XML** ([`LayoutKind::RootXml`]). An actions file sits in the pack
//!    root. Images live in `img/` when that is a real directory, and otherwise
//!    in the pack root. `conf/` is not consulted. This is the shimejis.xyz /
//!    Neuroling shape: `actions.xml` and `behaviors.xml` next to `img/`.
//! 3. **Dropped image set** ([`LayoutKind::DroppedImageSet`]). `conf/` has an
//!    actions file and there is no `img/` directory. PNGs sit in the pack
//!    root. This is the hololive shape: an image-set folder dropped in on its
//!    own.
//! 4. **Classic single** ([`LayoutKind::ClassicSingle`]). `conf/` has an
//!    actions file and `img/` is a directory of files, not character folders.
//!    One character, named after the pack directory. The image-set root is
//!    `img/` itself, not `img/<name>/`. groupfinity-shime and the bundled
//!    shime are this shape: the plan's "classic single-character" sentence.
//!    They are not image sets, because looking under `img/<pack name>/` would
//!    miss `img/shime1.png`.
//!
//! Config lookup follows Shimeji-ee for every layout except root XML. Actions
//! and behaviors are searched independently, first regular file wins:
//! `img/<Set>/conf/`, then `conf/<Set>/`, then `conf/`. Root XML searches the
//! pack root only, so a root `actions.xml` hides `conf/`. The accepted names,
//! in order, are `actions.xml`, `動作.xml`, `one.xml`, `1.xml` and
//! `behaviors.xml`, `behavior.xml`, `行動.xml`, `two.xml`, `2.xml`. `info.xml`
//! uses the same directories.
//!
//! An image attribute such as `/shime1.png` is relative to the image-set root.
//! A leading slash is that root, not the filesystem root (Java
//! `Path.of(imageSet, "/shime1.png")` drops the extra absolute component).
//! `..`, NUL, and a `C:` drive prefix are rejected. Names are matched in NFC
//! because macOS stores NFD. An exact NFC match wins. If there is none, one
//! case-insensitive match in the same directory is used, for images and for
//! `actions.xml`, `behaviors.xml`, `conf/`, `img/`, and the image-set folder.
//! Two case-insensitive candidates and no exact match are reported missing.
//! Characters are sorted by name so the result does not depend on directory
//! order.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use shimeji_core::diagnostics::{Diagnostic, DiagnosticKind, Severity};
use shimeji_core::model::{
    Action, ActionNode, Character, Information, LayoutKind, Pack, ResolvedImage,
};
use unicode_normalization::UnicodeNormalization;

use crate::LoadOutcome;
use crate::xml::{self, LoadedDoc};

const ACTION_FILES: &[&str] = &["actions.xml", "動作.xml", "one.xml", "1.xml"];
const BEHAVIOR_FILES: &[&str] = &[
    "behaviors.xml",
    "behavior.xml",
    "行動.xml",
    "two.xml",
    "2.xml",
];

pub(crate) fn load_dir(path: &Path) -> io::Result<LoadOutcome> {
    if crate::native::is_native_dir(path) {
        return crate::native::load_native(path);
    }
    if crate::codex::is_codex_dir(path) {
        return crate::codex::load_codex(path);
    }
    let meta = fs::metadata(path)?;
    if !meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pack path is not a directory",
        ));
    }
    let pack_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(nfc)
        .unwrap_or_else(|| "pack".to_string());
    let mut diagnostics = Vec::new();
    let img = take_child(path, "img", &pack_name, &mut diagnostics);
    let image_sets = img
        .as_ref()
        .is_some_and(|dir| !image_set_dirs(dir).is_empty());
    let root_xml = !image_sets && action_file_present(path);
    // Root XML does not consult `conf/`, so a case clash there is not a hit.
    let conf = if root_xml {
        None
    } else {
        take_child(path, "conf", &pack_name, &mut diagnostics)
    };
    let layout = if image_sets {
        Some(LayoutKind::ImageSets)
    } else if root_xml {
        Some(LayoutKind::RootXml)
    } else if conf.as_ref().is_some_and(|dir| action_file_present(dir)) {
        Some(if img.is_some() {
            LayoutKind::ClassicSingle
        } else {
            LayoutKind::DroppedImageSet
        })
    } else {
        None
    };
    let Some(layout) = layout else {
        diagnostics.push(Diagnostic::new(
            Severity::Error,
            DiagnosticKind::MissingConfig,
            &pack_name,
            "",
            None,
            "no actions file found",
        ));
        return Ok(LoadOutcome {
            pack: Pack {
                name: pack_name,
                layout: LayoutKind::ClassicSingle,
                characters: Vec::new(),
            },
            diagnostics,
            display_scale: None,
        });
    };
    let roots = Roots { img, conf };

    let pending = character_sources(path, layout, &pack_name, &roots);
    let mut characters = Vec::new();
    for source in pending {
        if let Some(character) =
            load_character(path, &pack_name, layout, &source, &roots, &mut diagnostics)
        {
            characters.push(character);
        }
    }
    characters.sort_by(|left, right| left.name.cmp(&right.name));
    if characters.is_empty() {
        diagnostics.push(Diagnostic::new(
            Severity::Error,
            DiagnosticKind::MissingConfig,
            &pack_name,
            "",
            None,
            "no characters loaded",
        ));
    }
    Ok(LoadOutcome {
        pack: Pack {
            name: pack_name,
            layout,
            characters,
        },
        diagnostics,
        display_scale: None,
    })
}

struct Source {
    name: String,
    /// Real `img/<Set>` directory. `None` when the layout has a single image root.
    set_dir: Option<PathBuf>,
    image_root: PathBuf,
}

struct Roots {
    img: Option<PathBuf>,
    conf: Option<PathBuf>,
}

fn character_sources(
    pack: &Path,
    layout: LayoutKind,
    pack_name: &str,
    roots: &Roots,
) -> Vec<Source> {
    match layout {
        LayoutKind::ImageSets => {
            let Some(img) = roots.img.as_deref() else {
                return Vec::new();
            };
            image_set_dirs(img)
                .into_iter()
                .map(|set| Source {
                    name: set.name,
                    image_root: set.path.clone(),
                    set_dir: Some(set.path),
                })
                .collect()
        }
        LayoutKind::ClassicSingle => vec![Source {
            name: pack_name.to_string(),
            set_dir: None,
            image_root: roots.img.clone().unwrap_or_else(|| pack.join("img")),
        }],
        LayoutKind::RootXml => {
            let image_root = roots.img.clone().unwrap_or_else(|| pack.to_path_buf());
            vec![Source {
                name: pack_name.to_string(),
                set_dir: None,
                image_root,
            }]
        }
        LayoutKind::DroppedImageSet => vec![Source {
            name: pack_name.to_string(),
            set_dir: None,
            image_root: pack.to_path_buf(),
        }],
    }
}

struct SetDir {
    name: String,
    path: PathBuf,
}

fn image_set_dirs(img: &Path) -> Vec<SetDir> {
    let Ok(entries) = fs::read_dir(img) else {
        return Vec::new();
    };
    let mut sets = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(raw) = name.to_str() else {
            continue;
        };
        if raw.starts_with('.') || raw.eq_ignore_ascii_case("unused") {
            continue;
        }
        let path = entry.path();
        if !is_real_dir(&path) {
            continue;
        }
        sets.push(SetDir {
            name: nfc(raw),
            path,
        });
    }
    sets.sort_by(|left, right| left.name.cmp(&right.name));
    sets
}

fn load_character(
    pack: &Path,
    pack_name: &str,
    layout: LayoutKind,
    source: &Source,
    roots: &Roots,
    diags: &mut Vec<Diagnostic>,
) -> Option<Character> {
    let dirs = config_dirs(
        pack,
        layout,
        &source.name,
        source.set_dir.as_deref(),
        roots,
        pack_name,
        diags,
    );
    let actions_hit = find_in_dirs(&dirs, ACTION_FILES);
    let behaviors_hit = find_in_dirs(&dirs, BEHAVIOR_FILES);
    let actions_path = take_config(
        &actions_hit,
        pack,
        pack_name,
        &source.name,
        "actions",
        diags,
    );
    let behaviors_path = take_config(
        &behaviors_hit,
        pack,
        pack_name,
        &source.name,
        "behaviors",
        diags,
    );
    if actions_path.is_none() || behaviors_path.is_none() {
        return None;
    }
    let actions_path = actions_path.unwrap();
    let behaviors_path = behaviors_path.unwrap();
    let actions_file = rel_slash(pack, &actions_path);
    let behaviors_file = rel_slash(pack, &behaviors_path);
    let actions_doc = read_doc(pack_name, &actions_path, &actions_file, diags)?;
    let behaviors_doc = read_doc(pack_name, &behaviors_path, &behaviors_file, diags)?;

    let mut constants = actions_doc.constants;
    for (key, value) in behaviors_doc.constants.into_pairs() {
        constants.insert(key, value);
    }
    let mut actions = actions_doc.actions;
    for (key, value) in behaviors_doc.actions.into_pairs() {
        actions.insert(key, value);
    }
    let mut behaviors = actions_doc.behaviors;
    for (key, value) in behaviors_doc.behaviors.into_pairs() {
        behaviors.insert(key, value);
    }
    let mut behavior_tree = actions_doc.behavior_tree;
    behavior_tree.extend(behaviors_doc.behavior_tree);
    let mut info = actions_doc.info;
    merge_info(&mut info, behaviors_doc.info);

    let info_hit = find_in_dirs(&dirs, &["info.xml"]);
    let info_file = match info_hit {
        Hit::Found {
            path,
            on_disk,
            reference,
            folded,
        } => {
            if folded {
                note_match(
                    diags,
                    pack_name,
                    &rel_slash(pack, &path),
                    None,
                    &reference,
                    &on_disk,
                );
            }
            let info_file = rel_slash(pack, &path);
            match fs::read(&path) {
                Ok(bytes) => {
                    let (doc, more) = xml::parse_document(&bytes, pack_name, &info_file);
                    diags.extend(more);
                    if let Some(doc) = doc {
                        merge_info(&mut info, doc.info);
                    }
                }
                Err(error) => diags.push(Diagnostic::new(
                    Severity::Warning,
                    DiagnosticKind::Xml,
                    pack_name,
                    &info_file,
                    None,
                    format!("could not read info.xml: {error}"),
                )),
            }
            Some(info_file)
        }
        Hit::Ambiguous(candidates) => {
            diags.push(Diagnostic::new(
                Severity::Warning,
                DiagnosticKind::MissingConfig,
                pack_name,
                "",
                None,
                format!("info.xml is missing; candidates: {}", candidates.join(", ")),
            ));
            None
        }
        Hit::Missing => None,
    };

    let image_root = nfc_slash(&rel_slash(pack, &source.image_root));
    let index = index_images(&source.image_root);
    let mut character = Character {
        name: source.name.clone(),
        schema: actions_doc.schema,
        behavior_schema: behaviors_doc.schema,
        image_root,
        actions_file,
        behaviors_file,
        info_file,
        constants,
        actions,
        behaviors,
        behavior_tree,
        info,
    };
    resolve_images(
        &mut character,
        &index,
        &source.image_root,
        diags,
        pack_name,
        &HashSet::new(),
    );
    validate_refs(&character, diags, pack_name);
    Some(character)
}

fn read_doc(pack: &str, path: &Path, file: &str, diags: &mut Vec<Diagnostic>) -> Option<LoadedDoc> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            diags.push(Diagnostic::new(
                Severity::Error,
                DiagnosticKind::Xml,
                pack,
                file,
                None,
                format!("could not read file: {error}"),
            ));
            return None;
        }
    };
    let (doc, more) = xml::parse_document(&bytes, pack, file);
    diags.extend(more);
    doc
}

fn missing_config(diags: &mut Vec<Diagnostic>, pack: &str, message: &str) {
    diags.push(Diagnostic::new(
        Severity::Error,
        DiagnosticKind::MissingConfig,
        pack,
        "",
        None,
        message,
    ));
}

fn merge_info(into: &mut Information, from: Information) {
    if from.display_name.is_some() {
        into.display_name = from.display_name;
    }
    if from.preview_image.is_some() {
        into.preview_image = from.preview_image;
    }
    if from.splash_image.is_some() {
        into.splash_image = from.splash_image;
    }
    for contributor in from.contributors {
        into.set_contributor(contributor);
    }
}

fn config_dirs(
    pack: &Path,
    layout: LayoutKind,
    set_name: &str,
    set_dir: Option<&Path>,
    roots: &Roots,
    pack_name: &str,
    diags: &mut Vec<Diagnostic>,
) -> Vec<PathBuf> {
    if layout == LayoutKind::RootXml {
        return vec![pack.to_path_buf()];
    }
    let mut dirs = Vec::new();
    let set_root = if let Some(dir) = set_dir {
        Some(dir.to_path_buf())
    } else {
        roots
            .img
            .as_deref()
            .and_then(|img| take_child(img, set_name, pack_name, diags))
    };
    if let Some(root) = set_root.as_deref() {
        if let Some(conf) = take_child(root, "conf", pack_name, diags) {
            dirs.push(conf);
        }
    }
    if let Some(conf) = roots.conf.as_deref() {
        if let Some(set_conf) = take_child(conf, set_name, pack_name, diags) {
            dirs.push(set_conf);
        }
        dirs.push(conf.to_path_buf());
    }
    dirs
}

fn find_in_dirs(dirs: &[PathBuf], names: &[&str]) -> Hit {
    for dir in dirs {
        match find_named_file(dir, names) {
            Hit::Missing => continue,
            hit => return hit,
        }
    }
    Hit::Missing
}

fn action_file_present(dir: &Path) -> bool {
    !matches!(find_named_file(dir, ACTION_FILES), Hit::Missing)
}

fn find_named_file(dir: &Path, names: &[&str]) -> Hit {
    let entries = list_entries(dir, EntryKind::File);
    for name in names {
        let wanted = nfc(name);
        if let Some(entry) = entries.iter().find(|entry| entry.name == wanted) {
            return Hit::Found {
                path: entry.path.clone(),
                on_disk: entry.name.clone(),
                reference: wanted,
                folded: false,
            };
        }
    }
    for name in names {
        let wanted = nfc(name);
        match pick_names(entries.iter().map(|entry| &entry.name), &wanted) {
            FoldPick::Folded(on_disk) => {
                let Some(entry) = entries.iter().find(|entry| entry.name == on_disk) else {
                    continue;
                };
                return Hit::Found {
                    path: entry.path.clone(),
                    on_disk,
                    reference: wanted,
                    folded: true,
                };
            }
            FoldPick::Ambiguous(candidates) => return Hit::Ambiguous(candidates),
            FoldPick::Exact | FoldPick::Missing => continue,
        }
    }
    Hit::Missing
}

fn take_config(
    hit: &Hit,
    pack: &Path,
    pack_name: &str,
    character: &str,
    role: &str,
    diags: &mut Vec<Diagnostic>,
) -> Option<PathBuf> {
    match hit {
        Hit::Found {
            path,
            on_disk,
            reference,
            folded,
        } => {
            if *folded {
                note_match(
                    diags,
                    pack_name,
                    &rel_slash(pack, path),
                    None,
                    reference,
                    on_disk,
                );
            }
            Some(path.clone())
        }
        Hit::Ambiguous(candidates) => {
            missing_config(
                diags,
                pack_name,
                &format!(
                    "{role} file not found for {character}; candidates: {}",
                    candidates.join(", ")
                ),
            );
            None
        }
        Hit::Missing => {
            missing_config(
                diags,
                pack_name,
                &format!("{role} file not found for {character}"),
            );
            None
        }
    }
}

fn take_child(
    parent: &Path,
    name: &str,
    pack: &str,
    diags: &mut Vec<Diagnostic>,
) -> Option<PathBuf> {
    match find_child(parent, name) {
        Hit::Found {
            path,
            on_disk,
            reference,
            folded,
        } => {
            if folded {
                note_match(diags, pack, "", None, &reference, &on_disk);
            }
            Some(path)
        }
        Hit::Ambiguous(candidates) => {
            diags.push(Diagnostic::new(
                Severity::Error,
                DiagnosticKind::MissingConfig,
                pack,
                "",
                None,
                format!(
                    "directory {name} is missing; candidates: {}",
                    candidates.join(", ")
                ),
            ));
            None
        }
        Hit::Missing => None,
    }
}

fn find_child(parent: &Path, name: &str) -> Hit {
    let wanted = nfc(name);
    let entries = list_entries(parent, EntryKind::Dir);
    match pick_names(entries.iter().map(|entry| &entry.name), &wanted) {
        FoldPick::Exact => entry_hit(entries, &wanted, &wanted, false),
        FoldPick::Folded(on_disk) => entry_hit(entries, &on_disk, &wanted, true),
        FoldPick::Ambiguous(candidates) => Hit::Ambiguous(candidates),
        FoldPick::Missing => Hit::Missing,
    }
}

fn entry_hit(entries: Vec<Listed>, on_disk: &str, reference: &str, folded: bool) -> Hit {
    match entries.into_iter().find(|entry| entry.name == on_disk) {
        Some(entry) => Hit::Found {
            path: entry.path,
            on_disk: entry.name,
            reference: reference.to_string(),
            folded,
        },
        None => Hit::Missing,
    }
}

struct Listed {
    name: String,
    path: PathBuf,
}

enum EntryKind {
    Dir,
    File,
}

fn list_entries(dir: &Path, kind: EntryKind) -> Vec<Listed> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut listed = Vec::new();
    for entry in entries.flatten() {
        let raw_name = entry.file_name();
        let Some(raw) = raw_name.to_str() else {
            continue;
        };
        let path = entry.path();
        let keep = match kind {
            EntryKind::Dir => is_real_dir(&path),
            // `symlink_metadata` does not follow links. A symlink must not
            // count as the config file it points at.
            EntryKind::File => fs::symlink_metadata(&path)
                .map(|meta| meta.file_type().is_file())
                .unwrap_or(false),
        };
        if keep {
            listed.push(Listed {
                name: nfc(raw),
                path,
            });
        }
    }
    listed
}

enum Hit {
    Found {
        path: PathBuf,
        on_disk: String,
        reference: String,
        folded: bool,
    },
    Ambiguous(Vec<String>),
    Missing,
}

enum FoldPick {
    Exact,
    Folded(String),
    Ambiguous(Vec<String>),
    Missing,
}

/// Exact NFC name wins. Otherwise one case-insensitive name, or every candidate.
fn pick_names<'a>(names: impl Iterator<Item = &'a String>, wanted: &str) -> FoldPick {
    let folded = wanted.to_lowercase();
    let mut exact = false;
    let mut hits = Vec::new();
    for name in names {
        if name == wanted {
            exact = true;
        } else if name.to_lowercase() == folded {
            hits.push(name.clone());
        }
    }
    if exact {
        return FoldPick::Exact;
    }
    hits.sort();
    match hits.len() {
        0 => FoldPick::Missing,
        1 => FoldPick::Folded(hits.remove(0)),
        _ => FoldPick::Ambiguous(hits),
    }
}

fn note_match(
    diags: &mut Vec<Diagnostic>,
    pack: &str,
    file: &str,
    line: Option<u32>,
    reference: &str,
    found: &str,
) {
    diags.push(Diagnostic::new(
        Severity::Info,
        DiagnosticKind::Other,
        pack,
        file,
        line,
        format!("{reference} matched {found}"),
    ));
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_dir())
        .unwrap_or(false)
}

/// Keys are NFC. Each entry keeps its stored name too, because a
/// normalization-sensitive file system opens only the name as stored.
#[derive(Debug, Default)]
struct DirNode {
    on_disk: String,
    dirs: HashMap<String, DirNode>,
    files: HashMap<String, String>,
}

#[derive(Debug)]
struct ImageIndex {
    root: DirNode,
}

#[derive(Debug, PartialEq, Eq)]
enum ImageHit {
    Found {
        relative: String,
        on_disk: String,
        folded: bool,
    },
    Ambiguous(Vec<String>),
    Missing,
}

impl ImageIndex {
    fn lookup(&self, relative: &str) -> ImageHit {
        let parts: Vec<&str> = relative
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        if parts.is_empty() {
            return ImageHit::Missing;
        }
        let mut resolved = Vec::with_capacity(parts.len());
        let mut on_disk = Vec::with_capacity(parts.len());
        let mut folded = false;
        match lookup_at(&self.root, &parts, &mut resolved, &mut on_disk, &mut folded) {
            Ok(()) => ImageHit::Found {
                relative: resolved.join("/"),
                on_disk: on_disk.join("/"),
                folded,
            },
            Err(hit) => hit,
        }
    }
}

fn lookup_at(
    node: &DirNode,
    parts: &[&str],
    resolved: &mut Vec<String>,
    on_disk: &mut Vec<String>,
    folded: &mut bool,
) -> Result<(), ImageHit> {
    let Some((part, rest)) = parts.split_first() else {
        return Err(ImageHit::Missing);
    };
    if rest.is_empty() {
        let name = match pick_names(node.files.keys(), part) {
            FoldPick::Exact => (*part).to_string(),
            FoldPick::Folded(name) => {
                *folded = true;
                name
            }
            FoldPick::Ambiguous(candidates) => return Err(ImageHit::Ambiguous(candidates)),
            FoldPick::Missing => return Err(ImageHit::Missing),
        };
        let Some(stored) = node.files.get(&name) else {
            return Err(ImageHit::Missing);
        };
        on_disk.push(stored.clone());
        resolved.push(name);
        return Ok(());
    }
    let next_name = match pick_names(node.dirs.keys(), part) {
        FoldPick::Exact => (*part).to_string(),
        FoldPick::Folded(name) => {
            *folded = true;
            name
        }
        FoldPick::Ambiguous(candidates) => return Err(ImageHit::Ambiguous(candidates)),
        FoldPick::Missing => return Err(ImageHit::Missing),
    };
    let Some(next) = node.dirs.get(&next_name) else {
        return Err(ImageHit::Missing);
    };
    on_disk.push(next.on_disk.clone());
    resolved.push(next_name);
    lookup_at(next, rest, resolved, on_disk, folded)
}

fn index_images(root: &Path) -> ImageIndex {
    let mut index = ImageIndex {
        root: DirNode::default(),
    };
    index_into(root, &mut index.root, 0);
    index
}

fn index_into(dir: &Path, node: &mut DirNode, depth: usize) {
    // `depth` is how many components sit above `dir`. Files in this directory
    // have `depth + 1` components. Stop at [`crate::import::limits::MAX_DEPTH`]
    // so a hostile tree cannot recurse without a bound.
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let raw_name = entry.file_name();
        let Some(raw) = raw_name.to_str() else {
            continue;
        };
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let kind = meta.file_type();
        if kind.is_symlink() {
            continue;
        }
        let key = nfc(raw);
        // NFC and NFD spellings of one name can both exist where the file
        // system keeps them apart. The NFC spelling wins, whatever the order.
        let keep = |stored: &mut String| {
            if stored.is_empty() || raw == key {
                *stored = raw.to_string();
            }
        };
        if kind.is_dir() {
            if depth + 1 >= crate::import::limits::MAX_DEPTH {
                continue;
            }
            let child = node.dirs.entry(key.clone()).or_default();
            keep(&mut child.on_disk);
            index_into(&path, child, depth + 1);
        } else if kind.is_file() {
            if depth + 1 > crate::import::limits::MAX_DEPTH {
                continue;
            }
            keep(node.files.entry(key.clone()).or_default());
        }
    }
}

pub(crate) fn attach_images(
    character: &mut Character,
    image_root: &Path,
    diags: &mut Vec<Diagnostic>,
    pack: &str,
    quiet_missing: &HashSet<String>,
) {
    let index = index_images(image_root);
    resolve_images(character, &index, image_root, diags, pack, quiet_missing);
}

fn resolve_images(
    character: &mut Character,
    index: &ImageIndex,
    image_root: &Path,
    diags: &mut Vec<Diagnostic>,
    pack: &str,
    quiet_missing: &HashSet<String>,
) {
    let file = character.actions_file.clone();
    let mut sink = ImageSink {
        index,
        image_root,
        reported_missing: HashSet::new(),
        reported_fold: HashSet::new(),
        reported_escape: HashSet::new(),
        sizes: HashMap::new(),
        diags,
        pack,
        file: &file,
        quiet_missing,
    };
    character.actions.for_each_value_mut(|action| {
        walk_action(action, &mut |action| {
            for animation in &mut action.animations {
                for pose in &mut animation.poses {
                    if let Some(source) = pose.image_source.clone() {
                        pose.image = assign_image(&mut sink, &source, pose.line);
                    }
                    if let Some(source) = pose.image_right_source.clone() {
                        pose.image_right = assign_image(&mut sink, &source, pose.line);
                    }
                    if let Some(image) = &pose.image {
                        let sibling = right_sibling(&image.relative);
                        if let ImageHit::Found {
                            relative, on_disk, ..
                        } = index.lookup(&sibling)
                        {
                            pose.right_sibling = Some(resolved_image(
                                &mut sink, sibling, relative, &on_disk, pose.line,
                            ));
                        }
                    }
                }
            }
        });
    });
}

fn walk_action(action: &mut Action, visit: &mut impl FnMut(&mut Action)) {
    visit(action);
    for child in &mut action.children {
        if let ActionNode::Action(child) = child {
            walk_action(child, visit);
        }
    }
}

struct ImageSink<'a> {
    index: &'a ImageIndex,
    image_root: &'a Path,
    reported_missing: HashSet<String>,
    reported_fold: HashSet<String>,
    reported_escape: HashSet<String>,
    /// IHDR size by on-disk relative path. Failed reads are cached as 0×0.
    sizes: HashMap<String, (i32, i32)>,
    diags: &'a mut Vec<Diagnostic>,
    pack: &'a str,
    file: &'a str,
    /// Image attribute text that may be absent without a missing-image warning.
    /// Empty atlas cells are not written, and the capability pass prunes them.
    quiet_missing: &'a HashSet<String>,
}

fn assign_image(sink: &mut ImageSink<'_>, source: &str, line: u32) -> Option<ResolvedImage> {
    match logical_relative(source) {
        Err(()) => {
            if sink.reported_escape.insert(source.to_string()) {
                sink.diags.push(Diagnostic::new(
                    Severity::Error,
                    DiagnosticKind::ImageEscape,
                    sink.pack,
                    sink.file,
                    Some(line),
                    format!("image path escapes the image set: {source}"),
                ));
            }
            None
        }
        Ok(relative) => match sink.index.lookup(&relative) {
            ImageHit::Found {
                relative: resolved,
                on_disk,
                folded,
            } => {
                if folded && sink.reported_fold.insert(source.to_string()) {
                    note_match(
                        sink.diags,
                        sink.pack,
                        sink.file,
                        Some(line),
                        source,
                        &resolved,
                    );
                }
                Some(resolved_image(
                    sink,
                    source.to_string(),
                    resolved,
                    &on_disk,
                    line,
                ))
            }
            ImageHit::Ambiguous(candidates) => {
                if sink.reported_missing.insert(source.to_string()) {
                    sink.diags.push(Diagnostic::new(
                        Severity::Warning,
                        DiagnosticKind::MissingImage,
                        sink.pack,
                        sink.file,
                        Some(line),
                        format!(
                            "image not found: {source} (candidates: {})",
                            candidates.join(", ")
                        ),
                    ));
                }
                None
            }
            ImageHit::Missing => {
                if sink.quiet_missing.contains(source) {
                    return None;
                }
                if sink.reported_missing.insert(source.to_string()) {
                    sink.diags.push(Diagnostic::new(
                        Severity::Warning,
                        DiagnosticKind::MissingImage,
                        sink.pack,
                        sink.file,
                        Some(line),
                        format!("image not found: {source}"),
                    ));
                }
                None
            }
        },
    }
}

fn resolved_image(
    sink: &mut ImageSink<'_>,
    source: String,
    relative: String,
    on_disk: &str,
    line: u32,
) -> ResolvedImage {
    let (width, height) = pixel_size(sink, on_disk, &source, line);
    ResolvedImage {
        source,
        relative,
        width,
        height,
    }
}

/// Header width and height. A missing file never reaches this function.
/// An unreadable header keeps 0×0 and one warning, and does not drop the path.
fn pixel_size(sink: &mut ImageSink<'_>, on_disk: &str, source: &str, line: u32) -> (i32, i32) {
    if let Some(size) = sink.sizes.get(on_disk) {
        return *size;
    }
    let path = sink.image_root.join(on_disk);
    let size = match crate::images::png_pixel_size(&path) {
        Ok((width, height)) => (width as i32, height as i32),
        Err(message) => {
            sink.diags.push(Diagnostic::new(
                Severity::Warning,
                DiagnosticKind::Other,
                sink.pack,
                sink.file,
                Some(line),
                format!("{source}: {message}"),
            ));
            (0, 0)
        }
    };
    sink.sizes.insert(on_disk.to_string(), size);
    size
}

fn right_sibling(relative: &str) -> String {
    let (dir, file) = match relative.rsplit_once('/') {
        Some((dir, file)) => (Some(dir), file),
        None => (None, relative),
    };
    let renamed = match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}-r.{ext}"),
        _ => format!("{file}-r"),
    };
    match dir {
        Some(dir) => format!("{dir}/{renamed}"),
        None => renamed,
    }
}

pub(crate) fn validate_refs(character: &Character, diags: &mut Vec<Diagnostic>, pack: &str) {
    let mut missing_actions = HashSet::new();
    let mut missing_behaviors = HashSet::new();
    for action in character.actions.values() {
        check_action(
            action,
            character,
            diags,
            pack,
            &mut missing_actions,
            &mut missing_behaviors,
        );
    }
    for behavior in character.behaviors.values() {
        if !character.actions.contains(&behavior.action)
            && missing_actions.insert(behavior.action.clone())
        {
            diags.push(Diagnostic::new(
                Severity::Warning,
                DiagnosticKind::MissingReference,
                pack,
                &character.behaviors_file,
                Some(behavior.line),
                format!(
                    "behavior {} uses missing action {}",
                    behavior.name, behavior.action
                ),
            ));
        }
        for reference in behavior.next_refs() {
            if !character.behaviors.contains(&reference.name)
                && missing_behaviors.insert(reference.name.clone())
            {
                diags.push(Diagnostic::new(
                    Severity::Warning,
                    DiagnosticKind::MissingReference,
                    pack,
                    &character.behaviors_file,
                    Some(reference.line),
                    format!("next-list references missing behavior {}", reference.name),
                ));
            }
        }
    }
    for name in character.behavior_schema.required_behaviors() {
        if !character.behaviors.contains(name) {
            diags.push(Diagnostic::new(
                Severity::Warning,
                DiagnosticKind::MissingConfig,
                pack,
                &character.behaviors_file,
                None,
                format!("missing required behavior {name}"),
            ));
        }
    }
}

fn check_action(
    action: &Action,
    character: &Character,
    diags: &mut Vec<Diagnostic>,
    pack: &str,
    missing_actions: &mut HashSet<String>,
    missing_behaviors: &mut HashSet<String>,
) {
    for node in &action.children {
        match node {
            ActionNode::Action(child) => check_action(
                child,
                character,
                diags,
                pack,
                missing_actions,
                missing_behaviors,
            ),
            ActionNode::Reference(reference) => {
                if !character.actions.contains(&reference.name)
                    && missing_actions.insert(reference.name.clone())
                {
                    diags.push(Diagnostic::new(
                        Severity::Warning,
                        DiagnosticKind::MissingReference,
                        pack,
                        &character.actions_file,
                        Some(reference.line),
                        format!("action reference {} was not defined", reference.name),
                    ));
                }
            }
        }
    }
    for animation in &action.animations {
        for hotspot in &animation.hotspots {
            if let Some(name) = &hotspot.behavior {
                if !character.behaviors.contains(name) && missing_behaviors.insert(name.clone()) {
                    diags.push(Diagnostic::new(
                        Severity::Warning,
                        DiagnosticKind::MissingReference,
                        pack,
                        &character.actions_file,
                        Some(hotspot.line),
                        format!("hotspot references missing behavior {name}"),
                    ));
                }
            }
        }
    }
}

/// Image-set-relative path. `Err` is a path that leaves the image-set root.
pub(crate) fn logical_relative(source: &str) -> Result<String, ()> {
    if source.contains('\0') {
        return Err(());
    }
    let source = source.trim().replace('\\', "/");
    if source.is_empty() {
        return Err(());
    }
    let trimmed = source.trim_start_matches('/');
    if trimmed.is_empty() {
        return Err(());
    }
    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || is_drive(part) {
            return Err(());
        }
        parts.push(nfc(part));
    }
    if parts.is_empty() {
        Err(())
    } else {
        Ok(parts.join("/"))
    }
}

fn is_drive(part: &str) -> bool {
    let bytes = part.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn nfc(text: &str) -> String {
    text.nfc().collect()
}

fn nfc_slash(path: &str) -> String {
    if path.is_empty() {
        String::new()
    } else {
        path.split('/').map(nfc).collect::<Vec<_>>().join("/")
    }
}

fn rel_slash(pack: &Path, file: &Path) -> String {
    match file.strip_prefix(pack) {
        Ok(relative) => slash_path(relative),
        Err(_) => slash_path(file),
    }
}

fn slash_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::{DirNode, ImageHit, ImageIndex, logical_relative};

    fn insert(node: &mut DirNode, parts: &[&str]) {
        let Some((first, rest)) = parts.split_first() else {
            return;
        };
        if rest.is_empty() {
            node.files
                .insert((*first).to_string(), (*first).to_string());
        } else {
            let child = node.dirs.entry((*first).to_string()).or_default();
            child.on_disk = (*first).to_string();
            insert(child, rest);
        }
    }

    fn index_with(files: &[&str]) -> ImageIndex {
        let mut root = DirNode::default();
        for file in files {
            insert(&mut root, &file.split('/').collect::<Vec<_>>());
        }
        ImageIndex { root }
    }

    #[test]
    fn leading_slash_stays_inside_the_image_set() {
        assert_eq!(logical_relative("/shime1.png").as_deref(), Ok("shime1.png"));
    }

    #[test]
    fn parent_and_drive_paths_are_rejected() {
        assert!(logical_relative("/../secret.png").is_err());
        assert!(logical_relative("C:/windows/notepad.png").is_err());
        assert!(logical_relative("a/../../etc/passwd").is_err());
    }

    #[test]
    fn exact_image_name_wins_over_a_case_variant() {
        let index = index_with(&["abc.png", "ABC.png"]);
        assert_eq!(
            index.lookup("abc.png"),
            ImageHit::Found {
                relative: "abc.png".to_string(),
                on_disk: "abc.png".to_string(),
                folded: false,
            }
        );
        assert_eq!(
            index.lookup("ABC.png"),
            ImageHit::Found {
                relative: "ABC.png".to_string(),
                on_disk: "ABC.png".to_string(),
                folded: false,
            }
        );
    }

    #[test]
    fn ambiguous_image_names_list_both_candidates() {
        let index = index_with(&["a.png", "A.png"]);
        assert_eq!(
            index.lookup("a.PNG"),
            ImageHit::Ambiguous(vec!["A.png".to_string(), "a.png".to_string()])
        );
    }

    #[test]
    fn case_fold_stays_inside_the_matched_directory() {
        let index = index_with(&["Foo/a.png", "foo/A.png"]);
        assert_eq!(
            index.lookup("Foo/A.png"),
            ImageHit::Found {
                relative: "Foo/a.png".to_string(),
                on_disk: "Foo/a.png".to_string(),
                folded: true,
            }
        );
        assert_eq!(
            index.lookup("foo/a.png"),
            ImageHit::Found {
                relative: "foo/A.png".to_string(),
                on_disk: "foo/A.png".to_string(),
                folded: true,
            }
        );
        assert_eq!(
            index.lookup("FOO/a.png"),
            ImageHit::Ambiguous(vec!["Foo".to_string(), "foo".to_string()])
        );
    }

    #[test]
    fn nfc_lookup_opens_the_stored_nfd_name() {
        let mut dir = DirNode {
            on_disk: "Cafe\u{0301}".to_string(),
            ..DirNode::default()
        };
        dir.files.insert(
            "caf\u{00e9}.png".to_string(),
            "cafe\u{0301}.png".to_string(),
        );
        let mut root = DirNode::default();
        root.dirs.insert("Caf\u{00e9}".to_string(), dir);
        let index = ImageIndex { root };
        assert_eq!(
            index.lookup("Caf\u{00e9}/caf\u{00e9}.png"),
            ImageHit::Found {
                relative: "Caf\u{00e9}/caf\u{00e9}.png".to_string(),
                on_disk: "Cafe\u{0301}/cafe\u{0301}.png".to_string(),
                folded: false,
            }
        );
    }
}

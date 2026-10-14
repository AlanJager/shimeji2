//! The four import outcomes the settings window shows.
//!
//! Every call goes through [`shimeji_packs::import_pack`]. This module does
//! not call `load_dir` on the path the user dropped.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use shimeji_core::model::Pack;
use shimeji_packs::{
    CollisionPolicy, DecodedImage, DecodedWav, Diagnostic, ImportError, ImportFailure,
    ImportOutcome, import_pack,
};

use crate::library::is_degraded;

/// What to do when `packs/<id>/` is already there.
///
/// `None` reports the collision and installs nothing. Replace and keep-both
/// are the two choices the duplicate dialog offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Replace,
    KeepBoth,
}

/// One warning the Pets tab can show on a pack row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub file: String,
    pub line: Option<u32>,
    pub message: String,
}

/// A pack that was installed. `characters` is the count the success row shows.
#[derive(Debug)]
pub struct Installed {
    pub id: String,
    pub path: PathBuf,
    pub characters: usize,
    pub warnings: Vec<Warning>,
    pub pack: Pack,
    pub diagnostics: Vec<Diagnostic>,
    /// Pixels and PCM from `import_pack`. The shell draws these; it does not
    /// call `load_dir` on the dropped path.
    pub images: Vec<DecodedImage>,
    pub sounds: Vec<DecodedWav>,
}

/// The outcome named in the plan.
#[derive(Debug)]
pub enum Outcome {
    /// Installed, with no warning or error diagnostics.
    Success(Installed),
    /// Installed, and at least one feature was degraded.
    Degraded(Installed),
    /// Nothing new was kept under `packs/`. `message` is shown inline.
    Error { message: String },
    /// `packs/<id>/` is already installed. The caller asks replace or keep-both.
    Duplicate { id: String },
}

impl Outcome {
    pub fn installed(&self) -> Option<&Installed> {
        match self {
            Self::Success(installed) | Self::Degraded(installed) => Some(installed),
            Self::Error { .. } | Self::Duplicate { .. } => None,
        }
    }
}

/// Import `source` (a folder or a zip) into `packs_root`.
///
/// The pack is validated in a staging directory inside `packs_root` first. A replace moves that
/// tree into place only after it has at least one character, so a bad drop
/// cannot swap out the installed pack. Staging is removed on every path.
pub fn import(source: &Path, packs_root: &Path, choice: Option<Choice>) -> Outcome {
    let staging = Staging::create(packs_root);
    validate_then_place(source, packs_root, staging.path(), choice)
}

/// A hidden directory inside `packs_root`, next to `packs/`. Same filesystem,
/// so the commit is a rename, and the library scan of `packs/` never sees it.
struct Staging {
    path: PathBuf,
}

impl Staging {
    fn create(packs_root: &Path) -> Self {
        static N: AtomicU64 = AtomicU64::new(1);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path = packs_root.join(format!(".import-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        let _ = fs::create_dir_all(&path);
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn validate_then_place(
    source: &Path,
    packs_root: &Path,
    staging_root: &Path,
    choice: Option<Choice>,
) -> Outcome {
    // An empty staging root never collides, so Report installs there and
    // leaves `packs_root` alone until the result is known.
    let outcome = match import_pack(source, staging_root, CollisionPolicy::Report) {
        Ok(outcome) => outcome,
        Err(error) => return from_error(error),
    };
    let packs_dir = packs_root.join("packs");
    let dest = packs_dir.join(&outcome.id);
    if outcome.pack.characters.is_empty() {
        return Outcome::Error {
            message: empty_message(&outcome),
        };
    }
    let occupied = dest.exists();
    if occupied && choice.is_none() {
        return Outcome::Duplicate { id: outcome.id };
    }
    if let Err(error) = fs::create_dir_all(&packs_dir) {
        return io_error(&error);
    }
    if !occupied {
        if let Err(error) = fs::rename(&outcome.path, &dest) {
            return io_error(&error);
        }
        return classify(outcome, dest);
    }
    match choice {
        Some(Choice::Replace) => {
            if let Err(error) = exchange_dir(&outcome.path, &dest, staging_root) {
                return io_error(&error);
            }
            classify(outcome, dest)
        }
        Some(Choice::KeepBoth) => {
            let id = free_id(&packs_dir, &outcome.id);
            let dest = packs_dir.join(&id);
            if let Err(error) = fs::rename(&outcome.path, &dest) {
                return io_error(&error);
            }
            classify_as(outcome, id, dest)
        }
        None => Outcome::Duplicate { id: outcome.id },
    }
}

fn empty_message(outcome: &ImportOutcome) -> String {
    outcome
        .diagnostics
        .first()
        .map(|diagnostic| diagnostic.message.clone())
        .unwrap_or_else(|| "not a readable pack".to_string())
}

fn exchange_dir(staged: &Path, dest: &Path, staging_root: &Path) -> io::Result<()> {
    let aside = staging_root.join("previous");
    fs::rename(dest, &aside)?;
    if let Err(error) = fs::rename(staged, dest) {
        if fs::rename(&aside, dest).is_err() {
            let name = dest
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("pack");
            let rescue = dest.with_file_name(format!(".{name}.restore-{}", std::process::id()));
            let _ = fs::rename(&aside, rescue);
        }
        return Err(error);
    }
    Ok(())
}

/// Same suffix sequence as `import_pack`: `<id>-2`, then `<id>-3`, and so on.
fn free_id(packs_dir: &Path, base: &str) -> String {
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}-{n}");
        if !packs_dir.join(&candidate).exists() {
            return candidate;
        }
        n = n.saturating_add(1);
        if n == u32::MAX {
            return format!("{base}-{n}");
        }
    }
}

fn classify(outcome: ImportOutcome, path: PathBuf) -> Outcome {
    let id = outcome.id.clone();
    classify_as(outcome, id, path)
}

fn classify_as(mut outcome: ImportOutcome, id: String, path: PathBuf) -> Outcome {
    debug_assert!(
        !outcome.pack.characters.is_empty(),
        "a pack with no characters is an error, not an install"
    );
    if outcome.id != id {
        outcome.pack.name = id.clone();
        for diagnostic in &mut outcome.diagnostics {
            diagnostic.pack = id.clone();
        }
    }
    let warnings: Vec<Warning> = outcome
        .diagnostics
        .iter()
        .filter(|diagnostic| is_degraded(diagnostic))
        .map(|diagnostic| Warning {
            file: diagnostic.file.clone(),
            line: diagnostic.line,
            message: diagnostic.message.clone(),
        })
        .collect();
    let installed = Installed {
        id,
        path,
        characters: outcome.pack.characters.len(),
        warnings,
        pack: outcome.pack,
        diagnostics: outcome.diagnostics,
        images: outcome.images,
        sounds: outcome.sounds,
    };
    if installed.warnings.is_empty() {
        Outcome::Success(installed)
    } else {
        Outcome::Degraded(installed)
    }
}

fn io_error(error: &io::Error) -> Outcome {
    Outcome::Error {
        message: format!("import failed: {error}"),
    }
}

fn from_error(error: ImportError) -> Outcome {
    if let ImportFailure::Collision { id } = error.failure {
        return Outcome::Duplicate { id };
    }
    let message = error.diagnostics.first().map_or_else(
        || "import failed".to_string(),
        |diagnostic| diagnostic.message.clone(),
    );
    Outcome::Error { message }
}

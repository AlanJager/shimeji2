//! Import a zip or a folder into `packs/<id>/`.
//!
//! The source is copied into `packs_root/staging/<job>/<id>/` with the caps
//! applied during the copy. [`crate::load_dir`] then validates that copy, and
//! PNG/WAV files are decoded under the byte budget. The staged directory is
//! renamed into `packs/<id>/`. On failure the job directory is removed, so
//! nothing is left in `packs/` or in `staging`.
//!
//! A new id is a plain `rename`. A replace is [`crate::staging::replace_dir`]:
//! `renamex_np(RENAME_SWAP)` on macOS, and the portable fallback everywhere else.

pub(crate) mod error;
pub(crate) mod fs;
pub(crate) mod limits;
pub(crate) mod paths;
pub(crate) mod zip;

pub use error::{CollisionPolicy, ImportError, ImportFailure};
pub use limits::{
    MAX_DECODED_BYTES, MAX_DEPTH, MAX_ENTRIES, MAX_IMAGE_EDGE, MAX_UNCOMPRESSED_BYTES,
};

use std::fs as stdfs;
use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

use crate::images::{DecodedImage, decode_tree};
use crate::sounds::DecodedWav;
use crate::staging::{self, Job};
use crate::{Diagnostic, load_dir};

use error::CollisionPolicy as Policy;

/// A pack that was installed under `packs_root/packs/<id>/`.
#[derive(Debug)]
pub struct ImportOutcome {
    pub id: String,
    pub path: PathBuf,
    pub pack: crate::model::Pack,
    pub diagnostics: Vec<Diagnostic>,
    pub images: Vec<DecodedImage>,
    pub sounds: Vec<DecodedWav>,
    /// Codex is `Some(0.62)`. A native pack copies `art.displayScale`. Classic is `None`.
    pub display_scale: Option<f64>,
    /// Set when an existing pack was in the way. `Replace` names the id that
    /// was swapped out. `KeepBoth` names the id that was left in place; `id`
    /// is the suffixed directory that was installed beside it.
    pub collision: Option<Collision>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collision {
    pub existing_id: String,
    pub policy: CollisionPolicy,
}

/// Copy `source` (a zip file or a directory) into `packs_root` and validate it.
///
/// `on_collision` chooses what happens when `packs/<id>/` already exists.
/// [`CollisionPolicy::Report`] installs nothing and returns
/// [`ImportFailure::Collision`].
pub fn import_pack(
    source: impl AsRef<Path>,
    packs_root: impl AsRef<Path>,
    on_collision: CollisionPolicy,
) -> Result<ImportOutcome, ImportError> {
    let source = source.as_ref();
    let packs_root = packs_root.as_ref();
    let meta = stdfs::symlink_metadata(source).map_err(|error| ImportError::io("", error))?;
    if meta.file_type().is_symlink() {
        return Err(ImportError::symlink(source.display().to_string()));
    }
    let is_zip = meta.is_file();
    if !is_zip && !meta.is_dir() {
        return Err(ImportError::special(
            source.display().to_string(),
            "special file",
        ));
    }
    let base_id = pack_id(source, is_zip)?;
    let job = Job::create(packs_root).map_err(|error| ImportError::io(&base_id, error))?;
    let staged = job.path().join(&base_id);
    stdfs::create_dir(&staged).map_err(|error| ImportError::io(&base_id, error))?;
    if is_zip {
        zip::extract_zip(source, &staged, &base_id)?;
    } else {
        fs::copy_tree(source, &staged, &base_id)?;
    }

    let mut loaded = load_dir(&staged).map_err(|error| ImportError::io(&base_id, error))?;
    let decoded = decode_tree(&staged, &base_id)?;
    loaded.diagnostics.extend(decoded.diagnostics);

    let packs_dir = packs_root.join("packs");
    stdfs::create_dir_all(&packs_dir).map_err(|error| ImportError::io(&base_id, error))?;
    let existing = packs_dir.join(&base_id);
    let (final_id, collision) = place(&staged, &packs_dir, &base_id, &existing, on_collision)?;
    if final_id != base_id {
        loaded.pack.name = final_id.clone();
        for diagnostic in &mut loaded.diagnostics {
            diagnostic.pack = final_id.clone();
        }
    }
    Ok(ImportOutcome {
        path: packs_dir.join(&final_id),
        id: final_id,
        pack: loaded.pack,
        diagnostics: loaded.diagnostics,
        images: decoded.images,
        sounds: decoded.sounds,
        display_scale: loaded.display_scale,
        collision,
    })
}

fn place(
    staged: &Path,
    packs_dir: &Path,
    base_id: &str,
    existing: &Path,
    policy: Policy,
) -> Result<(String, Option<Collision>), ImportError> {
    let occupied = existing.exists();
    match policy {
        Policy::Report if occupied => Err(ImportError::new(
            base_id,
            ImportFailure::Collision {
                id: base_id.to_string(),
            },
        )),
        Policy::Replace if occupied => {
            staging::replace_dir(staged, existing)
                .map_err(|error| ImportError::io(base_id, error))?;
            Ok((
                base_id.to_string(),
                Some(Collision {
                    existing_id: base_id.to_string(),
                    policy,
                }),
            ))
        }
        Policy::KeepBoth if occupied => {
            let id = free_id(packs_dir, base_id);
            let dest = packs_dir.join(&id);
            stdfs::rename(staged, &dest).map_err(|error| ImportError::io(base_id, error))?;
            Ok((
                id,
                Some(Collision {
                    existing_id: base_id.to_string(),
                    policy,
                }),
            ))
        }
        Policy::Report | Policy::Replace | Policy::KeepBoth => {
            stdfs::rename(staged, existing).map_err(|error| ImportError::io(base_id, error))?;
            Ok((base_id.to_string(), None))
        }
    }
}

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

fn pack_id(source: &Path, is_zip: bool) -> Result<String, ImportError> {
    let Some(raw) = source.file_name().and_then(|name| name.to_str()) else {
        return Err(ImportError::new("", ImportFailure::BadName));
    };
    let raw = if is_zip && raw.len() > 4 && raw[raw.len() - 4..].eq_ignore_ascii_case(".zip") {
        &raw[..raw.len() - 4]
    } else {
        raw
    };
    let id: String = raw.nfc().collect();
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.contains('/')
        || id.contains('\\')
        || id.contains('\0')
    {
        return Err(ImportError::new("", ImportFailure::BadName));
    }
    Ok(id)
}

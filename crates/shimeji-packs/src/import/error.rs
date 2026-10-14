//! Failures that reject a whole import. A bad individual image is not one of
//! these; that is a diagnostic on [`super::ImportOutcome`].

use std::fmt;
use std::io;

use shimeji_core::diagnostics::{Diagnostic, DiagnosticKind, Severity};

use super::limits::{MAX_DEPTH, MAX_ENTRIES, MAX_IMAGE_EDGE, MAX_UNCOMPRESSED_BYTES};

/// What the caller does when `packs/<id>/` already exists.
///
/// [`CollisionPolicy::Report`] is how the caller learns about the collision.
/// Replacing and keeping both are explicit. Nothing is swapped unless the
/// caller asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionPolicy {
    /// Leave the installed pack untouched and return [`ImportFailure::Collision`].
    Report,
    /// Exchange the directories, then delete the previous tree.
    Replace,
    /// Install beside the existing pack as `<id>-2`, `<id>-3`, and so on.
    KeepBoth,
}

/// An import that did not install a pack.
#[derive(Debug)]
pub struct ImportError {
    pub failure: ImportFailure,
    pub diagnostics: Vec<Diagnostic>,
}

impl ImportError {
    pub(crate) fn new(pack: &str, failure: ImportFailure) -> Self {
        let diagnostics = failure.diagnostic(pack).into_iter().collect();
        Self {
            failure,
            diagnostics,
        }
    }

    pub(crate) fn io(pack: &str, error: io::Error) -> Self {
        Self::new(pack, ImportFailure::Io(error))
    }

    pub(crate) fn path(entry: impl Into<String>, reason: impl Into<String>) -> Self {
        let entry = entry.into();
        let reason = reason.into();
        Self::new(
            "",
            ImportFailure::UnsafePath {
                entry: entry.clone(),
                reason: reason.clone(),
            },
        )
    }

    pub(crate) fn too_deep(entry: impl Into<String>) -> Self {
        Self::new("", ImportFailure::TooDeep { path: entry.into() })
    }

    pub(crate) fn entries(count: u64) -> Self {
        Self::new("", ImportFailure::TooManyEntries { count })
    }

    pub(crate) fn declared_size(bytes: u64) -> Self {
        Self::new("", ImportFailure::DeclaredSize { bytes })
    }

    pub(crate) fn actual_size(bytes: u64) -> Self {
        Self::new("", ImportFailure::ActualSize { bytes })
    }

    pub(crate) fn symlink(path: impl Into<String>) -> Self {
        Self::new("", ImportFailure::Symlink { path: path.into() })
    }

    pub(crate) fn special(path: impl Into<String>, kind: &'static str) -> Self {
        Self::new(
            "",
            ImportFailure::Special {
                path: path.into(),
                kind,
            },
        )
    }

    pub(crate) fn with_pack(mut self, pack: &str) -> Self {
        for diagnostic in &mut self.diagnostics {
            if diagnostic.pack.is_empty() {
                diagnostic.pack = pack.to_string();
            }
        }
        self
    }
}

/// Why the pack was rejected. Budget and size-cap failures are here.
/// A single bad PNG or WAV is not.
#[derive(Debug)]
pub enum ImportFailure {
    Io(io::Error),
    /// The source is not a directory and not a zip archive.
    NotZip,
    /// The source name cannot be used as a pack id.
    BadName,
    UnsafePath {
        entry: String,
        reason: String,
    },
    Symlink {
        path: String,
    },
    /// FIFO, socket, or device file. `kind` is a short name such as `"fifo"`.
    Special {
        path: String,
        kind: &'static str,
    },
    TooManyEntries {
        count: u64,
    },
    TooDeep {
        path: String,
    },
    /// Central-directory sizes already exceed the cap. Nothing was extracted.
    DeclaredSize {
        bytes: u64,
    },
    /// Decompressed or copied bytes exceeded the cap. Extraction stopped.
    ActualSize {
        bytes: u64,
    },
    Compression {
        entry: String,
        method: String,
    },
    Encrypted {
        entry: String,
    },
    ImageDimensions {
        path: String,
        width: u32,
        height: u32,
    },
    /// `decoded_images` is how many images were decoded before this one was refused.
    DecodeBudget {
        path: String,
        decoded_images: usize,
        used_bytes: u64,
        next_bytes: u64,
    },
    Collision {
        id: String,
    },
    /// Two central-directory names are one path after NFC and case-folding.
    /// Found before extraction, so a case-sensitive disk rejects it too.
    DuplicateEntry {
        first: String,
        second: String,
    },
    Zip(String),
}

impl ImportFailure {
    fn diagnostic(&self, pack: &str) -> Option<Diagnostic> {
        let (file, message) = match self {
            ImportFailure::Io(error) => (String::new(), format!("import failed: {error}")),
            ImportFailure::NotZip => (String::new(), "source is not a zip archive".to_string()),
            ImportFailure::BadName => (String::new(), "pack name is empty or unsafe".to_string()),
            ImportFailure::UnsafePath { entry, reason } => (
                entry.clone(),
                format!("zip entry escapes the pack ({reason}): {entry}"),
            ),
            ImportFailure::Symlink { path } => (path.clone(), format!("symlink: {path}")),
            ImportFailure::Special { path, kind } => (path.clone(), format!("{kind}: {path}")),
            ImportFailure::TooManyEntries { count } => (
                String::new(),
                format!("{count} entries exceeds the {MAX_ENTRIES} entry cap"),
            ),
            ImportFailure::TooDeep { path } => (
                path.clone(),
                format!("{path} is deeper than {MAX_DEPTH} components"),
            ),
            ImportFailure::DeclaredSize { bytes } => (
                String::new(),
                format!(
                    "declared uncompressed size {bytes} exceeds {MAX_UNCOMPRESSED_BYTES} before extraction"
                ),
            ),
            ImportFailure::ActualSize { bytes } => (
                String::new(),
                format!("uncompressed size {bytes} exceeds {MAX_UNCOMPRESSED_BYTES}"),
            ),
            ImportFailure::Compression { entry, method } => (
                entry.clone(),
                format!("{entry} uses {method}; only stored and deflate are accepted"),
            ),
            ImportFailure::Encrypted { entry } => (entry.clone(), format!("{entry} is encrypted")),
            ImportFailure::ImageDimensions {
                path,
                width,
                height,
            } => (
                path.clone(),
                format!(
                    "{path} is {width}x{height}, over the {MAX_IMAGE_EDGE}x{MAX_IMAGE_EDGE} cap"
                ),
            ),
            ImportFailure::DecodeBudget {
                path,
                decoded_images,
                used_bytes,
                next_bytes,
            } => (
                path.clone(),
                format!(
                    "{path} would decode to {next_bytes} bytes after {used_bytes} already decoded ({decoded_images} images)"
                ),
            ),
            ImportFailure::Collision { id } => {
                (String::new(), format!("pack {id} is already installed"))
            }
            ImportFailure::DuplicateEntry { first, second } => (
                second.clone(),
                format!("duplicate entry after normalisation: {first} and {second}"),
            ),
            ImportFailure::Zip(message) => (String::new(), message.clone()),
        };
        Some(Diagnostic::new(
            Severity::Error,
            DiagnosticKind::Other,
            pack,
            file,
            None,
            message,
        ))
    }

    pub fn is_fifo(&self) -> bool {
        matches!(self, ImportFailure::Special { kind: "fifo", .. })
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(diagnostic) = self.diagnostics.first() {
            write!(f, "{}", diagnostic.message)
        } else {
            write!(f, "import failed")
        }
    }
}

impl std::error::Error for ImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.failure {
            ImportFailure::Io(error) => Some(error),
            _ => None,
        }
    }
}

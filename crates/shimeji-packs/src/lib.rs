//! Pack loaders and export. Dependencies of this crate stay pure Rust.
//!
//! [`load_dir`] reads a directory that is already on disk. [`import_pack`] is
//! the entry point for untrusted input: it stages a zip or a folder, enforces
//! the size caps, rejects links and special files, decodes PNG and WAV in
//! Rust, then moves the copy into `packs/<id>/`.

mod capabilities;
mod codex;
mod export;
mod images;
mod import;
mod layout;
mod native;
mod sounds;
mod staging;
pub mod standard_atlas;
mod xml;

use std::path::Path;

pub use capabilities::{PoseCapability, pose_capabilities};
pub use codex::CODEX_DISPLAY_SCALE;
pub use export::{CodexExport, MissingRow, export_classic, export_codex};
pub use images::DecodedImage;
pub use import::{
    Collision, CollisionPolicy, ImportError, ImportFailure, ImportOutcome, MAX_DECODED_BYTES,
    MAX_DEPTH, MAX_ENTRIES, MAX_IMAGE_EDGE, MAX_UNCOMPRESSED_BYTES, import_pack,
};
pub use native::write_standard_pack;
pub use shimeji_core::diagnostics::{Diagnostic, DiagnosticKind, Severity};
pub use shimeji_core::model;
pub use sounds::DecodedWav;

/// A pack and every diagnostic recorded while reading it.
#[derive(Debug)]
pub struct LoadOutcome {
    pub pack: model::Pack,
    pub diagnostics: Vec<Diagnostic>,
    /// Renderer scale. Codex pets are `Some(0.62)`. Classic packs are `None`.
    pub display_scale: Option<f64>,
}

/// Load `path` as a pack directory.
///
/// Returns an error when `path` itself cannot be read. A pack that fails to
/// parse still returns [`Ok`]: the outcome carries the diagnostics, and
/// `pack.characters` is empty when nothing usable was found.
pub fn load_dir(path: impl AsRef<Path>) -> std::io::Result<LoadOutcome> {
    layout::load_dir(path.as_ref())
}

/// Where a pack directory came from. Pixel size wins over a Codex version field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackOrigin {
    CodexV1,
    CodexV2,
    Native,
}

/// `pack.json` is native. `pet.json` plus a Codex-sized sheet is Codex.
/// Anything else is not one of these formats.
pub fn directory_origin(path: impl AsRef<Path>) -> Option<PackOrigin> {
    let path = path.as_ref();
    if native::is_native_dir(path) {
        return Some(PackOrigin::Native);
    }
    codex::origin(path)
}

/// Crate name. Dependents call this so the workspace link is real.
pub fn crate_name() -> &'static str {
    let _linked = shimeji_core::crate_name();
    "shimeji-packs"
}

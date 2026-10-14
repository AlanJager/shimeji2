//! Caps shared by zip and folder import.
//!
//! Directory depth is 16 components under the pack root. Classic and Shimeji-ee
//! layouts are at most `img/<set>/conf/<file>` (4). Sixteen is past any pack in
//! the corpus and still far under `PATH_MAX`, so a nest of `a/a/a/…` is rejected
//! before it can blow the stack or the path limit.

/// Uncompressed file bytes, summed while copying. Declared zip sizes are checked
/// against the same number before extraction.
pub const MAX_UNCOMPRESSED_BYTES: u64 = 200 * 1024 * 1024;

/// Files and directories, not counting the pack root itself.
pub const MAX_ENTRIES: u64 = 5_000;

/// Path components under the pack root. A file at `a/b/c.png` has depth 3.
pub const MAX_DEPTH: usize = 16;

/// Each edge of a PNG, in pixels. Checked on the IHDR before the decoder runs.
pub const MAX_IMAGE_EDGE: u32 = 4096;

/// RGBA and WAV PCM bytes for one pack. Checked before each decode.
pub const MAX_DECODED_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Default)]
pub(crate) struct CopyBudget {
    pub bytes: u64,
    pub entries: u64,
}

impl CopyBudget {
    pub(crate) fn add_entry(&mut self) -> Result<(), super::error::ImportError> {
        let next = self.entries.saturating_add(1);
        if next > MAX_ENTRIES {
            return Err(super::error::ImportError::entries(next));
        }
        self.entries = next;
        Ok(())
    }

    /// `declared` is the central-directory uncompressed size. Nothing is extracted
    /// when this fails.
    pub(crate) fn add_declared(&mut self, n: u64) -> Result<(), super::error::ImportError> {
        let next = self.bytes.saturating_add(n);
        if next > MAX_UNCOMPRESSED_BYTES {
            return Err(super::error::ImportError::declared_size(next));
        }
        self.bytes = next;
        Ok(())
    }
}

/// Bytes actually written while extracting or copying. Independent of the
/// declared-size counter: a zip can lie about `uncompressed_size`.
#[derive(Debug, Default)]
pub(crate) struct OutputBudget {
    pub bytes: u64,
}

impl OutputBudget {
    pub(crate) fn add(&mut self, n: u64) -> Result<(), super::error::ImportError> {
        let next = self.bytes.saturating_add(n);
        if next > MAX_UNCOMPRESSED_BYTES {
            return Err(super::error::ImportError::actual_size(next));
        }
        self.bytes = next;
        Ok(())
    }
}

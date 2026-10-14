//! One pending forge install. Confirming is the only path that imports.

use std::path::Path;

use shimeji_proto::InstallRequest;

use crate::import::{self, Outcome};

/// A second request arrived while one was still unanswered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Busy;

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("an install request is already pending")
    }
}

/// Holds at most one install request. [`Self::offer`] does not import.
#[derive(Debug, Default)]
pub struct ForgeInstall {
    pending: Option<InstallRequest>,
}

impl ForgeInstall {
    pub fn new() -> Self {
        Self { pending: None }
    }

    pub fn pending(&self) -> Option<&InstallRequest> {
        self.pending.as_ref()
    }

    /// The confirmation line. The buttons are Install and Ignore.
    pub fn prompt(request: &InstallRequest) -> String {
        format!(
            "forge wants to install {} from {}: Install / Ignore",
            request.name, request.path
        )
    }

    /// Remember `request`. A second call while one is pending returns [`Busy`]
    /// and leaves the first request in place.
    pub fn offer(&mut self, request: InstallRequest) -> Result<(), Busy> {
        if self.pending.is_some() {
            return Err(Busy);
        }
        self.pending = Some(request);
        Ok(())
    }

    pub fn ignore(&mut self) {
        self.pending = None;
    }

    /// Import the pending pack. `None` means nothing was pending.
    pub fn confirm(&mut self, packs_root: &Path) -> Option<Outcome> {
        let request = self.pending.take()?;
        Some(import::import(Path::new(&request.path), packs_root, None))
    }
}

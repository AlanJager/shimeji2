//! Settings, library, window rules, journal, agent state, and runtime.

pub mod agent;
pub mod first_run;
pub mod import;
pub mod journal;
pub mod library;
pub mod mischief_state;
pub mod runtime;
pub mod settings;
pub mod window_rules;

pub use runtime::{Command, Frame, Runtime};

/// Crate name. Dependents call this so the workspace link is real.
pub fn crate_name() -> &'static str {
    let _core = shimeji_core::crate_name();
    let _packs = shimeji_packs::crate_name();
    let _proto = shimeji_proto::crate_name();
    "shimeji-app"
}

//! Agent reactions that do not need AppKit: state, pose names, coverage, install.

mod coverage;
mod forge_install;
mod install;
mod pose_map;
mod state;

pub use coverage::{AgentCoverage, coverage, drives};
pub use forge_install::{Busy, ForgeInstall};
pub use install::{
    FileDigest, Freshness, InstallError, InstallPlan, MARKER, apply_install, apply_uninstall,
    freshness, location_block, plan_install, plan_uninstall, shell_command,
};
pub use pose_map::{PoseChoice, resolve};
pub use state::{AgentTracker, DONE_FOR_MS, STALE_AFTER_MS};

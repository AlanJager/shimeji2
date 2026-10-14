//! Window mischief on the Mac shell.
//!
//! The queue, the carry gate, and the worker are platform-free behind traits.
//! Accessibility and AppKit stay in `ax` and `session`.

mod ax_worker;
mod controller;
mod permission;

#[cfg(target_os = "macos")]
mod ax;
#[cfg(target_os = "macos")]
mod session;

pub use ax_worker::{
    AxBackend, CarryTracker, Cmd, Decision, Ended, JournalPort, MoveMiss, MoveQueue, PendingMove,
    TargetView,
};
pub use controller::{Service, WindowHandle};
pub use permission::{
    ACCESSIBILITY_PANE, MenuLook, TRUST_INTERVAL, enabled_from_env, menu_item, phase_label,
    phase_words, poll_due, trust_on_activate,
};

#[cfg(target_os = "macos")]
pub use permission::open_accessibility_pane;
#[cfg(target_os = "macos")]
pub use session::Session;

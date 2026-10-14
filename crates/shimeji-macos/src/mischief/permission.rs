//! When to ask `AXIsProcessTrusted`, and the pane the settings UI will open.
//!
//! The check itself is local: it is not an AX element message, so the clock
//! thread may call it. Element messages stay on the worker.

use std::time::Duration;

use shimeji_app::mischief_state::Phase;

/// How often a running clock re-reads the grant while the phase polls.
pub const TRUST_INTERVAL: Duration = Duration::from_secs(1);

/// System Settings → Privacy & Security → Accessibility.
pub const ACCESSIBILITY_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

/// `SHIMEJI_MISCHIEF=1` after trimming. Anything else, including unset, is off.
pub fn enabled_from_env(value: Option<&str>) -> bool {
    value.is_some_and(shimeji_app::settings::mischief_from_env)
}

/// How the status-menu row looks for one phase.
///
/// Checked means the user has mischief switched on, so choosing the row turns
/// it off. [`Phase::PermissionMissing`] stays checked: the grant is gone, and
/// the switch is still on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuLook {
    pub title: &'static str,
    pub checked: bool,
}

pub fn menu_item(phase: Phase) -> MenuLook {
    match phase {
        Phase::Off => MenuLook {
            title: "Mischief",
            checked: false,
        },
        Phase::On => MenuLook {
            title: "Mischief",
            checked: true,
        },
        Phase::AwaitingPermission => MenuLook {
            title: "Mischief (waiting for Accessibility)",
            checked: true,
        },
        Phase::PermissionMissing => MenuLook {
            title: "Mischief (permission missing)",
            checked: true,
        },
    }
}

/// The Mischief tab's phase line, in words.
pub fn phase_words(phase: Phase) -> &'static str {
    match phase {
        Phase::Off => "Off",
        Phase::AwaitingPermission => "Waiting for Accessibility",
        Phase::On => "On",
        Phase::PermissionMissing => "Permission missing",
    }
}

/// Periodic poll. Off and [`Phase::PermissionMissing`] do not poll, and a
/// stopped clock does not either.
pub fn poll_due(phase: Phase, clock_on: bool, since: Duration) -> bool {
    clock_on && phase.polls() && since >= TRUST_INTERVAL
}

/// One extra check when this app becomes active, including from
/// [`Phase::PermissionMissing`]. Off stays off: the user has not asked.
pub fn trust_on_activate(phase: Phase) -> bool {
    phase != Phase::Off
}

pub fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Off => "off",
        Phase::AwaitingPermission => "awaiting",
        Phase::On => "on",
        Phase::PermissionMissing => "missing",
    }
}

/// One process-wide grant check. Not an element message.
pub trait ProcessTrust {
    fn is_trusted(&self) -> bool;
}

#[cfg(target_os = "macos")]
pub struct AxTrust;

#[cfg(target_os = "macos")]
impl ProcessTrust for AxTrust {
    fn is_trusted(&self) -> bool {
        unsafe { objc2_application_services::AXIsProcessTrusted() }
    }
}

/// Open the Accessibility pane. U11c calls this; U11b only provides it.
#[cfg(target_os = "macos")]
pub fn open_accessibility_pane() {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    let url = NSString::from_str(ACCESSIBILITY_PANE);
    let Some(url) = NSURL::URLWithString(&url) else {
        return;
    };
    let _ = NSWorkspace::sharedWorkspace().openURL(&url);
}

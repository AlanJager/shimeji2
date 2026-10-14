//! A badge click activates `host`. When that app is not running, activate the
//! terminal that was frontmost most recently.

use std::sync::Mutex;

use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
use objc2_foundation::NSString;

use crate::ui::lossy;

const TERMINALS: &[&str] = &[
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "dev.warp.Warp-Stable",
    "com.github.wez.wezterm",
    "net.kovidgoyal.kitty",
    "com.mitchellh.ghostty",
    "org.alacritty",
];

static LAST_TERMINAL: Mutex<Option<String>> = Mutex::new(None);

/// Remember the frontmost app when it is a terminal. Called from the workspace
/// activation notification and once at launch.
pub(crate) fn note_frontmost() {
    let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
        return;
    };
    let Some(bundle) = app.bundleIdentifier() else {
        return;
    };
    let id = lossy(&bundle);
    if TERMINALS.contains(&id.as_str()) {
        *last_terminal() = Some(id);
    }
}

/// `host` is a bundle id or a short token (`iTerm.app`, `Apple_Terminal`).
pub(crate) fn activate(host: Option<&str>) {
    if let Some(host) = host {
        if activate_host(host) {
            return;
        }
    }
    if let Some(bundle) = last_terminal().clone() {
        let _ = activate_bundle(&bundle);
    }
}

fn activate_host(host: &str) -> bool {
    if host.contains('.') {
        return activate_bundle(host);
    }
    let Some(bundle) = token_bundle(host) else {
        return false;
    };
    activate_bundle(bundle)
}

fn token_bundle(token: &str) -> Option<&'static str> {
    match token {
        "Terminal" | "Apple_Terminal" => Some("com.apple.Terminal"),
        "iTerm" | "iTerm.app" => Some("com.googlecode.iterm2"),
        "Warp" => Some("dev.warp.Warp-Stable"),
        "WezTerm" => Some("com.github.wez.wezterm"),
        "kitty" => Some("net.kovidgoyal.kitty"),
        "ghostty" => Some("com.mitchellh.ghostty"),
        "Alacritty" => Some("org.alacritty"),
        _ => None,
    }
}

fn activate_bundle(bundle: &str) -> bool {
    let apps =
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle));
    for app in apps.iter() {
        if app.isTerminated() {
            continue;
        }
        if activate_app(&app) {
            return true;
        }
    }
    false
}

#[allow(deprecated)]
fn activate_app(app: &NSRunningApplication) -> bool {
    app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps)
}

fn last_terminal() -> std::sync::MutexGuard<'static, Option<String>> {
    LAST_TERMINAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

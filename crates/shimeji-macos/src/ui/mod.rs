//! Menu bar, settings window, import, and first run.
//!
//! AppKit stays here. The decisions it shows live in `shimeji-app`.

pub(crate) mod actions;
pub(crate) mod first_run;
pub(crate) mod import_flow;
pub(crate) mod mischief_prompt;
pub(crate) mod settings;
pub(crate) mod status_menu;

pub use actions::install_shell;
pub use first_run::show_popover;

use objc2::msg_send;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

pub(crate) fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect {
        origin: NSPoint { x, y },
        size: NSSize { width, height },
    }
}

pub(crate) fn lossy(text: &NSString) -> String {
    unsafe {
        let ptr: *const std::ffi::c_char = msg_send![text, UTF8String];
        if ptr.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

//! The explanation while Accessibility is missing, and the warning when it is lost.
//!
//! One window at a time. It is a sheet on Settings when that window is open,
//! and a small window otherwise.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSButton, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::NSString;
use shimeji_app::mischief_state::Phase;
use shimeji_app::window_rules::IMMUTABLE_APPS;

use crate::bridge::ShellClock;

use super::rect;

pub const TAG_OPEN: isize = 1;
pub const TAG_NOT_NOW: isize = 2;

pub struct Prompt {
    pub window: Retained<NSWindow>,
    pub parent: Option<Retained<NSWindow>>,
    pub kind: Phase,
}

pub fn sync(clock: &ShellClock) {
    let phase = clock.ivars().state.borrow().mischief.phase();
    let want = if phase.shows_explanation() {
        Some(Phase::AwaitingPermission)
    } else if phase.warns_open_settings() {
        Some(Phase::PermissionMissing)
    } else {
        None
    };
    let current = clock
        .ivars()
        .mischief_prompt
        .borrow()
        .as_ref()
        .map(|prompt| prompt.kind);
    if current == want {
        return;
    }
    close(clock);
    if let Some(kind) = want {
        if let Some(prompt) = present(clock, kind) {
            *clock.ivars().mischief_prompt.borrow_mut() = Some(prompt);
        }
    }
}

fn close(clock: &ShellClock) {
    let Some(prompt) = clock.ivars().mischief_prompt.borrow_mut().take() else {
        return;
    };
    if let Some(parent) = &prompt.parent {
        parent.endSheet(&prompt.window);
    }
    prompt.window.orderOut(None);
}

fn present(clock: &ShellClock, kind: Phase) -> Option<Prompt> {
    let mtm = MainThreadMarker::new()?;
    let parent: Option<Retained<NSWindow>> = clock
        .ivars()
        .settings_ui
        .borrow()
        .as_ref()
        .filter(|ui| ui.window.isVisible())
        .map(|ui| ui.window.clone());

    let explain = kind == Phase::AwaitingPermission;
    let title = if explain {
        "Window mischief"
    } else {
        "Accessibility permission is missing"
    };
    let body = if explain {
        explanation()
    } else {
        "Accessibility permission was turned off. A window that was being carried is put back. Open Accessibility Settings to grant it again."
            .to_string()
    };

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect(0.0, 0.0, 460.0, 260.0),
            NSWindowStyleMask::Titled,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    window.setTitle(&NSString::from_str(title));
    unsafe { window.setReleasedWhenClosed(false) };

    let content = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 460.0, 260.0));
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(&body), mtm);
    label.setFrame(rect(20.0, 64.0, 420.0, 180.0));
    content.addSubview(&label);

    let target = std::ptr::from_ref(clock).cast::<AnyObject>();
    let target = unsafe { &*target };
    let open = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("Open Accessibility Settings"),
            Some(target),
            Some(sel!(mischiefPromptAct:)),
            mtm,
        )
    };
    open.setFrame(rect(20.0, 16.0, 230.0, 32.0));
    open.setTag(TAG_OPEN);
    content.addSubview(&open);
    if explain {
        let later = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Not now"),
                Some(target),
                Some(sel!(mischiefPromptAct:)),
                mtm,
            )
        };
        later.setFrame(rect(260.0, 16.0, 120.0, 32.0));
        later.setTag(TAG_NOT_NOW);
        content.addSubview(&later);
    }
    window.setContentView(Some(&content));

    if let Some(parent) = parent {
        // No completion work: closing the sheet is `endSheet` from `close`.
        unsafe {
            let _: () = objc2::msg_send![
                &*parent,
                beginSheet: &*window,
                completionHandler: std::ptr::null::<AnyObject>()
            ];
        }
        Some(Prompt {
            window,
            parent: Some(parent),
            kind,
        })
    } else {
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        window.center();
        window.makeKeyAndOrderFront(None);
        Some(Prompt {
            window,
            parent: None,
            kind,
        })
    }
}

fn explanation() -> String {
    let never = IMMUTABLE_APPS.join(", ");
    format!(
        "Pets may pick up and throw the frontmost window. Windows are put back when you quit.\n\n\
         These apps are never touched: {never}.\n\n\
         Mischief needs Accessibility permission.\n\n\
         After this app is rebuilt, macOS treats it as a new app, so the Accessibility checkbox must be ticked again."
    )
}

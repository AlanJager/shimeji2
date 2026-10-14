//! One-time popover on the status item, and launch-at-login.
//!
//! The popover is marked seen when it is shown. The checkbox starts off unless
//! `SMAppService` is already enabled. An unsigned dev binary may fail to
//! register; the error is printed and the process keeps running.

use std::cell::Cell;
use std::panic::AssertUnwindSafe;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSButton, NSControlStateValueOff, NSControlStateValueOn,
    NSPopover, NSPopoverBehavior, NSTextField, NSView, NSViewController,
};
use objc2_foundation::{NSNotification, NSRectEdge, NSSize, NSString};
use objc2_service_management::{SMAppService, SMAppServiceStatus, kSMErrorAlreadyRegistered};
use shimeji_app::first_run;
use shimeji_core::env::Point;

use crate::bridge::ShellClock;
use crate::platform::coords::{self, CocoaSpace, Rect};

use super::{lossy, rect};

pub struct AppDelegateIvars {
    clock: Cell<*const ShellClock>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = AppDelegateIvars]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn application_should_handle_reopen(
            &self,
            _sender: &NSApplication,
            _has_visible_windows: bool,
        ) -> bool {
            let ptr = self.ivars().clock.get();
            if !ptr.is_null() {
                println!("reopen=settings");
                super::actions::show_settings(unsafe { &*ptr });
            }
            true
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn application_will_terminate(&self, _notification: &NSNotification) {
            let ptr = self.ivars().clock.get();
            if !ptr.is_null() {
                unsafe { &*ptr }.shutdown_mischief();
            }
        }
    }
);

impl AppDelegate {
    pub fn install(clock: &ShellClock) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars {
            clock: Cell::new(std::ptr::from_ref(clock)),
        });
        let delegate: Retained<Self> = unsafe { msg_send![super(this), init] };
        let app = NSApplication::sharedApplication(mtm);
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        *clock.ivars().delegate.borrow_mut() = Some(delegate);
    }
}

pub fn launch_enabled() -> bool {
    let service = unsafe { SMAppService::mainAppService() };
    let status = unsafe { service.status() };
    status == SMAppServiceStatus::Enabled
}

pub fn set_launch_at_login(on: bool) -> Result<(), String> {
    let service = unsafe { SMAppService::mainAppService() };
    let result = if on {
        unsafe { service.registerAndReturnError() }
    } else {
        unsafe { service.unregisterAndReturnError() }
    };
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            if on && error.code() == kSMErrorAlreadyRegistered as isize {
                return Ok(());
            }
            let status = unsafe { service.status() };
            if on && status == SMAppServiceStatus::Enabled {
                return Ok(());
            }
            if !on
                && matches!(
                    status,
                    SMAppServiceStatus::NotRegistered | SMAppServiceStatus::NotFound
                )
            {
                return Ok(());
            }
            Err(format!(
                "{} ({})",
                lossy(&error.localizedDescription()),
                error.code()
            ))
        }
    }
}

/// Work area of the primary display (frame origin at `(0, 0)`), else the first screen.
pub fn primary_work(space: &CocoaSpace) -> Option<Rect> {
    let screen = space
        .screens
        .iter()
        .find(|screen| screen.frame.x == 0 && screen.frame.y == 0)
        .or_else(|| space.screens.first())?;
    Some(coords::work_area(screen, space.primary_height))
}

/// Work-area foot point on the primary display (menu bar at the top, Dock inset).
pub fn main_floor(space: &CocoaSpace) -> Option<Point> {
    let work = primary_work(space)?;
    let (x, y) = first_run::floor_anchor(work.x, work.right(), work.bottom());
    Some(Point::new(x, y))
}

pub fn show_popover(clock: &ShellClock, mention_animate_anyway: bool, hotkey: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let popover = NSPopover::new(mtm);
    popover.setBehavior(NSPopoverBehavior::Transient);
    let view = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 320.0, 168.0));
    let body = if mention_animate_anyway {
        format!(
            "Hide and show pets with {hotkey}.\n\nReduce Motion is on, so this pet is standing on the floor. Turn on Animate anyway in Settings → Advanced to let it move."
        )
    } else {
        format!(
            "Hide and show pets with {hotkey}.\n\nThe menu bar icon opens Characters, Pause, and Settings."
        )
    };
    let label = NSTextField::labelWithString(&NSString::from_str(&body), mtm);
    label.setFrame(rect(16.0, 48.0, 288.0, 108.0));
    let target = std::ptr::from_ref(clock).cast::<AnyObject>();
    let check = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str("Launch at login"),
            Some(&*target),
            Some(sel!(loginAct:)),
            mtm,
        )
    };
    check.setFrame(rect(16.0, 12.0, 288.0, 24.0));
    let enabled = launch_enabled();
    check.setState(if enabled {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    println!(
        "launch_at_login={}",
        if enabled { "enabled" } else { "off" }
    );
    view.addSubview(&label);
    view.addSubview(&check);
    let controller =
        NSViewController::initWithNibName_bundle(NSViewController::alloc(mtm), None, None);
    controller.setView(&view);
    popover.setContentViewController(Some(&controller));
    popover.setContentSize(NSSize {
        width: 320.0,
        height: 168.0,
    });

    let button = clock
        .ivars()
        .status_item
        .borrow()
        .as_ref()
        .and_then(|item| item.button(mtm));
    let Some(button) = button else {
        println!("first_run_popover=no-button");
        return;
    };
    let bounds = button.bounds();
    let shown = objc2::exception::catch(AssertUnwindSafe(|| {
        popover.showRelativeToRect_ofView_preferredEdge(bounds, &button, NSRectEdge::MinY);
    }));
    match shown {
        Ok(()) => {
            let dir = clock.ivars().state.borrow().support_dir.clone();
            if let Err(error) = first_run::mark_seen(&dir) {
                eprintln!("first_run_seen={error}");
            }
            println!("first_run_popover=shown");
            *clock.ivars().popover.borrow_mut() = Some(popover);
        }
        Err(error) => {
            let reason = error
                .and_then(|exc| {
                    let text: Option<Retained<NSString>> = unsafe { msg_send![&*exc, reason] };
                    text.map(|text| lossy(&text))
                })
                .unwrap_or_else(|| "exception".to_string());
            eprintln!("first_run_popover={reason}");
        }
    }
}

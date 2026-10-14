//! A small non-activating panel parked on the reacting pet.
//!
//! Waiting and failed are larger and saturated. Every other shown state keeps
//! a quieter chip. The panel is not the pet's slack window: it follows the
//! sprite rectangle.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSButton, NSColor, NSFloatingWindowLevel, NSFont, NSPanel, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use shimeji_proto::AgentState;

use crate::platform::coords::Rect;

pub(crate) struct Badge {
    window: Retained<NSPanel>,
    button: Retained<NSButton>,
    last: Option<(Rect, AgentState)>,
}

impl Badge {
    pub(crate) fn new(mtm: MainThreadMarker, target: &AnyObject) -> Self {
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle;
        let window = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            chip_rect(0.0, 0.0, false),
            style,
            objc2_app_kit::NSBackingStoreType::Buffered,
            false,
        );
        unsafe { window.setReleasedWhenClosed(false) };
        window.setLevel(NSFloatingWindowLevel);
        window.setFloatingPanel(true);
        window.setHidesOnDeactivate(false);
        window.setBecomesKeyOnlyIfNeeded(true);
        window.setCollectionBehavior(behavior);
        window.setHasShadow(false);
        window.setOpaque(true);

        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("idle"),
                Some(target),
                Some(sel!(agentBadge:)),
                mtm,
            )
        };
        button.setBordered(false);
        button.setFrame(chip_rect(0.0, 0.0, false));
        window.setContentView(Some(&button));

        Self {
            window,
            button,
            last: None,
        }
    }

    pub(crate) fn show(&mut self, sprite: Rect, state: AgentState) {
        if self.last == Some((sprite, state)) {
            return;
        }
        let prominent = matches!(state, AgentState::Waiting | AgentState::Failed);
        let size = if prominent {
            (128.0, 32.0)
        } else {
            (78.0, 22.0)
        };
        let origin_x = f64::from(sprite.x);
        let origin_y = f64::from(sprite.y + sprite.height + 4);
        self.window
            .setFrame_display(chip_rect(origin_x, origin_y, prominent), true);
        self.button.setFrame(NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: size.0,
                height: size.1,
            },
        });
        let (fill, ink) = colors(state);
        self.window.setBackgroundColor(Some(&fill));
        self.button.setContentTintColor(Some(&ink));
        self.button
            .setFont(Some(&NSFont::boldSystemFontOfSize(if prominent {
                13.0
            } else {
                11.0
            })));
        self.button.setTitle(&NSString::from_str(state_word(state)));
        self.window.orderFrontRegardless();
        self.last = Some((sprite, state));
    }

    pub(crate) fn hide(&mut self) {
        self.last = None;
        self.window.orderOut(None);
    }
}

fn chip_rect(x: f64, y: f64, prominent: bool) -> NSRect {
    let (width, height) = if prominent {
        (128.0, 32.0)
    } else {
        (78.0, 22.0)
    };
    NSRect {
        origin: NSPoint { x, y },
        size: NSSize { width, height },
    }
}

fn colors(state: AgentState) -> (Retained<NSColor>, Retained<NSColor>) {
    let (red, green, blue, ink_white) = match state {
        AgentState::Failed => (0.86, 0.22, 0.18, true),
        AgentState::Waiting => (0.95, 0.62, 0.12, false),
        _ => (0.16, 0.17, 0.20, true),
    };
    let fill = NSColor::colorWithRed_green_blue_alpha(red, green, blue, 1.0);
    let ink = if ink_white {
        NSColor::whiteColor()
    } else {
        NSColor::blackColor()
    };
    (fill, ink)
}

pub(crate) fn state_word(state: AgentState) -> &'static str {
    match state {
        AgentState::Idle => "idle",
        AgentState::Working => "working",
        AgentState::Review => "review",
        AgentState::Waiting => "waiting",
        AgentState::Failed => "failed",
        AgentState::Done => "done",
    }
}

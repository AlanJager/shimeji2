//! Optional passive patience card centered over the companion's head.
use super::rect;
use crate::platform::coords::Rect;
use crate::platform::focus_layout::{CARD_HEIGHT, CARD_WIDTH, head_card};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBox, NSBoxType, NSColor, NSFloatingWindowLevel, NSFont, NSPanel,
    NSScreen, NSTextField, NSTitlePosition, NSView, NSWindowAnimationBehavior,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSString};
use shimeji_app::pomodoro::Patience;

const TRACK_WIDTH: f64 = 200.0;
const TRACK_HEIGHT: f64 = 6.0;

pub(crate) struct Badge {
    window: Retained<NSPanel>,
    label: Retained<NSTextField>,
    fill: Retained<NSBox>,
    last: Option<(Rect, Patience)>,
    visible: bool,
    work_areas: Vec<Rect>,
}
impl Badge {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let size = rect(0.0, 0.0, f64::from(CARD_WIDTH), f64::from(CARD_HEIGHT));
        let window = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            size,
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        unsafe { window.setReleasedWhenClosed(false) };
        window.setLevel(NSFloatingWindowLevel);
        window.setFloatingPanel(true);
        window.setHidesOnDeactivate(false);
        window.setIgnoresMouseEvents(true);
        window.setAnimationBehavior(NSWindowAnimationBehavior::None);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(true);
        let content = NSView::initWithFrame(NSView::alloc(mtm), size);
        let background = color_box(mtm, size, &NSColor::controlBackgroundColor(), 8.0);
        content.addSubview(&background);
        let label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        label.setFrame(rect(12.0, 21.0, TRACK_WIDTH, 17.0));
        label.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            11.0, 0.2,
        )));
        label.setTextColor(Some(&NSColor::labelColor()));
        content.addSubview(&label);
        let track_frame = rect(12.0, 9.0, TRACK_WIDTH, TRACK_HEIGHT);
        let track = color_box(mtm, track_frame, &NSColor::separatorColor(), 3.0);
        content.addSubview(&track);
        let fill = color_box(mtm, track_frame, &NSColor::systemGreenColor(), 3.0);
        content.addSubview(&fill);
        window.setContentView(Some(&content));
        Self {
            window,
            label,
            fill,
            last: None,
            visible: false,
            work_areas: Vec::new(),
        }
    }

    pub(crate) fn show(&mut self, sprite: Rect, patience: &Patience) {
        let work_areas: Vec<_> = MainThreadMarker::new()
            .map(|mtm| {
                NSScreen::screens(mtm)
                    .iter()
                    .map(|screen| {
                        let work = screen.visibleFrame();
                        Rect::new(
                            work.origin.x.round() as i32,
                            work.origin.y.round() as i32,
                            work.size.width.round() as i32,
                            work.size.height.round() as i32,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.work_areas = work_areas;
        let placement = head_card(sprite, &self.work_areas);
        if self.visible
            && self
                .last
                .as_ref()
                .is_some_and(|(last_rect, last)| *last_rect == placement && last == patience)
        {
            return;
        }
        self.window.setFrame_display(
            rect(
                f64::from(placement.x),
                f64::from(placement.y),
                f64::from(placement.width),
                f64::from(placement.height),
            ),
            true,
        );
        self.label
            .setStringValue(&NSString::from_str(&patience.label));
        let [red, green, blue] = patience.color.rgb();
        self.fill
            .setFillColor(&NSColor::colorWithRed_green_blue_alpha(
                f64::from(red) / 255.0,
                f64::from(green) / 255.0,
                f64::from(blue) / 255.0,
                1.0,
            ));
        let width = TRACK_WIDTH * f64::from(patience.per_mille.min(1000)) / 1000.0;
        self.fill.setFrame(rect(12.0, 9.0, width, TRACK_HEIGHT));
        self.fill.setHidden(patience.per_mille == 0);
        self.window.orderFrontRegardless();
        self.visible = true;
        self.last = Some((placement, patience.clone()));
    }

    pub(crate) fn has_visible(&self) -> bool {
        self.visible
    }

    /// Animation callbacks move or reshow the cached card. The model, header,
    /// colors and fill are still refreshed by the existing 1 Hz focus timer.
    pub(crate) fn follow(&mut self, sprite: Rect) {
        let Some((last_rect, _)) = self.last.as_mut() else {
            return;
        };
        let placement = head_card(sprite, &self.work_areas);
        if *last_rect != placement {
            self.window
                .setFrameOrigin(NSPoint::new(f64::from(placement.x), f64::from(placement.y)));
            *last_rect = placement;
        }
        if !self.visible {
            self.window.orderFrontRegardless();
            self.visible = true;
        }
    }

    pub(crate) fn hide(&mut self) {
        if self.visible {
            self.window.orderOut(None);
            self.visible = false;
        }
    }
}

fn color_box(
    mtm: MainThreadMarker,
    frame: objc2_foundation::NSRect,
    color: &NSColor,
    radius: f64,
) -> Retained<NSBox> {
    let view = NSBox::initWithFrame(NSBox::alloc(mtm), frame);
    view.setBoxType(NSBoxType::Custom);
    view.setTitlePosition(NSTitlePosition::NoTitle);
    view.setBorderWidth(0.0);
    view.setCornerRadius(radius);
    view.setFillColor(color);
    view
}

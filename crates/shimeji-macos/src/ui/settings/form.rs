//! Shared native form layout. A flipped document keeps the first row visible.
use crate::ui::rect;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAutoresizingMaskOptions as Resize, NSBox, NSBoxType, NSButton, NSColor, NSControl, NSFont,
    NSSwitch, NSTextField, NSTitlePosition, NSView,
};
use objc2_foundation::{NSSize, NSString};

pub const WIDTH: f64 = 640.0;

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    pub struct FormView;
    impl FormView {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool { true }
    }
    unsafe impl NSObjectProtocol for FormView {}
);

pub fn view(mtm: MainThreadMarker, width: f64, height: f64) -> Retained<NSView> {
    let this = FormView::alloc(mtm);
    let view: Retained<FormView> =
        unsafe { msg_send![this, initWithFrame: rect(0.0, 0.0, width, height)] };
    view.setAutoresizingMask(Resize::ViewWidthSizable);
    unsafe { Retained::cast_unchecked(view) }
}

pub fn text(
    parent: &NSView,
    title: &str,
    frame: objc2_foundation::NSRect,
    size: f64,
    secondary: bool,
    bold: bool,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&NSString::from_str(title), mtm);
    field.setFrame(frame);
    field.setFont(Some(&NSFont::systemFontOfSize_weight(
        size,
        if bold { 0.23 } else { 0.0 },
    )));
    let color = if secondary {
        NSColor::secondaryLabelColor()
    } else {
        NSColor::labelColor()
    };
    field.setTextColor(Some(&color));
    field.setAutoresizingMask(Resize::ViewWidthSizable);
    parent.addSubview(&field);
    field
}

pub fn page(mtm: MainThreadMarker, title: &str, subtitle: &str, height: f64) -> Retained<NSView> {
    let page = view(mtm, WIDTH, height);
    text(
        &page,
        title,
        rect(28.0, 28.0, WIDTH - 56.0, 34.0),
        25.0,
        false,
        true,
        mtm,
    );
    text(
        &page,
        subtitle,
        rect(28.0, 70.0, WIDTH - 56.0, 38.0),
        12.0,
        true,
        false,
        mtm,
    );
    page
}

pub fn box_view(
    parent: &NSView,
    frame: objc2_foundation::NSRect,
    color: &NSColor,
    mtm: MainThreadMarker,
) -> Retained<NSBox> {
    let box_view = NSBox::initWithFrame(NSBox::alloc(mtm), frame);
    box_view.setBoxType(NSBoxType::Custom);
    box_view.setTitlePosition(NSTitlePosition::NoTitle);
    box_view.setCornerRadius(10.0);
    box_view.setBorderWidth(0.0);
    box_view.setFillColor(color);
    box_view.setAutoresizingMask(Resize::ViewWidthSizable);
    parent.addSubview(&box_view);
    box_view
}

pub fn section(
    parent: &NSView,
    top: f64,
    title: &str,
    height: f64,
    mtm: MainThreadMarker,
) -> Retained<NSView> {
    text(
        parent,
        title,
        rect(28.0, top, WIDTH - 56.0, 22.0),
        13.0,
        true,
        true,
        mtm,
    );
    let box_view = box_view(
        parent,
        rect(24.0, top + 30.0, WIDTH - 48.0, height),
        &NSColor::controlBackgroundColor(),
        mtm,
    );
    box_view.setContentViewMargins(NSSize::new(0.0, 0.0));
    let content = view(mtm, WIDTH - 48.0, height);
    content.setAutoresizingMask(Resize::ViewWidthSizable | Resize::ViewHeightSizable);
    box_view.setContentView(Some(&content));
    content
}

pub fn row_labels(
    parent: &NSView,
    y: f64,
    title: &str,
    hint: &str,
    reserve: f64,
    mtm: MainThreadMarker,
) {
    let width = parent.bounds().size.width - 40.0 - reserve;
    text(
        parent,
        title,
        rect(18.0, y + 10.0, width, 22.0),
        13.0,
        false,
        false,
        mtm,
    );
    if !hint.is_empty() {
        text(
            parent,
            hint,
            rect(18.0, y + 33.0, width, 28.0),
            11.0,
            true,
            false,
            mtm,
        );
    }
}

pub fn accessible(control: &NSControl, title: &str) {
    let label = NSString::from_str(title);
    unsafe {
        let _: () = msg_send![control, setAccessibilityLabel: &*label];
    }
}

pub fn toggle(
    parent: &NSView,
    y: f64,
    label: (&str, &str),
    tag: isize,
    target: &AnyObject,
    action: Sel,
    mtm: MainThreadMarker,
) -> Retained<NSSwitch> {
    let (title, hint) = label;
    row_labels(parent, y, title, hint, 70.0, mtm);
    let toggle = NSSwitch::initWithFrame(
        NSSwitch::alloc(mtm),
        rect(parent.bounds().size.width - 64.0, y + 17.0, 42.0, 26.0),
    );
    toggle.setTag(tag);
    unsafe {
        toggle.setTarget(Some(target));
        toggle.setAction(Some(action));
    }
    toggle.setAutoresizingMask(Resize::ViewMinXMargin);
    accessible(&toggle, title);
    parent.addSubview(&toggle);
    toggle
}

pub fn button(
    parent: &NSView,
    title: &str,
    frame: objc2_foundation::NSRect,
    tag: isize,
    target: &AnyObject,
    action: Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    };
    button.setFrame(frame);
    button.setTag(tag);
    parent.addSubview(&button);
    button
}

//! Appearance and behavior preferences, using the existing action tags.
use super::form;
use crate::bridge::ShellClock;
use crate::ui::rect;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions as Resize, NSControl, NSPopUpButton, NSSlider, NSTextField, NSView,
};
use objc2_foundation::NSString;

pub struct BehaviorTab {
    pub view: Retained<NSView>,
    pub controls: Vec<Retained<NSControl>>,
    pub values: Vec<(isize, Retained<NSTextField>)>,
}

pub fn build(mtm: MainThreadMarker, clock: &ShellClock) -> BehaviorTab {
    let view = form::page(
        mtm,
        "外观与动作",
        "调整大小和活动方式，让桌宠适合你的桌面。",
        950.0,
    );
    let target = unsafe { &*std::ptr::from_ref(clock).cast::<AnyObject>() };
    let mut controls = Vec::new();
    let mut values = Vec::new();
    let appearance = form::section(&view, 114.0, "外观", 226.0, mtm);
    for (tag, title, y, min, max) in [
        (1, "宠物大小", 0.0, 0.25, 8.0),
        (3, "不透明度", 148.0, 0.0, 1.0),
    ] {
        form::text(
            &appearance,
            title,
            rect(18.0, y + 10.0, 350.0, 22.0),
            13.0,
            false,
            false,
            mtm,
        );
        let value = form::text(
            &appearance,
            "100%",
            rect(appearance.bounds().size.width - 90.0, y + 10.0, 70.0, 22.0),
            13.0,
            true,
            false,
            mtm,
        );
        value.setAutoresizingMask(Resize::ViewMinXMargin);
        values.push((tag, value));
        let slider = unsafe {
            NSSlider::sliderWithTarget_action(Some(target), Some(sel!(settingsAct:)), mtm)
        };
        slider.setFrame(rect(
            18.0,
            y + 39.0,
            appearance.bounds().size.width - 36.0,
            24.0,
        ));
        slider.setMinValue(min);
        slider.setMaxValue(max);
        slider.setTag(tag);
        slider.setContinuous(true);
        slider.setAutoresizingMask(Resize::ViewWidthSizable);
        form::accessible(&slider, title);
        appearance.addSubview(&slider);
        controls.push(unsafe { Retained::cast_unchecked(slider) });
    }
    form::row_labels(
        &appearance,
        76.0,
        "缩放方式",
        "像素清晰适合像素画，柔和缩放适合插画。",
        250.0,
        mtm,
    );
    let filter = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(appearance.bounds().size.width - 226.0, 93.0, 208.0, 28.0),
        false,
    );
    for title in ["像素清晰", "柔和缩放", "HQX（暂不可用）"] {
        filter.addItemWithTitle(&NSString::from_str(title));
    }
    if let Some(menu) = filter.menu() {
        menu.setAutoenablesItems(false);
    }
    if let Some(item) = filter.itemAtIndex(2) {
        item.setEnabled(false);
    }
    filter.setTag(2);
    filter.setAutoresizingMask(Resize::ViewMinXMargin);
    unsafe {
        filter.setTarget(Some(target));
        filter.setAction(Some(sel!(settingsAct:)));
    }
    form::accessible(&filter, "缩放方式");
    appearance.addSubview(&filter);
    controls.push(unsafe { Retained::cast_unchecked(filter) });
    let movement = form::section(&view, 394.0, "移动与互动", 192.0, mtm);
    for (tag, title, hint, y) in [
        (
            14,
            "保持在屏幕内",
            "碰到边缘时停住或抓住边缘，避免越界消失。",
            0.0,
        ),
        (
            9,
            "跨显示器活动",
            "允许宠物在连接的多个显示器之间移动。",
            64.0,
        ),
        (7, "允许抛掷", "拖住宠物后松手，可以把它甩出去。", 128.0),
    ] {
        let t = form::toggle(
            &movement,
            y,
            (title, hint),
            tag,
            target,
            sel!(settingsAct:),
            mtm,
        );
        controls.push(unsafe { Retained::cast_unchecked(t) });
    }
    let population = form::section(&view, 640.0, "数量与变化", 256.0, mtm);
    form::row_labels(
        &population,
        0.0,
        "宠物数量上限",
        "限制同时在桌面活动的宠物数量。",
        120.0,
        mtm,
    );
    let cap = NSTextField::textFieldWithString(&NSString::from_str("1"), mtm);
    cap.setFrame(rect(
        population.bounds().size.width - 100.0,
        18.0,
        80.0,
        26.0,
    ));
    cap.setTag(10);
    cap.setAutoresizingMask(Resize::ViewMinXMargin);
    form::accessible(&cap, "宠物数量上限");
    unsafe {
        cap.setTarget(Some(target));
        cap.setAction(Some(sel!(settingsAct:)));
    }
    population.addSubview(&cap);
    controls.push(unsafe { Retained::cast_unchecked(cap) });
    for (tag, title, hint, y) in [
        (4, "自动繁殖", "允许支持此动作的角色产生新的宠物。", 64.0),
        (5, "临时分身", "允许角色动作产生短暂存在的分身。", 128.0),
        (6, "角色变换", "此版本暂未开放。", 192.0),
    ] {
        let t = form::toggle(
            &population,
            y,
            (title, hint),
            tag,
            target,
            sel!(settingsAct:),
            mtm,
        );
        if tag == 6 {
            t.setEnabled(false);
        }
        controls.push(unsafe { Retained::cast_unchecked(t) });
    }
    BehaviorTab {
        view,
        controls,
        values,
    }
}

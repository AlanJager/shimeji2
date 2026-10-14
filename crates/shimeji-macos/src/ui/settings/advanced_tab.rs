//! Keyboard, sound, accessibility and diagnostics preferences.
use super::form;
use crate::bridge::ShellClock;
use crate::ui::rect;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSAutoresizingMaskOptions as Resize, NSControl, NSTextField, NSView};
use objc2_foundation::NSString;

pub fn build(
    mtm: MainThreadMarker,
    clock: &ShellClock,
) -> (Retained<NSView>, Vec<Retained<NSControl>>) {
    let view = form::page(mtm, "高级", "快捷键、辅助功能与问题排查。", 650.0);
    let target = unsafe { &*std::ptr::from_ref(clock).cast::<AnyObject>() };
    let mut controls = Vec::new();
    let keys = form::section(&view, 114.0, "快捷键", 106.0, mtm);
    form::row_labels(
        &keys,
        0.0,
        "隐藏 / 显示所有宠物",
        "修改后按 Return 生效。",
        260.0,
        mtm,
    );
    let field = NSTextField::textFieldWithString(&NSString::from_str("ctrl+shift+s"), mtm);
    field.setFrame(rect(keys.bounds().size.width - 248.0, 17.0, 228.0, 26.0));
    field.setTag(11);
    field.setAutoresizingMask(Resize::ViewMinXMargin);
    form::accessible(&field, "隐藏与显示宠物的快捷键");
    unsafe {
        field.setTarget(Some(target));
        field.setAction(Some(sel!(settingsAct:)));
    }
    keys.addSubview(&field);
    controls.push(unsafe { Retained::cast_unchecked(field) });
    form::text(
        &keys,
        "使用 Control 或 Command 组合键，例如 ctrl+shift+s。",
        rect(18.0, 70.0, 550.0, 24.0),
        11.0,
        true,
        false,
        mtm,
    );
    let access = form::section(&view, 280.0, "声音与辅助功能", 128.0, mtm);
    for (tag, title, hint, y) in [
        (8, "播放动作声音", "只播放角色素材包包含的声音。", 0.0),
        (
            12,
            "减少动态效果时继续动画",
            "在系统开启“减少动态效果”后，仍让桌宠活动。",
            64.0,
        ),
    ] {
        let toggle = form::toggle(
            &access,
            y,
            (title, hint),
            tag,
            target,
            sel!(settingsAct:),
            mtm,
        );
        controls.push(unsafe { Retained::cast_unchecked(toggle) });
    }
    let diagnostics = form::section(&view, 466.0, "问题排查", 78.0, mtm);
    form::row_labels(
        &diagnostics,
        5.0,
        "诊断信息",
        "查看素材加载和运行状态，便于报告问题。",
        160.0,
        mtm,
    );
    let button = form::button(
        &diagnostics,
        "查看诊断…",
        rect(diagnostics.bounds().size.width - 144.0, 24.0, 124.0, 28.0),
        13,
        target,
        sel!(settingsAct:),
        mtm,
    );
    button.setAutoresizingMask(Resize::ViewMinXMargin);
    (view, controls)
}

//! Mischief tab: the same switch as the status menu, plus the rule lists.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions as Resize, NSButton, NSPopUpButton, NSSwitch, NSTextField, NSView,
};
use objc2_foundation::NSString;
use shimeji_app::mischief_state::Phase;
use shimeji_app::window_rules::{DEFAULT_BLOCKED_APPS, WindowRules};

use super::form;
use crate::bridge::ShellClock;

use super::super::rect;

pub const TAG_ENABLE: isize = 100;
pub const TAG_DEFAULT: isize = 101;
pub const TAG_ADD: isize = 120;
pub const TAG_REMOVE: isize = 121;

pub struct MischiefTab {
    pub view: Retained<NSView>,
    pub enable: Retained<NSSwitch>,
    pub phase: Retained<NSTextField>,
    pub defaults: Vec<(String, Retained<NSButton>)>,
    pub rules: Retained<NSPopUpButton>,
    pub remove: Retained<NSButton>,
    pub pattern: Retained<NSTextField>,
    pub target: Retained<NSPopUpButton>,
    pub effect: Retained<NSPopUpButton>,
}

pub fn build(mtm: MainThreadMarker, clock: &ShellClock) -> MischiefTab {
    let view = form::page(
        mtm,
        "窗口互动",
        "让宠物和前台窗口互动，并决定哪些应用可以被搬动。",
        840.0,
    );
    let target = unsafe { &*std::ptr::from_ref(clock).cast::<AnyObject>() };
    let master = form::section(&view, 114.0, "恶作剧", 108.0, mtm);
    let enable = form::toggle(
        &master,
        0.0,
        (
            "允许宠物搬动窗口",
            "需要辅助功能权限，开启后会显示授权指引。",
        ),
        TAG_ENABLE,
        target,
        sel!(mischiefAct:),
        mtm,
    );
    let phase = form::text(
        &master,
        "状态：已关闭",
        rect(18.0, 74.0, 550.0, 22.0),
        12.0,
        true,
        false,
        mtm,
    );
    let protection = form::section(&view, 278.0, "默认保护的应用", 158.0, mtm);
    let mut defaults = Vec::new();
    for (index, name) in DEFAULT_BLOCKED_APPS.iter().enumerate() {
        let display = match *name {
            "Keychain Access" => "钥匙串访问",
            "Terminal" => "终端",
            other => other,
        };
        let check = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &NSString::from_str(&format!("允许 {display}")),
                Some(target),
                Some(sel!(mischiefAct:)),
                mtm,
            )
        };
        check.setFrame(rect(
            18.0 + index as f64 % 2.0 * 286.0,
            18.0 + (index / 2) as f64 * 34.0,
            250.0,
            24.0,
        ));
        check.setTag(TAG_DEFAULT + index as isize);
        protection.addSubview(&check);
        defaults.push(((*name).to_string(), check));
    }
    form::text(
        &protection,
        "这些应用默认不会被搬动。系统登录、授权和安全设置窗口始终受到保护。",
        rect(18.0, 94.0, 550.0, 44.0),
        11.0,
        true,
        false,
        mtm,
    );
    let custom = form::section(&view, 492.0, "自定义窗口规则", 178.0, mtm);
    let rules = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(18.0, 14.0, 430.0, 28.0),
        false,
    );
    rules.addItemWithTitle(&NSString::from_str("尚未添加规则"));
    rules.setAutoresizingMask(Resize::ViewWidthSizable);
    custom.addSubview(&rules);
    form::accessible(&rules, "已有窗口规则");
    let remove = form::button(
        &custom,
        "移除",
        rect(462.0, 14.0, 108.0, 28.0),
        TAG_REMOVE,
        target,
        sel!(mischiefAct:),
        mtm,
    );
    remove.setAutoresizingMask(Resize::ViewMinXMargin);
    remove.setEnabled(false);
    let pattern = NSTextField::textFieldWithString(&NSString::from_str(""), mtm);
    pattern.setFrame(rect(18.0, 59.0, 552.0, 28.0));
    pattern.setEditable(true);
    pattern.setPlaceholderString(Some(&NSString::from_str("输入应用名或窗口标题")));
    pattern.setAutoresizingMask(Resize::ViewWidthSizable);
    form::accessible(&pattern, "规则匹配文字");
    custom.addSubview(&pattern);
    let kind = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(18.0, 100.0, 158.0, 28.0),
        false,
    );
    for t in ["匹配应用名", "匹配窗口标题"] {
        kind.addItemWithTitle(&NSString::from_str(t));
    }
    form::accessible(&kind, "匹配范围");
    custom.addSubview(&kind);
    let effect = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(186.0, 100.0, 158.0, 28.0),
        false,
    );
    for t in ["允许搬动", "禁止搬动"] {
        effect.addItemWithTitle(&NSString::from_str(t));
    }
    form::accessible(&effect, "规则效果");
    custom.addSubview(&effect);
    let add = form::button(
        &custom,
        "添加规则",
        rect(462.0, 100.0, 108.0, 28.0),
        TAG_ADD,
        target,
        sel!(mischiefAct:),
        mtm,
    );
    add.setAutoresizingMask(Resize::ViewMinXMargin);
    form::text(
        &custom,
        "按包含文字匹配；窗口标题规则需要辅助功能权限。",
        rect(18.0, 144.0, 550.0, 22.0),
        11.0,
        true,
        false,
        mtm,
    );
    form::text(
        &view,
        "添加“允许”规则后，只会搬动匹配这些规则的窗口。",
        rect(28.0, 724.0, 584.0, 44.0),
        12.0,
        true,
        false,
        mtm,
    );

    MischiefTab {
        view,
        enable,
        phase,
        defaults,
        rules,
        remove,
        pattern,
        target: kind,
        effect,
    }
}

pub fn status_line(phase: Phase) -> String {
    match phase {
        Phase::Off => "状态：已关闭",
        Phase::AwaitingPermission => "状态：等待辅助功能授权",
        Phase::On => "状态：已开启，可以搬动前台窗口",
        Phase::PermissionMissing => "状态：辅助功能权限已失效，请重新授权",
    }
    .to_string()
}

pub fn fill_rules(popup: &NSPopUpButton, remove: &NSButton, rules: &WindowRules) {
    popup.removeAllItems();
    if rules.rules.is_empty() {
        popup.addItemWithTitle(&NSString::from_str("尚未添加规则"));
        popup.setEnabled(false);
        remove.setEnabled(false);
        return;
    }
    popup.setEnabled(true);
    remove.setEnabled(true);
    for rule in &rules.rules {
        let effect = if rule.effect == shimeji_app::window_rules::Effect::Allow {
            "允许"
        } else {
            "禁止"
        };
        let target = if rule.target == shimeji_app::window_rules::Target::App {
            "应用"
        } else {
            "标题"
        };
        popup.addItemWithTitle(&NSString::from_str(&format!(
            "{effect} · {target}包含“{}”",
            rule.text
        )));
    }
}

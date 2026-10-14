//! Agents tab: the switch, which pet reacts, coverage, listener status, and
//! the install review. Apply is a second click. This code does not write the
//! real hook files until that click.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions as Resize, NSButton, NSControlStateValue, NSControlStateValueOn,
    NSFont, NSPopUpButton, NSScrollView, NSSwitch, NSTextField, NSTextView, NSView,
};
use objc2_foundation::NSString;
use shimeji_app::agent::{
    InstallError, InstallPlan, apply_install, apply_uninstall, plan_install, plan_uninstall,
};

use super::form;
use crate::agent::react::{self, coverage_text, hooks_sentence, listener_sentence, review_text};
use crate::bridge::ShellClock;
use crate::ui::rect;
use objc2_foundation::NSSize;

pub const TAG_ENABLE: isize = 40;
pub const TAG_PET: isize = 41;
pub const TAG_INSTALL: isize = 42;
pub const TAG_REPAIR: isize = 43;
pub const TAG_UNINSTALL: isize = 44;
pub const TAG_APPLY: isize = 45;

enum Pending {
    Install(InstallPlan),
    Uninstall(InstallPlan),
}

pub struct AgentsTab {
    pub view: Retained<NSView>,
    pub enable: Retained<NSSwitch>,
    pub pet: Retained<NSPopUpButton>,
    pub keys: RefCell<Vec<String>>,
    pub listener: Retained<NSTextField>,
    pub hooks: Retained<NSTextField>,
    pub review: Retained<NSTextView>,
    pub apply: Retained<NSButton>,
    plan: RefCell<Option<Pending>>,
}

pub fn build(mtm: MainThreadMarker, clock: &ShellClock) -> AgentsTab {
    let view = form::page(
        mtm,
        "智能体",
        "让桌宠随 Claude Code 和 Codex 的任务状态作出反应。",
        920.0,
    );
    let target = unsafe { &*std::ptr::from_ref(clock).cast::<AnyObject>() };
    let general = form::section(&view, 114.0, "任务反应", 158.0, mtm);
    let enable = form::toggle(
        &general,
        0.0,
        ("启用任务反应", "需要安装事件钩子；安装前会先展示更改内容。"),
        TAG_ENABLE,
        target,
        sel!(settingsAct:),
        mtm,
    );
    form::row_labels(
        &general,
        72.0,
        "响应角色",
        "选择由哪一个宠物响应任务。",
        310.0,
        mtm,
    );
    let pet = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        rect(284.0, 90.0, 286.0, 28.0),
        false,
    );
    pet.setTag(TAG_PET);
    pet.setAutoresizingMask(Resize::ViewMinXMargin);
    form::accessible(&pet, "响应任务的角色");
    unsafe {
        pet.setTarget(Some(target));
        pet.setAction(Some(sel!(settingsAct:)));
    }
    general.addSubview(&pet);
    let connection = form::section(&view, 328.0, "连接与安装", 176.0, mtm);
    let coverage = coverage_text()
        .replace("working", "工作中")
        .replace("waiting", "等待输入")
        .replace("failed", "失败")
        .replace("done", "完成")
        .replace(
            "Review is ranked. Neither agent emits it.",
            "暂未接入代码审查事件。",
        );
    form::text(
        &connection,
        &coverage,
        rect(18.0, 12.0, 550.0, 64.0),
        11.0,
        true,
        false,
        mtm,
    );
    let listener = form::text(
        &connection,
        "任务监听已关闭",
        rect(18.0, 79.0, 550.0, 20.0),
        12.0,
        false,
        false,
        mtm,
    );
    let hooks = form::text(
        &connection,
        "事件钩子尚未安装",
        rect(18.0, 103.0, 550.0, 20.0),
        12.0,
        true,
        false,
        mtm,
    );
    for (title, tag, x) in [
        ("安装…", TAG_INSTALL, 18.0),
        ("修复…", TAG_REPAIR, 152.0),
        ("卸载…", TAG_UNINSTALL, 286.0),
    ] {
        form::button(
            &connection,
            title,
            rect(x, 137.0, 122.0, 28.0),
            tag,
            target,
            sel!(settingsAct:),
            mtm,
        );
    }
    let review_group = form::section(&view, 558.0, "更改预览", 250.0, mtm);
    let scroll =
        NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(12.0, 12.0, 568.0, 224.0));
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setAutoresizingMask(Resize::ViewWidthSizable);
    let review = NSTextView::initWithFrame(NSTextView::alloc(mtm), rect(0.0, 0.0, 568.0, 224.0));
    review.setEditable(false);
    review.setSelectable(true);
    review.setRichText(false);
    review.setVerticallyResizable(true);
    review.setHorizontallyResizable(false);
    review.setMinSize(NSSize::new(0.0, 224.0));
    review.setMaxSize(NSSize::new(f64::MAX, f64::MAX));
    review.setAutoresizingMask(Resize::ViewWidthSizable);
    review.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(11.0, 0.0)));
    review.setString(&NSString::from_str(
        "点击安装、修复或卸载，先查看具体更改，再确认应用。",
    ));
    if let Some(container) = unsafe { review.textContainer() } {
        container.setWidthTracksTextView(true);
    }
    scroll.setDocumentView(Some(&review));
    review_group.addSubview(&scroll);
    let apply = form::button(
        &view,
        "应用更改",
        rect(24.0, 850.0, 132.0, 30.0),
        TAG_APPLY,
        target,
        sel!(settingsAct:),
        mtm,
    );
    apply.setEnabled(false);

    AgentsTab {
        view,
        enable,
        pet,
        keys: RefCell::new(Vec::new()),
        listener,
        hooks,
        review,
        apply,
        plan: RefCell::new(None),
    }
}

pub fn act(clock: &ShellClock, sender: &AnyObject, tag: isize) {
    match tag {
        TAG_ENABLE => toggle(clock, sender),
        TAG_PET => choose_pet(clock),
        TAG_INSTALL | TAG_REPAIR => stage_install(clock),
        TAG_UNINSTALL => stage_uninstall(clock),
        TAG_APPLY => apply_staged(clock),
        _ => {}
    }
}

pub fn refresh(clock: &ShellClock) {
    let ui = clock.ivars().settings_ui.borrow();
    let Some(ui) = ui.as_ref() else {
        return;
    };
    ui.agents.sync(clock);
}

impl AgentsTab {
    /// Fill the pet list and the status lines. Does not touch `settings_ui`.
    pub fn sync(&self, clock: &ShellClock) {
        let (rows, selected, on, status) = {
            let state = clock.ivars().state.borrow();
            let mut rows = Vec::new();
            for pack in state.catalog.packs() {
                for character in &pack.characters {
                    rows.push((
                        format!("{} — {}", pack.id, character.name),
                        format!("{}/{}", pack.id, character.name),
                    ));
                }
            }
            let selected = if state.settings.agent_character.is_empty() {
                format!("{}/{}", state.pack_id, state.character_name)
            } else {
                state.settings.agent_character.clone()
            };
            (
                rows,
                selected,
                state.settings.agents,
                state.agent.status.clone(),
            )
        };
        self.fill(rows, selected);
        set_check(&self.enable, on);
        let listener = match status.as_str() {
            "off" => "任务监听已关闭".to_string(),
            "listening" => "任务监听已连接".to_string(),
            _ => listener_sentence(&status),
        };
        self.listener.setStringValue(&NSString::from_str(&listener));
        let hooks = match react::hooks_word() {
            "current" => "事件钩子已安装".to_string(),
            "out_of_date" => "事件钩子需要更新，可点击修复".to_string(),
            "not_installed" => "事件钩子尚未安装".to_string(),
            other => hooks_sentence(other),
        };
        self.hooks.setStringValue(&NSString::from_str(&hooks));
    }

    fn fill(&self, rows: Vec<(String, String)>, selected: String) {
        self.pet.removeAllItems();
        let mut keys = Vec::new();
        let mut select = 0isize;
        if rows.is_empty() {
            self.pet
                .addItemWithTitle(&NSString::from_str("暂无可选角色"));
        }
        for (index, (title, key)) in rows.iter().enumerate() {
            self.pet.addItemWithTitle(&NSString::from_str(title));
            if key == &selected {
                select = index as isize;
            }
            keys.push(key.clone());
        }
        if !keys.is_empty() {
            self.pet.selectItemAtIndex(select);
        }
        *self.keys.borrow_mut() = keys;
    }
}

fn toggle(clock: &ShellClock, sender: &AnyObject) {
    let on = control_on(sender);
    let (dir, stored) = {
        let mut state = clock.ivars().state.borrow_mut();
        state.settings.agents = on;
        (state.support_dir.clone(), state.settings.clone())
    };
    if let Err(error) = shimeji_app::settings::save(&dir, &stored) {
        eprintln!("settings_save={error}");
    }
    if on {
        react::start(clock);
    } else {
        react::stop(clock);
    }
    refresh(clock);
}

fn choose_pet(clock: &ShellClock) {
    let key = {
        let ui = clock.ivars().settings_ui.borrow();
        let Some(ui) = ui.as_ref() else {
            return;
        };
        let index = ui.agents.pet.indexOfSelectedItem();
        if index < 0 {
            return;
        }
        ui.agents.keys.borrow().get(index as usize).cloned()
    };
    let Some(key) = key else {
        return;
    };
    let (dir, stored) = {
        let mut state = clock.ivars().state.borrow_mut();
        state.settings.agent_character = key;
        (state.support_dir.clone(), state.settings.clone())
    };
    if let Err(error) = shimeji_app::settings::save(&dir, &stored) {
        eprintln!("settings_save={error}");
    }
}

fn stage_install(clock: &ShellClock) {
    let Some((claude, codex)) = react::config_paths() else {
        show_review(clock, "could not read the home directory", None);
        return;
    };
    let hook = react::hook_bin();
    match plan_install(&claude, &codex, &hook) {
        Ok(plan) => {
            let text = review_text(&plan.claude_diff, &plan.codex_diff);
            show_review(clock, &text, Some(Pending::Install(plan)));
        }
        Err(error) => show_review(clock, &error.to_string(), None),
    }
}

fn stage_uninstall(clock: &ShellClock) {
    let Some((claude, codex)) = react::config_paths() else {
        show_review(clock, "could not read the home directory", None);
        return;
    };
    match plan_uninstall(&claude, &codex) {
        Ok(plan) => {
            let text = review_text(&plan.claude_diff, &plan.codex_diff);
            show_review(clock, &text, Some(Pending::Uninstall(plan)));
        }
        Err(error) => show_review(clock, &error.to_string(), None),
    }
}

fn apply_staged(clock: &ShellClock) {
    let pending = {
        let ui = clock.ivars().settings_ui.borrow();
        let Some(ui) = ui.as_ref() else {
            return;
        };
        ui.agents.plan.borrow_mut().take()
    };
    let Some(pending) = pending else {
        return;
    };
    let Some((claude, codex)) = react::config_paths() else {
        show_review(clock, "could not read the home directory", None);
        return;
    };
    let outcome = match &pending {
        Pending::Install(plan) => apply_install(plan, &claude, &codex, &react::hook_bin()),
        Pending::Uninstall(plan) => apply_uninstall(plan, &claude, &codex),
    };
    match outcome {
        Ok(()) => {
            let text = match pending {
                Pending::Install(_) => "安装完成。",
                Pending::Uninstall(_) => "卸载完成。",
            };
            show_review(clock, text, None);
            refresh(clock);
        }
        Err(InstallError::Changed {
            claude_diff,
            codex_diff,
        }) => {
            let text = review_text(&claude_diff, &codex_diff);
            let fresh = match pending {
                Pending::Install(_) => plan_install(&claude, &codex, &react::hook_bin())
                    .ok()
                    .map(Pending::Install),
                Pending::Uninstall(_) => {
                    plan_uninstall(&claude, &codex).ok().map(Pending::Uninstall)
                }
            };
            show_review(clock, &text, fresh);
        }
        Err(error) => show_review(clock, &error.to_string(), None),
    }
}

fn show_review(clock: &ShellClock, text: &str, plan: Option<Pending>) {
    let ui = clock.ivars().settings_ui.borrow();
    let Some(ui) = ui.as_ref() else {
        return;
    };
    ui.agents.review.setString(&NSString::from_str(text));
    ui.agents.apply.setEnabled(plan.is_some());
    *ui.agents.plan.borrow_mut() = plan;
}

fn control_on(sender: &AnyObject) -> bool {
    let state: NSControlStateValue = unsafe { objc2::msg_send![sender, state] };
    state == NSControlStateValueOn
}

fn set_check(button: &NSSwitch, on: bool) {
    let state = if on {
        NSControlStateValueOn
    } else {
        objc2_app_kit::NSControlStateValueOff
    };
    unsafe {
        let _: () = objc2::msg_send![button, setState: state];
    }
}

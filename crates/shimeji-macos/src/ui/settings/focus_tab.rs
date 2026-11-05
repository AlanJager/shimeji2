//! Focus timer controls and persistent preferences.
use super::form;
use crate::bridge::ShellClock;
use crate::pomodoro;
use crate::ui::rect;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, sel};
use objc2_app_kit::{
    NSButton, NSColor, NSControl, NSControlStateValueOff, NSControlStateValueOn, NSTextField,
    NSView,
};
use objc2_foundation::NSString;
use shimeji_app::pomodoro::{Phase, Status};

pub struct FocusTab {
    pub view: Retained<NSView>,
    controls: Vec<Retained<NSControl>>,
    status: Retained<NSTextField>,
    rounds: Retained<NSTextField>,
    save_error: Retained<NSTextField>,
    start: Retained<NSButton>,
    pause: Retained<NSButton>,
    cancel: Retained<NSButton>,
}

pub fn build(mtm: MainThreadMarker, clock: &ShellClock) -> FocusTab {
    let view = form::page(
        mtm,
        "专注",
        "桌宠陪你专注。锁屏或休眠时自动暂停，回来后手动继续。",
        840.0,
    );
    let target = unsafe { &*std::ptr::from_ref(clock).cast::<AnyObject>() };
    let session = form::section(&view, 115.0, "这一轮", 150.0, mtm);
    let status = form::text(
        &session,
        "准备开始专注",
        rect(18.0, 12.0, 530.0, 28.0),
        20.0,
        false,
        true,
        mtm,
    );
    let rounds = form::text(
        &session,
        "已完成 0 轮",
        rect(18.0, 48.0, 530.0, 23.0),
        12.0,
        true,
        false,
        mtm,
    );
    let start = form::button(
        &session,
        "开始专注",
        rect(18.0, 96.0, 155.0, 30.0),
        pomodoro::START,
        target,
        sel!(focusAct:),
        mtm,
    );
    let pause = form::button(
        &session,
        "暂停",
        rect(189.0, 96.0, 155.0, 30.0),
        pomodoro::PAUSE,
        target,
        sel!(focusAct:),
        mtm,
    );
    let cancel = form::button(
        &session,
        "结束本轮",
        rect(360.0, 96.0, 155.0, 30.0),
        pomodoro::CANCEL,
        target,
        sel!(focusAct:),
        mtm,
    );
    let save_error = form::text(
        &view,
        "",
        rect(28.0, 299.0, 550.0, 24.0),
        12.0,
        false,
        false,
        mtm,
    );
    save_error.setTextColor(Some(&NSColor::systemRedColor()));
    let times = form::section(&view, 325.0, "时长", 195.0, mtm);
    let mut controls = Vec::new();
    for (index, (tag, title)) in [
        (pomodoro::FOCUS_MINUTES, "专注"),
        (pomodoro::SHORT_MINUTES, "短休息"),
        (pomodoro::LONG_MINUTES, "长休息"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = index as f64 * 64.0;
        form::row_labels(
            &times,
            y,
            title,
            "1–180 分钟；修改后按 Return，下阶段生效。",
            350.0,
            mtm,
        );
        let field = NSTextField::textFieldWithString(&NSString::from_str("25"), mtm);
        field.setFrame(rect(440.0, y + 20.0, 100.0, 26.0));
        field.setTag(tag);
        form::accessible(&field, &format!("{title}分钟数"));
        unsafe {
            field.setTarget(Some(target));
            field.setAction(Some(sel!(focusPreference:)));
        }
        times.addSubview(&field);
        controls.push(unsafe { Retained::cast_unchecked(field) });
    }
    let options = form::section(&view, 580.0, "提醒与陪伴", 195.0, mtm);
    for (index, (tag, title, hint)) in [
        (
            pomodoro::NOTIFICATIONS,
            "系统提醒",
            "首次启用时申请通知权限；拒绝后仍可正常计时。",
        ),
        (
            pomodoro::SOUND,
            "到点提示音",
            "独立于角色包声音，默认关闭。",
        ),
        (
            pomodoro::BADGE,
            "头顶耐心条与倒计时",
            "耐心随时间递减，绿→黄→橙→红；默认关闭。",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let toggle = form::toggle(
            &options,
            index as f64 * 64.0,
            (title, hint),
            tag,
            target,
            sel!(focusPreference:),
            mtm,
        );
        controls.push(unsafe { Retained::cast_unchecked(toggle) });
    }
    FocusTab {
        view,
        controls,
        status,
        rounds,
        save_error,
        start,
        pause,
        cancel,
    }
}

impl FocusTab {
    pub fn set_save_failed(&self, failed: bool) {
        self.save_error
            .setStringValue(&NSString::from_str(if failed {
                "偏好未能保存，已恢复原值。请检查配置目录权限后重试。"
            } else {
                ""
            }));
    }

    pub fn sync(&self, clock: &ShellClock) {
        let state = clock.ivars().state.borrow();
        let preferences = &state.settings.pomodoro;
        for control in &self.controls {
            match control.tag() {
                pomodoro::FOCUS_MINUTES => control
                    .setStringValue(&NSString::from_str(&preferences.focus_minutes.to_string())),
                pomodoro::SHORT_MINUTES => control.setStringValue(&NSString::from_str(
                    &preferences.short_break_minutes.to_string(),
                )),
                pomodoro::LONG_MINUTES => control.setStringValue(&NSString::from_str(
                    &preferences.long_break_minutes.to_string(),
                )),
                tag => {
                    let on = match tag {
                        pomodoro::NOTIFICATIONS => preferences.notifications,
                        pomodoro::SOUND => preferences.sound,
                        _ => preferences.show_badge,
                    };
                    let value = if on {
                        NSControlStateValueOn
                    } else {
                        NSControlStateValueOff
                    };
                    unsafe {
                        let _: () = objc2::msg_send![&**control, setState: value];
                    }
                }
            }
        }
        let time = state.origin.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        self.sync_status(
            &state.pomodoro.label(time),
            state.pomodoro.status(),
            state.pomodoro.can_resume(),
            state.pomodoro.interrupted(),
            state.pomodoro.next_phase(),
            state.pomodoro.completed_focus(),
        );
    }

    pub fn sync_status(
        &self,
        label: &str,
        status: Status,
        can_resume: bool,
        interrupted: bool,
        next: Phase,
        rounds: u32,
    ) {
        self.status.setStringValue(&NSString::from_str(label));
        self.rounds.setStringValue(&NSString::from_str(&format!(
            "本次已完成 {rounds} 轮专注 · 每 4 轮后长休息"
        )));
        self.start.setTitle(&NSString::from_str(match next {
            Phase::Focus => "开始专注",
            Phase::ShortBreak => "开始短休息",
            Phase::LongBreak => "开始长休息",
        }));
        self.start
            .setEnabled(matches!(status, Status::Idle | Status::Completed) && !interrupted);
        self.pause
            .setTitle(&NSString::from_str(if status == Status::Paused {
                "继续"
            } else {
                "暂停"
            }));
        self.pause
            .setEnabled(status == Status::Running || (status == Status::Paused && can_resume));
        self.cancel.setEnabled(status != Status::Idle);
    }
}

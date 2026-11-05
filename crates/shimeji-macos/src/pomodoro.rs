//! Native orchestration for the platform-free focus timer. Its timer never
//! depends on whether the mascot animation clock is running.

use std::ffi::c_void;

use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker};
use objc2_foundation::NSString;
use shimeji_app::pomodoro::{Interruption, Phase, Status};
use shimeji_app::settings;
use shimeji_core::MascotDelta;
use shimeji_core::env::Snapshot;

use crate::bridge::ShellClock;
use crate::platform::clock::GcdTimer;
use crate::platform::render::Renderer;
use crate::platform::sound;

pub const START: isize = 5_000;
pub const PAUSE: isize = 5_001;
pub const CANCEL: isize = 5_002;
pub const SETTINGS: isize = 5_003;
pub const STATUS: isize = 5_004;
pub const FOCUS_MINUTES: isize = 5_010;
pub const SHORT_MINUTES: isize = 5_011;
pub const LONG_MINUTES: isize = 5_012;
pub const NOTIFICATIONS: isize = 5_013;
pub const SOUND: isize = 5_014;
pub const BADGE: isize = 5_015;

pub fn now(clock: &ShellClock) -> u64 {
    clock
        .ivars()
        .state
        .borrow()
        .origin
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

pub fn act(clock: &ShellClock, tag: isize) -> bool {
    if ![START, PAUSE, CANCEL, SETTINGS].contains(&tag) {
        return false;
    }
    if tag == SETTINGS {
        crate::ui::actions::show_settings(clock);
        if let Some(ui) = clock.ivars().settings_ui.borrow().as_ref() {
            ui.select_page(4);
        }
        return true;
    }
    tick(clock);
    let time = now(clock);
    let mut authorize = false;
    {
        let mut state = clock.ivars().state.borrow_mut();
        match tag {
            START => {
                let preferences = state.settings.pomodoro.clone();
                authorize =
                    state.pomodoro.start_next(time, &preferences) && preferences.notifications;
            }
            PAUSE => {
                if state.pomodoro.running() {
                    state.pomodoro.pause(time);
                } else {
                    state.pomodoro.resume(time);
                }
            }
            CANCEL => state.pomodoro.cancel(),
            _ => {}
        }
    }
    if authorize {
        clock.ivars().focus_reminder.borrow_mut().authorize();
    }
    sync(clock);
    true
}

pub fn preference(clock: &ShellClock, sender: &AnyObject) {
    let tag: isize = unsafe { objc2::msg_send![sender, tag] };
    let (previous, stored) = {
        let mut state = clock.ivars().state.borrow_mut();
        let previous = state.settings.pomodoro.clone();
        let preferences = &mut state.settings.pomodoro;
        if [FOCUS_MINUTES, SHORT_MINUTES, LONG_MINUTES].contains(&tag) {
            let value: isize = unsafe { objc2::msg_send![sender, integerValue] };
            let value = value.clamp(1, 180) as u16;
            match tag {
                FOCUS_MINUTES => preferences.focus_minutes = value,
                SHORT_MINUTES => preferences.short_break_minutes = value,
                LONG_MINUTES => preferences.long_break_minutes = value,
                _ => {}
            }
        } else {
            let on: isize = unsafe { objc2::msg_send![sender, state] };
            match tag {
                NOTIFICATIONS => preferences.notifications = on != 0,
                SOUND => preferences.sound = on != 0,
                BADGE => preferences.show_badge = on != 0,
                _ => return,
            }
        }
        preferences.normalize();
        (
            previous,
            (state.support_dir.clone(), state.settings.clone()),
        )
    };
    let saved = match settings::save(&stored.0, &stored.1) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("pomodoro_settings_save={error}");
            clock.ivars().state.borrow_mut().settings.pomodoro = previous;
            false
        }
    };
    if let Some(ui) = clock.ivars().settings_ui.borrow().as_ref() {
        ui.focus.set_save_failed(!saved);
    }
    if saved && stored.1.pomodoro.notifications && tag == NOTIFICATIONS {
        clock.ivars().focus_reminder.borrow_mut().authorize();
    }
    sync(clock);
    if let Some(ui) = clock.ivars().settings_ui.borrow().as_ref() {
        ui.sync_from(clock);
    }
}

pub fn interrupt(clock: &ShellClock, reason: Interruption, active: bool) {
    tick(clock);
    let time = now(clock);
    clock
        .ivars()
        .state
        .borrow_mut()
        .pomodoro
        .set_interruption(reason, active, time);
    sync(clock);
}

pub fn tick(clock: &ShellClock) {
    let time = now(clock);
    let completion = clock.ivars().state.borrow_mut().pomodoro.update(time);
    if let Some(completion) = completion {
        let (notifications, sound_on) = {
            let state = clock.ivars().state.borrow();
            (
                state.settings.pomodoro.notifications,
                state.settings.pomodoro.sound,
            )
        };
        let message = if completion.phase == Phase::Focus {
            "这一轮完成了，休息一下吧。"
        } else {
            "休息结束，准备好再开始。"
        };
        println!("pomodoro_complete={:?}", completion.phase);
        if notifications {
            clock.ivars().focus_reminder.borrow_mut().deliver(message);
        }
        if sound_on {
            let frames = sound::to_float_frames(&sound::debug_tone(), 0.6);
            let mut state = clock.ivars().state.borrow_mut();
            let player = state.player.get_or_insert_with(sound::Player::new);
            if !player.play_frames(&frames) {
                eprintln!("pomodoro_sound=failed");
            }
        }
    }
    sync(clock);
}

pub fn sync(clock: &ShellClock) {
    let quiet = clock.ivars().state.borrow().pomodoro.quiet();
    let changed = {
        let mut state = clock.ivars().state.borrow_mut();
        let changed = state.power.companion_mode() != quiet;
        state.power.set_companion_mode(quiet);
        if changed {
            state.drag = None;
            for extra in &mut state.extras {
                extra.drag = None;
            }
            state.agent.pending_pose = None;
            state.agent.asked = None;
            state.agent.posed_for = None;
            if quiet {
                // set_enabled(false) uses the existing carry/restore shutdown
                // path but does not overwrite the user's saved preference.
                state.mischief.set_enabled(false);
            } else {
                let enabled = state.settings.mischief;
                state.mischief.set_enabled(enabled);
            }
        }
        changed
    };
    if changed {
        clock.sync_sources();
        clock.present_now();
    }
    let running = clock.ivars().state.borrow().pomodoro.running();
    let timer_on = clock.ivars().focus_timer.borrow().is_some();
    if running && !timer_on {
        let target = std::ptr::from_ref(clock).cast::<AnyObject>();
        *clock.ivars().focus_timer.borrow_mut() = Some(GcdTimer::start_interval(
            target,
            timer_fired,
            1_000_000_000,
            50_000_000,
        ));
    } else if !running && timer_on {
        clock.ivars().focus_timer.borrow_mut().take();
    }
    refresh(clock);
}

unsafe extern "C" fn timer_fired(context: *mut c_void) {
    let clock = unsafe { &*context.cast::<ShellClock>() };
    tick(clock);
}

pub fn apply_companion(state: &mut crate::bridge::AppState, snapshot: &Snapshot) {
    let quiet = state.pomodoro.quiet();
    let changed = state.runtime.companion_mode() != quiet;
    state.runtime.set_companion_mode(quiet, snapshot);
    if changed {
        let frames = state.runtime.frames();
        let deltas: Vec<_> = frames
            .iter()
            .map(|frame| MascotDelta {
                id: frame.id,
                anchor_changed: true,
                image_changed: true,
            })
            .collect();
        state.renderer.commit(&frames, &deltas);
    }
    for extra in &mut state.extras {
        let changed = extra.runtime.companion_mode() != quiet;
        extra.runtime.set_companion_mode(quiet, snapshot);
        if changed {
            let frames = extra.runtime.frames();
            let deltas: Vec<_> = frames
                .iter()
                .map(|frame| MascotDelta {
                    id: frame.id,
                    anchor_changed: true,
                    image_changed: true,
                })
                .collect();
            extra.renderer.commit(&frames, &deltas);
        }
    }
}

pub fn refresh(clock: &ShellClock) {
    let time = now(clock);
    let (label, status, paused, interrupted, next, rounds, show_badge, sprite, visible, patience) = {
        let state = clock.ivars().state.borrow();
        let sprite = state.renderer.first_sprite_cocoa().or_else(|| {
            state
                .extras
                .iter()
                .find_map(|extra| extra.renderer.first_sprite_cocoa())
        });
        (
            state.pomodoro.label(time),
            state.pomodoro.status(),
            state.pomodoro.can_resume(),
            state.pomodoro.interrupted(),
            state.pomodoro.next_phase(),
            state.pomodoro.completed_focus(),
            state.settings.pomodoro.show_badge,
            sprite,
            state.power.pets_visible() && !state.power.screen_unavailable(),
            state.pomodoro.patience(time),
        )
    };
    let title = if status == Status::Idle {
        "し".to_string()
    } else {
        label.clone()
    };
    if let Some(item) = clock.ivars().status_item.borrow().as_ref() {
        if let Some(mtm) = MainThreadMarker::new() {
            if let Some(button) = item.button(mtm) {
                if button.title().to_string() != title {
                    button.setTitle(&NSString::from_str(&title));
                }
            }
        }
    }
    if let Some(menu) = clock.ivars().status_menu.borrow().as_ref() {
        if let Some(item) = menu.itemWithTag(STATUS) {
            item.setTitle(&NSString::from_str(&label));
        }
        if let Some(item) = menu.itemWithTag(START) {
            item.setTitle(&NSString::from_str(start_title(next)));
            item.setEnabled(matches!(status, Status::Idle | Status::Completed) && !interrupted);
        }
        if let Some(item) = menu.itemWithTag(PAUSE) {
            item.setTitle(&NSString::from_str(if status == Status::Paused {
                "继续计时"
            } else {
                "暂停计时"
            }));
            item.setEnabled(status == Status::Running || paused);
        }
        if let Some(item) = menu.itemWithTag(CANCEL) {
            item.setEnabled(status != Status::Idle);
        }
    }
    if let Some(ui) = clock.ivars().settings_ui.borrow().as_ref() {
        ui.focus
            .sync_status(&label, status, paused, interrupted, next, rounds);
    }
    let mut badge = clock.ivars().focus_badge.borrow_mut();
    if show_badge && status != Status::Idle && visible {
        if let (Some(sprite), Some(mtm), Some(patience)) =
            (sprite, MainThreadMarker::new(), patience)
        {
            if badge.is_none() {
                *badge = Some(crate::ui::focus_badge::Badge::new(mtm));
            }
            if let Some(badge) = badge.as_mut() {
                badge.show(sprite, &patience);
            }
            return;
        }
    }
    if let Some(badge) = badge.as_mut() {
        badge.hide();
    }
}

pub fn add_menu(
    clock: &ShellClock,
    menu: &objc2_app_kit::NSMenu,
    mtm: MainThreadMarker,
    target: &AnyObject,
) {
    use crate::ui::status_menu::row;
    let time = now(clock);
    let state = clock.ivars().state.borrow();
    let timer = &state.pomodoro;
    menu.addItem(&row(mtm, &timer.label(time), Some(STATUS), false, target));
    let start_title = start_title(timer.next_phase());
    menu.addItem(&row(
        mtm,
        start_title,
        Some(START),
        matches!(timer.status(), Status::Idle | Status::Completed) && !timer.interrupted(),
        target,
    ));
    menu.addItem(&row(
        mtm,
        if timer.status() == Status::Paused {
            "继续计时"
        } else {
            "暂停计时"
        },
        Some(PAUSE),
        timer.running() || timer.can_resume(),
        target,
    ));
    menu.addItem(&row(
        mtm,
        "结束本轮",
        Some(CANCEL),
        timer.status() != Status::Idle,
        target,
    ));
    menu.addItem(&row(mtm, "专注设置…", Some(SETTINGS), true, target));
    menu.addItem(&objc2_app_kit::NSMenuItem::separatorItem(mtm));
}

fn start_title(phase: Phase) -> &'static str {
    match phase {
        Phase::Focus => "开始专注",
        Phase::ShortBreak => "开始短休息",
        Phase::LongBreak => "开始长休息",
    }
}

/// Existing animation/input callbacks keep the visible card attached to the
/// head. A newly available head refreshes a missing or hidden card once.
pub fn follow_head(clock: &ShellClock) {
    let sprite = {
        let state = clock.ivars().state.borrow();
        if !state.settings.pomodoro.show_badge
            || state.pomodoro.status() == Status::Idle
            || !state.power.pets_visible()
            || state.power.screen_unavailable()
        {
            return;
        }
        state.renderer.first_sprite_cocoa().or_else(|| {
            state
                .extras
                .iter()
                .find_map(|extra| extra.renderer.first_sprite_cocoa())
        })
    };
    let needs_refresh = sprite.is_some()
        && clock
            .ivars()
            .focus_badge
            .borrow()
            .as_ref()
            .is_none_or(|badge| !badge.has_visible());
    if needs_refresh {
        refresh(clock);
        return;
    }
    if let Some(badge) = clock.ivars().focus_badge.borrow_mut().as_mut() {
        if let Some(sprite) = sprite {
            badge.follow(sprite);
        } else {
            badge.hide();
        }
    }
}

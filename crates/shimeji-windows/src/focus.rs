//! Platform-free Windows menu/settings decisions. No Win32 types escape the shell.

use std::path::{Path, PathBuf};

use shimeji_app::pomodoro::{Phase, Pomodoro, Preferences, Status};
#[cfg(any(target_os = "windows", test))]
use shimeji_core::env::{Point, ScreenSample, WorkRect};

pub fn profile_directory(
    explicit: Option<&Path>,
    local_app_data: Option<&Path>,
) -> Result<PathBuf, String> {
    explicit
        .map(Path::to_path_buf)
        .or_else(|| local_app_data.map(|path| path.join("shimeji2")))
        .ok_or_else(|| "LOCALAPPDATA is unavailable; use --profile PATH".to_string())
}

pub fn parse_minutes(values: [&str; 3], preferences: &Preferences) -> Result<Preferences, String> {
    let mut minutes = [0; 3];
    for (index, value) in values.into_iter().enumerate() {
        minutes[index] = value
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|value| (1..=180).contains(value))
            .ok_or_else(|| "请填写 1–180 之间的整数分钟。".to_string())?;
    }
    let mut result = preferences.clone();
    result.focus_minutes = minutes[0];
    result.short_break_minutes = minutes[1];
    result.long_break_minutes = minutes[2];
    Ok(result)
}

pub fn next_title(timer: &Pomodoro) -> &'static str {
    match timer.next_phase() {
        Phase::Focus => "开始专注",
        Phase::ShortBreak => "开始短休息",
        Phase::LongBreak => "开始长休息",
    }
}

pub fn action_enabled(timer: &Pomodoro) -> bool {
    !timer.interrupted()
        && (matches!(timer.status(), Status::Idle | Status::Completed)
            || (timer.status() == Status::Paused && timer.can_resume())
            || timer.status() == Status::Running)
}

pub fn action_title(timer: &Pomodoro) -> &'static str {
    match timer.status() {
        Status::Running => "暂停计时",
        Status::Paused => {
            if timer.can_resume() {
                "继续计时"
            } else {
                "等待解锁或唤醒"
            }
        }
        Status::Idle | Status::Completed => next_title(timer),
    }
}

pub fn completion_message(phase: Phase) -> (&'static str, &'static str) {
    match phase {
        Phase::Focus => (
            "这一轮完成了",
            "休息一下吧。右键托盘图标，准备好后开始休息。",
        ),
        Phase::ShortBreak | Phase::LongBreak => {
            ("休息结束", "准备好后，右键托盘图标开始下一轮专注。")
        }
    }
}

/// Physical-pixel placement of the 224 × 44 logical-pixel head card.
#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PatienceCard {
    pub position: Point,
    pub width: i32,
    pub height: i32,
    pub dpi: u32,
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn patience_card(pet: WorkRect, screens: &[ScreenSample], dpi: u32) -> PatienceCard {
    let dpi = dpi.clamp(96, 768);
    let scale = |logical: i32| (i64::from(logical) * i64::from(dpi) + 48) / 96;
    let center = Point::new(
        pet.x.saturating_add(pet.width / 2),
        pet.y.saturating_add(pet.height / 2),
    );
    let work = screens
        .iter()
        .map(|screen| {
            screen.work.unwrap_or(WorkRect {
                x: screen.x,
                y: screen.y,
                width: screen.width,
                height: screen.height,
            })
        })
        .min_by_key(|work| {
            let nearest_x = center
                .x
                .clamp(work.x, work.x.saturating_add(work.width.max(0)));
            let nearest_y = center
                .y
                .clamp(work.y, work.y.saturating_add(work.height.max(0)));
            (i64::from(center.x) - i64::from(nearest_x)).abs()
                + (i64::from(center.y) - i64::from(nearest_y)).abs()
        });
    let mut result = PatienceCard {
        position: Point::new(
            center.x.saturating_sub(scale(224) as i32 / 2),
            pet.y.saturating_sub(scale(44 + 6) as i32),
        ),
        width: scale(224) as i32,
        height: scale(44) as i32,
        dpi,
    };
    if let Some(work) = work {
        result.width = result.width.min(work.width.max(1));
        result.height = result.height.min(work.height.max(1));
        result.position.x = result.position.x.clamp(
            work.x,
            work.x
                .saturating_add(work.width.saturating_sub(result.width).max(0)),
        );
        result.position.y = result.position.y.clamp(
            work.y,
            work.y
                .saturating_add(work.height.saturating_sub(result.height).max(0)),
        );
    }
    result
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn patience_fill_width(track_width: i32, per_mille: u16) -> i32 {
    (i64::from(track_width.max(0)) * i64::from(per_mille.min(1000)) / 1000) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patience_card_is_head_centered_and_scaled_at_current_pet_dpi() {
        let pet = WorkRect {
            x: 500,
            y: 500,
            width: 128,
            height: 128,
        };
        let one = patience_card(pet, &[], 96);
        assert_eq!((one.width, one.height), (224, 44));
        assert_eq!(one.dpi, 96);
        assert_eq!(one.position, Point::new(452, 450));
        let two = patience_card(pet, &[], 192);
        assert_eq!((two.width, two.height), (448, 88));
        assert_eq!(two.dpi, 192);
        assert_eq!(two.position, Point::new(340, 400));
    }

    #[test]
    fn patience_card_clamps_to_its_negative_monitor_work_area_and_tiny_desktops() {
        let screen = ScreenSample {
            id: "left".into(),
            x: -1920,
            y: -200,
            width: 1920,
            height: 1080,
            work: Some(WorkRect {
                x: -1920,
                y: -200,
                width: 1920,
                height: 1040,
            }),
        };
        let other = ScreenSample {
            id: "right".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            work: None,
        };
        let at_edge = patience_card(
            WorkRect {
                x: -1940,
                y: -180,
                width: 80,
                height: 80,
            },
            &[other, screen],
            96,
        );
        assert_eq!(at_edge.position, Point::new(-1920, -200));
        let tiny = ScreenSample {
            id: "tiny".into(),
            x: 0,
            y: 0,
            width: 160,
            height: 40,
            work: None,
        };
        let card = patience_card(
            WorkRect {
                x: 0,
                y: 40,
                width: 20,
                height: 20,
            },
            &[tiny],
            192,
        );
        assert_eq!(
            (card.position, card.width, card.height),
            (Point::new(0, 0), 160, 40)
        );
    }

    #[test]
    fn patience_fill_width_shrinks_without_negative_or_overflowing_paint_rects() {
        assert_eq!(patience_fill_width(208, 1000), 208);
        assert_eq!(patience_fill_width(208, 500), 104);
        assert_eq!(patience_fill_width(208, 1), 0);
        assert_eq!(patience_fill_width(208, 0), 0);
        assert_eq!(patience_fill_width(208, u16::MAX), 208);
        assert_eq!(patience_fill_width(-4, 500), 0);
    }

    #[test]
    fn explicit_profile_isolated_from_default_and_missing_environment_is_an_error() {
        assert_eq!(
            profile_directory(
                Some(Path::new("/tmp/番茄 验证")),
                Some(Path::new("/personal"))
            )
            .unwrap(),
            Path::new("/tmp/番茄 验证")
        );
        assert_eq!(
            profile_directory(None, Some(Path::new("/local"))).unwrap(),
            Path::new("/local/shimeji2")
        );
        assert!(profile_directory(None, None).is_err());
    }

    #[test]
    fn native_settings_reject_invalid_duration_and_keep_reminder_preferences() {
        let preferences = Preferences {
            sound: true,
            notifications: false,
            show_badge: true,
            ..Preferences::default()
        };
        let result = parse_minutes([" 30 ", "10", "20"], &preferences).unwrap();
        assert_eq!(result.focus_minutes, 30);
        assert!(result.sound && result.show_badge && !result.notifications);
        for input in ["0", "181", "NaN", "-3", "1.5", "", "65536"] {
            assert!(parse_minutes([input, "5", "15"], &preferences).is_err());
        }
    }

    #[test]
    fn menu_actions_follow_shared_timer_and_session_blockers() {
        use shimeji_app::pomodoro::Interruption;
        let mut timer = Pomodoro::default();
        let preferences = Preferences::default();
        assert_eq!(action_title(&timer), "开始专注");
        timer.set_interruption(Interruption::Locked, true, 0);
        assert!(!action_enabled(&timer));
        assert!(!timer.start_next(0, &preferences));
        timer.set_interruption(Interruption::Locked, false, 0);
        assert!(action_enabled(&timer));
        assert!(timer.start_next(0, &preferences));
        assert_eq!(action_title(&timer), "暂停计时");
        timer.set_interruption(Interruption::Locked, true, 1_000);
        assert!(!action_enabled(&timer));
        timer.set_interruption(Interruption::Locked, false, 2_000);
        assert_eq!(action_title(&timer), "继续计时");
        assert!(timer.resume(2_000));
        assert!(timer.update(25 * 60_000 + 1_000).is_some());
        assert_eq!(action_title(&timer), "开始短休息");
        assert!(action_enabled(&timer));
        timer.cancel();
        assert_eq!(action_title(&timer), "开始专注");
    }
}

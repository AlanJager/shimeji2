use shimeji_app::pomodoro::{Completion, Interruption, Phase, Pomodoro, Preferences, Status};

fn quick() -> Preferences {
    Preferences {
        focus_minutes: 1,
        short_break_minutes: 1,
        long_break_minutes: 2,
        ..Preferences::default()
    }
}

#[test]
fn deadline_is_independent_of_callbacks_and_completes_once() {
    let mut timer = Pomodoro::default();
    assert!(timer.start_next(100, &quick()));
    assert!(!timer.start_next(101, &quick()));
    assert_eq!(timer.remaining_ms(30_100), 30_000);
    assert_eq!(
        timer.update(999_999),
        Some(Completion {
            phase: Phase::Focus
        })
    );
    assert_eq!(timer.status(), Status::Completed);
    assert_eq!(timer.completed_focus(), 1);
    assert!(!timer.quiet());
    assert_eq!(timer.update(999_999), None);
    assert_eq!(timer.update(u64::MAX), None);
    assert_eq!(timer.next_phase(), Phase::ShortBreak);
}

#[test]
fn pause_resume_uses_remaining_and_holds_companion_mode() {
    let mut timer = Pomodoro::default();
    timer.start_next(0, &quick());
    timer.pause(10_000);
    assert!(timer.quiet());
    assert_eq!(timer.remaining_ms(1_000_000), 50_000);
    assert!(timer.resume(1_000_000));
    assert_eq!(timer.update(1_049_999), None);
    assert_eq!(
        timer.update(1_050_000),
        Some(Completion {
            phase: Phase::Focus
        })
    );
}

#[test]
fn overlapping_lock_sleep_session_require_explicit_continue() {
    let mut timer = Pomodoro::default();
    timer.start_next(0, &quick());
    timer.set_interruption(Interruption::Locked, true, 10_000);
    timer.set_interruption(Interruption::Sleep, true, 20_000);
    timer.set_interruption(Interruption::Session, true, 30_000);
    timer.set_interruption(Interruption::Locked, false, 90_000);
    assert!(!timer.resume(90_000));
    timer.set_interruption(Interruption::Sleep, false, 100_000);
    assert!(!timer.resume(100_000));
    timer.set_interruption(Interruption::Session, false, 110_000);
    assert_eq!(timer.status(), Status::Paused);
    assert!(timer.can_resume());
    assert_eq!(timer.remaining_ms(110_000), 50_000);
    assert!(timer.resume(110_000));
}

#[test]
fn idle_blocker_prevents_start_and_cancel_does_not_clear_blocker() {
    let mut timer = Pomodoro::default();
    timer.set_interruption(Interruption::Sleep, true, 0);
    timer.cancel();
    assert!(!timer.start_next(50, &quick()));
    timer.set_interruption(Interruption::Sleep, false, 100);
    assert!(timer.start_next(100, &quick()));
}

#[test]
fn fourth_completed_round_offers_long_break_without_auto_starting() {
    let mut timer = Pomodoro::default();
    let prefs = quick();
    let mut now = 0;
    for round in 1..=4 {
        assert!(timer.start_next(now, &prefs));
        now += 60_000;
        assert_eq!(timer.update(now).unwrap().phase, Phase::Focus);
        assert_eq!(timer.completed_focus(), round);
        assert_eq!(
            timer.next_phase(),
            if round == 4 {
                Phase::LongBreak
            } else {
                Phase::ShortBreak
            }
        );
        assert!(timer.start_next(now, &prefs));
        assert!(!timer.quiet());
        let duration = if round == 4 { 120_000 } else { 60_000 };
        assert_eq!(timer.remaining_ms(now), duration);
        now += duration;
        assert!(timer.update(now).is_some());
        assert_eq!(timer.next_phase(), Phase::Focus);
    }
}

#[test]
fn cancellation_discards_round_and_settings_changes_only_affect_next_stage() {
    let mut timer = Pomodoro::default();
    let mut prefs = quick();
    timer.start_next(0, &prefs);
    prefs.focus_minutes = 100;
    assert_eq!(timer.remaining_ms(0), 60_000);
    timer.cancel();
    assert_eq!(timer.completed_focus(), 0);
    assert_eq!(timer.phase(), None);
    assert!(!timer.quiet());
    assert_eq!(timer.update(100_000), None);
    timer.start_next(100_000, &prefs);
    assert_eq!(timer.remaining_ms(100_000), 6_000_000);
}

#[test]
fn backward_timestamps_do_not_extend_timer_and_overflow_is_safe() {
    let mut timer = Pomodoro::default();
    timer.start_next(10_000, &quick());
    timer.update(20_000);
    assert_eq!(timer.remaining_ms(0), 50_000);
    timer.pause(0);
    timer.resume(0);
    assert_eq!(timer.remaining_ms(0), 50_000);
    assert!(timer.update(u64::MAX).is_some());
    assert!(timer.start_next(u64::MAX, &quick()));
    assert_eq!(timer.remaining_ms(u64::MAX), 60_000);
}

#[test]
fn completion_at_pause_boundary_is_not_lost() {
    let mut timer = Pomodoro::default();
    timer.start_next(0, &quick());
    timer.pause(60_000);
    assert_eq!(timer.status(), Status::Completed);
    assert_eq!(
        timer.update(60_000),
        Some(Completion {
            phase: Phase::Focus
        })
    );
    assert_eq!(timer.update(60_000), None);
}

#[test]
fn preferences_default_missing_fields_and_clamp_invalid_durations() {
    let mut prefs: Preferences =
        serde_json::from_str(r#"{"focus_minutes":0,"long_break_minutes":65535}"#).unwrap();
    prefs.normalize();
    assert_eq!(prefs.focus_minutes, 1);
    assert_eq!(prefs.short_break_minutes, 5);
    assert_eq!(prefs.long_break_minutes, 180);
    assert!(!prefs.sound);
    assert!(prefs.notifications);
    let settings: shimeji_app::settings::Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.pomodoro, Preferences::default());
}

#[test]
fn label_ceil_seconds_and_paused_text_are_stable() {
    let mut timer = Pomodoro::default();
    timer.start_next(0, &quick());
    assert_eq!(timer.label(1), "专注 · 01:00");
    assert_eq!(timer.label(1000), "专注 · 00:59");
    timer.pause(1000);
    assert_eq!(timer.label(999_999), "已暂停 · 00:59");
}

#[test]
fn restarting_from_deadline_keeps_previous_completion_until_consumed() {
    let mut timer = Pomodoro::default();
    timer.start_next(0, &quick());
    assert!(timer.start_next(60_000, &quick()));
    assert_eq!(timer.phase(), Some(Phase::ShortBreak));
    assert_eq!(timer.update(60_001).unwrap().phase, Phase::Focus);
    assert_eq!(timer.update(60_002), None);
}

use shimeji_app::pomodoro::{Interruption, PatienceColor, Phase, Pomodoro, Preferences};

fn preferences() -> Preferences {
    Preferences {
        focus_minutes: 4,
        short_break_minutes: 2,
        long_break_minutes: 3,
        ..Preferences::default()
    }
}

#[test]
fn exact_quarter_boundaries_change_color_without_rounding_early() {
    let mut timer = Pomodoro::default();
    assert!(timer.patience(0).is_none());
    timer.start_next(0, &preferences());
    for (elapsed, fill, color) in [
        (0, 1000, PatienceColor::Green),
        (59_999, 750, PatienceColor::Green),
        (60_000, 750, PatienceColor::Yellow),
        (119_999, 500, PatienceColor::Yellow),
        (120_000, 500, PatienceColor::Orange),
        (179_999, 250, PatienceColor::Orange),
        (180_000, 250, PatienceColor::Red),
        (239_999, 0, PatienceColor::Red),
        (240_000, 0, PatienceColor::Red),
        (u64::MAX, 0, PatienceColor::Red),
    ] {
        let badge = timer.patience(elapsed).unwrap();
        assert_eq!(badge.per_mille, fill, "elapsed {elapsed}");
        assert_eq!(badge.color, color, "elapsed {elapsed}");
    }
    assert_eq!(timer.patience(239_999).unwrap().label, "耐心：00:01");
}

#[test]
fn pause_resume_preserves_original_denominator_and_freezes_display() {
    let mut timer = Pomodoro::default();
    timer.start_next(0, &preferences());
    timer.pause(60_000);
    let paused = timer.patience(60_000).unwrap();
    assert_eq!(paused.per_mille, 750);
    assert_eq!(paused.label, "耐心：03:00 · 已暂停");
    assert_eq!(timer.patience(1_000_000).unwrap(), paused);
    assert!(timer.resume(1_000_000));
    let later = timer.patience(1_060_000).unwrap();
    assert_eq!(later.per_mille, 500);
    assert_eq!(later.color, PatienceColor::Orange);
    assert_eq!(later.label, "耐心：02:00");
}

#[test]
fn lock_and_sleep_do_not_refill_patience_or_resume_it() {
    let mut timer = Pomodoro::default();
    timer.start_next(0, &preferences());
    timer.set_interruption(Interruption::Locked, true, 180_000);
    let paused = timer.patience(180_000).unwrap();
    assert_eq!(paused.color, PatienceColor::Red);
    timer.set_interruption(Interruption::Sleep, true, 500_000);
    timer.set_interruption(Interruption::Locked, false, 800_000);
    timer.set_interruption(Interruption::Sleep, false, 1_000_000);
    assert_eq!(timer.patience(1_000_000).unwrap(), paused);
    assert!(timer.resume(1_000_000));
    assert_eq!(timer.patience(1_030_000).unwrap().per_mille, 125);
}

#[test]
fn completed_is_empty_red_and_cancel_hides_the_badge() {
    let mut timer = Pomodoro::default();
    timer.start_next(0, &preferences());
    timer.update(240_000).unwrap();
    let done = timer.patience(240_000).unwrap();
    assert_eq!(done.per_mille, 0);
    assert_eq!(done.color, PatienceColor::Red);
    assert_eq!(done.label, "耐心：00:00 · 已完成");
    timer.cancel();
    assert!(timer.patience(1_000_000).is_none());
}

#[test]
fn preferences_only_change_next_stage_and_breaks_start_with_full_patience() {
    let mut timer = Pomodoro::default();
    let mut preferences = preferences();
    timer.start_next(0, &preferences);
    preferences.focus_minutes = 180;
    preferences.short_break_minutes = 10;
    assert_eq!(timer.patience(120_000).unwrap().per_mille, 500);
    timer.update(240_000).unwrap();
    assert!(timer.start_next(240_000, &preferences));
    let rest = timer.patience(240_000).unwrap();
    assert_eq!(rest.per_mille, 1000);
    assert_eq!(rest.color, PatienceColor::Green);
    assert_eq!(rest.label, "耐心：10:00 · 短休息");
    assert_eq!(timer.patience(540_000).unwrap().per_mille, 500);
}

#[test]
fn fourth_round_long_break_uses_its_own_duration() {
    let mut timer = Pomodoro::default();
    let preferences = preferences();
    let mut now = 0;
    for round in 1..=4 {
        timer.start_next(now, &preferences);
        now += 240_000;
        timer.update(now).unwrap();
        timer.start_next(now, &preferences);
        if round < 4 {
            now += 120_000;
            timer.update(now).unwrap();
        }
    }
    assert_eq!(timer.phase(), Some(Phase::LongBreak));
    assert_eq!(timer.patience(now).unwrap().label, "耐心：03:00 · 长休息");
    assert_eq!(timer.patience(now + 90_000).unwrap().per_mille, 500);
}

#[test]
fn palette_has_the_requested_distinct_hues() {
    let [r, g, b] = PatienceColor::Green.rgb();
    assert!(g > r && g > b);
    let [r, g, b] = PatienceColor::Yellow.rgb();
    assert!(r > 150 && g > 150 && b < 100);
    let [r, g, b] = PatienceColor::Orange.rgb();
    assert!(r > g && g > b);
    let [r, g, b] = PatienceColor::Red.rgb();
    assert!(r > g && r > b);
}

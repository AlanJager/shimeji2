//! Power-controller state machine. No WindowServer.

use shimeji_macos::platform::power::{Activity, Power, animate_anyway_from_env};

fn sources_on(power: &Power) -> bool {
    let sources = power.sources();
    sources.clock && sources.poller && sources.ax_trust_poll
}

#[test]
fn running_keeps_every_source_on() {
    let power = Power::default();
    assert_eq!(power.activity(), Activity::Running);
    assert!(sources_on(&power));
    assert!(!power.engine_frozen());
    assert!(power.pets_visible());
}

#[test]
fn pause_stops_sources_and_leaves_pets_visible() {
    let mut power = Power::default();
    power.set_user_paused(true);
    assert_eq!(power.activity(), Activity::Paused);
    assert!(!sources_on(&power));
    assert!(power.engine_frozen());
    assert!(power.pets_visible());
}

#[test]
fn hidden_stops_sources_and_hides_pets() {
    let mut power = Power::default();
    power.set_user_paused(true);
    assert!(power.toggle_hidden());
    assert_eq!(power.activity(), Activity::Hidden);
    assert!(!sources_on(&power));
    assert!(power.engine_frozen());
    assert!(!power.pets_visible());
    assert!(!power.toggle_hidden());
    assert_eq!(power.activity(), Activity::Paused);
    assert!(power.pets_visible());
}

#[test]
fn each_display_flag_freezes_and_resume_waits_for_all_of_them() {
    let mut power = Power::default();
    power.set_user_paused(true);
    power.set_screens_asleep(true);
    assert_eq!(power.activity(), Activity::ScreenAsleep);
    power.set_screens_asleep(false);
    assert_eq!(power.activity(), Activity::Paused);

    power.set_system_sleep(true);
    power.set_session_inactive(true);
    power.set_locked(true);
    power.set_occluded(true);
    assert_eq!(power.activity(), Activity::ScreenAsleep);
    power.set_system_sleep(false);
    power.set_session_inactive(false);
    power.set_locked(false);
    assert_eq!(power.activity(), Activity::ScreenAsleep);
    power.set_occluded(false);
    assert_eq!(power.activity(), Activity::Paused);
    power.set_user_paused(false);
    assert_eq!(power.activity(), Activity::Running);
    assert!(sources_on(&power));
}

#[test]
fn reduce_motion_freezes_unless_animate_anyway() {
    let mut power = Power::default();
    power.set_reduce_motion(true);
    assert_eq!(power.activity(), Activity::ReduceMotion);
    assert!(power.engine_frozen());
    assert!(power.pets_visible());
    power.set_animate_anyway(true);
    assert_eq!(power.activity(), Activity::Running);
    assert!(sources_on(&power));
    power.set_user_paused(true);
    assert_eq!(power.activity(), Activity::Paused);
}

#[test]
fn arming_forces_sources_until_it_is_cleared() {
    let mut power = Power::default();
    power.set_reduce_motion(true);
    power.set_arming(true);
    assert!(power.arming());
    assert!(sources_on(&power));
    assert!(!power.engine_frozen());
    power.set_arming(false);
    assert_eq!(power.activity(), Activity::ReduceMotion);
    assert!(power.engine_frozen());
    assert!(!sources_on(&power));
}

#[test]
fn screen_wake_while_user_paused_stays_paused() {
    let mut power = Power::default();
    power.set_screens_asleep(true);
    power.set_user_paused(true);
    power.set_screens_asleep(false);
    assert_eq!(power.activity(), Activity::Paused);
    assert!(!sources_on(&power));
}

#[test]
fn animate_anyway_env_accepts_the_stub_words() {
    assert!(animate_anyway_from_env(Some("1")));
    assert!(animate_anyway_from_env(Some("true")));
    assert!(animate_anyway_from_env(Some(" yes ")));
    assert!(animate_anyway_from_env(Some("ON")));
    assert!(!animate_anyway_from_env(None));
    assert!(!animate_anyway_from_env(Some("0")));
    assert!(!animate_anyway_from_env(Some("")));
}

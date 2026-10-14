//! Counter rates and `ps` time parsing.

use shimeji_macos::platform::counters::{Totals, gap_is_late, parse_ps_time};

#[test]
fn rates_divide_by_the_wall_span() {
    let totals = Totals {
        callbacks: 100,
        ticks: 50,
        late_ticks: 2,
        resyncs: 1,
        window_moves: 4,
        layer_moves: 40,
        contents_sets: 10,
        window_queries: 4,
        window_query_us: 400,
        mouse_events: 8,
        timer_wakeups: 100,
        decoded_image_bytes: 2_000_000,
    };
    let rates = totals.rates(2.0);
    assert_eq!(rates.callbacks_per_s, 50.0);
    assert_eq!(rates.ticks_per_s, 25.0);
    assert_eq!(rates.late_ticks_per_s, 1.0);
    assert_eq!(rates.window_moves_per_s, 2.0);
    assert_eq!(rates.layer_moves_per_s, 20.0);
    assert_eq!(rates.contents_sets_per_s, 5.0);
    assert_eq!(rates.window_queries_per_s, 2.0);
    assert_eq!(rates.query_us, 100.0);
    assert_eq!(rates.mouse_events_per_s, 4.0);
    assert_eq!(rates.timer_wakeups_per_s, 50.0);
    assert_eq!(rates.decoded_image_mb, 2.0);
    assert_eq!(Totals::default().rates(0.0).callbacks_per_s, 0.0);
    assert_eq!(Totals::default().query_us_each(), 0.0);
}

#[test]
fn a_gap_is_late_past_the_step_plus_slack() {
    assert!(!gap_is_late(40, 40));
    assert!(!gap_is_late(48, 40));
    assert!(gap_is_late(49, 40));
}

#[test]
fn ps_time_accepts_the_forms_macos_prints() {
    assert_eq!(parse_ps_time("1:02.50"), Some(62.5));
    assert_eq!(parse_ps_time("34033:57.65"), Some(34033.0 * 60.0 + 57.65));
    assert_eq!(
        parse_ps_time("1-02:03:04"),
        Some(86_400.0 + 2.0 * 3600.0 + 3.0 * 60.0 + 4.0)
    );
    assert_eq!(parse_ps_time("12.5"), Some(12.5));
    assert_eq!(parse_ps_time("  3:00 "), Some(180.0));
    assert_eq!(parse_ps_time(""), None);
    assert_eq!(parse_ps_time("a:b"), None);
}

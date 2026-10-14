//! Advanced → Diagnostics. The numbers are the same counters printed at exit.

use objc2::{DefinedClass, MainThreadMarker};
use objc2_app_kit::NSAlert;
use objc2_foundation::NSString;

use crate::bridge::ShellClock;

pub fn show(clock: &ShellClock) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let window = clock
        .ivars()
        .settings_ui
        .borrow()
        .as_ref()
        .map(|ui| ui.window.clone());
    let Some(window) = window else {
        return;
    };
    let elapsed = clock.ivars().state.borrow().origin.elapsed();
    let totals = clock.snapshot_counters();
    let rates = totals.rates(elapsed.as_secs_f64());
    let text = format!(
        "callbacks: {}\n\
         ticks: {} ({:.2}/s)\n\
         late ticks: {}\n\
         resyncs: {}\n\
         window moves: {} ({:.2}/s)\n\
         layer moves: {} ({:.2}/s)\n\
         contents sets: {} ({:.2}/s)\n\
         window queries: {} ({:.2}/s)\n\
         µs per query: {:.1}\n\
         mouse events: {} ({:.2}/s)\n\
         timer wakeups: {}\n\
         decoded image MB: {:.3}\n\
         elapsed: {:.1}s",
        totals.callbacks,
        totals.ticks,
        rates.ticks_per_s,
        totals.late_ticks,
        totals.resyncs,
        totals.window_moves,
        rates.window_moves_per_s,
        totals.layer_moves,
        rates.layer_moves_per_s,
        totals.contents_sets,
        rates.contents_sets_per_s,
        totals.window_queries,
        rates.window_queries_per_s,
        rates.query_us,
        totals.mouse_events,
        rates.mouse_events_per_s,
        totals.timer_wakeups,
        rates.decoded_image_mb,
        elapsed.as_secs_f64(),
    );
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str("Diagnostics"));
    alert.setInformativeText(&NSString::from_str(&text));
    alert.addButtonWithTitle(&NSString::from_str("OK"));
    alert.beginSheetModalForWindow_completionHandler(&window, None);
    *clock.ivars().diagnostics_alert.borrow_mut() = Some(alert);
}

//! U2 instrumentation. Unit 10 shows this in Advanced → Diagnostics.
//! The shell also prints it on exit.

/// Absolute counts over a run. Rates are derived from a wall-clock span.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub callbacks: u64,
    pub ticks: u64,
    /// GCD callbacks whose gap from the previous one was over 48 ms
    /// (40 ms step plus 8 ms of slack). Separate from [`Self::resyncs`],
    /// which counts accumulator drops after more than two ticks of debt.
    pub late_ticks: u64,
    pub resyncs: u64,
    pub window_moves: u64,
    pub layer_moves: u64,
    pub contents_sets: u64,
    pub window_queries: u64,
    /// Sum of window-query durations, in microseconds.
    pub window_query_us: u64,
    pub mouse_events: u64,
    /// GCD timer handler invocations. Same source as `callbacks`; named on
    /// its own so a paused run can show 0 timer-driven wakeups.
    pub timer_wakeups: u64,
    pub decoded_image_bytes: u64,
}

/// Per-second view plus the two levels that are not rates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    pub callbacks_per_s: f64,
    pub ticks_per_s: f64,
    pub late_ticks_per_s: f64,
    pub resyncs_per_s: f64,
    pub window_moves_per_s: f64,
    pub layer_moves_per_s: f64,
    pub contents_sets_per_s: f64,
    pub window_queries_per_s: f64,
    /// Mean microseconds per window query. 0 when nothing was queried.
    pub query_us: f64,
    pub mouse_events_per_s: f64,
    pub timer_wakeups_per_s: f64,
    pub decoded_image_mb: f64,
}

impl Totals {
    pub fn rates(self, wall_s: f64) -> Rates {
        let per = |count: u64| {
            if wall_s > 0.0 {
                count as f64 / wall_s
            } else {
                0.0
            }
        };
        Rates {
            callbacks_per_s: per(self.callbacks),
            ticks_per_s: per(self.ticks),
            late_ticks_per_s: per(self.late_ticks),
            resyncs_per_s: per(self.resyncs),
            window_moves_per_s: per(self.window_moves),
            layer_moves_per_s: per(self.layer_moves),
            contents_sets_per_s: per(self.contents_sets),
            window_queries_per_s: per(self.window_queries),
            query_us: self.query_us_each(),
            mouse_events_per_s: per(self.mouse_events),
            timer_wakeups_per_s: per(self.timer_wakeups),
            decoded_image_mb: self.decoded_image_bytes as f64 / 1_000_000.0,
        }
    }

    pub fn query_us_each(self) -> f64 {
        if self.window_queries == 0 {
            0.0
        } else {
            self.window_query_us as f64 / self.window_queries as f64
        }
    }
}

/// A callback is late when the gap since the previous one exceeds the step
/// by more than this slack. 8 ms covers the 1 ms leeway and scheduling noise
/// without calling a 50 ms stall on time.
pub const LATE_TICK_SLACK_MS: u64 = 8;

pub fn gap_is_late(gap_ms: u64, step_ms: u64) -> bool {
    gap_ms > step_ms.saturating_add(LATE_TICK_SLACK_MS)
}

/// `ps -o time` on macOS: `[dd-]hh:mm:ss`, `mm:ss`, or `mm:ss.ss`.
pub fn parse_ps_time(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (days, rest) = if let Some((days, rest)) = text.split_once('-') {
        (days.parse::<f64>().ok()?, rest)
    } else {
        (0.0, text)
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let seconds = match *parts.as_slice() {
        [seconds] => seconds.parse::<f64>().ok()?,
        [minutes, seconds] => minutes
            .parse::<f64>()
            .ok()?
            .mul_add(60.0, seconds.parse().ok()?),
        [hours, minutes, seconds] => hours.parse::<f64>().ok()?.mul_add(
            3600.0,
            minutes
                .parse::<f64>()
                .ok()?
                .mul_add(60.0, seconds.parse().ok()?),
        ),
        _ => return None,
    };
    Some(days.mul_add(86_400.0, seconds))
}

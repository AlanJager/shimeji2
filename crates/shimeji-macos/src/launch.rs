//! Which seed a process runs with.
//!
//! `--seed N` is that value. A normal `run` with no flag mixes the clock so two
//! launches a second apart do not share a seed. The energy scenarios stay on 1.

/// Nanoseconds since the Unix epoch. Zero when the clock is before the epoch.
pub fn clock_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// `explicit` is the `--seed` flag when it was present.
///
/// Only `scenario == "run"` draws from `clock_nanos`. `baseline` and `s1`–`s5`
/// stay on 1 so an energy run does not move unless the flag says so.
pub fn launch_seed(explicit: Option<i64>, scenario: &str, clock_nanos: u64) -> i64 {
    if let Some(seed) = explicit {
        return seed;
    }
    if scenario == "run" {
        return mix_clock(clock_nanos);
    }
    1
}

/// Ordinary app launches run until Quit; measurements remain bounded by default.
pub fn launch_seconds(explicit: Option<f64>, scenario: &str) -> Result<Option<f64>, &'static str> {
    if let Some(seconds) = explicit {
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err("--seconds must be a finite number > 0");
        }
        return Ok(Some(seconds));
    }
    Ok(if scenario == "run" { None } else { Some(20.0) })
}

/// SplitMix64 finalizer. Different nanosecond counts produce different seeds,
/// including counts one second (1e9 ns) apart.
fn mix_clock(nanos: u64) -> i64 {
    let mut z = nanos.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    i64::from_ne_bytes(z.to_ne_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_launch_has_no_automatic_exit_deadline() {
        assert_eq!(launch_seconds(None, "run"), Ok(None));
    }

    #[test]
    fn measurement_launches_keep_the_twenty_second_default() {
        for scenario in ["baseline", "s1", "s2", "s3", "s4", "s5"] {
            assert_eq!(launch_seconds(None, scenario), Ok(Some(20.0)));
        }
    }

    #[test]
    fn explicit_duration_overrides_both_launch_modes() {
        assert_eq!(launch_seconds(Some(0.5), "run"), Ok(Some(0.5)));
        assert_eq!(launch_seconds(Some(90.0), "s2"), Ok(Some(90.0)));
        for seconds in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(launch_seconds(Some(seconds), "run").is_err());
        }
    }

    #[test]
    fn an_explicit_seed_wins() {
        assert_eq!(launch_seed(Some(42), "run", 1_000_000_000), 42);
        assert_eq!(launch_seed(Some(42), "s3", 2_000_000_000), 42);
        assert_eq!(launch_seed(Some(0), "run", 5), 0);
        assert_eq!(launch_seed(Some(1), "baseline", 5), 1);
    }

    #[test]
    fn run_without_a_seed_is_not_one_across_two_clocks() {
        let first = launch_seed(None, "run", 1_000_000_000);
        let second = launch_seed(None, "run", 2_000_000_000);
        assert_ne!(first, second);
        assert_ne!(first, 1);
        assert_ne!(second, 1);
        assert_ne!(
            launch_seed(None, "run", 5_000_000_000),
            launch_seed(None, "run", 5_000_000_001)
        );
    }

    #[test]
    fn energy_scenarios_stay_on_one() {
        for scenario in ["baseline", "s1", "s2", "s3", "s4", "s5"] {
            assert_eq!(launch_seed(None, scenario, 1_000_000_000), 1);
            assert_eq!(launch_seed(None, scenario, 2_000_000_000), 1);
        }
    }
}

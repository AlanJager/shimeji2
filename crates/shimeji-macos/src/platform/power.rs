//! Which periodic sources are running.
//!
//! One controller owns the GCD clock, the window poller, and (later) the
//! AX-trust poll. Pause, Hide, display sleep, and Reduce Motion stop all of
//! them. The macOS notification observer lives in the shell.

/// Env var that overrides the saved Animate anyway setting for this process.
/// `1` / `true` / `yes` / `on` means animate anyway. Any other present value
/// means off. Unset leaves the saved setting.
pub const ANIMATE_ANYWAY_ENV: &str = "SHIMEJI_ANIMATE_ANYWAY";

/// What the shell is doing. Hidden wins over the other freeze reasons because
/// the windows are ordered out; the rest only change which flag resumes last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Running,
    Paused,
    Hidden,
    ScreenAsleep,
    ReduceMotion,
}

/// Periodic work. `ax_trust_poll` follows the clock; Unit 11 starts the poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sources {
    pub clock: bool,
    pub poller: bool,
    pub ax_trust_poll: bool,
}

/// Freeze flags. Independent: clearing one leaves the others in place.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Power {
    user_paused: bool,
    hidden: bool,
    screens_asleep: bool,
    system_sleep: bool,
    session_inactive: bool,
    locked: bool,
    occluded: bool,
    reduce_motion: bool,
    animate_anyway: bool,
    /// Keep sources running until the first pets exist, then honor the flags.
    arming: bool,
}

impl Power {
    pub fn activity(&self) -> Activity {
        if self.hidden {
            return Activity::Hidden;
        }
        if self.display_down() {
            return Activity::ScreenAsleep;
        }
        if self.reduce_motion && !self.animate_anyway {
            return Activity::ReduceMotion;
        }
        if self.user_paused {
            return Activity::Paused;
        }
        Activity::Running
    }

    pub fn sources(&self) -> Sources {
        let on = self.arming || self.activity() == Activity::Running;
        Sources {
            clock: on,
            poller: on,
            ax_trust_poll: on,
        }
    }

    /// The engine clock should not advance behaviors.
    pub fn engine_frozen(&self) -> bool {
        !self.arming && self.activity() != Activity::Running
    }

    pub fn pets_visible(&self) -> bool {
        !self.hidden
    }

    pub fn user_paused(&self) -> bool {
        self.user_paused
    }

    pub fn hidden(&self) -> bool {
        self.hidden
    }

    pub fn animate_anyway(&self) -> bool {
        self.animate_anyway
    }

    pub fn arming(&self) -> bool {
        self.arming
    }

    pub fn set_arming(&mut self, arming: bool) {
        self.arming = arming;
    }

    pub fn set_user_paused(&mut self, paused: bool) {
        self.user_paused = paused;
    }

    pub fn toggle_user_paused(&mut self) -> bool {
        self.user_paused = !self.user_paused;
        self.user_paused
    }

    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
    }

    pub fn toggle_hidden(&mut self) -> bool {
        self.hidden = !self.hidden;
        self.hidden
    }

    pub fn set_screens_asleep(&mut self, asleep: bool) {
        self.screens_asleep = asleep;
    }

    pub fn set_system_sleep(&mut self, sleeping: bool) {
        self.system_sleep = sleeping;
    }

    pub fn set_session_inactive(&mut self, inactive: bool) {
        self.session_inactive = inactive;
    }

    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    pub fn set_occluded(&mut self, occluded: bool) {
        self.occluded = occluded;
    }

    pub fn set_reduce_motion(&mut self, on: bool) {
        self.reduce_motion = on;
    }

    pub fn set_animate_anyway(&mut self, on: bool) {
        self.animate_anyway = on;
    }

    fn display_down(&self) -> bool {
        self.screens_asleep
            || self.system_sleep
            || self.session_inactive
            || self.locked
            || self.occluded
    }
}

pub fn animate_anyway_from_env(value: Option<&str>) -> bool {
    matches!(
        value
            .map(|text| text.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

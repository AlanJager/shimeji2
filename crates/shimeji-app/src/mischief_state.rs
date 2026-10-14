//! Whether window mischief is off, waiting for Accessibility, on, or revoked.
//!
//! The menu and the settings window share one [`Phase`]. A poll of
//! `AXIsProcessTrusted` is due about once a second while the phase is
//! awaiting or on. [`Phase::On`] is reached only from a granted poll.

/// What the shell is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Off,
    /// The user turned mischief on. An explanation is up until a grant or a dismiss.
    AwaitingPermission,
    On,
    /// The grant disappeared while mischief was on. The warning offers Open Settings.
    PermissionMissing,
}

/// What the shell feeds the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    UserEnabled,
    UserDisabled,
    /// One `AXIsProcessTrusted` result. `true` is the only way into [`Phase::On`].
    Trust(bool),
    /// The user closed the awaiting-permission explanation.
    ExplanationDismissed,
}

/// The phase after one input, plus whether a carry that was in progress must end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub phase: Phase,
    /// `true` when this input leaves [`Phase::On`]. The shell drops the window
    /// and the pet falls. Permission lost mid-carry is the restore case.
    pub end_carry: bool,
}

impl Phase {
    pub fn step(self, input: Input) -> Step {
        let phase = next(self, input);
        Step {
            phase,
            end_carry: self == Phase::On && phase != Phase::On,
        }
    }

    /// Periodic trust poll. Off and [`Phase::PermissionMissing`] do not poll.
    /// A trust result is still accepted in the missing phase, so one check
    /// after Open Settings can return to [`Phase::On`].
    pub fn polls(self) -> bool {
        matches!(self, Phase::AwaitingPermission | Phase::On)
    }

    /// The warning with an Open Settings action.
    pub fn warns_open_settings(self) -> bool {
        self == Phase::PermissionMissing
    }

    /// The explanation shown while waiting for the first grant.
    pub fn shows_explanation(self) -> bool {
        self == Phase::AwaitingPermission
    }
}

fn next(phase: Phase, input: Input) -> Phase {
    match (phase, input) {
        (Phase::Off, Input::UserEnabled) => Phase::AwaitingPermission,
        (Phase::AwaitingPermission, Input::UserDisabled | Input::ExplanationDismissed) => {
            Phase::Off
        }
        (Phase::AwaitingPermission, Input::Trust(true)) => Phase::On,
        (Phase::On, Input::UserDisabled) => Phase::Off,
        (Phase::On, Input::Trust(false)) => Phase::PermissionMissing,
        (Phase::PermissionMissing, Input::UserDisabled) => Phase::Off,
        (Phase::PermissionMissing, Input::Trust(true)) => Phase::On,
        (phase, _) => phase,
    }
}

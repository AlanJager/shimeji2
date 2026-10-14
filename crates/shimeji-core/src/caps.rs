//! Live-mascot cap and the shared per-tick condition budget.
//!
//! The cap is checked when a spawn is requested, before that child's behavior
//! is built, so a refused spawn does not draw. The reference builds the
//! behavior first; traces below the cap stay the same.
//!
//! The condition budget is one counter for the whole tick. Behavior
//! conditions, action `Condition`s, and animation conditions spend it. When
//! it is gone, a later condition is false and the tick records one
//! diagnostic. The next tick that follows a trip starts at the next mascot.

use std::cell::Cell;

/// Default live mascots, from R30.
pub const DEFAULT_MASCOT_CAP: usize = 50;

/// R30 names a hard ceiling and not its value. 100 is the ceiling this engine
/// accepts from `Engine::set_mascot_cap`.
pub const HARD_MASCOT_CAP: usize = 100;

/// Condition evaluations allowed in one tick, shared by every mascot.
///
/// Shime's selection walks a few dozen conditions. 4_096 leaves golden runs
/// alone and still stops a 10,000-condition chain.
pub const DEFAULT_EVAL_BUDGET: u32 = 4_096;

/// How many instant `hasNext == false` hand-offs `init` will follow.
/// The reference recurses until the stack dies. This bound disposes the mascot.
pub(crate) const MAX_BEHAVIOR_CHAIN: u32 = 32;

/// Remaining condition evaluations for the mascot currently ticking.
///
/// Part A's tests leave [`TickBudget::left`] at `u32::MAX`, so they never trip.
#[derive(Debug)]
pub struct TickBudget {
    left: Cell<u32>,
    tripped: Cell<bool>,
}

impl Default for TickBudget {
    fn default() -> Self {
        Self {
            left: Cell::new(u32::MAX),
            tripped: Cell::new(false),
        }
    }
}

impl Clone for TickBudget {
    fn clone(&self) -> Self {
        Self {
            left: Cell::new(self.left.get()),
            tripped: Cell::new(self.tripped.get()),
        }
    }
}

impl PartialEq for TickBudget {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl TickBudget {
    pub(crate) fn set_left(&self, left: u32) {
        self.left.set(left);
    }

    pub(crate) fn left(&self) -> u32 {
        self.left.get()
    }

    pub(crate) fn tripped(&self) -> bool {
        self.tripped.get()
    }

    pub(crate) fn clear_tripped(&self) {
        self.tripped.set(false);
    }

    /// Spend one condition evaluation. `false` means the caller treats the
    /// condition as false and does not run it.
    pub(crate) fn allow(&self) -> bool {
        let left = self.left.get();
        if left == 0 {
            self.tripped.set(true);
            return false;
        }
        self.left.set(left - 1);
        true
    }
}

pub(crate) fn clamp_cap(cap: usize) -> usize {
    cap.clamp(1, HARD_MASCOT_CAP)
}

//! `Select` adds no override. `init` seeks the first child whose `hasNext` is
//! true, and later ticks stay on that child.

use shimeji_expr::JavaRandom;

use super::{ActionError, RunningAction};
use crate::mascot::Mascot;

pub(super) fn has_next(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    action.complex_has_next(mascot, rng)
}

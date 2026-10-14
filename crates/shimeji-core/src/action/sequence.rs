//! `Sequence.hasNext` seeks, then asks `ComplexAction.hasNext`.
//! `Loop` (`繰り返し`) is read in `setCurrentAction`, including the initial index 0.

use shimeji_expr::JavaRandom;

use super::{ActionError, RunningAction};
use crate::mascot::Mascot;

pub(super) fn has_next(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    action.seek(mascot, rng)?;
    action.complex_has_next(mascot, rng)
}

pub(super) fn set_current(
    action: &mut RunningAction,
    index: usize,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    let looped = action.eval_bool("Loop", false, mascot, rng)?;
    let len = action.children.len();
    let index = if looped && len > 0 {
        index % len
    } else {
        index
    };
    action.complex_set_current(index, mascot, rng)
}

//! `Offset`. Instant translate by `X` and `Y`. Not scaled.

use shimeji_expr::JavaRandom;

use super::super::{ActionError, RunningAction};
use crate::mascot::Mascot;

pub(crate) fn init(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    if action.base_has_next(mascot, rng)? {
        let x = action.eval_int("X", 0, mascot, rng)?;
        let y = action.eval_int("Y", 0, mascot, rng)?;
        mascot.anchor.x = mascot.anchor.x.wrapping_add(x);
        mascot.anchor.y = mascot.anchor.y.wrapping_add(y);
    }
    Ok(())
}

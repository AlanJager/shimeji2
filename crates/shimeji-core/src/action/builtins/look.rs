//! `Look`. Instant: `init` applies `LookRight` (default is the opposite facing).
//! `hasNext` is always false.

use shimeji_expr::JavaRandom;

use super::super::{ActionError, RunningAction};
use crate::mascot::Mascot;

pub(crate) fn init(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    if action.base_has_next(mascot, rng)? {
        let fallback = !mascot.look_right;
        mascot.look_right = action.eval_bool("LookRight", fallback, mascot, rng)?;
    }
    Ok(())
}

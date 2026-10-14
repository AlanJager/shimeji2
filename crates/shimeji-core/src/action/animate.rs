//! `Animate.hasNext` is `ActionBase.hasNext` and `time < animation.duration`.
//! One cycle: the next tick's first `hasNext` is false, and `next` is not called.

use shimeji_expr::JavaRandom;

use super::{ActionError, RunningAction};
use crate::mascot::Mascot;

pub(super) fn has_next(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    if !action.base_has_next(mascot, rng)? {
        return Ok(false);
    }
    let Some(index) = action.pick(mascot, rng, None)? else {
        return Err(ActionError::NoPose);
    };
    Ok(action.time_of(mascot) < action.animations[index].duration)
}

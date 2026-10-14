//! `Stay.tick`: ride, `LostGround`, then the current pose.

use shimeji_expr::JavaRandom;

use super::{ActionError, RunningAction, apply_pose, pose_at};
use crate::action::bordered;
use crate::mascot::Mascot;

pub(super) fn tick(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    bordered::ride(action, mascot);
    bordered::lost_ground(action, mascot)?;
    // An action with no animation (unknown class rewritten to Stay, or a
    // fast-follow stand-in) keeps the current image. A real Stay whose
    // conditions all fail still has animations and returns `NoPose`.
    if action.animation_count() == 0 {
        return Ok(());
    }
    let Some(index) = action.pick(mascot, rng, None)? else {
        return Err(ActionError::NoPose);
    };
    let time = action.time_of(mascot);
    let pose = pose_at(&action.animations[index], time)?;
    apply_pose(mascot, pose);
    Ok(())
}

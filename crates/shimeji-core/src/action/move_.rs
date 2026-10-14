//! `Move`. Both configured axes must arrive before `hasNext` is false.
//! While `turning` is set, `hasNext` does not read the targets.

use shimeji_expr::JavaRandom;

use super::{ActionError, RunningAction, apply_pose, pose_at};
use crate::action::bordered;
use crate::mascot::Mascot;

const TARGET_MAX: i32 = i32::MAX;

pub(super) fn has_next(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    if !action.base_has_next(mascot, rng)? {
        return Ok(false);
    }
    if action.turning {
        return Ok(true);
    }
    let target_x = action.eval_int("TargetX", TARGET_MAX, mascot, rng)?;
    let target_y = action.eval_int("TargetY", TARGET_MAX, mascot, rng)?;
    Ok((target_x != TARGET_MAX && mascot.anchor.x != target_x)
        || (target_y != TARGET_MAX && mascot.anchor.y != target_y))
}

pub(super) fn tick(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    bordered::ride(action, mascot);
    bordered::lost_ground(action, mascot)?;

    let target_x = action.eval_int("TargetX", TARGET_MAX, mascot, rng)?;
    let target_y = action.eval_int("TargetY", TARGET_MAX, mascot, rng)?;

    if target_x != TARGET_MAX && mascot.anchor.x != target_x {
        let facing_right = mascot.anchor.x < target_x;
        action.turning =
            action.has_turning() && (action.turning || facing_right != mascot.look_right);
        mascot.look_right = facing_right;
    }
    let down = target_y != TARGET_MAX && mascot.anchor.y < target_y;

    let Some(mut index) = action.pick(mascot, rng, Some(action.turning))? else {
        return Err(ActionError::NoPose);
    };
    if action.turning && action.time_of(mascot) >= action.animations[index].duration {
        action.turning = false;
        index = action
            .pick(mascot, rng, Some(false))?
            .ok_or(ActionError::NoPose)?;
    }
    let time = action.time_of(mascot);
    let pose = pose_at(&action.animations[index], time)?;
    apply_pose(mascot, pose);

    if target_x != TARGET_MAX {
        let passed = (mascot.look_right && mascot.anchor.x >= target_x)
            || (!mascot.look_right && mascot.anchor.x <= target_x);
        if passed {
            mascot.anchor.x = target_x;
        }
    }
    if target_y != TARGET_MAX {
        let passed =
            (down && mascot.anchor.y >= target_y) || (!down && mascot.anchor.y <= target_y);
        if passed {
            mascot.anchor.y = target_y;
        }
    }
    Ok(())
}

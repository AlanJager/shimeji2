//! `BorderedAction`: choose the border once in `init`, ride it in `tick`.

use shimeji_expr::JavaRandom;

use super::{ActionError, RunningAction};
use crate::mascot::Mascot;

pub(super) fn grab(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    let Some(value) = action.eval_raw("BorderType", mascot, rng)? else {
        action.border = None;
        return Ok(());
    };
    let text = action.as_string(value)?;
    action.border = match text.as_str() {
        "Ceiling" | "天井" => Some(mascot.env.ceiling(mascot.anchor)),
        "Wall" | "壁" => Some(mascot.env.wall(mascot.anchor, mascot.look_right)),
        "Floor" | "地面" => Some(mascot.env.floor(mascot.anchor)),
        _ => None,
    };
    Ok(())
}

pub(super) fn ride(action: &RunningAction, mascot: &mut Mascot) {
    if let Some(border) = &action.border {
        mascot.anchor = mascot.env.move_on(border, mascot.anchor);
    }
}

pub(super) fn lost_ground(action: &RunningAction, mascot: &Mascot) -> Result<(), ActionError> {
    if let Some(border) = &action.border {
        if !mascot.env.is_on(border, mascot.anchor) {
            return Err(ActionError::LostGround);
        }
    }
    Ok(())
}

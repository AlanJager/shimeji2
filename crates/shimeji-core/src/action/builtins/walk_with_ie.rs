//! `WalkWithIE`. `Move`, after a corner check, then a window-move request.

use shimeji_expr::JavaRandom;

use super::super::{ActionError, RunningAction};
use super::{holds_corner, ie_offsets, queue_ie_move};
use crate::action::move_;
use crate::mascot::Mascot;

pub(crate) fn has_next(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    Ok(mascot.throwing && move_::has_next(action, mascot, rng)?)
}

pub(crate) fn tick(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    let (offset_x, offset_y) = ie_offsets(action, mascot, rng)?;
    match holds_corner(mascot, offset_x, offset_y) {
        None | Some(false) => return Err(ActionError::LostGround),
        Some(true) => {}
    }
    move_::tick(action, mascot, rng)?;
    queue_ie_move(mascot, offset_x, offset_y);
    Ok(())
}

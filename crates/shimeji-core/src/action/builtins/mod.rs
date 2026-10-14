//! The ten built-in action classes. Fast-follow classes stay on [`super::RunsAs::Stay`].

pub(super) mod breed;
pub(super) mod dragged;
pub(super) mod fall;
pub(super) mod fall_with_ie;
pub(super) mod jump;
pub(super) mod look;
pub(super) mod offset;
pub(super) mod regist;
pub(super) mod throw_ie;
pub(super) mod walk_with_ie;

use shimeji_expr::JavaRandom;

use super::{ActionError, RunningAction};
use crate::mascot::Mascot;

/// `IeOffsetX` / `IeOffsetY`. The vendored IE actions do not scale these.
pub(crate) fn ie_offsets(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<(i32, i32), ActionError> {
    let x = action.eval_int("IeOffsetX", 0, mascot, rng)?;
    let y = action.eval_int("IeOffsetY", 0, mascot, rng)?;
    Ok((x, y))
}

/// Corner test shared by `WalkWithIE` and `FallWithIE`.
///
/// `None` means the window is invisible. `Some(false)` means the anchor is
/// not on the corner. Both are `LostGround` in the reference.
pub(crate) fn holds_corner(mascot: &Mascot, offset_x: i32, offset_y: i32) -> Option<bool> {
    let ie = mascot.env.active_ie();
    if !ie.visible {
        return None;
    }
    let y_ok = mascot.anchor.y.wrapping_add(offset_y) == ie.bottom;
    let x_ok = if mascot.look_right {
        mascot.anchor.x.wrapping_sub(offset_x) == ie.left
    } else {
        mascot.anchor.x.wrapping_add(offset_x) == ie.right
    };
    Some(x_ok && y_ok)
}

/// Top-left the reference passes to `moveActiveIE` after the body tick.
pub(crate) fn carried_origin(mascot: &Mascot, offset_x: i32, offset_y: i32) -> Option<(i32, i32)> {
    let ie = mascot.env.active_ie();
    if !ie.visible {
        return None;
    }
    let y = mascot
        .anchor
        .y
        .wrapping_add(offset_y)
        .wrapping_sub(ie.height());
    let x = if mascot.look_right {
        mascot.anchor.x.wrapping_sub(offset_x)
    } else {
        mascot
            .anchor
            .x
            .wrapping_add(offset_x)
            .wrapping_sub(ie.width())
    };
    Some((x, y))
}

pub(crate) fn queue_ie_move(mascot: &mut Mascot, offset_x: i32, offset_y: i32) {
    if let Some(origin) = carried_origin(mascot, offset_x, offset_y) {
        mascot.ie_move = Some(origin);
    }
}

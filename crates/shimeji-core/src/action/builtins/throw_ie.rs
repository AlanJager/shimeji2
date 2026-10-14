//! `ThrowIE`. `Animate`, then throw the window captured in `init`.

use shimeji_expr::JavaRandom;

use super::super::{ActionError, Extra, RunningAction, java_round};
use crate::action::{animate, stay};
use crate::mascot::Mascot;

pub(crate) fn init(action: &mut RunningAction, mascot: &Mascot) -> Result<(), ActionError> {
    action.extra = Extra::ThrowIe {
        scaling: mascot.scaling,
        window_id: mascot.env.active_id(),
    };
    Ok(())
}

pub(crate) fn has_next(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    let Extra::ThrowIe { window_id, .. } = action.extra else {
        return Ok(false);
    };
    Ok(mascot.throwing
        && animate::has_next(action, mascot, rng)?
        && mascot.env.active_ie().visible
        && window_id == mascot.env.active_id())
}

pub(crate) fn tick(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    let Extra::ThrowIe { scaling, .. } = action.extra else {
        return Ok(());
    };
    stay::tick(action, mascot, rng)?;
    let ie = *mascot.env.active_ie();
    if !ie.visible {
        return Ok(());
    }
    let vx = java_round(f64::from(action.eval_int("InitialVX", 32, mascot, rng)?) * scaling);
    let vy = action.eval_int("InitialVY", -10, mascot, rng)?;
    let gravity = action.eval_f64("Gravity", 0.5, mascot, rng)?;
    let time = action.time_of(mascot);
    let dy = java_round(f64::from(vy) * scaling + f64::from(time) * gravity * scaling);
    let dx = if mascot.look_right {
        vx
    } else {
        vx.wrapping_neg()
    };
    mascot.ie_move = Some((ie.left.wrapping_add(dx), ie.top.wrapping_add(dy)));
    Ok(())
}

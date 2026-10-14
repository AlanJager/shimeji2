//! `Regist`. Stays while the cursor's x is within 5 px of the anchor, then
//! one facing draw and `LostGround`. Does not move the anchor.

use shimeji_expr::{JavaRandom, Value};

use super::super::{ActionError, Extra, RunningAction, java_round};
use crate::mascot::Mascot;

pub(crate) fn init(action: &mut RunningAction, mascot: &Mascot) -> Result<(), ActionError> {
    action.extra = Extra::Regist {
        scaling: mascot.scaling,
    };
    Ok(())
}

fn offset_x(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<i32, ActionError> {
    let scaling = match action.extra {
        Extra::Regist { scaling } => scaling,
        _ => mascot.scaling,
    };
    let mut offset = java_round(f64::from(action.eval_int("OffsetX", 0, mascot, rng)?) * scaling);
    if is_origin(action, mascot, rng)? {
        let center_x = mascot.image_anchor.map(|point| point.x).unwrap_or(0);
        offset = center_x.wrapping_sub(offset);
    }
    Ok(offset)
}

pub(crate) fn has_next(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    if !action.base_has_next(mascot, rng)? {
        return Ok(false);
    }
    let offset = offset_x(action, mascot, rng)?;
    let delta = mascot
        .env
        .cursor()
        .x
        .wrapping_sub(mascot.anchor.x)
        .wrapping_add(offset)
        .unsigned_abs();
    Ok(delta < 5)
}

pub(crate) fn tick(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    mascot.dragging = true;
    action.apply_current(mascot, rng)?;
    let duration = action.current_duration(mascot, rng)?;
    if action.time_of(mascot) + 1 >= duration {
        mascot.look_right = rng.next_double() < 0.5;
        return Err(ActionError::LostGround);
    }
    Ok(())
}

fn is_origin(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    let Some(value) = action.eval_raw("OffsetType", mascot, rng)? else {
        return Ok(false);
    };
    match value {
        Value::Str(_) => Ok(action.value_text(value) == Some("Origin")),
        other => Err(ActionError::BadCast {
            expected: "string",
            got: other.type_name(),
        }),
    }
}

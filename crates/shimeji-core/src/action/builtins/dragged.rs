//! `Dragged`. The anchor follows the cursor. `footX` springs. One escape draw
//! in `tick` when `time == timeToResist - 1`. `hasNext` draws nothing.

use shimeji_expr::{JavaRandom, Value};

use super::super::{ActionError, Extra, RunningAction, java_round};
use crate::mascot::Mascot;

pub(crate) fn init(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    let scaling = mascot.scaling;
    let offset_x = action.eval_int("OffsetX", 0, mascot, rng)?;
    let foot_x =
        f64::from(mascot.env.cursor().x) + f64::from(java_round(f64::from(offset_x) * scaling));
    action.extra = Extra::Dragged {
        foot_x,
        foot_dx: 0.0,
        time_to_resist: 250,
        scaling,
    };
    Ok(())
}

pub(crate) fn has_next(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    let time_to_resist = match action.extra {
        Extra::Dragged { time_to_resist, .. } => time_to_resist,
        _ => 250,
    };
    Ok(action.base_has_next(mascot, rng)? && action.time_of(mascot) < time_to_resist)
}

pub(crate) fn tick(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    let Extra::Dragged {
        mut foot_x,
        mut foot_dx,
        mut time_to_resist,
        scaling,
    } = action.extra
    else {
        return Ok(());
    };
    mascot.look_right = false;
    mascot.dragging = true;
    let cursor = mascot.env.cursor();
    let mut offset_x = java_round(f64::from(action.eval_int("OffsetX", 0, mascot, rng)?) * scaling);
    let mut offset_y =
        java_round(f64::from(action.eval_int("OffsetY", 120, mascot, rng)?) * scaling);
    if is_origin(action, mascot, rng)? {
        let center = mascot.image_anchor.unwrap_or(crate::env::Point::new(0, 0));
        offset_x = center.x.wrapping_sub(offset_x);
        offset_y = center.y.wrapping_sub(offset_y);
    }
    if cursor
        .x
        .wrapping_add(offset_x)
        .wrapping_sub(mascot.anchor.x)
        .unsigned_abs()
        >= 5
    {
        action.set_time(mascot, 0);
    }
    foot_dx = (foot_dx + (f64::from(cursor.x) - foot_x) * 0.1) * 0.8;
    foot_x += foot_dx;
    let names = action.foot_names();
    action.put_number(names.1, foot_dx);
    action.put_number(names.0, foot_x);
    action.extra = Extra::Dragged {
        foot_x,
        foot_dx,
        time_to_resist,
        scaling,
    };
    action.apply_current(mascot, rng)?;
    let cursor = mascot.env.cursor();
    mascot.anchor.x = cursor.x.wrapping_add(offset_x);
    mascot.anchor.y = cursor.y.wrapping_add(offset_y);
    let time = action.time_of(mascot);
    if time == time_to_resist - 1 && rng.next_double() >= 0.1 {
        time_to_resist += 1;
        action.extra = Extra::Dragged {
            foot_x,
            foot_dx,
            time_to_resist,
            scaling,
        };
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

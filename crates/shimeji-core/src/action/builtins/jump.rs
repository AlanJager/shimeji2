//! `Jump`. Arc toward `(TargetX, TargetY - |dx| / 2)`, then a snap when the
//! remaining distance is within `VelocityParam`.

use shimeji_expr::JavaRandom;

use super::super::{ActionError, Extra, RunningAction, java_round};
use crate::mascot::Mascot;

pub(crate) fn init(action: &mut RunningAction, mascot: &Mascot) -> Result<(), ActionError> {
    action.extra = Extra::Jump {
        scaling: mascot.scaling,
    };
    Ok(())
}

fn measure(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<(i32, i32, f64), ActionError> {
    let target_x = action.eval_int("TargetX", 0, mascot, rng)?;
    let target_y = action.eval_int("TargetY", 0, mascot, rng)?;
    let distance_x = f64::from(target_x.wrapping_sub(mascot.anchor.x));
    let distance_y = f64::from(target_y.wrapping_sub(mascot.anchor.y)) - distance_x.abs() / 2.0;
    let distance = (distance_x * distance_x + distance_y * distance_y).sqrt();
    Ok((target_x, target_y, distance))
}

pub(crate) fn has_next(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    if !action.base_has_next(mascot, rng)? {
        return Ok(false);
    }
    let (_, _, distance) = measure(action, mascot, rng)?;
    Ok(distance != 0.0)
}

pub(crate) fn tick(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    let scaling = match action.extra {
        Extra::Jump { scaling } => scaling,
        _ => mascot.scaling,
    };
    let target_x = action.eval_int("TargetX", 0, mascot, rng)?;
    let target_y = action.eval_int("TargetY", 0, mascot, rng)?;
    if mascot.anchor.x != target_x {
        mascot.look_right = mascot.anchor.x < target_x;
    }
    let distance_x = f64::from(target_x.wrapping_sub(mascot.anchor.x));
    let distance_y = f64::from(target_y.wrapping_sub(mascot.anchor.y)) - distance_x.abs() / 2.0;
    let distance = (distance_x * distance_x + distance_y * distance_y).sqrt();
    let velocity = action.eval_f64("VelocityParam", 20.0, mascot, rng)? * scaling;
    if distance != 0.0 {
        let velocity_x = velocity * distance_x / distance;
        let velocity_y = velocity * distance_y / distance;
        let names = action.velocity_names();
        action.put_number(names.0, velocity_x);
        action.put_number(names.1, velocity_y);
        mascot.anchor.x = mascot.anchor.x.wrapping_add(java_round(velocity_x));
        mascot.anchor.y = mascot.anchor.y.wrapping_add(java_round(velocity_y));
        action.apply_current(mascot, rng)?;
    }
    if distance <= velocity {
        mascot.anchor.x = target_x;
        mascot.anchor.y = target_y;
    }
    Ok(())
}

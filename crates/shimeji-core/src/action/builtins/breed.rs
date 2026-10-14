//! `Breed`. `Animate`, then on the penultimate frame spawn `BornCount` children.
//! `BornInterval` is not consulted (contract disagreement 10). A spawn the
//! cap refuses evaluates no `Born*` script.

use shimeji_expr::{JavaRandom, Value};

use super::super::{ActionError, Extra, RunningAction};
use crate::action::stay;
use crate::env::Point;
use crate::mascot::{Birth, Mascot};

pub(crate) fn init(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    action.extra = Extra::Breed {
        scaling: mascot.scaling,
    };
    let count = action.eval_int("BornCount", 1, mascot, rng)?;
    if count < 1 {
        return Err(ActionError::Compile(
            "BornCount must be positive".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn tick(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    stay::tick(action, mascot, rng)?;
    let time = action.time_of(mascot);
    let Ok(duration) = action.current_duration(mascot, rng) else {
        return Ok(());
    };
    if time == duration - 1 && enabled(action, mascot, rng)? {
        produce(action, mascot, rng)?;
    }
    Ok(())
}

fn enabled(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    let transient = action.eval_bool("BornTransient", false, mascot, rng)?;
    Ok(if transient {
        mascot.transients
    } else {
        mascot.breeding
    })
}

fn produce(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    let scaling = match action.extra {
        Extra::Breed { scaling } => scaling,
        _ => mascot.scaling,
    };
    let mut born = 0;
    loop {
        if mascot.spawn_room == 0 {
            break;
        }
        let count = action.eval_int("BornCount", 1, mascot, rng)?;
        if born >= count {
            break;
        }
        let born_x = action.eval_int("BornX", 0, mascot, rng)?;
        let born_y = action.eval_int("BornY", 0, mascot, rng)?;
        let behavior = born_behavior(action, mascot, rng)?;
        let dx = java_delta(born_x, scaling);
        let dy = java_delta(born_y, scaling);
        let x = if mascot.look_right {
            mascot.anchor.x.wrapping_sub(dx)
        } else {
            mascot.anchor.x.wrapping_add(dx)
        };
        if !behavior.is_empty() {
            mascot.births.push(Birth {
                anchor: Point::new(x, mascot.anchor.y.wrapping_add(dy)),
                look_right: mascot.look_right,
                behavior,
            });
            mascot.spawn_room -= 1;
        }
        born += 1;
    }
    Ok(())
}

fn java_delta(value: i32, scaling: f64) -> i32 {
    super::super::java_round(f64::from(value) * scaling)
}

fn born_behavior(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<String, ActionError> {
    let Some(value) = action.eval_raw("BornBehavior", mascot, rng)? else {
        return Ok(String::new());
    };
    match value {
        Value::Str(_) => Ok(action.value_text(value).unwrap_or("").to_string()),
        other => Err(ActionError::BadCast {
            expected: "string",
            got: other.type_name(),
        }),
    }
}

//! `Fall`. Gravity, air resistance, sub-pixel accumulation, an interpolated
//! step, and an 80 px upward floor probe. Does not extend `BorderedAction`.

use shimeji_expr::JavaRandom;

use super::super::{ActionError, Extra, RunningAction, java_round};
use crate::env::Point;
use crate::mascot::Mascot;

const PROBE: i32 = 80;

pub(crate) fn init(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    let scaling = mascot.scaling;
    let vx = f64::from(action.eval_int("InitialVX", 0, mascot, rng)?) * scaling;
    let vy = f64::from(action.eval_int("InitialVY", 0, mascot, rng)?) * scaling;
    // Optional pack extension. Reference Fall semantics remain the default.
    let deploy_at_apex = action.eval_bool("DeployAtApex", false, mascot, rng)?;
    let deployed_at = (deploy_at_apex && vy >= 0.0).then_some(0);
    if deploy_at_apex {
        action.put_number("DeployTime", if deployed_at.is_some() { 0.0 } else { -1.0 });
    }
    action.extra = Extra::Fall {
        vx,
        vy,
        mod_x: 0.0,
        mod_y: 0.0,
        scaling,
        deploy_at_apex,
        deployed_at,
    };
    Ok(())
}

pub(crate) fn has_next(
    action: &mut RunningAction,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    if !action.base_has_next(mascot, rng)? {
        return Ok(false);
    }
    let Extra::Fall { vy, .. } = action.extra else {
        return Ok(false);
    };
    let on_floor = mascot.env.on_floor(mascot.anchor, false);
    let on_wall = mascot.env.on_wall(mascot.anchor, mascot.look_right, false);
    Ok((vy < 0.0 || !on_floor) && !on_wall)
}

pub(crate) fn tick(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    let Extra::Fall {
        vx,
        vy,
        mod_x,
        mod_y,
        scaling,
        deploy_at_apex,
        mut deployed_at,
    } = action.extra
    else {
        return Ok(());
    };
    if vx != 0.0 {
        mascot.look_right = vx > 0.0;
    }
    let mut vy = vy;
    if deploy_at_apex {
        // A retained throw reaching the ceiling has reached its visible apex.
        let top = mascot
            .env
            .work_area()
            .top
            .saturating_add(mascot.image_anchor.map_or(0, |p| p.y.max(0)));
        if deployed_at.is_none() && mascot.keep_on_screen && mascot.anchor.y <= top && vy < 0.0 {
            deployed_at = Some(action.time_of(mascot));
            vy = 0.0;
        }
        let elapsed = deployed_at.map_or(-1, |start| action.time_of(mascot).wrapping_sub(start));
        action.put_number("DeployTime", f64::from(elapsed));
    }
    let resistance_x = action.eval_f64("ResistanceX", 0.05, mascot, rng)?;
    let resistance_y = action.eval_f64("ResistanceY", 0.1, mascot, rng)?;
    let gravity = action.eval_f64("Gravity", 2.0, mascot, rng)?;
    let vx = vx - vx * resistance_x;
    let vy = vy - vy * resistance_y + gravity * scaling;
    if deploy_at_apex && deployed_at.is_none() && vy >= 0.0 {
        deployed_at = Some(action.time_of(mascot));
        action.put_number("DeployTime", 0.0);
    }
    let names = action.velocity_names();
    action.put_number(names.0, vx);
    action.put_number(names.1, vy);

    let mut mod_x = mod_x + (vx % 1.0);
    let mut mod_y = mod_y + (vy % 1.0);
    let dx = java_round(vx + mod_x);
    let dy = java_round(vy + mod_y);
    mod_x %= 1.0;
    mod_y %= 1.0;
    action.extra = Extra::Fall {
        vx,
        vy,
        mod_x,
        mod_y,
        scaling,
        deploy_at_apex,
        deployed_at,
    };

    let dev = 1.max(dx.wrapping_abs()).max(dy.wrapping_abs());
    let anchor_x = mascot.anchor.x;
    let anchor_y = mascot.anchor.y;
    'steps: for step in 0..=dev {
        let x = anchor_x.wrapping_add(dx.wrapping_mul(step).wrapping_div(dev));
        let y = anchor_y.wrapping_add(dy.wrapping_mul(step).wrapping_div(dev));
        if dy > 0 {
            for probe in -PROBE..=0 {
                mascot.anchor = Point::new(x, y.wrapping_add(probe));
                if mascot.env.on_floor(mascot.anchor, true) {
                    break 'steps;
                }
            }
        } else {
            mascot.anchor = Point::new(x, y);
        }
        if mascot.env.on_wall(mascot.anchor, mascot.look_right, true) {
            break;
        }
    }
    action.apply_current(mascot, rng)
}

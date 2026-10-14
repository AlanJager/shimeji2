//! Shimeji model, compiler, engine, and diagnostics.

pub mod action;
mod behavior;
pub mod caps;
pub mod compile;
pub mod delta;
pub mod diagnostics;
pub mod engine;
pub mod env;
mod manager;
pub mod mascot;
pub mod model;
pub mod window_control;

#[cfg(feature = "testing")]
pub mod testing;

pub use action::{ActionError, RunningAction};
pub use caps::{DEFAULT_EVAL_BUDGET, DEFAULT_MASCOT_CAP, HARD_MASCOT_CAP};
pub use compile::{Program, compile_character};
pub use delta::{MascotDelta, MascotId, TickDelta};
pub use engine::{Engine, EngineSettings, MascotFrame, MouseButton};
pub use mascot::Mascot;
pub use shimeji_expr::{JavaRandom, Value};
pub use window_control::{NopWindow, SoundEvent, WindowControl, WindowEvent, WindowMove};

/// Crate name. Dependents call this so the workspace link is real.
pub fn crate_name() -> &'static str {
    let _linked = shimeji_expr::crate_name();
    "shimeji-core"
}

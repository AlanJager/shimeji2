//! Scenario replay and the trace writer. Enabled with the `testing` feature.
//!
//! The runner follows the reference harness: apply tick 0 (cursor placed),
//! spawn, then for each tick inject that tick's mouse events and call
//! [`Engine::tick`](crate::Engine::tick). Tick 0 is applied a second time,
//! and that second apply smooths the cursor.

mod scenario;
mod trace;

pub use scenario::{Scenario, ScenarioError};
pub use trace::{Divergence, RunOutput, first_divergence, run, run_output};

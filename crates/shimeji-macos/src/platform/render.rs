//! Renderer trait. The panel backend is the MVP; an overlay can replace it.

use shimeji_core::delta::{MascotDelta, MascotId};
use shimeji_core::engine::MascotFrame;
use shimeji_core::env::Point;

/// What the clock commits each tick. Implementors own on-screen presence.
pub trait Renderer {
    fn attach(&mut self, id: MascotId, frame: &MascotFrame);
    fn detach(&mut self, id: MascotId);
    /// Dirty-only: contents when the image key changes, layer offset on every
    /// move, `setFrameOrigin` only when the sprite leaves the slack.
    fn commit(&mut self, frames: &[MascotFrame], deltas: &[MascotDelta]);
    fn hit_test(&self, engine_point: Point) -> Option<MascotId>;
    fn show_all(&mut self);
    fn hide_all(&mut self);
}

/// Counters the clock prints on exit. Sitting pets send 0 contents and 0
/// window moves.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommitCounters {
    pub contents_sets: u64,
    pub layer_moves: u64,
    pub window_moves: u64,
}

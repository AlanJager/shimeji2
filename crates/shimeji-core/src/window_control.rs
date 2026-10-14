//! Non-blocking window moves. The engine asks; the platform answers next tick.
//!
//! [`NopWindow`] is the default. With it, a carry's believed rectangle and the
//! next snapshot disagree, and the carry ends the way the reference ends a
//! carry when the window did not move.

use crate::delta::MascotId;

/// One `moveActiveIE` the engine wants applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowMove {
    pub id: u64,
    /// Top-left, in screen pixels.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Result of a previous [`WindowMove`], delivered before the next tick's snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowEvent {
    /// The window server placed the window here.
    Moved {
        id: u64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    /// The request was rejected. The snapshot stays the truth.
    Failed { id: u64 },
}

/// A pose sound. The shell plays it; the engine only records the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundEvent {
    pub mascot: MascotId,
    pub name: String,
    pub volume: Option<String>,
}

/// Asks the platform to move a window. Must not block.
///
/// The engine does not call this itself. The runtime (or the shell) does,
/// then feeds a [`WindowEvent`] or an updated snapshot on the next tick.
pub trait WindowControl {
    fn request_move(&mut self, request: &WindowMove);
}

/// Accepts every request and reports nothing.
#[derive(Debug, Default)]
pub struct NopWindow;

impl WindowControl for NopWindow {
    fn request_move(&mut self, _request: &WindowMove) {}
}

/// How far a confirmed window may sit from the rectangle the action requested.
///
/// One pixel absorbs a window-server clamp. The reference's corner test is
/// exact, so a confirmed move is written back at the requested point and the
/// carry continues. Shime's slowest IE walk is 2 px per tick, so a window
/// that did not move (the no-op control, or a rejected request) disagrees by
/// at least 2 px and the carry ends.
pub const CARRY_TOLERANCE_PX: i32 = 1;

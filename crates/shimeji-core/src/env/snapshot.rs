//! Platform snapshot. The engine does not collect it.

use super::WorkRect;
use super::area::Point;

/// One display. `work` omitted means the work area equals the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSample {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub work: Option<WorkRect>,
}

/// Active window. `visible: false` keeps the last rectangle and clears `isOn`.
///
/// `id` is the platform window identity. `0` means "the current visible window"
/// and is stored as `1` so ThrowIE can tell "gone" (`0`) from "still this one".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSample {
    pub visible: bool,
    pub id: u64,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// What the platform publishes once per tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub screens: Vec<ScreenSample>,
    pub cursor: Point,
    pub active_window: Option<WindowSample>,
    /// When false, [`super::MascotEnvironment::sync`] keeps the sticky work area.
    pub multiscreen: bool,
}

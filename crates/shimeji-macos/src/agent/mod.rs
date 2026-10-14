//! Agent reactions on the Mac: the socket listener, the badge, and focus.
//!
//! The listener thread never touches AppKit. It hands a parsed event to the
//! main queue, the same way a worker wakes the clock.

mod badge;
pub(crate) mod focus;
pub mod listener;
pub(crate) mod react;

pub(crate) use badge::Badge;
pub(crate) use focus::note_frontmost;
pub(crate) use react::{flush_pending, hooks_word, note_pose, place_badge, start, stop};

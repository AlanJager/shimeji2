//! The one-time first-run decisions. Launch at login itself is `SMAppService`
//! in the macOS shell; this module only records that the popover was shown
//! and how the first pet should appear.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "first-run.json";

/// The popover's checkbox starts off. The system service is not registered
/// until the user turns it on.
pub const LAUNCH_AT_LOGIN_DEFAULT: bool = false;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FirstRun {
    pub seen: bool,
}

/// What this launch should do before the pet is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub show_popover: bool,
    /// The usual spawn: the pet drops from above the main display.
    pub drop_from_top: bool,
    /// Reduce Motion, and Animate anyway is off. Place the pet on the floor.
    pub place_on_floor: bool,
    /// The popover names the Animate anyway setting.
    pub mention_animate_anyway: bool,
    /// Always [`LAUNCH_AT_LOGIN_DEFAULT`] until the user opts in.
    pub launch_at_login: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct File {
    seen: bool,
}

/// Missing or unreadable file means the popover has not been shown.
pub fn load(support_dir: &Path) -> FirstRun {
    let path = support_dir.join(FILE_NAME);
    let Ok(bytes) = fs::read(&path) else {
        return FirstRun::default();
    };
    match serde_json::from_slice::<File>(&bytes) {
        Ok(file) => FirstRun { seen: file.seen },
        Err(_) => FirstRun::default(),
    }
}

/// Record that the popover was shown. The next launch does not show it.
pub fn mark_seen(support_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(support_dir)?;
    let bytes = serde_json::to_vec_pretty(&File { seen: true }).map_err(io::Error::other)?;
    let path = support_dir.join(FILE_NAME);
    let tmp = support_dir.join("first-run.json.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// `reduce_motion` is the system setting. `animate_anyway` is the user's override.
pub fn plan(state: FirstRun, reduce_motion: bool, animate_anyway: bool) -> Plan {
    let place_on_floor = reduce_motion && !animate_anyway;
    Plan {
        show_popover: !state.seen,
        drop_from_top: !place_on_floor,
        place_on_floor,
        mention_animate_anyway: reduce_motion && !state.seen,
        launch_at_login: LAUNCH_AT_LOGIN_DEFAULT,
    }
}

/// Foot anchor on the bottom edge of a work area, centered horizontally.
///
/// Engine coordinates: x grows right, y grows down. `bottom` is the work-area
/// floor. The caller passes the main display's work rect.
pub fn floor_anchor(left: i32, right: i32, bottom: i32) -> (i32, i32) {
    let width = right.saturating_sub(left);
    let x = left.saturating_add(width / 2);
    (x, bottom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch(label: &str) -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "shimeji-u10-first-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_file_has_not_been_seen_and_the_popover_shows_once() {
        let dir = scratch("missing");
        let state = load(&dir);
        assert!(!state.seen);
        let first = plan(state, false, false);
        assert!(first.show_popover);
        assert!(first.drop_from_top);
        assert!(!first.place_on_floor);
        assert!(!first.mention_animate_anyway);
        assert!(!first.launch_at_login);
        mark_seen(&dir).unwrap();
        let again = plan(load(&dir), false, false);
        assert!(!again.show_popover);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reduce_motion_places_the_pet_on_the_floor_and_mentions_animate_anyway() {
        let still = plan(FirstRun { seen: false }, true, false);
        assert!(still.place_on_floor);
        assert!(!still.drop_from_top);
        assert!(still.mention_animate_anyway);
        assert!(still.show_popover);
        let moving = plan(FirstRun { seen: false }, true, true);
        assert!(!moving.place_on_floor);
        assert!(moving.drop_from_top);
        assert!(moving.mention_animate_anyway);
        let later = plan(FirstRun { seen: true }, true, false);
        assert!(later.place_on_floor);
        assert!(!later.show_popover);
        assert!(!later.mention_animate_anyway);
    }

    #[test]
    fn floor_anchor_is_centered_on_the_work_area_bottom() {
        assert_eq!(floor_anchor(0, 1280, 800), (640, 800));
        assert_eq!(floor_anchor(100, 200, 50), (150, 50));
    }
}

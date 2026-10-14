//! Where a newly added pet starts falling, and when its window can be created.
//!
//! The engine's own off-screen respawn still picks an x across the whole work
//! area. The shell uses this for the launch spawn and the status menu's Add,
//! so those pets appear in the middle half.

use std::collections::VecDeque;

use shimeji_core::JavaRandom;
use shimeji_core::delta::MascotId;
use shimeji_core::env::{Point, WindowSample};

use super::coords::Rect;

/// Inclusive x interval `[left + width/4, right - width/4]`.
///
/// A work area too narrow for that inset collapses to one x.
pub fn middle_half_x(work: Rect) -> (i32, i32) {
    let inset = work.width / 4;
    let min_x = work.x.saturating_add(inset);
    let max_x = work.right().saturating_sub(inset);
    if max_x < min_x {
        let mid = work.x.saturating_add(work.width / 2);
        (mid, mid)
    } else {
        (min_x, max_x)
    }
}

/// `top - 256`. Same vertical placement as the engine's off-screen drop.
pub fn y_above_top(top: i32) -> i32 {
    top.wrapping_sub(256)
}

/// One anchor in [`middle_half_x`], at [`y_above_top`].
///
/// `rng` is the shell's stream, seeded from the run's seed, so `--seed` repeats
/// the same x sequence.
pub fn spawn_anchor(work: Rect, rng: &mut JavaRandom) -> Point {
    let (min_x, max_x) = middle_half_x(work);
    let span = i64::from(max_x) - i64::from(min_x) + 1;
    let span = i32::try_from(span).unwrap_or(i32::MAX).max(1);
    let x = min_x.saturating_add(rng.next_int_bound(span));
    Point::new(x, y_above_top(work.y))
}

/// A window close to the desktop ceiling can catch an above-screen drop before
/// its body becomes visible. Start below that top instead, with room for the
/// character above its anchor. Zero headroom leaves reference placement intact.
pub fn visible_drop_anchor(
    mut anchor: Point,
    work: Rect,
    window: Option<&WindowSample>,
    headroom: i32,
) -> Point {
    let Some(window) = window else {
        return anchor;
    };
    let visible_y = work.y.saturating_add(headroom);
    if headroom > 0
        && window.visible
        && window.width > 0
        && window.height > 0
        && window.x <= anchor.x
        && anchor.x <= window.x.saturating_add(window.width)
        && anchor.y < window.y
        && window.y < visible_y
    {
        anchor.y = visible_y.min(work.bottom());
    }
    anchor
}

/// Ids whose windows can be created now.
///
/// `SpawnAt` reports the id on the frame that queues the mascot. The mascot
/// joins `frames()` on the next tick, when the engine flushes that queue.
/// `awaiting` holds ids that are not live yet. An id that is still missing
/// after a later tick, and was not reported again this frame, was removed
/// before it was admitted.
pub fn take_ready(
    awaiting: &mut Vec<MascotId>,
    spawned: &[MascotId],
    ticked: bool,
    is_live: impl Fn(MascotId) -> bool,
) -> Vec<MascotId> {
    for id in spawned {
        if !awaiting.contains(id) {
            awaiting.push(*id);
        }
    }
    let pending = std::mem::take(awaiting);
    let mut ready = Vec::new();
    for id in pending {
        if is_live(id) {
            ready.push(id);
        } else if !ticked || spawned.contains(&id) {
            awaiting.push(id);
        }
    }
    ready
}

/// Pair `SpawnAt` anchors with the ids reported for them, in order.
///
/// `buildNextBehavior` teleports a pet that is above the screen, because no
/// behavior matches there. The runtime publishes this frame's snapshot before
/// the spawn, which is not enough for the bundled program: nothing above the
/// screen has weight, so the selection still teleports, onto the real screen.
/// The shell writes these anchors back with `MoveFrozen` before that pet's
/// first tick. Ids past the queue (a birth during the same frame) are left
/// alone.
pub fn take_fixups(pending: &mut VecDeque<Point>, spawned: &[MascotId]) -> Vec<(MascotId, Point)> {
    let mut paired = Vec::new();
    for id in spawned {
        let Some(anchor) = pending.pop_front() else {
            break;
        };
        paired.push((*id, anchor));
    }
    paired
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_x_range_is_the_middle_half_and_y_is_above_the_top() {
        let work = Rect::new(0, 0, 1920, 1080);
        assert_eq!(middle_half_x(work), (480, 1440));
        assert_eq!(y_above_top(work.y), -256);

        let shifted = Rect::new(100, 40, 800, 600);
        assert_eq!(middle_half_x(shifted), (300, 700));
        assert_eq!(y_above_top(shifted.y), -216);

        let odd = Rect::new(10, 5, 1001, 100);
        assert_eq!(middle_half_x(odd), (260, 761));
        assert_eq!(y_above_top(odd.y), -251);
    }

    #[test]
    fn a_seeded_draw_stays_in_that_range_and_repeats() {
        let work = Rect::new(0, 0, 1920, 1080);
        let (min_x, max_x) = middle_half_x(work);
        let mut rng = JavaRandom::new(1);
        let mut again = JavaRandom::new(1);
        for _ in 0..32 {
            let point = spawn_anchor(work, &mut rng);
            assert_eq!(point, spawn_anchor(work, &mut again));
            assert!(point.x >= min_x && point.x <= max_x, "{}", point.x);
            assert_eq!(point.y, -256);
        }
        let mut other = JavaRandom::new(2);
        let from_one = {
            let mut first = JavaRandom::new(1);
            spawn_anchor(work, &mut first)
        };
        let from_two = spawn_anchor(work, &mut other);
        assert_ne!(from_one.x, from_two.x);
    }

    #[test]
    fn a_spawn_at_id_waits_until_the_mascot_is_live() {
        let mut awaiting = Vec::new();
        let spawned = [MascotId(1)];
        let ready = take_ready(&mut awaiting, &spawned, false, |_| false);
        assert!(ready.is_empty());
        assert_eq!(awaiting, vec![MascotId(1)]);

        let ready = take_ready(&mut awaiting, &[MascotId(1)], false, |_| false);
        assert!(ready.is_empty());
        assert_eq!(awaiting, vec![MascotId(1)]);

        let ready = take_ready(&mut awaiting, &[], false, |_| false);
        assert!(ready.is_empty());
        assert_eq!(awaiting, vec![MascotId(1)]);

        let ready = take_ready(&mut awaiting, &[], true, |id| id == MascotId(1));
        assert_eq!(ready, vec![MascotId(1)]);
        assert!(awaiting.is_empty());
    }

    #[test]
    fn a_mascot_already_live_on_the_spawn_frame_is_ready() {
        let mut awaiting = Vec::new();
        let ready = take_ready(&mut awaiting, &[MascotId(7)], true, |id| id == MascotId(7));
        assert_eq!(ready, vec![MascotId(7)]);
        assert!(awaiting.is_empty());
    }

    #[test]
    fn a_birth_during_the_tick_waits_and_a_missed_flush_is_dropped() {
        let mut awaiting = Vec::new();
        let ready = take_ready(&mut awaiting, &[MascotId(3)], true, |_| false);
        assert!(ready.is_empty());
        assert_eq!(awaiting, vec![MascotId(3)]);

        let ready = take_ready(&mut awaiting, &[], true, |_| false);
        assert!(ready.is_empty());
        assert!(awaiting.is_empty());
    }

    #[test]
    fn drop_anchors_pair_with_spawned_ids_and_leave_a_later_birth() {
        let mut pending = VecDeque::from([Point::new(480, -256), Point::new(900, -256)]);
        let spawned = [MascotId(1), MascotId(2), MascotId(9)];
        assert_eq!(
            take_fixups(&mut pending, &spawned),
            vec![
                (MascotId(1), Point::new(480, -256)),
                (MascotId(2), Point::new(900, -256))
            ]
        );
        assert!(pending.is_empty());
        assert!(take_fixups(&mut pending, &[MascotId(9)]).is_empty());
    }
}

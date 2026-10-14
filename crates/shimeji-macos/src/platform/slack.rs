//! Slack-panel geometry. Pure integer math; the panel backend commits it.
//!
//! Default `slack = 4` gives a 384 px margin around a 128 px sprite (panel
//! 896×896). The layer tracks the
//! sprite every tick; the window moves only when the sprite would leave the
//! margin. Windows are placed so the slack hangs *ahead* of the sprite's
//! motion, and so the sprite — not the window — stays reachable.

use super::coords::Rect;
use super::poses::OpaqueSpan;
use shimeji_core::env::Point;

/// Default slack multiplier. Margin is `(slack - 1) * sprite` on each side.
pub const DEFAULT_SLACK: u32 = 4;

/// Whether the panel window should take `next` as its content size.
///
/// Either axis counts. A pose may keep the previous frame's width and differ
/// in height; comparing width alone clips the taller frame.
pub fn panel_needs_resize(current: (f64, f64), next: (f64, f64)) -> bool {
    (current.0 - next.0).abs() > 0.5 || (current.1 - next.1).abs() > 0.5
}

/// Panel size for a sprite at `slack`. `slack == 1` is a snug window.
pub fn panel_size(sprite_w: i32, sprite_h: i32, slack: u32) -> (i32, i32) {
    let factor = slack.max(1).saturating_mul(2).saturating_sub(1) as i32;
    (
        sprite_w.saturating_mul(factor).max(sprite_w),
        sprite_h.saturating_mul(factor).max(sprite_h),
    )
}

/// Transparent margin on each side.
pub fn margin(sprite_w: i32, sprite_h: i32, slack: u32) -> (i32, i32) {
    let (panel_w, panel_h) = panel_size(sprite_w, sprite_h, slack);
    (
        (panel_w.wrapping_sub(sprite_w) / 2).max(0),
        (panel_h.wrapping_sub(sprite_h) / 2).max(0),
    )
}

/// Engine-space sprite top-left from the mascot anchor and pose centre.
pub fn sprite_origin(anchor: Point, image_anchor: Point) -> Point {
    Point::new(
        anchor.x.wrapping_sub(image_anchor.x),
        anchor.y.wrapping_sub(image_anchor.y),
    )
}

/// Pose anchor used to place the sprite.
///
/// A mirrored left-facing image is flipped about the layer centre, so the
/// anchor is `width - anchor.x` (the reference right-hand image). An explicit
/// right-facing image is its own pixels and keeps the pose anchor. `y` is unchanged.
pub fn placement_anchor(
    anchor: Point,
    width: i32,
    look_right: bool,
    has_right_image: bool,
) -> Point {
    if look_right && !has_right_image {
        Point::new(width.wrapping_sub(anchor.x), anchor.y)
    } else {
        anchor
    }
}

/// Shift `sprite.x` so the opaque columns stay inside the work area that holds
/// `anchor`. Transparent margins may cross the edge. The engine anchor is not
/// moved. `y` is never changed: a pet enters by dropping from above the screen.
///
/// `opaque` is that image's horizontal extent (`right` exclusive). A mirrored
/// pose passes the flipped span. An empty image passes the full width, which
/// is the old rectangle clamp.
///
/// The work area is the one that contains `anchor`, including its edges. When
/// the anchor is above or below every work area but still inside one's horizontal
/// span, that span is used. A side whose neighboring work edge is within 1 px,
/// with overlapping vertical ranges, is left open so the pet can cross displays.
/// An opaque extent wider than the chosen work area is centred in it.
pub fn clamp_sprite_x(mut sprite: Rect, anchor: Point, works: &[Rect], opaque: OpaqueSpan) -> Rect {
    let Some(index) = work_index_for_anchor(anchor, works) else {
        return sprite;
    };
    let work = works[index];
    let span = opaque.clipped(sprite.width);
    let visible = span.right.wrapping_sub(span.left);
    if visible > work.width {
        let centred = work.x.wrapping_add(work.width.wrapping_sub(visible) / 2);
        sprite.x = centred.wrapping_sub(span.left);
        return sprite;
    }
    let open_left = works
        .iter()
        .enumerate()
        .any(|(other, rect)| other != index && side_continues_left(work, *rect));
    let open_right = works
        .iter()
        .enumerate()
        .any(|(other, rect)| other != index && side_continues_right(work, *rect));
    let left_edge = sprite.x.wrapping_add(span.left);
    let right_edge = sprite.x.wrapping_add(span.right);
    if !open_left && left_edge < work.x {
        sprite.x = work.x.wrapping_sub(span.left);
    } else if !open_right && right_edge > work.right() {
        sprite.x = work.right().wrapping_sub(span.right);
    }
    sprite
}

fn work_index_for_anchor(anchor: Point, works: &[Rect]) -> Option<usize> {
    if let Some(index) = works.iter().position(|work| contains_anchor(*work, anchor)) {
        return Some(index);
    }
    let mut best: Option<(i32, usize)> = None;
    for (index, work) in works.iter().enumerate() {
        if anchor.x < work.x || anchor.x > work.right() {
            continue;
        }
        let dy = if anchor.y < work.y {
            work.y.wrapping_sub(anchor.y)
        } else if anchor.y > work.bottom() {
            anchor.y.wrapping_sub(work.bottom())
        } else {
            0
        };
        if best.is_none_or(|(best_dy, _)| dy < best_dy) {
            best = Some((dy, index));
        }
    }
    best.map(|(_, index)| index)
}

fn contains_anchor(work: Rect, anchor: Point) -> bool {
    anchor.x >= work.x
        && anchor.x <= work.right()
        && anchor.y >= work.y
        && anchor.y <= work.bottom()
}

fn vertical_overlap(a: Rect, b: Rect) -> bool {
    a.y < b.bottom() && b.y < a.bottom()
}

fn within_one(a: i32, b: i32) -> bool {
    let gap = a.wrapping_sub(b);
    (-1..=1).contains(&gap)
}

/// `other` meets `this`'s left edge within 1 px and their vertical ranges overlap.
fn side_continues_left(this: Rect, other: Rect) -> bool {
    within_one(this.x, other.right()) && vertical_overlap(this, other)
}

fn side_continues_right(this: Rect, other: Rect) -> bool {
    within_one(this.right(), other.x) && vertical_overlap(this, other)
}

/// Whether the sprite, in panel-local engine coordinates, sits fully inside
/// the slack region. Local origin is the panel's top-left (engine space).
pub fn sprite_inside(
    local: Point,
    sprite_w: i32,
    sprite_h: i32,
    panel_w: i32,
    panel_h: i32,
) -> bool {
    local.x >= 0
        && local.y >= 0
        && local.x.wrapping_add(sprite_w) <= panel_w
        && local.y.wrapping_add(sprite_h) <= panel_h
}

/// Next panel origin (engine, top-left) that puts the full slack in front of
/// `delta`. When the sprite is first placed, it is centred if it fits.
pub fn next_panel_origin(
    sprite: Rect,
    current_panel: Option<Rect>,
    panel_w: i32,
    panel_h: i32,
    delta: Point,
) -> Rect {
    if let Some(panel) = current_panel {
        let local = Point::new(
            sprite.x.wrapping_sub(panel.x),
            sprite.y.wrapping_sub(panel.y),
        );
        if sprite_inside(local, sprite.width, sprite.height, panel_w, panel_h) {
            return panel;
        }
        let x = origin_along(sprite.x, sprite.width, panel_w, delta.x);
        let y = origin_along(sprite.y, sprite.height, panel_h, delta.y);
        Rect::new(x, y, panel_w, panel_h)
    } else {
        let x = sprite
            .x
            .wrapping_sub((panel_w.wrapping_sub(sprite.width)) / 2);
        let y = sprite
            .y
            .wrapping_sub((panel_h.wrapping_sub(sprite.height)) / 2);
        Rect::new(x, y, panel_w, panel_h)
    }
}

fn origin_along(sprite: i32, sprite_span: i32, panel_span: i32, delta: i32) -> i32 {
    let hang = panel_span.wrapping_sub(sprite_span);
    if delta > 0 {
        sprite
    } else if delta < 0 {
        sprite.wrapping_sub(hang)
    } else {
        sprite.wrapping_sub(hang / 2)
    }
}

/// Shift `panel` so the sprite stays inside it after a window-server clamp.
pub fn keep_sprite_inside(sprite: Rect, mut panel: Rect) -> Rect {
    if sprite.x < panel.x {
        panel.x = sprite.x;
    } else if sprite.right() > panel.right() {
        panel.x = sprite.right().wrapping_sub(panel.width);
    }
    if sprite.y < panel.y {
        panel.y = sprite.y;
    } else if sprite.bottom() > panel.bottom() {
        panel.y = sprite.bottom().wrapping_sub(panel.height);
    }
    panel
}

/// Clamp the *sprite* into `reach` (union of screens), then place the panel
/// around it. The slack may hang off the display.
pub fn clamp_sprite_to_reach(mut sprite: Rect, reach: Rect) -> Rect {
    if sprite.width > reach.width || sprite.x < reach.x {
        sprite.x = reach.x;
    } else if sprite.right() > reach.right() {
        sprite.x = reach.right().wrapping_sub(sprite.width);
    }
    if sprite.height > reach.height || sprite.y < reach.y {
        sprite.y = reach.y;
    } else if sprite.bottom() > reach.bottom() {
        sprite.y = reach.bottom().wrapping_sub(sprite.height);
    }
    sprite
}

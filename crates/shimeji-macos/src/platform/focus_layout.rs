//! Native focus-card placement in Cocoa coordinates (bottom-left, y up).
//! Pure geometry keeps multi-display and screen-edge policy testable headlessly.

use super::coords::Rect;

pub const CARD_WIDTH: i32 = 224;
pub const CARD_HEIGHT: i32 = 44;
const HEAD_GAP: i64 = 6;

/// Center over the sprite's head, using the nearest display's work area.
/// If there is no room above, place below the sprite, then clamp to that work
/// area. A very small work area keeps the fixed card size and its origin inside.
pub fn head_card(sprite: Rect, work_areas: &[Rect]) -> Rect {
    let x = i64::from(sprite.x) + (i64::from(sprite.width) - i64::from(CARD_WIDTH)) / 2;
    let above = i64::from(sprite.y) + i64::from(sprite.height) + HEAD_GAP;
    let Some(work) = work_areas
        .iter()
        .filter(|area| area.width > 0 && area.height > 0)
        .min_by_key(|area| distance_to_area(sprite, **area))
    else {
        return card(x, above);
    };
    let left = i64::from(work.x);
    let bottom = i64::from(work.y);
    let right = left + i64::from(work.width);
    let top = bottom + i64::from(work.height);
    let y = if above + i64::from(CARD_HEIGHT) > top {
        i64::from(sprite.y) - i64::from(CARD_HEIGHT) - HEAD_GAP
    } else {
        above
    };
    card(
        x.clamp(left, (right - i64::from(CARD_WIDTH)).max(left)),
        y.clamp(bottom, (top - i64::from(CARD_HEIGHT)).max(bottom)),
    )
}

fn card(x: i64, y: i64) -> Rect {
    Rect::new(
        x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        CARD_WIDTH,
        CARD_HEIGHT,
    )
}

fn distance_to_area(sprite: Rect, area: Rect) -> i128 {
    // Doubled coordinates keep odd-sized sprites centered without floats.
    let center_x = i64::from(sprite.x) * 2 + i64::from(sprite.width);
    let center_y = i64::from(sprite.y) * 2 + i64::from(sprite.height);
    let left = i64::from(area.x) * 2;
    let right = (i64::from(area.x) + i64::from(area.width)) * 2;
    let bottom = i64::from(area.y) * 2;
    let top = (i64::from(area.y) + i64::from(area.height)) * 2;
    let dx = i128::from(center_x - center_x.clamp(left, right));
    let dy = i128::from(center_y - center_y.clamp(bottom, top));
    dx * dx + dy * dy
}

//! Slack-panel geometry. No WindowServer.

use shimeji_core::env::Point;
use shimeji_macos::platform::coords::Rect;
use shimeji_macos::platform::poses::{OpaqueSpan, opaque_columns};
use shimeji_macos::platform::slack::{
    DEFAULT_SLACK, clamp_sprite_to_reach, clamp_sprite_x, keep_sprite_inside, next_panel_origin,
    panel_needs_resize, panel_size, placement_anchor, sprite_inside, sprite_origin,
};

/// The pre-fix clamp: every column counts, including clear ones.
fn clamp_full(sprite: Rect, anchor: Point, works: &[Rect]) -> Rect {
    clamp_sprite_x(sprite, anchor, works, OpaqueSpan::full(sprite.width))
}

#[test]
fn a_same_width_taller_panel_needs_a_resize() {
    assert!(panel_needs_resize((128.0, 128.0), (128.0, 200.0)));
    assert!(!panel_needs_resize((128.0, 128.0), (128.0, 128.0)));
    assert!(panel_needs_resize((128.0, 128.0), (200.0, 128.0)));
}

#[test]
fn default_slack_four_is_896_around_a_128_sprite() {
    assert_eq!(DEFAULT_SLACK, 4);
    assert_eq!(panel_size(128, 128, 4), (896, 896));
    assert_eq!(panel_size(128, 128, 1), (128, 128));
}

#[test]
fn window_moves_only_when_the_sprite_leaves_the_margin() {
    let sprite = Rect::new(100, 200, 128, 128);
    let first = next_panel_origin(sprite, None, 896, 896, Point::new(0, 0));
    assert_eq!(first, Rect::new(100 - 384, 200 - 384, 896, 896));
    let still = Rect::new(120, 200, 128, 128);
    let same = next_panel_origin(still, Some(first), 896, 896, Point::new(20, 0));
    assert_eq!(same, first);
    assert!(sprite_inside(
        Point::new(still.x - first.x, still.y - first.y),
        128,
        128,
        896,
        896
    ));
    let leaving = Rect::new(first.right(), 200, 128, 128);
    let moved = next_panel_origin(leaving, Some(first), 896, 896, Point::new(20, 0));
    assert_eq!(moved.x, leaving.x);
    assert_ne!(moved, first);
}

#[test]
fn clamp_keeps_the_sprite_reachable() {
    let reach = Rect::new(0, 0, 1920, 1080);
    let hanging = Rect::new(-10, 10, 128, 128);
    let clamped = clamp_sprite_to_reach(hanging, reach);
    assert_eq!(clamped.x, 0);
    assert_eq!(clamped.y, 10);
    let panel = Rect::new(-384, -374, 896, 896);
    let kept = keep_sprite_inside(clamped, panel);
    assert!(sprite_inside(
        Point::new(clamped.x - kept.x, clamped.y - kept.y),
        128,
        128,
        896,
        896
    ));
}

#[test]
fn sprite_origin_subtracts_the_pose_anchor() {
    assert_eq!(
        sprite_origin(Point::new(400, 500), Point::new(64, 128)),
        Point::new(336, 372)
    );
}

#[test]
fn a_centred_anchor_is_unchanged_when_mirrored() {
    let anchor = Point::new(64, 128);
    assert_eq!(placement_anchor(anchor, 128, true, false), anchor);
}

#[test]
fn a_mirrored_anchor_of_96_in_128_becomes_32() {
    let placed = placement_anchor(Point::new(96, 110), 128, true, false);
    assert_eq!(placed, Point::new(32, 110));
    let engine = Point::new(400, 500);
    let mirrored = sprite_origin(engine, placed);
    let uncorrected = sprite_origin(engine, Point::new(96, 110));
    assert_eq!(mirrored.x - uncorrected.x, 64);
    assert_eq!(mirrored.y, uncorrected.y);
}

#[test]
fn an_explicit_right_facing_image_is_not_adjusted() {
    let anchor = Point::new(96, 110);
    assert_eq!(placement_anchor(anchor, 128, true, true), anchor);
    assert_eq!(placement_anchor(anchor, 128, false, false), anchor);
}

#[test]
fn air_wall_pushes_a_left_edge_centred_sprite_in_by_64() {
    let work = Rect::new(0, 0, 1920, 1080);
    let anchor = Point::new(0, 1080);
    let sprite = Rect::new(-64, 952, 128, 128);
    let clamped = clamp_full(sprite, anchor, &[work]);
    assert_eq!(clamped.x, 0);
    assert_eq!(clamped.y, 952);
}

#[test]
fn air_wall_leaves_a_sprite_that_is_fully_inside() {
    let work = Rect::new(0, 0, 1920, 1080);
    let sprite = Rect::new(400, 200, 128, 128);
    let anchor = Point::new(464, 328);
    assert_eq!(clamp_full(sprite, anchor, &[work]), sprite);
}

#[test]
fn air_wall_pushes_a_right_edge_centred_sprite_in_by_64() {
    let work = Rect::new(0, 0, 1920, 1080);
    let anchor = Point::new(1920, 1080);
    let sprite = Rect::new(1856, 952, 128, 128);
    let clamped = clamp_full(sprite, anchor, &[work]);
    assert_eq!(clamped.x, 1792);
    assert_eq!(clamped.right(), 1920);
    assert_eq!(clamped.y, 952);
}

#[test]
fn air_wall_does_not_clamp_a_side_with_an_adjacent_screen() {
    let left = Rect::new(0, 0, 1920, 1080);
    let right = Rect::new(1920, 0, 1920, 1080);
    let anchor = Point::new(1930, 500);
    let sprite = Rect::new(1866, 372, 128, 128);
    assert_eq!(clamp_full(sprite, anchor, &[left, right]), sprite);

    let gap = Rect::new(1921, 0, 1440, 900);
    let gap_anchor = Point::new(1921, 400);
    let gap_sprite = Rect::new(1857, 272, 128, 128);
    assert_eq!(clamp_full(gap_sprite, gap_anchor, &[left, gap]), gap_sprite);

    let far = Rect::new(1922, 0, 1440, 900);
    let far_anchor = Point::new(1922, 400);
    let far_sprite = Rect::new(1858, 272, 128, 128);
    let clamped = clamp_full(far_sprite, far_anchor, &[left, far]);
    assert_eq!(clamped.x, 1922);
    assert_eq!(clamped.y, 272);

    let stacked = Rect::new(-1920, 1080, 1920, 1080);
    let edge = Point::new(0, 500);
    let hanging = Rect::new(-64, 372, 128, 128);
    let held = clamp_full(hanging, edge, &[left, stacked]);
    assert_eq!(held.x, 0);
    assert_eq!(held.y, 372);
}

#[test]
fn air_wall_centres_a_sprite_wider_than_the_work_area() {
    let work = Rect::new(100, 50, 80, 1000);
    let sprite = Rect::new(0, 10, 128, 128);
    let anchor = Point::new(140, 100);
    let clamped = clamp_full(sprite, anchor, &[work]);
    assert_eq!(clamped.x, 76);
    assert_eq!(clamped.y, 10);
}

#[test]
fn air_wall_clamps_x_of_a_pet_above_the_screen_and_leaves_y() {
    let work = Rect::new(0, 0, 1920, 1080);
    let anchor = Point::new(0, -72);
    let sprite = Rect::new(-64, -200, 128, 128);
    let clamped = clamp_full(sprite, anchor, &[work]);
    assert_eq!(clamped.x, 0);
    assert_eq!(clamped.y, -200);
}

#[test]
fn air_wall_stops_on_the_opaque_columns() {
    let work = Rect::new(0, 0, 1920, 1080);
    let mut rgba = vec![0u8; 128 * 4];
    for column in 31..128 {
        rgba[column * 4 + 3] = 255;
    }
    let span = opaque_columns(&rgba, 128, 1);
    assert_eq!(
        span,
        OpaqueSpan {
            left: 31,
            right: 128
        }
    );

    let anchor = Point::new(0, 1080);
    let sprite = Rect::new(-64, 952, 128, 128);
    let clamped = clamp_sprite_x(sprite, anchor, &[work], span);
    assert_eq!(clamped.x, -31);
    assert_eq!(clamped.x + span.left, work.x);
    assert_eq!(clamped.y, 952);

    let mirrored = span.mirrored(128);
    assert_eq!(mirrored, OpaqueSpan { left: 0, right: 97 });
    let right_anchor = Point::new(1920, 1080);
    let right_sprite = Rect::new(1856, 952, 128, 128);
    let right = clamp_sprite_x(right_sprite, right_anchor, &[work], mirrored);
    assert_eq!(right.x, 1823);
    assert_eq!(right.x + mirrored.right, work.right());
    assert_eq!(128 - mirrored.right, 31);
    assert_eq!(right.y, 952);

    let full = opaque_columns(&[255u8; 128 * 4], 128, 1);
    assert_eq!(full, OpaqueSpan::full(128));
    assert_eq!(clamp_sprite_x(sprite, anchor, &[work], full).x, 0);
    assert_eq!(
        clamp_sprite_x(right_sprite, right_anchor, &[work], full).x,
        1792
    );

    let empty = opaque_columns(&[0u8; 128 * 4], 128, 1);
    assert_eq!(empty, OpaqueSpan::full(128));
    assert_eq!(clamp_sprite_x(sprite, anchor, &[work], empty).x, 0);
    assert_eq!(
        clamp_sprite_x(right_sprite, right_anchor, &[work], empty).x,
        1792
    );
}

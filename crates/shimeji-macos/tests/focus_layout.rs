use shimeji_macos::platform::coords::Rect;
use shimeji_macos::platform::focus_layout::{CARD_HEIGHT, CARD_WIDTH, head_card};

#[test]
fn centers_the_card_above_the_head_with_a_gap() {
    let sprite = Rect::new(400, 100, 128, 128);
    assert_eq!(
        head_card(sprite, &[Rect::new(0, 0, 1200, 800)]),
        Rect::new(352, 234, CARD_WIDTH, CARD_HEIGHT)
    );
}

#[test]
fn clamps_at_screen_edges_and_flips_below_a_high_head() {
    let work = [Rect::new(20, 40, 1000, 700)];
    assert_eq!(head_card(Rect::new(20, 40, 64, 64), &work).x, 20);
    assert_eq!(head_card(Rect::new(980, 40, 40, 64), &work).x, 796);
    assert_eq!(head_card(Rect::new(400, 700, 64, 40), &work).y, 650);
}

#[test]
fn uses_the_pets_display_including_negative_cocoa_coordinates() {
    let work = [
        Rect::new(0, 0, 1440, 900),
        Rect::new(-1920, -200, 1920, 1080),
    ];
    assert_eq!(
        head_card(Rect::new(-1000, -100, 100, 100), &work),
        Rect::new(-1062, 6, CARD_WIDTH, CARD_HEIGHT)
    );
    let outside = head_card(Rect::new(-2000, -300, 50, 50), &work);
    assert_eq!((outside.x, outside.y), (-1920, -200));
}

#[test]
fn invalid_or_tiny_screens_and_extreme_coordinates_do_not_panic() {
    assert_eq!(
        head_card(Rect::new(0, 0, 100, 100), &[]),
        Rect::new(-62, 106, CARD_WIDTH, CARD_HEIGHT)
    );
    assert_eq!(
        head_card(Rect::new(0, 0, 100, 100), &[Rect::new(0, 0, 0, 0)]),
        Rect::new(-62, 106, CARD_WIDTH, CARD_HEIGHT)
    );
    assert_eq!(
        head_card(Rect::new(0, 0, 100, 100), &[Rect::new(5, 6, 20, 20)]),
        Rect::new(5, 6, CARD_WIDTH, CARD_HEIGHT)
    );
    let edge = head_card(
        Rect::new(i32::MAX, i32::MAX, i32::MAX, i32::MAX),
        &[Rect::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX)],
    );
    assert_eq!((edge.width, edge.height), (CARD_WIDTH, CARD_HEIGHT));
}

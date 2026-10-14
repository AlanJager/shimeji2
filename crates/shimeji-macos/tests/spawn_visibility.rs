//! Initial drop visibility around a window near the desktop ceiling.
use shimeji_core::env::{Point, WindowSample};
use shimeji_macos::platform::coords::Rect;
use shimeji_macos::platform::spawn::visible_drop_anchor;

fn window(y: i32) -> WindowSample {
    WindowSample {
        id: 7,
        visible: true,
        x: 49,
        y,
        width: 1871,
        height: 1080 - y,
    }
}

#[test]
fn a_maximised_window_cannot_catch_the_initial_drop_above_the_visible_area() {
    let work = Rect::new(49, 25, 1871, 1055);
    let anchor = visible_drop_anchor(Point::new(667, -231), work, Some(&window(25)), 128);
    assert_eq!(anchor, Point::new(667, 153));
    assert!(anchor.y - 128 >= work.y);
    assert!(anchor.y > window(25).y);
}

#[test]
fn an_ordinary_window_with_headroom_keeps_the_original_drop() {
    let work = Rect::new(49, 25, 1871, 1055);
    let drop = Point::new(667, -231);
    assert_eq!(
        visible_drop_anchor(drop, work, Some(&window(200)), 128),
        drop
    );
    let mut beside = window(25);
    beside.x = 900;
    assert_eq!(visible_drop_anchor(drop, work, Some(&beside), 128), drop);
}

#[test]
fn retention_off_or_no_standable_window_keeps_the_original_drop() {
    let work = Rect::new(49, 25, 1871, 1055);
    let drop = Point::new(667, -231);
    assert_eq!(visible_drop_anchor(drop, work, Some(&window(25)), 0), drop);
    assert_eq!(visible_drop_anchor(drop, work, None, 128), drop);
    let mut full_screen = window(0);
    full_screen.visible = false;
    assert_eq!(
        visible_drop_anchor(drop, work, Some(&full_screen), 128),
        drop
    );
}

#[test]
fn a_scaled_character_on_a_shifted_display_still_starts_with_headroom() {
    let work = Rect::new(-1920, -300, 1920, 1080);
    let mut maximised = window(-300);
    maximised.x = -1920;
    maximised.width = 1920;
    let anchor = visible_drop_anchor(Point::new(-1000, -556), work, Some(&maximised), 256);
    assert_eq!(anchor, Point::new(-1000, -44));
    assert!(anchor.y - 256 >= work.y);
}

#[test]
fn the_bundled_pet_drops_to_the_desktop_floor_past_a_maximised_window_top() {
    use shimeji_core::Engine;
    use shimeji_core::env::{ScreenSample, Snapshot, WorkRect};
    let character = shimeji_packs::load_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/packs/shime"
    ))
    .unwrap()
    .pack
    .characters
    .remove(0);
    let work = Rect::new(49, 25, 1871, 1055);
    let snapshot = Snapshot {
        screens: vec![ScreenSample {
            id: "main".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            work: Some(WorkRect {
                x: work.x,
                y: work.y,
                width: work.width,
                height: work.height,
            }),
        }],
        cursor: Point::new(1000, 600),
        active_window: Some(window(25)),
        multiscreen: true,
    };
    let drop = visible_drop_anchor(
        Point::new(667, -231),
        work,
        snapshot.active_window.as_ref(),
        128,
    );
    let mut engine = Engine::new(&character, 1);
    engine.apply(&snapshot);
    let id = engine.spawn_at(drop).unwrap();
    // The shell writes its drop back after the engine's initial selection.
    engine.move_frozen(id, drop);
    let mut reached_floor = false;
    for _ in 0..60 {
        engine.tick(&snapshot);
        let frame = &engine.frames()[0];
        assert!(frame.anchor.y >= 153, "{frame:?}");
        if frame.anchor.y == work.bottom() {
            reached_floor = true;
            break;
        }
    }
    assert!(reached_floor);
}

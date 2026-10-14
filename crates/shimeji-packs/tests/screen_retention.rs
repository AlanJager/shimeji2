//! Opt-in throw boundaries; the default engine remains reference-compatible.
use shimeji_core::env::{Point, ScreenSample, Snapshot, WorkRect};
use shimeji_core::{Engine, EngineSettings, MouseButton};

fn thrown(keep: bool, multiscreen: bool, upward_only: bool) -> (Engine, Snapshot) {
    let c = shimeji_packs::load_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/packs/shime"
    ))
    .unwrap()
    .pack
    .characters
    .remove(0);
    let mut snapshot = Snapshot {
        screens: vec![ScreenSample {
            id: "main".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            work: Some(WorkRect {
                x: 49,
                y: 25,
                width: 1871,
                height: 1055,
            }),
        }],
        cursor: Point::new(1000, 600),
        active_window: None,
        multiscreen,
    };
    if multiscreen {
        snapshot.screens.push(ScreenSample {
            id: "right".into(),
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
            work: None,
        });
    }
    let mut engine = Engine::new(&c, 1);
    engine.set_settings(EngineSettings {
        keep_on_screen: keep,
        ..EngineSettings::default()
    });
    engine.apply(&snapshot);
    let id = engine.spawn_at(Point::new(960, 500)).unwrap();
    engine.tick(&snapshot);
    engine.move_frozen(id, Point::new(960, 500));
    engine.press(Some(id), MouseButton::Left);
    for cursor in [
        Point::new(1000, 600),
        Point::new(1000, 600),
        Point::new(if upward_only { 1000 } else { 1700 }, 0),
    ] {
        snapshot.cursor = cursor;
        engine.tick(&snapshot);
    }
    engine.release(Some(id), MouseButton::Left);
    engine.tick(&snapshot);
    (engine, snapshot)
}

#[test]
fn an_unconstrained_fast_throw_retains_reference_recycling() {
    assert!(!EngineSettings::default().keep_on_screen);
    let (engine, _) = thrown(false, false, false);
    let frame = &engine.frames()[0];
    assert_eq!(frame.behavior, "落下する");
    assert_eq!(frame.anchor.y, -231);
}

#[test]
fn a_retained_fast_throw_stops_and_grabs_the_edge_without_recycling() {
    let (mut engine, snapshot) = thrown(true, false, false);
    let frame = &engine.frames()[0];
    assert_eq!(frame.behavior, "投げられる");
    assert_eq!(frame.anchor.x, 1920);
    assert!(frame.anchor.y >= 153);
    for _ in 0..30 {
        engine.tick(&snapshot);
        let frame = &engine.frames()[0];
        assert_eq!(frame.action, "壁に掴まる");
        assert_eq!(frame.anchor.x, 1920);
        assert!((25..=1080).contains(&frame.anchor.y));
    }
}

#[test]
fn an_upward_throw_stops_below_the_top_and_eventually_falls() {
    let (mut engine, snapshot) = thrown(true, false, true);
    assert_eq!(engine.frames()[0].anchor, Point::new(1000, 153));
    for _ in 0..60 {
        engine.tick(&snapshot);
        assert!(engine.frames()[0].anchor.y >= 153);
    }
    assert!(engine.frames()[0].anchor.y > 153);
}

#[test]
fn retention_does_not_create_a_wall_between_adjacent_displays() {
    let (engine, _) = thrown(true, true, false);
    let frame = &engine.frames()[0];
    assert_eq!(frame.behavior, "投げられる");
    assert!(frame.anchor.x > 1920 && frame.anchor.x < 3840);
    assert!(frame.anchor.y >= 128);
}

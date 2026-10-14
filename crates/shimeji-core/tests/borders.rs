//! Borders, cursor smoothing, and the sticky work area.
//!
//! The 60 px / 90 px floor snap is `Fall.tick` in the vendored source, not
//! `FloorCeiling`. A point that far above a floor is not `isOn`. Part C owns the snap.

use shimeji_core::action::ActionError;
use shimeji_core::env::{Area, Cursor, Environment, Point, ScreenSample, Snapshot, WindowSample};
use shimeji_core::model::{
    Action, ActionKind, Animation, Character, Information, OrderedMap, Param, Pose, Schema,
    ScriptSource,
};
use shimeji_core::{JavaRandom, Mascot, compile_character};

fn param(key: &str, raw: &str) -> Param {
    let (value, error) = ScriptSource::parse_attribute(raw);
    assert!(error.is_none(), "{raw}");
    Param {
        key: key.to_string(),
        source_name: key.to_string(),
        value,
        line: 1,
    }
}

fn stay_on_floor() -> shimeji_core::RunningAction {
    let mut params = OrderedMap::new();
    params.insert("BorderType", param("BorderType", "Floor"));
    let action = Action {
        name: Some("Sit".to_string()),
        schema: Schema::English,
        kind: ActionKind::Stay,
        degraded_from: None,
        params,
        animations: vec![Animation {
            condition: None,
            is_turn: false,
            poses: vec![Pose {
                image_source: Some("/sit.png".to_string()),
                image: None,
                image_right_source: None,
                image_right: None,
                right_sibling: None,
                anchor_x: 0,
                anchor_y: 0,
                velocity_x: 0,
                velocity_y: 0,
                duration_ticks: 4,
                sound: None,
                volume: None,
                line: 1,
            }],
            hotspots: Vec::new(),
            line: 1,
        }],
        children: Vec::new(),
        line: 1,
    };
    let mut actions = OrderedMap::new();
    actions.insert("Sit", action);
    let character = Character {
        name: "test".to_string(),
        schema: Schema::English,
        behavior_schema: Schema::English,
        image_root: String::new(),
        actions_file: "actions.xml".to_string(),
        behaviors_file: "behaviors.xml".to_string(),
        info_file: None,
        constants: OrderedMap::new(),
        actions,
        behaviors: OrderedMap::new(),
        behavior_tree: Vec::new(),
        info: Information::default(),
    };
    compile_character(&character).instantiate("Sit").unwrap()
}

fn desktop(window: WindowSample) -> Snapshot {
    Snapshot {
        screens: vec![ScreenSample {
            id: "main".to_string(),
            x: 0,
            y: 0,
            width: 800,
            height: 600,
            work: None,
        }],
        cursor: Point::new(0, 0),
        active_window: Some(window),
        multiscreen: false,
    }
}

fn window(x: i32, visible: bool) -> WindowSample {
    WindowSample {
        visible,
        id: 0,
        x,
        y: 100,
        width: 200,
        height: 150,
    }
}

/// Establish the window, then move it, so the border delta is the move itself.
fn ride(dx: i32) -> Result<Point, ActionError> {
    let mut world = Environment::new();
    world.apply(&desktop(window(0, true)));
    let mut mascot = Mascot::new();
    mascot.anchor = Point::new(20, 100);
    mascot.sync(&world, false);

    let mut action = stay_on_floor();
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng)?;

    world.apply(&desktop(window(dx, true)));
    mascot.sync(&world, false);
    action.next(&mut mascot, &mut rng)?;
    Ok(mascot.anchor)
}

#[test]
fn floor_is_on_is_exact_and_does_not_snap_from_above() {
    let mut area = Area::default();
    area.set(0, 100, 400, 300);
    let floor = area.top_border();
    assert!(floor.is_on(Point::new(0, 100)));
    assert!(floor.is_on(Point::new(400, 100)));
    assert!(!floor.is_on(Point::new(401, 100)));
    assert!(!floor.is_on(Point::new(50, 101)));
    // 60 px and 90 px above the floor. Fall's probe would consider the first; this type does not.
    assert!(!floor.is_on(Point::new(50, 40)));
    assert!(!floor.is_on(Point::new(50, 10)));

    let wall = area.left_border();
    assert!(wall.is_on(Point::new(0, 100)));
    assert!(wall.is_on(Point::new(0, 300)));
    assert!(!wall.is_on(Point::new(1, 100)));
}

#[test]
fn window_top_rides_ten_pixels_and_not_one_hundred() {
    assert_eq!(ride(10).unwrap(), Point::new(30, 100));

    // `FloorCeiling.move` leaves the anchor put when the jump is at least 80 px.
    // The window edge is then no longer under it, so `Stay` raises `LostGround`.
    let mut world = Environment::new();
    world.apply(&desktop(window(0, true)));
    let mut mascot = Mascot::new();
    mascot.anchor = Point::new(20, 100);
    mascot.sync(&world, false);
    let mut action = stay_on_floor();
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng).unwrap();
    world.apply(&desktop(window(100, true)));
    mascot.sync(&world, false);
    let error = action.next(&mut mascot, &mut rng).unwrap_err();
    assert!(matches!(error, ActionError::LostGround));
    assert_eq!(mascot.anchor, Point::new(20, 100));
}

#[test]
fn floor_ride_thresholds_match_floor_ceiling_move() {
    let shifted = |dx: i32, dy: i32| {
        let mut area = Area::default();
        area.set(0, 100, 400, 300);
        area.set(dx, 100 + dy, 400 + dx, 300 + dy);
        area.top_border().move_point(Point::new(50, 100))
    };
    assert_eq!(shifted(10, 0), Point::new(60, 100));
    assert_eq!(shifted(79, 0), Point::new(129, 100));
    assert_eq!(shifted(80, 0), Point::new(50, 100));
    assert_eq!(shifted(0, 20), Point::new(50, 120));
    assert_eq!(shifted(0, 21), Point::new(50, 100));
    assert_eq!(shifted(0, -80), Point::new(50, 20));
    assert_eq!(shifted(0, -81), Point::new(50, 100));
}

#[test]
fn lost_ground_when_the_window_disappears() {
    let mut world = Environment::new();
    world.apply(&desktop(window(0, true)));
    let mut mascot = Mascot::new();
    mascot.anchor = Point::new(20, 100);
    mascot.sync(&world, false);

    let mut action = stay_on_floor();
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng).unwrap();
    assert!(action.has_next(&mut mascot, &mut rng).unwrap());

    world.apply(&desktop(window(0, false)));
    mascot.sync(&world, false);
    let error = action.next(&mut mascot, &mut rng).unwrap_err();
    assert!(matches!(error, ActionError::LostGround));
    assert_eq!(mascot.anchor, Point::new(20, 100));
}

#[test]
fn cursor_set_smooths_velocity() {
    let mut cursor = Cursor::default();
    cursor.place(0, 0);
    assert_eq!((cursor.dx, cursor.dy), (0, 0));
    cursor.set(10, 4);
    assert_eq!((cursor.x, cursor.y, cursor.dx, cursor.dy), (10, 4, 5, 2));
    cursor.set(10, 4);
    assert_eq!((cursor.dx, cursor.dy), (2, 1));

    let mut world = Environment::new();
    world.place_cursor(0, 0);
    world.apply(&Snapshot {
        screens: Vec::new(),
        cursor: Point::new(10, 4),
        active_window: None,
        multiscreen: false,
    });
    assert_eq!(world.cursor().dx, 5);
    assert_eq!(world.cursor().dy, 2);
}

#[test]
fn work_area_stays_sticky_until_forced() {
    let snap = |x_cursor: i32| Snapshot {
        screens: vec![
            ScreenSample {
                id: "a".to_string(),
                x: 0,
                y: 0,
                width: 100,
                height: 100,
                work: None,
            },
            ScreenSample {
                id: "b".to_string(),
                x: 100,
                y: 0,
                width: 100,
                height: 100,
                work: None,
            },
        ],
        cursor: Point::new(x_cursor, 10),
        active_window: None,
        multiscreen: false,
    };
    let mut world = Environment::new();
    world.apply(&snap(10));
    let mut mascot = Mascot::new();
    mascot.anchor = Point::new(10, 10);
    mascot.sync(&world, false);
    assert_eq!(mascot.env.work_area().left, 0);

    mascot.anchor = Point::new(150, 10);
    world.apply(&snap(150));
    mascot.sync(&world, false);
    assert_eq!(mascot.env.work_area().left, 0);

    mascot.sync(&world, true);
    assert_eq!(mascot.env.work_area().left, 100);
}

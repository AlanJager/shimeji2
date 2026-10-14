//! Poll choices are pure logic; these tests never call WindowServer.
use shimeji_core::env::{Point, WindowSample};
use shimeji_macos::platform::window_env::{PollPace, pace_for};

fn window() -> WindowSample {
    WindowSample {
        visible: true,
        id: 1,
        x: 200,
        y: 200,
        width: 400,
        height: 300,
    }
}

#[test]
fn far_near_and_riding_anchors_choose_the_planned_rates() {
    let w = window();
    assert_eq!(pace_for(&[Point::new(119, 200)], Some(&w)), PollPace::Far);
    assert_eq!(pace_for(&[Point::new(120, 200)], Some(&w)), PollPace::Near);
    assert_eq!(pace_for(&[Point::new(680, 580)], Some(&w)), PollPace::Near);
    assert_eq!(pace_for(&[Point::new(681, 581)], Some(&w)), PollPace::Far);
    for p in [
        Point::new(400, 200),
        Point::new(400, 500),
        Point::new(200, 350),
        Point::new(600, 350),
    ] {
        assert_eq!(pace_for(&[p], Some(&w)), PollPace::Ride);
    }
    assert_eq!(
        pace_for(&[Point::new(119, 200), Point::new(400, 200)], Some(&w)),
        PollPace::Ride
    );
}

#[test]
fn absent_or_invisible_windows_do_not_make_a_stationary_pet_ride() {
    let mut w = window();
    w.visible = false;
    assert_eq!(pace_for(&[Point::new(400, 200)], Some(&w)), PollPace::Far);
    assert_eq!(pace_for(&[Point::new(400, 200)], None), PollPace::Far);
    assert_eq!(pace_for(&[], Some(&window())), PollPace::Far);
}

fn character() -> shimeji_core::model::Character {
    shimeji_packs::load_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/packs/shime"
    ))
    .unwrap()
    .pack
    .characters
    .remove(0)
}

fn frame(action: &str, anchor: Point) -> shimeji_core::MascotFrame {
    shimeji_core::MascotFrame {
        id: shimeji_core::MascotId(1),
        behavior: String::new(),
        action: action.into(),
        anchor,
        image: None,
        look_right: false,
    }
}

#[test]
fn stopped_overrides_fall_and_carry_and_zero_velocity_carry_stays_fast() {
    use shimeji_macos::platform::window_env::PollingActions;
    let actions = PollingActions::new(&character());
    let far = Point::new(0, 0);
    assert_eq!(
        actions.pace(true, &[frame("Stay", far)], Some(&window())),
        PollPace::Far
    );
    assert_eq!(
        actions.pace(true, &[frame("Fall", far)], None),
        PollPace::Near
    );
    for action in ["WalkWithIE", "FallWithIE", "ThrowIE"] {
        let still = frame(action, far);
        assert_eq!(
            actions.pace(true, std::slice::from_ref(&still), Some(&window())),
            PollPace::Ride
        );
        assert_eq!(
            actions.pace(false, &[still], Some(&window())),
            PollPace::Stopped
        );
    }
    assert_eq!(
        actions.pace(false, &[frame("Fall", far)], None),
        PollPace::Stopped
    );
}

#[test]
fn pack_defined_fall_names_and_reference_overrides_are_recognised() {
    use shimeji_core::model::{
        ActionKind, ActionNode, ActionRef, Builtin, OrderedMap, ScriptSource,
    };
    use shimeji_macos::platform::window_env::PollingActions;
    let mut c = character();
    let fall_name = c
        .actions
        .iter()
        .find(|(_, a)| a.kind == ActionKind::Builtin(Builtin::Fall))
        .unwrap()
        .0
        .to_string();
    let mut fall = c.actions.get(&fall_name).unwrap().clone();
    let mut name = fall.params.get("Name").unwrap().clone();
    name.value = ScriptSource::raw("custom-fall");
    fall.params.insert("Name", name.clone());
    c.actions.insert(fall_name.clone(), fall);
    name.value = ScriptSource::raw("referenced-fall");
    let mut params = OrderedMap::new();
    params.insert("Name", name);
    let sequence_name = c
        .actions
        .iter()
        .find(|(_, a)| a.kind == ActionKind::Sequence)
        .unwrap()
        .0
        .to_string();
    let mut sequence = c.actions.get(&sequence_name).unwrap().clone();
    sequence.children = vec![ActionNode::Reference(ActionRef {
        name: fall_name,
        params,
        line: 1,
    })];
    c.actions.insert(sequence_name, sequence);
    let actions = PollingActions::new(&c);
    for action in ["custom-fall", "referenced-fall"] {
        assert_eq!(
            actions.pace(true, &[frame(action, Point::new(0, 0))], None),
            PollPace::Near
        );
    }
}

#[test]
fn an_extra_character_can_raise_the_shared_poll_rate() {
    use shimeji_macos::platform::window_env::PollingActions;
    let actions = PollingActions::new(&character());
    let primary = actions.pace(true, &[frame("Stay", Point::new(0, 0))], Some(&window()));
    let extra = actions.pace(
        true,
        &[frame("Stay", Point::new(400, 200))],
        Some(&window()),
    );
    assert_eq!(primary.merge(extra), PollPace::Ride);
    assert_eq!(PollPace::Far.merge(PollPace::Near), PollPace::Near);
}

#[test]
fn near_polling_catches_a_new_window_under_a_fall_that_far_polling_misses() {
    use shimeji_core::env::{Environment, ScreenSample, Snapshot};
    use shimeji_core::model::{ActionKind, Builtin, Param, Schema, ScriptSource};
    use shimeji_core::{JavaRandom, Mascot, compile_character};
    fn land_after(poll_ticks: u32) -> i32 {
        let mut c = character();
        let name = c
            .actions
            .iter()
            .find(|(_, a)| a.kind == ActionKind::Builtin(Builtin::Fall))
            .unwrap()
            .0
            .to_string();
        let mut fall = c.actions.get(&name).unwrap().clone();
        fall.schema = Schema::English;
        for key in ["Name", "InitialVY", "Gravity", "ResistanceY"] {
            let raw = match key {
                "Name" => "Fall",
                "InitialVY" => "20",
                _ => "0",
            };
            fall.params.insert(
                key,
                Param {
                    key: key.into(),
                    source_name: key.into(),
                    value: ScriptSource::raw(raw),
                    line: 1,
                },
            );
        }
        c.actions.insert(name.clone(), fall);
        let mut action = compile_character(&c).instantiate(&name).unwrap();
        let mut mascot = Mascot::new();
        mascot.anchor = Point::new(400, 300);
        let mut rng = JavaRandom::new(1);
        let mut world = Environment::new();
        let mut snapshot = Snapshot {
            screens: vec![ScreenSample {
                id: "main".into(),
                x: 0,
                y: 0,
                width: 1920,
                height: 2000,
                work: None,
            }],
            cursor: Point::new(0, 0),
            active_window: None,
            multiscreen: true,
        };
        world.apply(&snapshot);
        mascot.sync(&world, true);
        action.init(&mut mascot, &mut rng).unwrap();
        // A window appears just after the previous poll, 20 px below the pet.
        for tick in 1..=13 {
            if tick == poll_ticks {
                let mut w = window();
                w.y = 320;
                snapshot.active_window = Some(w);
            }
            world.apply(&snapshot);
            mascot.sync(&world, true);
            if !action.has_next(&mut mascot, &mut rng).unwrap() {
                break;
            }
            action.next(&mut mascot, &mut rng).unwrap();
            mascot.time += 1;
        }
        mascot.anchor.y
    }
    assert_eq!(land_after(PollPace::Near.period_ticks()), 320);
    assert!(land_after(PollPace::Far.period_ticks()) > 400);
}

#[test]
fn a_window_matching_the_work_area_still_requires_ride_on_its_top() {
    use shimeji_macos::platform::coords::Rect;
    use shimeji_macos::platform::window_env::{
        SelectInput, WindowRecord, select_active_ie, to_window_sample,
    };
    let windows = [WindowRecord {
        id: 1,
        owner_pid: 7,
        layer: 0,
        alpha: 1.0,
        bounds: Rect::new(49, 25, 1871, 1055),
    }];
    let active = select_active_ie(SelectInput {
        windows: &windows,
        frontmost_pid: 7,
        our_pid: 42,
        screens: &[Rect::new(0, 0, 1920, 1080)],
    })
    .unwrap();
    let sample = to_window_sample(&active);
    assert!(
        sample.visible,
        "maximised to the work area is not full-screen"
    );
    assert_eq!(
        pace_for(&[Point::new(667, 25)], Some(&sample)),
        PollPace::Ride
    );
}

#[test]
fn carry_activity_comes_from_actions_even_when_no_window_move_was_emitted() {
    use shimeji_macos::platform::window_env::PollingActions;
    let actions = PollingActions::new(&character());
    for action in ["WalkWithIE", "FallWithIE", "ThrowIE"] {
        assert_eq!(
            actions.carry_windows(&[frame(action, Point::new(0, 0))], Some(&window())),
            vec![1]
        );
    }
    assert!(
        actions
            .carry_windows(&[frame("Stay", Point::new(0, 0))], Some(&window()))
            .is_empty()
    );
    assert!(actions.carry_windows(&[], Some(&window())).is_empty());
    assert!(
        actions
            .carry_windows(&[frame("WalkWithIE", Point::new(0, 0))], None)
            .is_empty()
    );
    let mut hidden = window();
    hidden.visible = false;
    assert!(
        actions
            .carry_windows(&[frame("WalkWithIE", Point::new(0, 0))], Some(&hidden))
            .is_empty()
    );
}

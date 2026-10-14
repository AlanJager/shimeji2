//! Window edges flush with the desktop must not offer the opposite pose.
use shimeji_core::env::{Point, ScreenSample, Snapshot, WindowSample, WorkRect};
use shimeji_core::{Engine, EngineSettings, MascotId};

#[test]
fn a_stationary_desktop_pet_does_not_slide_when_the_side_dock_resizes_work_area() {
    for desktop in [true, false] {
        let mut sample = snapshot(100, 700);
        let (mut engine, id) = pet(
            EngineSettings {
                desktop_border_priority: desktop,
                ..EngineSettings::default()
            },
            &sample,
            Point::new(600, 1080),
        );
        assert!(engine.set_behavior(id, "立ってボーっとする"));
        engine.tick(&sample);
        sample.screens[0].work = Some(WorkRect {
            x: 0,
            y: 25,
            width: 1920,
            height: 1055,
        });
        engine.tick(&sample);
        let x = engine.frames()[0].anchor.x;
        if desktop {
            assert_eq!(x, 600);
        } else {
            assert_ne!(x, 600);
        }
    }
}

fn snapshot(top: i32, bottom: i32) -> Snapshot {
    Snapshot {
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
        active_window: Some(WindowSample {
            id: 42,
            visible: true,
            x: 49,
            y: top,
            width: 1871,
            height: bottom - top,
        }),
        cursor: Point::new(960, 500),
        multiscreen: false,
    }
}

fn pet(settings: EngineSettings, snapshot: &Snapshot, anchor: Point) -> (Engine, MascotId) {
    let character = shimeji_packs::load_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/packs/shime"
    ))
    .unwrap()
    .pack
    .characters
    .remove(0);
    let mut engine = Engine::new(&character, 1);
    engine.set_settings(settings);
    engine.apply(snapshot);
    let id = engine.spawn_at(anchor).unwrap();
    engine.tick(snapshot);
    engine.move_frozen(id, anchor);
    (engine, id)
}

fn weight(engine: &mut Engine, id: MascotId, name: &str) -> i32 {
    engine
        .candidate_weights(id)
        .unwrap()
        .into_iter()
        .find(|(candidate, _)| candidate == name)
        .map(|(_, weight)| weight)
        .unwrap_or(0)
}

fn app_settings(keep_on_screen: bool) -> EngineSettings {
    EngineSettings {
        keep_on_screen,
        desktop_border_priority: true,
        ..EngineSettings::default()
    }
}

#[test]
fn maximized_window_bottom_is_only_a_desktop_floor_in_app_mode() {
    for keep in [true, false] {
        for top in [25, 100] {
            let snapshot = snapshot(top, 1080);
            let (mut engine, id) = pet(app_settings(keep), &snapshot, Point::new(960, 1080));
            assert!(weight(&mut engine, id, "ワークエリアの下辺を歩く") > 0);
            for name in [
                "天井に掴まってボーっとする",
                "天井から落ちる",
                "ワークエリアの上辺を伝う",
                "IEの下辺を伝う",
            ] {
                assert_eq!(weight(&mut engine, id, name), 0, "{name}");
            }
        }
    }
}

#[test]
fn ordinary_window_undersides_and_the_desktop_ceiling_remain_climbable() {
    let snapshot = snapshot(200, 800);
    for y in [25, 800] {
        let (mut engine, id) = pet(app_settings(true), &snapshot, Point::new(960, y));
        assert!(weight(&mut engine, id, "天井に掴まってボーっとする") > 0);
    }
    let (mut engine, id) = pet(app_settings(true), &snapshot, Point::new(960, 200));
    assert!(weight(&mut engine, id, "IEの天井を歩く") > 0);
}

#[test]
fn a_window_top_flush_with_the_desktop_ceiling_is_not_a_floor() {
    let snapshot = snapshot(25, 1080);
    let (mut engine, id) = pet(app_settings(true), &snapshot, Point::new(960, 25));
    assert_eq!(weight(&mut engine, id, "IEの天井を歩く"), 0);
    assert_eq!(weight(&mut engine, id, "ワークエリアの下辺を歩く"), 0);
    assert!(weight(&mut engine, id, "ワークエリアの上辺を伝う") > 0);
}

#[test]
fn the_reference_mode_retains_overlapping_floor_and_ceiling_candidates() {
    let snapshot = snapshot(25, 1080);
    let (mut engine, id) = pet(EngineSettings::default(), &snapshot, Point::new(960, 1080));
    assert!(weight(&mut engine, id, "天井に掴まってボーっとする") > 0);
    assert!(weight(&mut engine, id, "IEの下辺を伝う") > 0);
}

#[test]
fn a_grabbed_window_ceiling_is_released_when_its_bottom_reaches_the_floor() {
    let initial = snapshot(100, 1060);
    let (mut engine, id) = pet(app_settings(true), &initial, Point::new(960, 1060));
    assert!(engine.set_behavior(id, "天井に掴まってボーっとする"));
    assert_eq!(engine.frames()[0].action, "天井に掴まる");
    let maximized = snapshot(25, 1080);
    engine.tick(&maximized);
    assert_ne!(engine.frames()[0].action, "天井に掴まる");
    assert_eq!(
        engine.frames()[0].anchor.y,
        1060,
        "do not ride the invalidated underside"
    );
    assert!(engine.diagnostics().is_empty());
}

#[test]
fn maximized_windows_never_render_ceiling_art_at_the_desktop_floor() {
    for top in [25, 100] {
        let snapshot = snapshot(top, 1080);
        let settings = EngineSettings {
            breeding: false,
            ..app_settings(true)
        };
        let (mut engine, _) = pet(settings, &snapshot, Point::new(960, 1080));
        engine.set_mascot_cap(1);
        for tick in 0..20_000 {
            engine.tick(&snapshot);
            let frame = &engine.frames()[0];
            if frame.anchor.y == 1080 {
                assert!(
                    !matches!(
                        frame.image.as_deref(),
                        Some("/shime23.png" | "/shime24.png" | "/shime25.png")
                    ),
                    "top={top}, tick={tick}, frame={frame:?}"
                );
            }
        }
        assert!(engine.diagnostics().is_empty());
    }
}

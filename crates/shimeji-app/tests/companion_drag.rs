//! A real classic Dragged action must not follow the cursor or throw after a
//! focus presentation has cancelled it.
use shimeji_app::{Command, Runtime};
use shimeji_core::env::{Point, ScreenSample, Snapshot};
use shimeji_core::{EngineSettings, MouseButton};
use std::path::PathBuf;

#[test]
fn bundled_mid_drag_to_companion_cancels_drag_and_pending_throw() {
    let loaded = shimeji_packs::load_dir(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime"),
    )
    .unwrap();
    let mut runtime = Runtime::new(&loaded.pack.characters[0], 1);
    let mut world = Snapshot {
        screens: vec![ScreenSample {
            id: "main".into(),
            x: 0,
            y: 0,
            width: 1200,
            height: 800,
            work: None,
        }],
        cursor: Point::new(200, 100),
        active_window: None,
        multiscreen: true,
    };
    runtime.push(Command::Settings(EngineSettings {
        breeding: false,
        ..EngineSettings::default()
    }));
    runtime.push(Command::SpawnAt(Point::new(300, 100)));
    runtime.on_frame(0.0, &world);
    runtime.on_frame(40.0, &world);
    let id = runtime.frames()[0].id;
    runtime.push(Command::Press {
        mascot: Some(id),
        button: MouseButton::Left,
    });
    runtime.on_frame(80.0, &world);
    assert_eq!(runtime.frames()[0].behavior, "ドラッグされる");
    runtime.set_companion_mode(true, &world);
    assert_eq!(runtime.frames()[0].anchor.y, 800);
    runtime.push(Command::Release {
        mascot: Some(id),
        button: MouseButton::Left,
    });
    runtime.on_frame(120.0, &world);
    runtime.set_companion_mode(false, &world);
    runtime.on_frame(1_000.0, &world);
    world.cursor = Point::new(1100, 40);
    for step in 1..=5 {
        runtime.on_frame(1_000.0 + f64::from(step) * 40.0, &world);
        let frames = runtime.frames();
        assert_eq!(frames.len(), 1);
        assert_ne!(frames[0].behavior, "ドラッグされる");
        assert_ne!(frames[0].behavior, "投げられる");
        assert!(
            frames[0].anchor.x < 1000,
            "old drag must not follow new cursor"
        );
    }
}

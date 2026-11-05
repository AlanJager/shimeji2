//! Exercise the complete shared focus chain with the distributed character,
//! without a window server or a real-time wait.

use std::path::PathBuf;

use shimeji_app::pomodoro::{Interruption, Phase, Pomodoro, Preferences, Status};
use shimeji_app::{Command, Runtime};
use shimeji_core::EngineSettings;
use shimeji_core::env::{Point, ScreenSample, Snapshot, WindowSample, WorkRect};

fn world() -> Snapshot {
    Snapshot {
        screens: vec![ScreenSample {
            id: "main".into(),
            x: 0,
            y: 0,
            width: 1200,
            height: 800,
            work: Some(WorkRect {
                x: 0,
                y: 25,
                width: 1200,
                height: 735,
            }),
        }],
        cursor: Point::new(300, 200),
        active_window: Some(WindowSample {
            id: 42,
            visible: true,
            x: 50,
            y: 80,
            width: 1000,
            height: 400,
        }),
        multiscreen: true,
    }
}

fn runtime() -> Runtime {
    let loaded = shimeji_packs::load_dir(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime"),
    )
    .unwrap();
    let mut runtime = Runtime::new(&loaded.pack.characters[0], 1);
    runtime.push(Command::Settings(EngineSettings {
        breeding: false,
        ..EngineSettings::default()
    }));
    runtime.push(Command::SpawnAt(Point::new(350, 150)));
    runtime.on_frame(0.0, &world());
    runtime.on_frame(40.0, &world());
    assert_eq!(runtime.frames().len(), 1);
    runtime
}

#[test]
fn default_full_focus_session_sits_through_lock_and_completes_once() {
    let snapshot = world();
    let mut runtime = runtime();
    let mut timer = Pomodoro::default();
    assert!(timer.start_next(0, &Preferences::default()));
    runtime.set_companion_mode(timer.quiet(), &snapshot);
    let parked = runtime.frames();
    assert_eq!(
        parked[0].anchor.y, 760,
        "use desktop work floor, not window"
    );
    assert_eq!(parked[0].image.as_deref(), Some("/shime11.png"));
    assert!(!runtime.paused(), "companion mode must not set user pause");

    timer.set_interruption(Interruption::Locked, true, 300_000);
    assert_eq!(timer.status(), Status::Paused);
    assert_eq!(timer.remaining_ms(9_000_000), 1_200_000);
    let frame = runtime.on_frame(9_000_000.0, &snapshot);
    assert_eq!(frame.ticks, 0);
    assert!(frame.sounds.is_empty());
    assert!(frame.window_moves.is_empty());
    assert_eq!(runtime.frames(), parked);

    timer.set_interruption(Interruption::Locked, false, 9_000_000);
    assert_eq!(timer.status(), Status::Paused);
    assert!(timer.resume(9_000_000));
    assert_eq!(timer.update(10_200_000).unwrap().phase, Phase::Focus);
    assert_eq!(timer.update(10_200_000), None);
    assert_eq!(timer.completed_focus(), 1);
    runtime.set_companion_mode(timer.quiet(), &snapshot);
    assert!(!runtime.companion_mode());
    assert_eq!(runtime.on_frame(10_200_000.0, &snapshot).ticks, 0);
    assert_eq!(runtime.on_frame(10_200_040.0, &snapshot).ticks, 1);
    assert!(timer.start_next(10_200_040, &Preferences::default()));
    assert_eq!(timer.phase(), Some(Phase::ShortBreak));
    assert!(!timer.quiet());
}

#[test]
fn companion_can_drag_add_and_remove_without_ticks_or_overwriting_settings() {
    let snapshot = world();
    let mut runtime = runtime();
    runtime.set_paused(true);
    runtime.set_companion_mode(true, &snapshot);
    let first = runtime.frames()[0].id;
    let dragged = Point::new(500, 360);
    runtime.push(Command::MoveFrozen {
        id: first,
        anchor: dragged,
    });
    runtime.push(Command::SpawnAt(Point::new(800, 100)));
    let mut changed = runtime.settings();
    changed.scaling = 1.5;
    changed.breeding = true;
    runtime.push(Command::Settings(changed));
    let frame = runtime.on_frame(100.0, &snapshot);
    assert_eq!(frame.ticks, 0);
    assert!(frame.sounds.is_empty());
    assert!(frame.window_moves.is_empty());
    let pets = runtime.frames();
    assert_eq!(pets.len(), 2);
    assert_eq!(
        pets.iter().find(|pet| pet.id == first).unwrap().anchor,
        dragged
    );
    let newest = pets.iter().find(|pet| pet.id != first).unwrap();
    assert_eq!(newest.anchor.y, 760);
    assert_eq!(newest.image.as_deref(), Some("/shime11.png"));
    runtime.push(Command::Dismiss(first));
    runtime.on_frame(200.0, &snapshot);
    assert_eq!(runtime.frames().len(), 1);
    runtime.set_companion_mode(false, &snapshot);
    assert!(runtime.paused(), "existing user pause must survive focus");
    assert_eq!(
        runtime.settings(),
        changed,
        "keep changes made during focus"
    );
    assert_eq!(runtime.on_frame(500_000.0, &snapshot).ticks, 0);
}

//! Exercise the same timer → companion runtime → Windows BGRA rendering chain
//! used by the native shell, without requiring a window server.

use std::path::Path;

use shimeji_app::Command;
use shimeji_app::pomodoro::{Phase, Pomodoro, Preferences, Status};
use shimeji_core::env::{Point, ScreenSample, Snapshot, WorkRect};
use shimeji_windows::focus;
use shimeji_windows::launch::Loaded;
use shimeji_windows::sprites::Atlas;

fn desktop() -> Snapshot {
    Snapshot {
        screens: vec![ScreenSample {
            id: "left".into(),
            x: -1920,
            y: -200,
            width: 1920,
            height: 1080,
            work: Some(WorkRect {
                x: -1920,
                y: -200,
                width: 1920,
                height: 1040,
            }),
        }],
        cursor: Point::new(-1000, 200),
        active_window: None,
        multiscreen: true,
    }
}

#[test]
fn focus_parks_existing_and_new_pets_renders_and_allows_direct_drag_and_removal() {
    let loaded = Loaded::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime"),
        None,
        None,
    )
    .unwrap();
    let mut runtime = loaded.runtime(42);
    let atlas = Atlas::new(&loaded.character, loaded.images, loaded.scale);
    let world = desktop();
    runtime.on_frame(-40.0, &world);
    runtime.on_frame(0.0, &world);
    runtime.push(Command::SpawnAt(Point::new(-1000, 200)));
    runtime.on_frame(40.0, &world);
    let mut timer = Pomodoro::default();
    assert!(timer.start_next(0, &Preferences::default()));
    runtime.set_companion_mode(timer.quiet(), &world);
    runtime.on_frame(80.0, &world);
    let first = runtime.frames()[0].clone();
    let drawing = atlas
        .draw(&first)
        .unwrap()
        .expect("calm pose is renderable");
    assert!(drawing.origin.x < 0);
    assert!(drawing.origin.y >= -200);
    assert!(drawing.origin.y + drawing.height <= 840);
    assert_eq!(runtime.on_frame(100_000.0, &world).ticks, 0);
    assert_eq!(runtime.frames()[0], first);
    runtime.push(Command::SpawnAt(Point::new(-600, 100)));
    runtime.on_frame(100_001.0, &world);
    assert_eq!(runtime.frames().len(), 2);
    for frame in runtime.frames() {
        let drawing = atlas.draw(&frame).unwrap().unwrap();
        assert!(drawing.origin.y + drawing.height <= 840);
    }
    runtime.push(Command::MoveFrozen {
        id: first.id,
        anchor: Point::new(-1200, 700),
    });
    runtime.on_frame(100_002.0, &world);
    assert_eq!(
        runtime
            .frames()
            .iter()
            .find(|frame| frame.id == first.id)
            .unwrap()
            .anchor,
        Point::new(-1200, 700)
    );
    runtime.on_frame(100_003.0, &world);
    assert_eq!(
        runtime
            .frames()
            .iter()
            .find(|frame| frame.id == first.id)
            .unwrap()
            .anchor,
        Point::new(-1200, 700)
    );
    runtime.push(Command::Dismiss(first.id));
    runtime.on_frame(100_004.0, &world);
    assert_eq!(runtime.frames().len(), 1);
    runtime.set_paused(true);
    timer.cancel();
    runtime.set_companion_mode(timer.quiet(), &world);
    assert!(runtime.paused(), "ending focus preserves user pause");
    assert_eq!(runtime.on_frame(100_005.0, &world).ticks, 0);
}

#[test]
fn manual_native_menu_cycle_reaches_long_break_after_four_completed_focus_rounds() {
    let preferences = Preferences {
        focus_minutes: 1,
        short_break_minutes: 1,
        long_break_minutes: 1,
        ..Preferences::default()
    };
    let mut timer = Pomodoro::default();
    let mut now = 0;
    for completed in 1..=4 {
        assert_eq!(focus::action_title(&timer), "开始专注");
        assert!(timer.start_next(now, &preferences));
        now += 60_000;
        let done = timer.update(now).expect("one completion");
        assert_eq!(done.phase, Phase::Focus);
        assert!(timer.update(now).is_none());
        assert_eq!(timer.completed_focus(), completed);
        assert_eq!(
            focus::next_title(&timer),
            if completed == 4 {
                "开始长休息"
            } else {
                "开始短休息"
            }
        );
        assert_eq!(timer.status(), Status::Completed);
        assert!(timer.start_next(now, &preferences));
        now += 60_000;
        assert!(timer.update(now).is_some());
    }
}

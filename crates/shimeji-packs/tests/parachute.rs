//! Optional apex deployment uses synthetic animations and upstream zlib images.
use shimeji_core::env::{Point, ScreenSample, Snapshot, WorkRect};
use shimeji_core::model::{Param, ScriptSource};
use shimeji_core::{Engine, EngineSettings, MouseButton};

fn flight(start_y: i32, cursor_delta: i32, padded: bool) -> Vec<shimeji_core::MascotFrame> {
    let mut c = shimeji_packs::load_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/packs/shime"
    ))
    .unwrap()
    .pack
    .characters
    .remove(0);
    let mut fall = c.actions.get("落ちる").unwrap().clone();
    for (key, raw) in [
        ("DeployAtApex", "true"),
        ("ResistanceX", "0.025"),
        ("ResistanceY", "#{DeployTime < 0 ? 0 : 0.2}"),
        ("Gravity", "#{DeployTime < 0 ? 1.5 : 0.6}"),
    ] {
        fall.params.insert(
            key,
            Param {
                key: key.into(),
                source_name: key.into(),
                value: ScriptSource::parse_attribute(raw).0,
                line: 0,
            },
        );
    }
    let original = fall.animations[0].clone();
    fall.animations = [
        ("#{DeployTime < 0}", "落ちる"),
        ("#{DeployTime >= 0 && DeployTime < 4}", "立つ"),
        ("#{DeployTime >= 4 && DeployTime < 8}", "歩く"),
        ("#{DeployTime >= 8}", "走る"),
    ]
    .into_iter()
    .map(|(cond, source)| {
        let mut a = original.clone();
        a.condition = Some(ScriptSource::parse_attribute(cond).0);
        a.poses = vec![c.actions.get(source).unwrap().animations[0].poses[0].clone()];
        if source == "歩く" {
            a.poses[0] = c.actions.get(source).unwrap().animations[0].poses[1].clone();
        }
        if source == "走る" {
            a.poses[0] = c.actions.get(source).unwrap().animations[0].poses[3].clone();
        }
        if padded {
            a.poses[0].image.as_mut().unwrap().height += 4;
        }
        a.poses[0].velocity_x = 0;
        a.poses[0].velocity_y = 0;
        a.poses[0].duration_ticks = 250;
        a
    })
    .collect();
    c.actions.insert("落ちる", fall);
    let mut s = Snapshot {
        screens: vec![ScreenSample {
            id: "main".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            work: Some(WorkRect {
                x: 0,
                y: 25,
                width: 1920,
                height: 1055,
            }),
        }],
        cursor: Point::new(960, start_y),
        active_window: None,
        multiscreen: false,
    };
    let mut e = Engine::new(&c, 1);
    e.set_settings(EngineSettings {
        keep_on_screen: true,
        breeding: false,
        ..EngineSettings::default()
    });
    e.apply(&s);
    let id = e.spawn_at(Point::new(960, start_y)).unwrap();
    e.tick(&s);
    e.move_frozen(id, Point::new(960, start_y));
    e.press(Some(id), MouseButton::Left);
    e.tick(&s);
    s.cursor.y += cursor_delta;
    e.tick(&s);
    e.release(Some(id), MouseButton::Left);
    let mut frames = Vec::new();
    for _ in 0..500 {
        e.tick(&s);
        assert!(!e.frames().is_empty(), "{:?}", e.diagnostics());
        let frame = e.frames()[0].clone();
        assert!(frame.anchor.y >= 25 && frame.anchor.y <= 1080);
        let landed = frame.anchor.y == 1080;
        frames.push(frame);
        if landed {
            break;
        }
    }
    assert!(e.diagnostics().is_empty(), "{:?}", e.diagnostics());
    assert_eq!(frames.last().unwrap().anchor.y, 1080);
    frames
}

#[test]
fn an_upward_throw_deploys_at_its_apex_and_completes_the_opening_sequence() {
    let frames = flight(850, -120, false);
    let lowest_y = frames.iter().map(|f| f.anchor.y).min().unwrap();
    let opening_y = frames
        .iter()
        .find(|f| f.image.as_deref() == Some("/shime1.png"))
        .map(|f| f.anchor.y);
    let seen: std::collections::HashSet<_> =
        frames.iter().filter_map(|f| f.image.clone()).collect();
    assert!(opening_y.is_some(), "umbrella never deployed");
    assert!(
        opening_y.unwrap() - lowest_y <= 3,
        "deployed after the apex"
    );
    for image in ["/shime4.png", "/shime1.png", "/shime2.png"] {
        assert!(seen.contains(image), "missing {image}");
    }
}

#[test]
fn a_downward_release_deploys_immediately_and_settles_to_gentle_descent() {
    let frames = flight(350, 60, false);
    assert_eq!(frames[0].image.as_deref(), Some("/shime1.png"));
    let steady: Vec<_> = frames
        .windows(2)
        .skip(40)
        .take(30)
        .map(|w| w[1].anchor.y - w[0].anchor.y)
        .collect();
    assert_eq!(steady.len(), 30);
    assert!(steady.iter().all(|dy| (2..=4).contains(dy)), "{steady:?}");
}

#[test]
fn a_retained_throw_at_the_ceiling_deploys_without_sticking_or_respawning() {
    let frames = flight(180, -120, false);
    assert!(
        frames
            .iter()
            .all(|f| f.anchor.y >= 153 && f.behavior == "投げられる")
    );
    assert!(
        frames
            .iter()
            .take(4)
            .any(|f| f.image.as_deref() == Some("/shime1.png"))
    );
}

#[test]
fn transparent_canvas_padding_does_not_prevent_a_retained_throw_from_landing() {
    let frames = flight(850, -120, true);
    assert_eq!(frames.last().unwrap().anchor.y, 1080);
}

use std::path::Path;

use shimeji_app::Command;
use shimeji_core::env::{Point, ScreenSample, Snapshot, WorkRect};
use shimeji_core::{MascotFrame, MascotId, MouseButton};
use shimeji_windows::launch::Loaded;
use shimeji_windows::sprites::Atlas;

fn bundled() -> Loaded {
    Loaded::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime"),
        None,
        None,
    )
    .unwrap()
}

fn desktop() -> Snapshot {
    Snapshot {
        screens: vec![ScreenSample {
            id: "left-monitor".into(),
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
        cursor: Point::new(-900, 300),
        active_window: None,
        multiscreen: true,
    }
}

#[test]
fn imported_bundled_pack_animates_and_renders_with_negative_desktop_coordinates() {
    let loaded = bundled();
    assert_eq!(loaded.images.len(), 46);
    let mut runtime = loaded.runtime(42);
    let atlas = Atlas::new(&loaded.character, loaded.images, loaded.scale);
    let mut world = desktop();
    runtime.on_frame(-40.0, &world);
    runtime.on_frame(0.0, &world);
    runtime.push(Command::SpawnAt(Point::new(-960, 200)));
    let mut visible = 0;
    let mut dragged = false;
    for tick in 0..1000 {
        if tick == 100 {
            let id = runtime.frames()[0].id;
            runtime.push(Command::Press {
                mascot: Some(id),
                button: MouseButton::Left,
            });
        }
        if (101..150).contains(&tick) {
            world.cursor.x += 2;
        }
        if tick == 150 {
            let id = runtime.frames()[0].id;
            runtime.push(Command::Release {
                mascot: Some(id),
                button: MouseButton::Left,
            });
        }
        runtime.on_frame(f64::from(tick * 40), &world);
        for frame in runtime.frames() {
            dragged |= frame.behavior == "ドラッグされる";
            if let Some(drawing) = atlas.draw(&frame).unwrap() {
                assert_eq!(
                    drawing.bgra.len(),
                    drawing.width as usize * drawing.height as usize * 4
                );
                assert!(drawing.origin.x < 0);
                assert!(drawing.bgra.chunks_exact(4).any(|pixel| pixel[3] > 0));
                visible += 1;
            }
        }
    }
    assert!(dragged);
    assert!(visible > 900);
    assert_eq!(runtime.population(), 1);
}

#[test]
fn every_bundled_pose_has_correct_mirrored_anchor_and_scaled_pixels() {
    let loaded = bundled();
    let atlas = Atlas::new(&loaded.character, loaded.images, 0.5);
    let mut count = 0;
    loaded.character.visit_actions(&mut |action| {
        for animation in &action.animations {
            for pose in &animation.poses {
                let Some(image) = &pose.image else { continue };
                for right in [false, true] {
                    let anchor_x =
                        if right && pose.image_right.is_none() && pose.right_sibling.is_none() {
                            image.width - pose.anchor_x
                        } else {
                            pose.anchor_x
                        };
                    let frame = MascotFrame {
                        id: MascotId(1),
                        behavior: String::new(),
                        action: String::new(),
                        anchor: Point::new(-400, 300),
                        image: Some(image.source.clone()),
                        look_right: right,
                        image_anchor: Some(Point::new(anchor_x, pose.anchor_y)),
                    };
                    let drawing = atlas.draw(&frame).unwrap().unwrap();
                    assert_eq!(
                        drawing.origin,
                        Point::new(
                            -400 - (f64::from(anchor_x) * 0.5).round() as i32,
                            300 - (f64::from(pose.anchor_y) * 0.5).round() as i32
                        )
                    );
                    assert_eq!(drawing.width, (f64::from(image.width) * 0.5).round() as i32);
                    count += 1;
                }
            }
        }
    });
    assert!(count > 46);
}

#[test]
fn character_selection_and_invalid_pack_are_reported() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime");
    assert!(Loaded::open(&source, Some("missing character"), None).is_err());
    assert!(Loaded::open(&source.join("missing"), None, None).is_err());
}

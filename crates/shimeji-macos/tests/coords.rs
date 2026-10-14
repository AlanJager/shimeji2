//! Coordinate conversion. No WindowServer.

use shimeji_core::env::Point;
use shimeji_macos::platform::coords::{
    CocoaScreen, CocoaSpace, Insets, Rect, cocoa_point_to_engine, cocoa_rect_to_engine,
    engine_point_to_cocoa, engine_rect_to_cocoa, work_area,
};

fn primary_1117() -> CocoaSpace {
    CocoaSpace {
        primary_height: 1117,
        screens: vec![CocoaScreen {
            id: "main".to_string(),
            frame: Rect::new(0, 0, 1920, 1117),
            visible: Rect::new(0, 0, 1920, 1080),
            safe: Insets::default(),
        }],
    }
}

#[test]
fn cocoa_point_on_primary_of_height_1117_maps_to_engine() {
    let space = primary_1117();
    assert_eq!(space.point_to_engine(100, 50), Point::new(100, 1067));
    assert_eq!(cocoa_point_to_engine(100, 50, 1117), Point::new(100, 1067));
}

#[test]
fn round_trip_with_a_secondary_screen_above_the_primary_is_lossless() {
    let space = CocoaSpace {
        primary_height: 1117,
        screens: vec![
            CocoaScreen {
                id: "main".to_string(),
                frame: Rect::new(0, 0, 1920, 1117),
                visible: Rect::new(0, 0, 1920, 1080),
                safe: Insets::default(),
            },
            CocoaScreen {
                id: "above".to_string(),
                frame: Rect::new(0, 1117, 1920, 1080),
                visible: Rect::new(0, 1117, 1920, 1080),
                safe: Insets::default(),
            },
        ],
    };
    // Cocoa top-left of the secondary: (0, 1117+1080) = (0, 2197).
    let cocoa_points = [
        (100, 50),
        (0, 0),
        (0, 1117),
        (0, 2197),
        (1920, 2197),
        (-10, 1500),
    ];
    for (x, y) in cocoa_points {
        let engine = space.point_to_engine(x, y);
        let back = space.point_to_cocoa(engine);
        assert_eq!(back, (x, y), "point ({x}, {y}) via {engine:?}");
    }
    for screen in &space.screens {
        let engine = cocoa_rect_to_engine(screen.frame, space.primary_height);
        let back = engine_rect_to_cocoa(engine, space.primary_height);
        assert_eq!(back, screen.frame, "frame {}", screen.id);
    }
    let above = cocoa_rect_to_engine(space.screens[1].frame, 1117);
    assert_eq!(above, Rect::new(0, -1080, 1920, 1080));
}

#[test]
fn round_trip_with_a_secondary_screen_left_of_the_primary_is_lossless() {
    let space = CocoaSpace {
        primary_height: 1117,
        screens: vec![
            CocoaScreen {
                id: "main".to_string(),
                frame: Rect::new(0, 0, 1920, 1117),
                visible: Rect::new(0, 0, 1920, 1080),
                safe: Insets::default(),
            },
            CocoaScreen {
                id: "left".to_string(),
                frame: Rect::new(-1920, 37, 1920, 1080),
                visible: Rect::new(-1920, 37, 1920, 1055),
                safe: Insets::default(),
            },
        ],
    };
    let cocoa_points = [(100, 50), (0, 0), (-1920, 37), (-1, 1117), (-960, 500)];
    for (x, y) in cocoa_points {
        let engine = cocoa_point_to_engine(x, y, space.primary_height);
        let back = engine_point_to_cocoa(engine, space.primary_height);
        assert_eq!(back, (x, y), "point ({x}, {y})");
    }
    let left = cocoa_rect_to_engine(space.screens[1].frame, 1117);
    assert_eq!(left, Rect::new(-1920, 0, 1920, 1080));
    let back = engine_rect_to_cocoa(left, 1117);
    assert_eq!(back, space.screens[1].frame);
}

#[test]
fn notched_display_work_area_excludes_menu_bar_and_notch() {
    // 1512×982 logical, 38 pt menu bar, 32 pt notch inset from the top of the
    // screen frame. Dock hidden, so visibleFrame sits on the cocoa origin.
    let screen = CocoaScreen {
        id: "built-in".to_string(),
        frame: Rect::new(0, 0, 1512, 982),
        visible: Rect::new(0, 0, 1512, 944),
        safe: Insets {
            top: 32,
            left: 0,
            bottom: 0,
            right: 0,
        },
    };
    let work = work_area(&screen, 982);
    // visibleFrame already starts 38 pt below the top; the notch (32) is
    // inside that strip. Work top is 38, height 944.
    assert_eq!(work, Rect::new(0, 38, 1512, 944));
    assert!(work.y >= 32, "notch strip is not standable");
    assert_eq!(work.y + work.height, 982, "dock-free bottom is the screen");

    // A taller notch than the menu bar wins.
    let deep_notch = CocoaScreen {
        id: "built-in".to_string(),
        frame: Rect::new(0, 0, 1512, 982),
        visible: Rect::new(0, 0, 1512, 944),
        safe: Insets {
            top: 50,
            left: 0,
            bottom: 0,
            right: 0,
        },
    };
    let work = work_area(&deep_notch, 982);
    assert_eq!(work, Rect::new(0, 50, 1512, 932));
}

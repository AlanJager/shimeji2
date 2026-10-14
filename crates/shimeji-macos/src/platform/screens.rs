//! NSScreen → engine snapshot. Refreshed on screen-parameter changes.

use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;

use super::coords::{CocoaScreen, CocoaSpace, Insets, Rect};

pub fn read_space(mtm: MainThreadMarker) -> CocoaSpace {
    let screens = NSScreen::screens(mtm);
    let primary_height = NSScreen::mainScreen(mtm)
        .map(|screen| screen.frame().size.height.round() as i32)
        .unwrap_or(0);
    let mut out = Vec::with_capacity(screens.count());
    for (index, screen) in screens.iter().enumerate() {
        let frame = screen.frame();
        let visible = screen.visibleFrame();
        let insets = screen.safeAreaInsets();
        let id = format!("{}#{index}", screen.localizedName());
        out.push(CocoaScreen {
            id,
            frame: Rect::new(
                frame.origin.x.round() as i32,
                frame.origin.y.round() as i32,
                frame.size.width.round() as i32,
                frame.size.height.round() as i32,
            ),
            visible: Rect::new(
                visible.origin.x.round() as i32,
                visible.origin.y.round() as i32,
                visible.size.width.round() as i32,
                visible.size.height.round() as i32,
            ),
            safe: Insets {
                top: insets.top.round() as i32,
                left: insets.left.round() as i32,
                bottom: insets.bottom.round() as i32,
                right: insets.right.round() as i32,
            },
        });
    }
    CocoaSpace {
        primary_height,
        screens: out,
    }
}

/// Token for the main screen's Cocoa frame, used to recreate the clock.
pub fn main_display_id(mtm: MainThreadMarker) -> Option<(i32, i32, i32, i32)> {
    NSScreen::mainScreen(mtm).map(|screen| {
        let frame = screen.frame();
        (
            frame.origin.x.round() as i32,
            frame.origin.y.round() as i32,
            frame.size.width.round() as i32,
            frame.size.height.round() as i32,
        )
    })
}

pub fn engine_screen_rects(space: &CocoaSpace) -> Vec<Rect> {
    space
        .screens
        .iter()
        .map(|screen| space.rect_to_engine(screen.frame))
        .collect()
}

pub fn reach_union(space: &CocoaSpace) -> Option<Rect> {
    let rects = engine_screen_rects(space);
    let mut iter = rects.into_iter();
    let first = iter.next()?;
    Some(iter.fold(first, |acc, next| {
        let x = acc.x.min(next.x);
        let y = acc.y.min(next.y);
        let right = acc.right().max(next.right());
        let bottom = acc.bottom().max(next.bottom());
        Rect::new(x, y, right.wrapping_sub(x), bottom.wrapping_sub(y))
    }))
}

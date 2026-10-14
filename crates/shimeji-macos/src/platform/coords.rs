//! Cocoa bottom-left ↔ engine top-left, integer logical points.
//!
//! The engine, CGWindowList and AX share a global top-left, y-down space whose
//! origin is the top-left of the primary display. Cocoa (`NSScreen.frame`,
//! `NSWindow.setFrameOrigin`, `NSEvent.mouseLocation`) is bottom-left, y-up,
//! with the same origin on the primary's bottom-left. Conversion happens only
//! here. Retina is a backing-scale concern, not a fractional coordinate.

use shimeji_core::env::{Point, ScreenSample, WorkRect};

/// Integer rectangle in one coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(self) -> i32 {
        self.x.wrapping_add(self.width)
    }

    pub fn bottom(self) -> i32 {
        self.y.wrapping_add(self.height)
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        let width = right.wrapping_sub(x);
        let height = bottom.wrapping_sub(y);
        if width <= 0 || height <= 0 {
            None
        } else {
            Some(Self::new(x, y, width, height))
        }
    }
}

/// Edge insets in points, matching `NSEdgeInsets` field order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Insets {
    pub top: i32,
    pub left: i32,
    pub bottom: i32,
    pub right: i32,
}

/// One display, described in Cocoa global coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CocoaScreen {
    pub id: String,
    pub frame: Rect,
    pub visible: Rect,
    pub safe: Insets,
}

/// The Cocoa screen list plus the primary's height, which is the conversion key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CocoaSpace {
    pub primary_height: i32,
    pub screens: Vec<CocoaScreen>,
}

impl CocoaSpace {
    /// Convert a Cocoa point (y up) into an engine point (y down).
    pub fn point_to_engine(&self, x: i32, y: i32) -> Point {
        cocoa_point_to_engine(x, y, self.primary_height)
    }

    /// Convert an engine point back into Cocoa.
    pub fn point_to_cocoa(&self, point: Point) -> (i32, i32) {
        engine_point_to_cocoa(point, self.primary_height)
    }

    /// Convert a Cocoa rect (origin at its bottom-left) into engine space
    /// (origin at its top-left).
    pub fn rect_to_engine(&self, rect: Rect) -> Rect {
        cocoa_rect_to_engine(rect, self.primary_height)
    }

    /// Convert an engine rect back into Cocoa.
    pub fn rect_to_cocoa(&self, rect: Rect) -> Rect {
        engine_rect_to_cocoa(rect, self.primary_height)
    }

    /// Engine snapshot screens, including work areas from `visibleFrame` and
    /// `safeAreaInsets`.
    pub fn snapshot_screens(&self) -> Vec<ScreenSample> {
        self.screens
            .iter()
            .map(|screen| {
                let frame = self.rect_to_engine(screen.frame);
                let work = work_area(screen, self.primary_height);
                ScreenSample {
                    id: screen.id.clone(),
                    x: frame.x,
                    y: frame.y,
                    width: frame.width,
                    height: frame.height,
                    work: Some(WorkRect {
                        x: work.x,
                        y: work.y,
                        width: work.width,
                        height: work.height,
                    }),
                }
            })
            .collect()
    }
}

pub fn cocoa_point_to_engine(x: i32, y: i32, primary_height: i32) -> Point {
    Point::new(x, primary_height.wrapping_sub(y))
}

pub fn engine_point_to_cocoa(point: Point, primary_height: i32) -> (i32, i32) {
    (point.x, primary_height.wrapping_sub(point.y))
}

pub fn cocoa_rect_to_engine(rect: Rect, primary_height: i32) -> Rect {
    Rect::new(
        rect.x,
        primary_height
            .wrapping_sub(rect.y)
            .wrapping_sub(rect.height),
        rect.width,
        rect.height,
    )
}

pub fn engine_rect_to_cocoa(rect: Rect, primary_height: i32) -> Rect {
    Rect::new(
        rect.x,
        primary_height
            .wrapping_sub(rect.y)
            .wrapping_sub(rect.height),
        rect.width,
        rect.height,
    )
}

/// Work area: intersection of `visibleFrame` and the screen frame inset by
/// `safeAreaInsets`, both converted to engine space. That excludes the menu
/// bar, Dock, and a notched camera housing.
pub fn work_area(screen: &CocoaScreen, primary_height: i32) -> Rect {
    let frame = cocoa_rect_to_engine(screen.frame, primary_height);
    let visible = cocoa_rect_to_engine(screen.visible, primary_height);
    let safe = Rect::new(
        frame.x.wrapping_add(screen.safe.left),
        frame.y.wrapping_add(screen.safe.top),
        frame
            .width
            .wrapping_sub(screen.safe.left)
            .wrapping_sub(screen.safe.right),
        frame
            .height
            .wrapping_sub(screen.safe.top)
            .wrapping_sub(screen.safe.bottom),
    );
    visible.intersect(safe).unwrap_or(visible)
}

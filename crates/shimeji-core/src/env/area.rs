//! `Area`: inclusive integer bounds and per-tick edge deltas.

use super::border::{FloorCeiling, Wall};

/// Integer point. Top-left origin, y down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// Rectangle stored as left/top/right/bottom, with deltas from the previous `set`.
///
/// `right` and `bottom` are `x + width` and `y + height`, matching `Area.set`.
/// `contains_point` includes those edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub dleft: i32,
    pub dtop: i32,
    pub dright: i32,
    pub dbottom: i32,
    pub visible: bool,
}

impl Default for Area {
    fn default() -> Self {
        Self {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
            dleft: 0,
            dtop: 0,
            dright: 0,
            dbottom: 0,
            visible: true,
        }
    }
}

impl Area {
    pub fn width(self) -> i32 {
        self.right.wrapping_sub(self.left)
    }

    pub fn height(self) -> i32 {
        self.bottom.wrapping_sub(self.top)
    }

    /// `Area.set`. Deltas are `new - old`.
    pub fn set(&mut self, left: i32, top: i32, right: i32, bottom: i32) {
        self.dleft = left.wrapping_sub(self.left);
        self.dtop = top.wrapping_sub(self.top);
        self.dright = right.wrapping_sub(self.right);
        self.dbottom = bottom.wrapping_sub(self.bottom);
        self.left = left;
        self.top = top;
        self.right = right;
        self.bottom = bottom;
    }

    pub fn set_rect(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.set(x, y, x.wrapping_add(width), y.wrapping_add(height));
    }

    pub fn reset_deltas(&mut self) {
        self.dleft = 0;
        self.dtop = 0;
        self.dright = 0;
        self.dbottom = 0;
    }

    pub fn contains_point(self, x: i32, y: i32) -> bool {
        if self.width() < 0 || self.height() < 0 {
            return false;
        }
        self.left <= x && x <= self.right && self.top <= y && y <= self.bottom
    }

    pub fn contains_area(self, other: Area) -> bool {
        if self.width() < 0 || self.height() < 0 || other.width() < 0 || other.height() < 0 {
            return false;
        }
        if other.left < self.left || other.top < self.top {
            return false;
        }
        if other.right <= other.left {
            if self.right >= self.left || other.right > self.right {
                return false;
            }
        } else if self.right >= self.left && other.right > self.right {
            return false;
        }
        if other.bottom <= other.top {
            if self.bottom >= self.top || other.bottom > self.bottom {
                return false;
            }
        } else if self.bottom >= self.top && other.bottom > self.bottom {
            return false;
        }
        true
    }

    /// `Area.intersects(Area)`. A zero or negative span does not intersect.
    pub fn intersects(self, other: Area) -> bool {
        let tw = self.width();
        let th = self.height();
        let aw = other.width();
        let ah = other.height();
        if aw <= 0 || ah <= 0 || tw <= 0 || th <= 0 {
            return false;
        }
        let tx = self.left;
        let ty = self.top;
        let ax = other.left;
        let ay = other.top;
        let tw = self.right;
        let th = self.bottom;
        let aw = other.right;
        let ah = other.bottom;
        (aw < ax || aw > tx) && (ah < ay || ah > ty) && (tw < tx || tw > ax) && (th < ty || th > ay)
    }

    pub fn top_border(self) -> FloorCeiling {
        FloorCeiling::horizontal(self, false)
    }

    pub fn bottom_border(self) -> FloorCeiling {
        FloorCeiling::horizontal(self, true)
    }

    pub fn left_border(self) -> Wall {
        Wall::vertical(self, false)
    }

    pub fn right_border(self) -> Wall {
        Wall::vertical(self, true)
    }
}

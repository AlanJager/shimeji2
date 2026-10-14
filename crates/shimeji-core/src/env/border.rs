//! `FloorCeiling` and `Wall`. `isOn` is exact-pixel equality.
//!
//! Ride rejection matches the vendored source: a floor or ceiling refuses
//! `abs(newX - x) >= 80`, `dy > 20`, or `dy < -80`. A wall refuses an 80 px
//! jump on either axis. The 80 px probe that snaps a falling mascot onto a
//! floor lives in `Fall.tick`, not here.

use super::area::{Area, Point};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloorCeiling {
    y: i32,
    left: i32,
    right: i32,
    dleft: i32,
    dright: i32,
    dy: i32,
    width: i32,
    visible: bool,
}

impl FloorCeiling {
    pub(crate) fn horizontal(area: Area, bottom: bool) -> Self {
        Self {
            y: if bottom { area.bottom } else { area.top },
            left: area.left,
            right: area.right,
            dleft: area.dleft,
            dright: area.dright,
            dy: if bottom { area.dbottom } else { area.dtop },
            width: area.width(),
            visible: area.visible,
        }
    }

    pub fn is_on(self, location: Point) -> bool {
        self.visible && self.y == location.y && self.left <= location.x && location.x <= self.right
    }

    pub fn move_point(self, location: Point) -> Point {
        if !self.visible {
            return location;
        }
        if self.dleft == 0 && self.dright == 0 && self.dy == 0 {
            return location;
        }
        let prev_left = self.left.wrapping_sub(self.dleft);
        let prev_width = self.right.wrapping_sub(self.dright).wrapping_sub(prev_left);
        if prev_width == 0 {
            return location;
        }
        let new_x = (location.x.wrapping_sub(prev_left))
            .wrapping_mul(self.width)
            .wrapping_div(prev_width)
            .wrapping_add(self.left);
        let new_y = location.y.wrapping_add(self.dy);
        if abs_delta(new_x, location.x) >= 80
            || new_y.wrapping_sub(location.y) > 20
            || new_y.wrapping_sub(location.y) < -80
        {
            return location;
        }
        Point::new(new_x, new_y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wall {
    x: i32,
    top: i32,
    bottom: i32,
    dtop: i32,
    dbottom: i32,
    dx: i32,
    height: i32,
    visible: bool,
}

impl Wall {
    pub(crate) fn vertical(area: Area, right: bool) -> Self {
        Self {
            x: if right { area.right } else { area.left },
            top: area.top,
            bottom: area.bottom,
            dtop: area.dtop,
            dbottom: area.dbottom,
            dx: if right { area.dright } else { area.dleft },
            height: area.height(),
            visible: area.visible,
        }
    }

    pub fn is_on(self, location: Point) -> bool {
        self.visible && self.x == location.x && self.top <= location.y && location.y <= self.bottom
    }

    pub fn move_point(self, location: Point) -> Point {
        if !self.visible {
            return location;
        }
        if self.dtop == 0 && self.dbottom == 0 && self.dx == 0 {
            return location;
        }
        let prev_top = self.top.wrapping_sub(self.dtop);
        let prev_height = self
            .bottom
            .wrapping_sub(self.dbottom)
            .wrapping_sub(prev_top);
        if prev_height == 0 {
            return location;
        }
        let new_x = location.x.wrapping_add(self.dx);
        let new_y = (location.y.wrapping_sub(prev_top))
            .wrapping_mul(self.height)
            .wrapping_div(prev_height)
            .wrapping_add(self.top);
        if abs_delta(new_x, location.x) >= 80 || abs_delta(new_y, location.y) >= 80 {
            return location;
        }
        Point::new(new_x, new_y)
    }
}

/// Java `Math.abs` for `int`: `Integer.MIN_VALUE` stays negative.
fn abs_delta(a: i32, b: i32) -> i32 {
    a.wrapping_sub(b).wrapping_abs()
}

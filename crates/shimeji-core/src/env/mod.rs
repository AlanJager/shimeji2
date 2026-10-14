//! Desktop snapshot, areas, borders, and each mascot's sticky work area.
//!
//! Coordinates are integer, top-left, y-down. Deltas are pixels since the
//! previous [`Environment::apply`]. The caller applies a snapshot once per tick.

mod area;
mod border;
mod snapshot;

pub use area::{Area, Point};
pub use border::{FloorCeiling, Wall};
pub use snapshot::{ScreenSample, Snapshot, WindowSample};

pub use area::Point as Anchor;

/// Smoothed cursor. [`Cursor::place`] is the trace harness's tick-0 rule.
/// [`Cursor::set`] is `Location.set`: `dx = (dx + x - oldX) / 2`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor {
    pub x: i32,
    pub y: i32,
    pub dx: i32,
    pub dy: i32,
}

impl Cursor {
    /// Store the point and zero the deltas. Does not smooth.
    pub fn place(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        self.dx = 0;
        self.dy = 0;
    }

    /// `Location.set`. Integer division truncates toward zero.
    pub fn set(&mut self, x: i32, y: i32) {
        self.dx = (self.dx + x - self.x) / 2;
        self.dy = (self.dy + y - self.y) / 2;
        self.x = x;
        self.y = y;
    }
}

/// Which edge an action grabbed at `init`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveBorder {
    /// `NotOnBorder`. `is_on` is false and `move` returns the point.
    NotOn,
    Active(Edge),
    Work {
        id: String,
        edge: Edge,
    },
    Screen {
        id: String,
        edge: Edge,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Shared world. Areas keep their identity across [`Environment::apply`] calls
/// so a stored [`LiveBorder`] sees the new deltas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    screen: Area,
    screens: Vec<(String, Area)>,
    work_areas: Vec<(String, Area)>,
    active_ie: Area,
    active_id: u64,
    cursor: Cursor,
    multiscreen: bool,
}

impl Environment {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn screen(&self) -> &Area {
        &self.screen
    }

    pub fn screens(&self) -> &[(String, Area)] {
        &self.screens
    }

    pub fn work_areas(&self) -> &[(String, Area)] {
        &self.work_areas
    }

    pub fn active_ie(&self) -> &Area {
        &self.active_ie
    }

    pub fn active_id(&self) -> u64 {
        self.active_id
    }

    /// Place the active window without inventing a resize delta.
    ///
    /// A carry updates the rect the actions see. The poller's own `set` already
    /// recorded the snapshot delta; this must not add a second one.
    pub fn place_active_ie(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.active_ie.left = x;
        self.active_ie.top = y;
        self.active_ie.right = x.wrapping_add(width);
        self.active_ie.bottom = y.wrapping_add(height);
        self.active_ie.visible = true;
        self.active_ie.reset_deltas();
    }

    /// Drop edge deltas on the active window without moving it.
    ///
    /// A rejected carry snaps the rect back to the snapshot. That correction
    /// is not a window the mascot was riding.
    pub fn clear_active_deltas(&mut self) {
        self.active_ie.reset_deltas();
    }

    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    pub fn multiscreen(&self) -> bool {
        self.multiscreen
    }

    /// Tick-0 cursor placement from the trace schema. Later samples use [`Self::apply`].
    pub fn place_cursor(&mut self, x: i32, y: i32) {
        self.cursor.place(x, y);
    }

    /// One environment tick. Recomputes edge deltas and smooths the cursor.
    pub fn apply(&mut self, snapshot: &Snapshot) {
        self.multiscreen = snapshot.multiscreen;
        retain_named(&mut self.screens, snapshot);
        retain_named(&mut self.work_areas, snapshot);

        let mut union_left = 0;
        let mut union_top = 0;
        let mut union_right = 0;
        let mut union_bottom = 0;
        let mut any = false;
        for screen in &snapshot.screens {
            set_named(
                &mut self.screens,
                &screen.id,
                screen.x,
                screen.y,
                screen.width,
                screen.height,
            );
            let work = screen.work.unwrap_or(WorkRect {
                x: screen.x,
                y: screen.y,
                width: screen.width,
                height: screen.height,
            });
            set_named(
                &mut self.work_areas,
                &screen.id,
                work.x,
                work.y,
                work.width,
                work.height,
            );
            let right = screen.x.wrapping_add(screen.width);
            let bottom = screen.y.wrapping_add(screen.height);
            if !any {
                union_left = screen.x;
                union_top = screen.y;
                union_right = right;
                union_bottom = bottom;
                any = true;
            } else {
                union_left = union_left.min(screen.x);
                union_top = union_top.min(screen.y);
                union_right = union_right.max(right);
                union_bottom = union_bottom.max(bottom);
            }
        }
        if any {
            self.screen
                .set(union_left, union_top, union_right, union_bottom);
            self.screen.visible = true;
        }

        match &snapshot.active_window {
            Some(window) if window.visible => {
                self.active_ie
                    .set_rect(window.x, window.y, window.width, window.height);
                self.active_ie.visible = true;
                self.active_id = if window.id == 0 { 1 } else { window.id };
            }
            Some(window) => {
                self.active_ie.visible = false;
                self.active_id = window.id;
            }
            None => {
                self.active_ie.visible = false;
                self.active_id = 0;
            }
        }

        self.cursor.set(snapshot.cursor.x, snapshot.cursor.y);
    }
}

fn retain_named(areas: &mut Vec<(String, Area)>, snapshot: &Snapshot) {
    areas.retain(|(id, _)| snapshot.screens.iter().any(|screen| screen.id == *id));
}

/// Copy areas in place when the ids already match, so a steady tick does not allocate.
fn assign_areas(dst: &mut Vec<(String, Area)>, src: &[(String, Area)]) {
    if dst.len() == src.len() && dst.iter().zip(src).all(|(left, right)| left.0 == right.0) {
        for (left, right) in dst.iter_mut().zip(src) {
            left.1 = right.1;
        }
        return;
    }
    dst.clear();
    dst.extend(src.iter().map(|(id, area)| (id.clone(), *area)));
}

/// `ComplexArea.set`: an exact rectangle already stored on any area skips the update.
fn set_named(areas: &mut Vec<(String, Area)>, name: &str, x: i32, y: i32, width: i32, height: i32) {
    if areas.iter().any(|(_, area)| {
        area.left == x && area.top == y && area.width() == width && area.height() == height
    }) {
        return;
    }
    if let Some((_, area)) = areas.iter_mut().find(|(id, _)| id == name) {
        area.set_rect(x, y, width, height);
        area.visible = true;
    } else {
        let mut area = Area::default();
        area.set_rect(x, y, width, height);
        area.visible = true;
        areas.push((name.to_string(), area));
    }
}

/// Work-area rect inside a [`ScreenSample`]. Omitted on the snapshot means the screen itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Sticky {
    Unset,
    Work(String),
    Screen(String),
    Missing,
}

impl Sticky {
    fn is_set(&self) -> bool {
        !matches!(self, Self::Unset)
    }
}

/// One mascot's view of the world, including the sticky work area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MascotEnvironment {
    pub(crate) desktop_border_priority: bool,
    screen: Area,
    screens: Vec<(String, Area)>,
    work_areas: Vec<(String, Area)>,
    active_ie: Area,
    active_id: u64,
    cursor: Cursor,
    multiscreen: bool,
    sticky: Sticky,
}

impl Default for MascotEnvironment {
    fn default() -> Self {
        Self {
            desktop_border_priority: false,
            screen: Area::default(),
            screens: Vec::new(),
            work_areas: Vec::new(),
            active_ie: Area::default(),
            active_id: 0,
            cursor: Cursor::default(),
            multiscreen: false,
            sticky: Sticky::Unset,
        }
    }
}

impl MascotEnvironment {
    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    pub fn screen(&self) -> &Area {
        &self.screen
    }

    pub fn active_ie(&self) -> &Area {
        &self.active_ie
    }

    pub fn active_id(&self) -> u64 {
        self.active_id
    }

    /// `MascotEnvironment.getWorkArea`. `force` is `refreshWorkArea`.
    pub fn sync(&mut self, anchor: Point, world: &Environment, force: bool) {
        self.screen = world.screen;
        assign_areas(&mut self.screens, &world.screens);
        assign_areas(&mut self.work_areas, &world.work_areas);
        self.active_ie = world.active_ie;
        self.active_id = world.active_id;
        self.cursor = world.cursor;
        self.multiscreen = world.multiscreen;
        let keep = !force && !self.multiscreen && self.sticky_present();
        if keep {
            return;
        }
        self.reselect(anchor);
    }

    pub fn work_area(&self) -> Area {
        match &self.sticky {
            Sticky::Work(id) => self
                .work_areas
                .iter()
                .find(|(k, _)| k == id)
                .map(|(_, a)| *a),
            Sticky::Screen(id) => self.screens.iter().find(|(k, _)| k == id).map(|(_, a)| *a),
            Sticky::Unset | Sticky::Missing => None,
        }
        .unwrap_or_else(invisible)
    }

    /// `getActiveIE`: a window that misses the sticky work area becomes an empty area
    /// when multiscreen is off.
    pub fn active_ie_for_script(&self) -> Area {
        if !self.multiscreen {
            if let Sticky::Work(id) | Sticky::Screen(id) = &self.sticky {
                let current = match &self.sticky {
                    Sticky::Work(_) => self.work_areas.iter().find(|(k, _)| k == id),
                    _ => self.screens.iter().find(|(k, _)| k == id),
                };
                if let Some((_, area)) = current {
                    if !area.intersects(self.active_ie) {
                        return Area::default();
                    }
                }
            }
        }
        self.active_ie
    }

    /// `getFloor(false)`. Active-window top, then the sticky work area's bottom.
    pub fn floor(&self, anchor: Point) -> LiveBorder {
        if self.desktop_border_priority {
            if let Some(border) = self.edge_if_on(anchor, Edge::Bottom) {
                return border;
            }
        }
        let ie = self.active_ie_for_script();
        if self.active_edge_available(Edge::Top) && ie.top_border().is_on(anchor) {
            return LiveBorder::Active(Edge::Top);
        }
        self.edge_if_on(anchor, Edge::Bottom)
            .unwrap_or(LiveBorder::NotOn)
    }

    /// `getCeiling(false)`. Active-window bottom, then the sticky work area's top.
    pub fn ceiling(&self, anchor: Point) -> LiveBorder {
        if self.desktop_border_priority {
            if let Some(border) = self.edge_if_on(anchor, Edge::Top) {
                return border;
            }
        }
        let ie = self.active_ie_for_script();
        if self.active_edge_available(Edge::Bottom) && ie.bottom_border().is_on(anchor) {
            return LiveBorder::Active(Edge::Bottom);
        }
        self.edge_if_on(anchor, Edge::Top)
            .unwrap_or(LiveBorder::NotOn)
    }

    /// `getWall(false)`. Window side from `look_right`, then the work-area side.
    pub fn wall(&self, anchor: Point, look_right: bool) -> LiveBorder {
        let ie = self.active_ie_for_script();
        let ie_edge = if look_right { Edge::Left } else { Edge::Right };
        if edge_on(&ie, ie_edge, anchor) {
            return LiveBorder::Active(ie_edge);
        }
        let work_edge = if look_right { Edge::Right } else { Edge::Left };
        self.edge_if_on(anchor, work_edge)
            .unwrap_or(LiveBorder::NotOn)
    }

    /// `getFloor(ignoreSeparator).isOn(point)` without building a border object.
    pub fn on_floor(&self, point: Point, ignore_separator: bool) -> bool {
        let ie = self.active_ie_for_script();
        if self.active_edge_available(Edge::Top) && ie.top_border().is_on(point) {
            return true;
        }
        let work = self.work_area();
        work.bottom_border().is_on(point) && (!ignore_separator || self.one_horizontal(point))
    }

    /// `getWall(ignoreSeparator).isOn(point)`.
    pub fn on_wall(&self, point: Point, look_right: bool, ignore_separator: bool) -> bool {
        let ie = self.active_ie_for_script();
        let ie_wall = if look_right {
            ie.left_border()
        } else {
            ie.right_border()
        };
        if ie_wall.is_on(point) {
            return true;
        }
        let work = self.work_area();
        let work_wall = if look_right {
            work.right_border()
        } else {
            work.left_border()
        };
        work_wall.is_on(point) && (!ignore_separator || self.one_vertical(point))
    }

    pub fn is_on(&self, border: &LiveBorder, point: Point) -> bool {
        match self.geometry(border) {
            Some(Geom::Floor(floor)) => floor.is_on(point),
            Some(Geom::Wall(wall)) => wall.is_on(point),
            None => false,
        }
    }

    pub fn move_on(&self, border: &LiveBorder, point: Point) -> Point {
        match self.geometry(border) {
            Some(Geom::Floor(floor)) => {
                if self.desktop_border_priority
                    && matches!(border, LiveBorder::Work { .. } | LiveBorder::Screen { .. })
                {
                    // A side Dock resizes the desktop; it is not a moving
                    // platform. Preserve x while still following vertical edges.
                    Point::new(point.x, floor.move_point(point).y)
                } else {
                    floor.move_point(point)
                }
            }
            Some(Geom::Wall(wall)) => wall.move_point(point),
            None => point,
        }
    }

    /// Keep the real window rectangle for movement and script coordinates;
    /// only its usable collision edges change in the app's desktop mode.
    pub(crate) fn is_on_active(&self, edge: Edge, point: Point) -> bool {
        self.active_edge_available(edge) && edge_on(&self.active_ie_for_script(), edge, point)
    }

    fn active_edge_available(&self, edge: Edge) -> bool {
        let work = self.work_area();
        if !self.desktop_border_priority || !work.visible {
            return true;
        }
        match edge {
            // Standing above a window flush with the desktop ceiling, or
            // hanging below one flush with its floor, puts the pose outside.
            Edge::Top => self.active_ie.top > work.top,
            Edge::Bottom => self.active_ie.bottom < work.bottom,
            Edge::Left | Edge::Right => true,
        }
    }

    fn sticky_present(&self) -> bool {
        match &self.sticky {
            Sticky::Unset => false,
            Sticky::Missing => true,
            Sticky::Work(id) => self.work_areas.iter().any(|(k, _)| k == id),
            Sticky::Screen(id) => self.screens.iter().any(|(k, _)| k == id),
        }
    }

    fn reselect(&mut self, anchor: Point) {
        let impl_work = self.find_work_index(anchor);
        if self.sticky.is_set() {
            if let Some(index) = impl_work {
                let different = match &self.sticky {
                    Sticky::Work(id) => self.work_areas.get(index).is_none_or(|(key, _)| key != id),
                    _ => true,
                };
                if different {
                    let next = self.work_areas.get(index).map(|(_, area)| *area);
                    if let (Some(current), Some(next)) = (self.sticky_area(), next) {
                        if current.contains_area(next) && next.contains_point(anchor.x, anchor.y) {
                            self.assign_work(index);
                            return;
                        }
                    }
                }
            }
            if let Some(current) = self.sticky_area() {
                if current.contains_point(anchor.x, anchor.y) {
                    return;
                }
            }
        }

        if let Some(index) = impl_work.or_else(|| self.find_work_index(anchor)) {
            if self
                .work_areas
                .get(index)
                .is_some_and(|(_, area)| area.contains_point(anchor.x, anchor.y))
            {
                self.assign_work(index);
                return;
            }
        }
        if let Some(index) = self.find_screen_index(anchor) {
            self.assign_screen(index);
            return;
        }
        self.sticky = Sticky::Missing;
    }

    fn assign_work(&mut self, index: usize) {
        let same = match &self.sticky {
            Sticky::Work(id) => self.work_areas.get(index).is_some_and(|(key, _)| key == id),
            _ => false,
        };
        if same {
            return;
        }
        let id = self.work_areas[index].0.clone();
        self.sticky = Sticky::Work(id);
    }

    fn assign_screen(&mut self, index: usize) {
        let same = match &self.sticky {
            Sticky::Screen(id) => self.screens.get(index).is_some_and(|(key, _)| key == id),
            _ => false,
        };
        if same {
            return;
        }
        let id = self.screens[index].0.clone();
        self.sticky = Sticky::Screen(id);
    }

    fn find_work_index(&self, anchor: Point) -> Option<usize> {
        self.work_areas
            .iter()
            .position(|(_, area)| area.contains_point(anchor.x, anchor.y))
    }

    fn find_screen_index(&self, anchor: Point) -> Option<usize> {
        self.screens
            .iter()
            .position(|(_, area)| area.contains_point(anchor.x, anchor.y))
    }

    fn work_by_id(&self, id: &str) -> Option<Area> {
        self.work_areas
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, area)| *area)
    }

    fn sticky_area(&self) -> Option<Area> {
        let area = self.work_area();
        if matches!(self.sticky, Sticky::Unset) {
            None
        } else {
            Some(area)
        }
    }

    /// `isScreenTopBottom`: exactly one screen top/bottom, else one work-area edge.
    fn one_horizontal(&self, point: Point) -> bool {
        self.one_edge(point, true)
    }

    /// `isScreenLeftRight`.
    fn one_vertical(&self, point: Point) -> bool {
        self.one_edge(point, false)
    }

    fn one_edge(&self, point: Point, horizontal: bool) -> bool {
        let mut count = 0;
        for (_, area) in &self.screens {
            count += edge_hits(area, point, horizontal);
        }
        if count == 0 {
            for (_, area) in &self.work_areas {
                count += edge_hits(area, point, horizontal);
            }
        }
        count == 1
    }

    fn edge_if_on(&self, anchor: Point, edge: Edge) -> Option<LiveBorder> {
        let area = self.work_area();
        if edge_on(&area, edge, anchor) {
            return Some(match &self.sticky {
                Sticky::Work(id) => LiveBorder::Work {
                    id: id.clone(),
                    edge,
                },
                Sticky::Screen(id) => LiveBorder::Screen {
                    id: id.clone(),
                    edge,
                },
                Sticky::Unset | Sticky::Missing => LiveBorder::NotOn,
            });
        }
        None
    }

    fn geometry(&self, border: &LiveBorder) -> Option<Geom> {
        let (area, edge) = match border {
            LiveBorder::NotOn => return None,
            LiveBorder::Active(edge) => {
                if !self.active_edge_available(*edge) {
                    return None;
                }
                (self.active_ie, *edge)
            }
            LiveBorder::Work { id, edge } => (self.work_by_id(id)?, *edge),
            LiveBorder::Screen { id, edge } => (
                self.screens
                    .iter()
                    .find(|(k, _)| k == id)
                    .map(|(_, area)| *area)?,
                *edge,
            ),
        };
        Some(geom_of(&area, edge))
    }
}

enum Geom {
    Floor(FloorCeiling),
    Wall(Wall),
}

fn geom_of(area: &Area, edge: Edge) -> Geom {
    match edge {
        Edge::Top => Geom::Floor(area.top_border()),
        Edge::Bottom => Geom::Floor(area.bottom_border()),
        Edge::Left => Geom::Wall(area.left_border()),
        Edge::Right => Geom::Wall(area.right_border()),
    }
}

fn edge_hits(area: &Area, point: Point, horizontal: bool) -> i32 {
    if horizontal {
        i32::from(area.top_border().is_on(point)) + i32::from(area.bottom_border().is_on(point))
    } else {
        i32::from(area.left_border().is_on(point)) + i32::from(area.right_border().is_on(point))
    }
}

fn edge_on(area: &Area, edge: Edge, point: Point) -> bool {
    match geom_of(area, edge) {
        Geom::Floor(floor) => floor.is_on(point),
        Geom::Wall(wall) => wall.is_on(point),
    }
}

fn invisible() -> Area {
    Area {
        visible: false,
        ..Area::default()
    }
}

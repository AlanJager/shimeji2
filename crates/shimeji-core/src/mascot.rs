//! One mascot's runtime state, and the [`Host`] `shimeji-expr` evaluates against.
//!
//! The manager (part B) owns the mascot list, the clock, and the RNG. This type
//! does not tick itself and does not draw random numbers. `target.*` anchor and
//! facing are stored here; `target.environment` reads this mascot's environment,
//! because part A has no second mascot to point at.

use shimeji_expr::{
    Accessor, Border, BorderKind, Field, Geom, Host, HostObject, HostVal, Root, Side, Value,
};

use crate::caps::TickBudget;
use crate::env::{Area, Edge, Environment, MascotEnvironment, Point};

/// Anchor, facing, image, time, and the sticky environment view.
#[derive(Debug, Clone, Default)]
pub struct Mascot {
    pub anchor: Point,
    pub look_right: bool,
    /// Java `Mascot.dragging`. `Dragged` and `Regist` set it from their own
    /// `tick`, so a sequence that wraps those classes still throws on release.
    pub(crate) dragging: bool,
    /// XML image path of the pose applied last. `None` until a pose runs.
    pub image_key: Option<String>,
    /// Anchor of the last non-null image (`prevImageAnchor`). Off-screen bounds subtract it.
    pub image_anchor: Option<Point>,
    /// Pixel size of that image (`prevImageSize`). Stays at the last image when a pose has none.
    pub image_width: i32,
    pub image_height: i32,
    /// Ticks since the mascot was created. Actions do not increment this.
    pub time: i32,
    pub total_count: i32,
    pub env: MascotEnvironment,
    pub target_anchor: Point,
    pub target_look_right: bool,
    pub target_total_count: i32,
    /// `ActionBase.next` clears this, then adds a non-empty `Affordance`.
    pub affordances: Vec<String>,
    /// Condition evaluations left in the current tick. Not part of equality.
    pub(crate) tick_budget: TickBudget,
    /// Captured by an action's `init` from [`crate::engine::EngineSettings`].
    pub(crate) scaling: f64,
    pub(crate) breeding: bool,
    pub(crate) transients: bool,
    pub(crate) throwing: bool,
    pub(crate) keep_on_screen: bool,
    /// Breed queued this tick. The engine admits them on the next flush.
    pub(crate) births: Vec<Birth>,
    /// Top-left of a window move requested this tick.
    pub(crate) ie_move: Option<(i32, i32)>,
    /// Pose sounds applied this tick. Empty in the steady state.
    pub(crate) sounds: Vec<(String, Option<String>)>,
    /// How many more mascots `Breed` may queue this tick. The engine sets it.
    pub(crate) spawn_room: usize,
    /// Set when [`Self::assign_image`] changes [`Self::image_key`]. The tick delta consumes it.
    pub(crate) image_dirty: bool,
    /// Recent image keys kept so a walking cycle can swap without allocating.
    image_spare: [Option<String>; 3],
}

/// One child [`Breed`](crate::model::Builtin::Breed) asked the engine to create.
#[derive(Debug, Clone)]
pub(crate) struct Birth {
    pub anchor: Point,
    pub look_right: bool,
    pub behavior: String,
}

impl PartialEq for Mascot {
    fn eq(&self, other: &Self) -> bool {
        self.anchor == other.anchor
            && self.look_right == other.look_right
            && self.image_key == other.image_key
            && self.image_anchor == other.image_anchor
            && self.image_width == other.image_width
            && self.image_height == other.image_height
            && self.time == other.time
            && self.total_count == other.total_count
            && self.env == other.env
            && self.target_anchor == other.target_anchor
            && self.target_look_right == other.target_look_right
            && self.target_total_count == other.target_total_count
            && self.affordances == other.affordances
    }
}

impl Mascot {
    pub fn new() -> Self {
        Self {
            scaling: 1.0,
            breeding: true,
            transients: true,
            throwing: true,
            ..Self::default()
        }
    }

    /// Copy the world and keep the sticky work area unless `force` is set.
    /// Part B calls this after [`Environment::apply`], before the behavior.
    pub fn sync(&mut self, world: &Environment, force: bool) {
        self.env.sync(self.anchor, world, force);
    }

    /// Spend one shared condition evaluation. `false` means the condition is
    /// false for this tick and must not run.
    pub(crate) fn allow_condition(&self) -> bool {
        self.tick_budget.allow()
    }

    /// Remember `next` as the current image. A key already in hand is swapped in.
    pub(crate) fn assign_image(&mut self, next: Option<&str>) {
        let Some(next) = next else {
            if self.image_key.is_some() {
                let previous = self.image_key.take();
                self.park(previous);
                self.image_dirty = true;
            }
            return;
        };
        if self.image_key.as_deref() == Some(next) {
            return;
        }
        if let Some(index) = self
            .image_spare
            .iter()
            .position(|item| item.as_deref() == Some(next))
        {
            let parked = self.image_spare[index].take();
            let previous = self.image_key.take();
            self.image_key = parked;
            self.image_spare[index] = previous;
            self.image_dirty = true;
            return;
        }
        let previous = self.image_key.replace(next.to_string());
        self.park(previous);
        self.image_dirty = true;
    }

    fn park(&mut self, previous: Option<String>) {
        if let Some(slot) = self.image_spare.iter_mut().find(|item| item.is_none()) {
            *slot = previous;
        } else if previous.is_some() {
            self.image_spare[0] = previous;
        }
    }
}

impl Host for Mascot {
    fn get(&self, accessor: Accessor) -> HostVal {
        let (anchor, look_right, total) = self.owner(accessor.root);
        let cursor = self.env.cursor();
        match accessor.field {
            Field::AnchorX => HostVal::Number(f64::from(anchor.x)),
            Field::AnchorY => HostVal::Number(f64::from(anchor.y)),
            Field::LookRight => HostVal::Bool(look_right),
            Field::TotalCount => HostVal::Number(f64::from(total)),
            Field::CursorX => HostVal::Number(f64::from(cursor.x)),
            Field::CursorY => HostVal::Number(f64::from(cursor.y)),
            Field::CursorDx => HostVal::Number(f64::from(cursor.dx)),
            Field::CursorDy => HostVal::Number(f64::from(cursor.dy)),
            Field::Screen(geom) => geom_val(self.env.screen(), geom),
            Field::WorkArea(geom) => geom_val(&self.env.work_area(), geom),
            Field::ActiveIe(geom) => geom_val(&self.env.active_ie_for_script(), geom),
        }
    }

    fn is_on(&self, border: Border, point: Value) -> bool {
        let Some(point) = self.anchor_value(point) else {
            return false;
        };
        let (owner, look_right, _) = self.owner(border.root);
        match border.kind {
            BorderKind::Floor => self.env.is_on(&self.env.floor(owner), point),
            BorderKind::Ceiling => self.env.is_on(&self.env.ceiling(owner), point),
            BorderKind::Wall => self.env.is_on(&self.env.wall(owner, look_right), point),
            BorderKind::Screen(side) => side_on(self.env.screen(), side, point),
            BorderKind::WorkArea(side) => side_on(&self.env.work_area(), side, point),
            BorderKind::ActiveIe(side) => self.env.is_on_active(
                match side {
                    Side::Left => Edge::Left,
                    Side::Right => Edge::Right,
                    Side::Top => Edge::Top,
                    Side::Bottom => Edge::Bottom,
                },
                point,
            ),
        }
    }
}

impl Mascot {
    fn owner(&self, root: Root) -> (Point, bool, i32) {
        match root {
            Root::Mascot => (self.anchor, self.look_right, self.total_count),
            Root::Target => (
                self.target_anchor,
                self.target_look_right,
                self.target_total_count,
            ),
        }
    }

    /// `isOn(mascot.anchor)` passes the anchor object, not its coordinates.
    fn anchor_value(&self, value: Value) -> Option<Point> {
        match value {
            Value::Host(HostObject::Anchor(Root::Mascot)) => Some(self.anchor),
            Value::Host(HostObject::Anchor(Root::Target)) => Some(self.target_anchor),
            _ => None,
        }
    }
}

fn geom_val(area: &Area, geom: Geom) -> HostVal {
    match geom {
        Geom::Left => HostVal::Number(f64::from(area.left)),
        Geom::Top => HostVal::Number(f64::from(area.top)),
        Geom::Right => HostVal::Number(f64::from(area.right)),
        Geom::Bottom => HostVal::Number(f64::from(area.bottom)),
        Geom::Width => HostVal::Number(f64::from(area.width())),
        Geom::Height => HostVal::Number(f64::from(area.height())),
        Geom::Visible => HostVal::Bool(area.visible),
    }
}

fn side_on(area: &Area, side: Side, point: Point) -> bool {
    match side {
        Side::Left => area.left_border().is_on(point),
        Side::Right => area.right_border().is_on(point),
        Side::Top => area.top_border().is_on(point),
        Side::Bottom => area.bottom_border().is_on(point),
    }
}

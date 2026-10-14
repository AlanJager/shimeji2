//! Host objects the expression language can read.
//!
//! Paths such as `mascot.environment.activeIE.topBorder` are folded to an
//! [`Accessor`] or a [`Border`] once, while the expression is compiled.
//! [`Host::get`] and [`Host::is_on`] receive those values and never a property
//! name. The engine (Unit 6) implements [`Host`]; [`StubHost`] is the test double.

use crate::value::{
    Accessor, Border, BorderKind, BuiltinFn, Field, Geom, HostObject, HostVal, Root, Side, Value,
};

/// Runtime view of the mascot and its environment.
///
/// `is_on` receives the argument expression's value. For `isOn(mascot.anchor)`
/// that value is [`Value::Host`]`(HostObject::Anchor(root))`. The stub ignores
/// the point and returns the flag it was given, which is what the Nashorn
/// vector harness does.
pub trait Host {
    fn get(&self, accessor: Accessor) -> HostVal;
    fn is_on(&self, border: Border, point: Value) -> bool;
}

/// Compile-time picture of a static value. Variable slots are not part of this;
/// the compiler checks those first.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Static {
    Number(f64),
    Undefined,
    Host(HostObject),
    Border(Border),
    Load(Accessor),
    Func(BuiltinFn),
    Math,
    IsOn(Border),
}

pub(crate) fn resolve_root(name: &str) -> Option<Static> {
    match name {
        "undefined" => Some(Static::Undefined),
        "NaN" => Some(Static::Number(f64::NAN)),
        "Infinity" => Some(Static::Number(f64::INFINITY)),
        "Math" => Some(Static::Math),
        "mascot" => Some(Static::Host(HostObject::Mascot(Root::Mascot))),
        "target" => Some(Static::Host(HostObject::Mascot(Root::Target))),
        _ => None,
    }
}

/// `Some` when `base` is a static object. Missing properties are
/// [`Static::Undefined`], matching a JavaScript read of a missing field.
/// `None` means the base is not a static object and the lookup stays dynamic.
pub(crate) fn resolve_prop(base: &Static, name: &str) -> Option<Static> {
    match base {
        Static::Host(object) => Some(prop_host(*object, name).unwrap_or(Static::Undefined)),
        Static::Border(border) => Some(if name == "isOn" {
            Static::IsOn(*border)
        } else {
            Static::Undefined
        }),
        Static::Math => Some(math_fn(name).map(Static::Func).unwrap_or(Static::Undefined)),
        _ => None,
    }
}

fn math_fn(name: &str) -> Option<BuiltinFn> {
    match name {
        "random" => Some(BuiltinFn::Random),
        "min" => Some(BuiltinFn::Min),
        "max" => Some(BuiltinFn::Max),
        "abs" => Some(BuiltinFn::Abs),
        "floor" => Some(BuiltinFn::Floor),
        "ceil" => Some(BuiltinFn::Ceil),
        "round" => Some(BuiltinFn::Round),
        _ => None,
    }
}

fn prop_host(object: HostObject, name: &str) -> Option<Static> {
    match object {
        HostObject::Mascot(root) => match name {
            "anchor" => Some(Static::Host(HostObject::Anchor(root))),
            "environment" => Some(Static::Host(HostObject::Environment(root))),
            "lookRight" => Some(load(root, Field::LookRight)),
            "totalCount" => Some(load(root, Field::TotalCount)),
            _ => None,
        },
        HostObject::Anchor(root) => match name {
            "x" => Some(load(root, Field::AnchorX)),
            "y" => Some(load(root, Field::AnchorY)),
            _ => None,
        },
        HostObject::Environment(root) => match name {
            "cursor" => Some(Static::Host(HostObject::Cursor(root))),
            "screen" => Some(Static::Host(HostObject::Screen(root))),
            "workArea" => Some(Static::Host(HostObject::WorkArea(root))),
            "activeIE" => Some(Static::Host(HostObject::ActiveIe(root))),
            "floor" => Some(border(root, BorderKind::Floor)),
            "ceiling" => Some(border(root, BorderKind::Ceiling)),
            "wall" => Some(border(root, BorderKind::Wall)),
            _ => None,
        },
        HostObject::Cursor(root) => match name {
            "x" => Some(load(root, Field::CursorX)),
            "y" => Some(load(root, Field::CursorY)),
            "dx" => Some(load(root, Field::CursorDx)),
            "dy" => Some(load(root, Field::CursorDy)),
            _ => None,
        },
        HostObject::Screen(root) => area_prop(root, AreaWhich::Screen, name),
        HostObject::WorkArea(root) => area_prop(root, AreaWhich::WorkArea, name),
        HostObject::ActiveIe(root) => area_prop(root, AreaWhich::ActiveIe, name),
    }
}

fn load(root: Root, field: Field) -> Static {
    Static::Load(Accessor { root, field })
}

fn border(root: Root, kind: BorderKind) -> Static {
    Static::Border(Border { root, kind })
}

enum AreaWhich {
    Screen,
    WorkArea,
    ActiveIe,
}

fn area_prop(root: Root, which: AreaWhich, name: &str) -> Option<Static> {
    if let Some(geom) = match name {
        "left" => Some(Geom::Left),
        "top" => Some(Geom::Top),
        "right" => Some(Geom::Right),
        "bottom" => Some(Geom::Bottom),
        "width" => Some(Geom::Width),
        "height" => Some(Geom::Height),
        "visible" => Some(Geom::Visible),
        _ => None,
    } {
        let field = match which {
            AreaWhich::Screen => Field::Screen(geom),
            AreaWhich::WorkArea => Field::WorkArea(geom),
            AreaWhich::ActiveIe => Field::ActiveIe(geom),
        };
        return Some(load(root, field));
    }
    let side = match name {
        "leftBorder" => Side::Left,
        "rightBorder" => Side::Right,
        "topBorder" => Side::Top,
        "bottomBorder" => Side::Bottom,
        _ => return None,
    };
    let kind = match which {
        AreaWhich::Screen => BorderKind::Screen(side),
        AreaWhich::WorkArea => BorderKind::WorkArea(side),
        AreaWhich::ActiveIe => BorderKind::ActiveIe(side),
    };
    Some(border(root, kind))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaFields {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub visible: bool,
    pub on_left: bool,
    pub on_right: bool,
    pub on_top: bool,
    pub on_bottom: bool,
}

impl Default for AreaFields {
    fn default() -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            visible: false,
            on_left: false,
            on_right: false,
            on_top: false,
            on_bottom: false,
        }
    }
}

impl AreaFields {
    pub fn width(self) -> f64 {
        self.right - self.left
    }

    pub fn height(self) -> f64 {
        self.bottom - self.top
    }

    fn side(self, side: Side) -> bool {
        match side {
            Side::Left => self.on_left,
            Side::Right => self.on_right,
            Side::Top => self.on_top,
            Side::Bottom => self.on_bottom,
        }
    }

    fn geom(self, geom: Geom) -> HostVal {
        match geom {
            Geom::Left => HostVal::Number(self.left),
            Geom::Top => HostVal::Number(self.top),
            Geom::Right => HostVal::Number(self.right),
            Geom::Bottom => HostVal::Number(self.bottom),
            Geom::Width => HostVal::Number(self.width()),
            Geom::Height => HostVal::Number(self.height()),
            Geom::Visible => HostVal::Bool(self.visible),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MascotFields {
    pub anchor_x: f64,
    pub anchor_y: f64,
    pub look_right: bool,
    pub total_count: f64,
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub cursor_dx: f64,
    pub cursor_dy: f64,
    pub screen: AreaFields,
    pub work_area: AreaFields,
    pub active_ie: AreaFields,
    pub floor: bool,
    pub ceiling: bool,
    pub wall: bool,
}

impl Default for MascotFields {
    fn default() -> Self {
        Self {
            anchor_x: 0.0,
            anchor_y: 0.0,
            look_right: false,
            total_count: 0.0,
            cursor_x: 0.0,
            cursor_y: 0.0,
            cursor_dx: 0.0,
            cursor_dy: 0.0,
            screen: AreaFields::default(),
            work_area: AreaFields::default(),
            active_ie: AreaFields::default(),
            floor: false,
            ceiling: false,
            wall: false,
        }
    }
}

/// Fixed host used by the Nashorn vectors and by tests that want the same shape.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StubHost {
    pub mascot: MascotFields,
    pub target: MascotFields,
}

impl StubHost {
    /// The stub captured in `tests/vectors/nashorn.json`.
    pub fn nashorn() -> Self {
        let screen = AreaFields {
            left: 0.0,
            top: 0.0,
            right: 1280.0,
            bottom: 800.0,
            visible: true,
            ..AreaFields::default()
        };
        let work_area = AreaFields {
            left: 0.0,
            top: 25.0,
            right: 1280.0,
            bottom: 800.0,
            visible: true,
            on_bottom: true,
            ..AreaFields::default()
        };
        let active_ie = AreaFields {
            left: 200.0,
            top: 100.0,
            right: 600.0,
            bottom: 400.0,
            visible: true,
            ..AreaFields::default()
        };
        Self {
            mascot: MascotFields {
                anchor_x: 100.0,
                anchor_y: 200.0,
                look_right: true,
                total_count: 3.0,
                cursor_x: 150.0,
                cursor_y: 80.0,
                cursor_dx: 2.0,
                cursor_dy: -1.0,
                screen,
                work_area,
                active_ie,
                floor: true,
                ceiling: false,
                wall: false,
            },
            target: MascotFields::default(),
        }
    }

    fn fields(&self, root: Root) -> &MascotFields {
        match root {
            Root::Mascot => &self.mascot,
            Root::Target => &self.target,
        }
    }
}

impl Host for StubHost {
    fn get(&self, accessor: Accessor) -> HostVal {
        let mascot = self.fields(accessor.root);
        match accessor.field {
            Field::AnchorX => HostVal::Number(mascot.anchor_x),
            Field::AnchorY => HostVal::Number(mascot.anchor_y),
            Field::LookRight => HostVal::Bool(mascot.look_right),
            Field::TotalCount => HostVal::Number(mascot.total_count),
            Field::CursorX => HostVal::Number(mascot.cursor_x),
            Field::CursorY => HostVal::Number(mascot.cursor_y),
            Field::CursorDx => HostVal::Number(mascot.cursor_dx),
            Field::CursorDy => HostVal::Number(mascot.cursor_dy),
            Field::Screen(geom) => mascot.screen.geom(geom),
            Field::WorkArea(geom) => mascot.work_area.geom(geom),
            Field::ActiveIe(geom) => mascot.active_ie.geom(geom),
        }
    }

    fn is_on(&self, border: Border, _point: Value) -> bool {
        let mascot = self.fields(border.root);
        match border.kind {
            BorderKind::Floor => mascot.floor,
            BorderKind::Ceiling => mascot.ceiling,
            BorderKind::Wall => mascot.wall,
            BorderKind::Screen(side) => mascot.screen.side(side),
            BorderKind::WorkArea(side) => mascot.work_area.side(side),
            BorderKind::ActiveIe(side) => mascot.active_ie.side(side),
        }
    }
}

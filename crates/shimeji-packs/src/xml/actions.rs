//! Action, animation, and pose elements.

use shimeji_core::diagnostics::{DiagnosticKind, Severity};
use shimeji_core::model::{
    Action, ActionKind, ActionNode, ActionRef, Animation, ClassResolution, Hotspot, Pose,
    resolve_class,
};

use super::dict::{CanonicalAttr, PlainType, Tag};
use super::{
    Ctx, Elem, LoadedDoc, collect_params, java_bool, parse_i32_trim, parse_pair, raw_attr,
    script_source,
};

pub(super) fn load_action_list(ctx: &mut Ctx<'_>, list: &Elem, doc: &mut LoadedDoc) {
    for child in &list.children {
        if ctx.dict.tag(&child.name) != Tag::Action {
            continue;
        }
        if let Some(action) = build_action(ctx, child, false) {
            let Some(name) = action.name.clone() else {
                continue;
            };
            doc.actions.insert(name, action);
        }
    }
}

fn build_action(ctx: &mut Ctx<'_>, elem: &Elem, anonymous: bool) -> Option<Action> {
    let params = collect_params(ctx, elem);
    let name = if anonymous {
        None
    } else {
        match params.get("Name") {
            Some(param) => Some(param.value.raw.clone()),
            None => {
                ctx.at(
                    Severity::Warning,
                    DiagnosticKind::MissingAttribute,
                    elem.line,
                    "action is missing a name",
                );
                return None;
            }
        }
    };
    let Some(type_raw) = params.get("Type").map(|param| param.value.raw.clone()) else {
        ctx.at(
            Severity::Warning,
            DiagnosticKind::MissingAttribute,
            elem.line,
            "action is missing a type",
        );
        return None;
    };
    let Some(plain) = ctx.dict.action_type(&type_raw) else {
        ctx.at(
            Severity::Warning,
            DiagnosticKind::UnknownActionType,
            elem.line,
            format!("unknown action type {type_raw}"),
        );
        return None;
    };
    let (kind, degraded_from) = match plain {
        PlainType::Move => (ActionKind::Move, None),
        PlainType::Stay => (ActionKind::Stay, None),
        PlainType::Animate => (ActionKind::Animate, None),
        PlainType::Sequence => (ActionKind::Sequence, None),
        PlainType::Select => (ActionKind::Select, None),
        PlainType::Embedded => match params.get("Class").map(|param| param.value.raw.clone()) {
            Some(class) => resolve_embedded(ctx, &class, elem.line),
            None => {
                ctx.at(
                    Severity::Warning,
                    DiagnosticKind::MissingAttribute,
                    elem.line,
                    "embedded action is missing a class",
                );
                return None;
            }
        },
    };

    let mut animations = Vec::new();
    let mut children = Vec::new();
    let mut saw_child = false;
    for child in &elem.children {
        match ctx.dict.tag(&child.name) {
            Tag::Animation => animations.push(build_animation(ctx, child)),
            Tag::Action => {
                saw_child = true;
                if let Some(action) = build_action(ctx, child, true) {
                    children.push(ActionNode::Action(action));
                }
            }
            Tag::ActionReference => {
                saw_child = true;
                if let Some(reference) = build_action_ref(ctx, child) {
                    children.push(ActionNode::Reference(reference));
                }
            }
            _ => {}
        }
    }
    if saw_child && !plain.is_complex() {
        ctx.at(
            Severity::Warning,
            DiagnosticKind::Other,
            elem.line,
            format!(
                "action type {type_raw} ignores child actions in the reference; they were kept"
            ),
        );
    }
    if plain.is_complex() && children.is_empty() {
        ctx.at(
            Severity::Error,
            DiagnosticKind::Xml,
            elem.line,
            format!("action type {type_raw} has no child actions"),
        );
    }

    Some(Action {
        name,
        schema: ctx.dict.schema(),
        kind,
        degraded_from,
        params,
        animations,
        children,
        line: elem.line,
    })
}

fn resolve_embedded(ctx: &mut Ctx<'_>, class: &str, line: u32) -> (ActionKind, Option<String>) {
    match resolve_class(class) {
        ClassResolution::Builtin(builtin) => (ActionKind::Builtin(builtin), None),
        ClassResolution::FastFollow(follow) => {
            ctx.diags.push(shimeji_core::diagnostics::Diagnostic::new(
                Severity::Info,
                DiagnosticKind::FastFollowClass,
                ctx.pack,
                ctx.file,
                Some(line),
                format!("class {class} is stored for a later unit"),
            ));
            (ActionKind::FastFollow(follow), None)
        }
        ClassResolution::Unknown => {
            ctx.diags.push(
                shimeji_core::diagnostics::Diagnostic::new(
                    Severity::Warning,
                    DiagnosticKind::UnknownClass,
                    ctx.pack,
                    ctx.file,
                    Some(line),
                    format!("unknown class {class}; using Stay"),
                )
                .with_fallback("Stay"),
            );
            (ActionKind::Stay, Some(class.to_string()))
        }
    }
}

fn build_action_ref(ctx: &mut Ctx<'_>, elem: &Elem) -> Option<ActionRef> {
    let params = collect_params(ctx, elem);
    let Some(name) = params.get("Name").map(|param| param.value.raw.clone()) else {
        ctx.at(
            Severity::Warning,
            DiagnosticKind::MissingAttribute,
            elem.line,
            "action reference is missing a name",
        );
        return None;
    };
    Some(ActionRef {
        name,
        params,
        line: elem.line,
    })
}

fn build_animation(ctx: &mut Ctx<'_>, elem: &Elem) -> Animation {
    let condition = raw_attr(ctx.dict, elem, CanonicalAttr::Condition)
        .map(|value| script_source(ctx, value, elem.line, "Condition"));
    let is_turn = match raw_attr(ctx.dict, elem, CanonicalAttr::IsTurn) {
        Some(value) => {
            let _ = script_source(ctx, value, elem.line, "IsTurn");
            java_bool(value)
        }
        None => false,
    };
    let mut poses = Vec::new();
    let mut hotspots = Vec::new();
    for child in &elem.children {
        match ctx.dict.tag(&child.name) {
            Tag::Pose => poses.push(build_pose(ctx, child)),
            Tag::Hotspot => hotspots.push(build_hotspot(ctx, child)),
            _ => {}
        }
    }
    if poses.is_empty() {
        ctx.at(
            Severity::Warning,
            DiagnosticKind::MissingAttribute,
            elem.line,
            "animation has no poses",
        );
    }
    Animation {
        condition,
        is_turn,
        poses,
        hotspots,
        line: elem.line,
    }
}

fn build_pose(ctx: &mut Ctx<'_>, elem: &Elem) -> Pose {
    let image_source = raw_attr(ctx.dict, elem, CanonicalAttr::Image).map(|value| {
        let _ = script_source(ctx, value, elem.line, "Image");
        value.to_string()
    });
    let image_right_source = raw_attr(ctx.dict, elem, CanonicalAttr::ImageRight).map(|value| {
        let _ = script_source(ctx, value, elem.line, "ImageRight");
        value.to_string()
    });
    let (anchor_x, anchor_y) = if image_source.is_some() {
        match raw_attr(ctx.dict, elem, CanonicalAttr::ImageAnchor) {
            Some(value) => {
                let _ = script_source(ctx, value, elem.line, "ImageAnchor");
                parse_pair(value).unwrap_or_else(|| {
                    ctx.at(
                        Severity::Warning,
                        DiagnosticKind::MissingAttribute,
                        elem.line,
                        format!("image anchor is not a pair: {value}"),
                    );
                    (0, 0)
                })
            }
            None => {
                ctx.at(
                    Severity::Warning,
                    DiagnosticKind::MissingAttribute,
                    elem.line,
                    "pose with an image is missing an anchor",
                );
                (0, 0)
            }
        }
    } else {
        (0, 0)
    };
    let (velocity_x, velocity_y) = match raw_attr(ctx.dict, elem, CanonicalAttr::Velocity) {
        Some(value) => {
            let _ = script_source(ctx, value, elem.line, "Velocity");
            parse_pair(value).unwrap_or_else(|| {
                ctx.at(
                    Severity::Warning,
                    DiagnosticKind::MissingAttribute,
                    elem.line,
                    format!("velocity is not a pair: {value}"),
                );
                (0, 0)
            })
        }
        None => {
            ctx.at(
                Severity::Warning,
                DiagnosticKind::MissingAttribute,
                elem.line,
                "pose is missing a velocity",
            );
            (0, 0)
        }
    };
    let duration_ticks = match raw_attr(ctx.dict, elem, CanonicalAttr::Duration) {
        Some(value) => {
            let _ = script_source(ctx, value, elem.line, "Duration");
            parse_i32_trim(value).unwrap_or_else(|| {
                ctx.at(
                    Severity::Warning,
                    DiagnosticKind::MissingAttribute,
                    elem.line,
                    format!("duration is not an integer: {value}"),
                );
                0
            })
        }
        None => {
            ctx.at(
                Severity::Warning,
                DiagnosticKind::MissingAttribute,
                elem.line,
                "pose is missing a duration",
            );
            0
        }
    };
    let sound = raw_attr(ctx.dict, elem, CanonicalAttr::Sound).map(str::to_string);
    let volume = raw_attr(ctx.dict, elem, CanonicalAttr::Volume).map(str::to_string);
    Pose {
        image_source,
        image: None,
        image_right_source,
        image_right: None,
        right_sibling: None,
        anchor_x,
        anchor_y,
        velocity_x,
        velocity_y,
        duration_ticks,
        sound,
        volume,
        line: elem.line,
    }
}

fn build_hotspot(ctx: &mut Ctx<'_>, elem: &Elem) -> Hotspot {
    let shape = required_text(ctx, elem, CanonicalAttr::Shape, "hotspot shape");
    let origin = required_text(ctx, elem, CanonicalAttr::Origin, "hotspot origin");
    let size = required_text(ctx, elem, CanonicalAttr::Size, "hotspot size");
    let behavior = raw_attr(ctx.dict, elem, CanonicalAttr::HotspotBehavior).map(str::to_string);
    if behavior.is_none() {
        ctx.at(
            Severity::Warning,
            DiagnosticKind::MissingAttribute,
            elem.line,
            "hotspot is missing a behavior",
        );
    }
    let (origin_x, origin_y) = parse_pair(&origin).unwrap_or_else(|| {
        if !origin.is_empty() {
            ctx.at(
                Severity::Warning,
                DiagnosticKind::MissingAttribute,
                elem.line,
                format!("hotspot origin is not a pair: {origin}"),
            );
        }
        (0, 0)
    });
    let (width, height) = parse_pair(&size).unwrap_or_else(|| {
        if !size.is_empty() {
            ctx.at(
                Severity::Warning,
                DiagnosticKind::MissingAttribute,
                elem.line,
                format!("hotspot size is not a pair: {size}"),
            );
        }
        (0, 0)
    });
    Hotspot {
        shape,
        origin,
        size,
        origin_x,
        origin_y,
        width,
        height,
        behavior,
        line: elem.line,
    }
}

fn required_text(ctx: &mut Ctx<'_>, elem: &Elem, attr: CanonicalAttr, label: &str) -> String {
    match raw_attr(ctx.dict, elem, attr) {
        Some(value) => value.to_string(),
        None => {
            ctx.at(
                Severity::Warning,
                DiagnosticKind::MissingAttribute,
                elem.line,
                format!("missing {label}"),
            );
            String::new()
        }
    }
}

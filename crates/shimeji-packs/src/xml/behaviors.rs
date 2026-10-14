//! Behavior lists, next-lists, constants, and `info.xml`.

use shimeji_core::diagnostics::{DiagnosticKind, Severity};
use shimeji_core::model::{
    Behavior, BehaviorNode, BehaviorRef, Constant, Contributor, ContributorRole, Information,
    NextList, Schema, ScriptSource,
};

use super::dict::{CanonicalAttr, Tag};
use super::{
    Ctx, Elem, LoadedDoc, collect_params, java_bool, parse_frequency, raw_attr, script_source,
    strip_structural,
};

pub(super) fn load_constant(ctx: &mut Ctx<'_>, elem: &Elem, doc: &mut LoadedDoc) {
    let Some(name) = raw_attr(ctx.dict, elem, CanonicalAttr::Name) else {
        ctx.at(
            Severity::Warning,
            DiagnosticKind::MissingAttribute,
            elem.line,
            "constant is missing a name",
        );
        return;
    };
    let Some(value) = raw_attr(ctx.dict, elem, CanonicalAttr::Value) else {
        ctx.at(
            Severity::Warning,
            DiagnosticKind::MissingAttribute,
            elem.line,
            "constant is missing a value",
        );
        return;
    };
    doc.constants.insert(
        name.to_string(),
        Constant {
            value: value.to_string(),
            line: elem.line,
        },
    );
}

pub(super) fn load_behavior_list(ctx: &mut Ctx<'_>, list: &Elem, doc: &mut LoadedDoc) {
    let mut nodes = Vec::new();
    walk_behaviors(ctx, list, &[], &mut nodes, doc);
    doc.behavior_tree.append(&mut nodes);
}

fn walk_behaviors(
    ctx: &mut Ctx<'_>,
    elem: &Elem,
    ancestors: &[ScriptSource],
    nodes: &mut Vec<BehaviorNode>,
    doc: &mut LoadedDoc,
) {
    for child in &elem.children {
        match ctx.dict.tag(&child.name) {
            Tag::Condition => {
                let own = raw_attr(ctx.dict, child, CanonicalAttr::Condition)
                    .map(|value| script_source(ctx, value, child.line, "Condition"));
                let mut next = ancestors.to_vec();
                if let Some(condition) = &own {
                    next.push(condition.clone());
                }
                let mut children = Vec::new();
                walk_behaviors(ctx, child, &next, &mut children, doc);
                nodes.push(BehaviorNode::Group {
                    condition: own,
                    children,
                    line: child.line,
                });
            }
            Tag::Behavior => {
                if let Some(behavior) = build_behavior(ctx, child, ancestors) {
                    nodes.push(BehaviorNode::Behavior {
                        name: behavior.name.clone(),
                        line: behavior.line,
                    });
                    doc.behaviors.insert(behavior.name.clone(), behavior);
                }
            }
            Tag::BehaviorReference => {
                if let Some(name) = raw_attr(ctx.dict, child, CanonicalAttr::Name) {
                    nodes.push(BehaviorNode::Reference {
                        name: name.to_string(),
                        line: child.line,
                    });
                }
            }
            _ => {}
        }
    }
}

fn build_behavior(ctx: &mut Ctx<'_>, elem: &Elem, ancestors: &[ScriptSource]) -> Option<Behavior> {
    let params_all = collect_params(ctx, elem);
    let Some(name) = params_all.get("Name").map(|param| param.value.raw.clone()) else {
        ctx.at(
            Severity::Warning,
            DiagnosticKind::MissingAttribute,
            elem.line,
            "behavior is missing a name",
        );
        return None;
    };
    let action = params_all
        .get("Action")
        .map(|param| param.value.raw.clone())
        .unwrap_or_else(|| name.clone());
    let frequency = match params_all.get("Frequency") {
        Some(param) => parse_frequency(&param.value.raw).unwrap_or_else(|| {
            ctx.at(
                Severity::Warning,
                DiagnosticKind::MissingAttribute,
                elem.line,
                format!(
                    "frequency is not a non-negative integer: {}",
                    param.value.raw
                ),
            );
            0
        }),
        None => {
            ctx.at(
                Severity::Warning,
                DiagnosticKind::MissingAttribute,
                elem.line,
                "behavior is missing a frequency",
            );
            0
        }
    };
    let hidden = params_all
        .get("Hidden")
        .is_some_and(|param| java_bool(&param.value.raw));
    let toggleable =
        if params_all.get("Toggleable").is_none() || is_required(ctx.dict.schema(), &name) {
            false
        } else {
            java_bool(&params_all.get("Toggleable").unwrap().value.raw)
        };
    let mut conditions = ancestors.to_vec();
    if let Some(condition) = params_all.get("Condition") {
        conditions.push(condition.value.clone());
    }
    let params = strip_structural(ctx.dict, params_all);
    let mut next_lists = Vec::new();
    for child in &elem.children {
        if ctx.dict.tag(&child.name) == Tag::NextBehaviorList {
            next_lists.push(build_next_list(ctx, child));
        }
    }
    let next_additive = next_lists.last().map(|list| list.add).unwrap_or(true);
    Some(Behavior {
        name,
        action,
        frequency,
        hidden,
        toggleable,
        conditions,
        params,
        next_lists,
        next_additive,
        line: elem.line,
    })
}

fn is_required(schema: Schema, name: &str) -> bool {
    schema.required_behaviors().contains(&name)
}

fn build_next_list(ctx: &mut Ctx<'_>, elem: &Elem) -> NextList {
    let add = match raw_attr(ctx.dict, elem, CanonicalAttr::Add) {
        Some(value) => {
            let _ = script_source(ctx, value, elem.line, "Add");
            java_bool(value)
        }
        None => {
            ctx.at(
                Severity::Error,
                DiagnosticKind::MissingAttribute,
                elem.line,
                "next-behavior list is missing Add",
            );
            false
        }
    };
    let mut nodes = Vec::new();
    let mut refs = Vec::new();
    walk_next(ctx, elem, &[], &mut nodes, &mut refs);
    NextList {
        add,
        nodes,
        refs,
        line: elem.line,
    }
}

fn walk_next(
    ctx: &mut Ctx<'_>,
    elem: &Elem,
    ancestors: &[ScriptSource],
    nodes: &mut Vec<BehaviorNode>,
    refs: &mut Vec<BehaviorRef>,
) {
    for child in &elem.children {
        match ctx.dict.tag(&child.name) {
            Tag::Condition => {
                let own = raw_attr(ctx.dict, child, CanonicalAttr::Condition)
                    .map(|value| script_source(ctx, value, child.line, "Condition"));
                let mut next = ancestors.to_vec();
                if let Some(condition) = &own {
                    next.push(condition.clone());
                }
                let mut children = Vec::new();
                let mut nested = Vec::new();
                walk_next(ctx, child, &next, &mut children, &mut nested);
                refs.append(&mut nested);
                nodes.push(BehaviorNode::Group {
                    condition: own,
                    children,
                    line: child.line,
                });
            }
            Tag::BehaviorReference => {
                if let Some(reference) = build_behavior_ref(ctx, child, ancestors) {
                    nodes.push(BehaviorNode::Reference {
                        name: reference.name.clone(),
                        line: reference.line,
                    });
                    refs.push(reference);
                }
            }
            Tag::Behavior => ctx.at(
                Severity::Warning,
                DiagnosticKind::Other,
                child.line,
                "behavior inside a next-list is ignored",
            ),
            _ => {}
        }
    }
}

fn build_behavior_ref(
    ctx: &mut Ctx<'_>,
    elem: &Elem,
    ancestors: &[ScriptSource],
) -> Option<BehaviorRef> {
    let params_all = collect_params(ctx, elem);
    let Some(name) = params_all.get("Name").map(|param| param.value.raw.clone()) else {
        ctx.at(
            Severity::Warning,
            DiagnosticKind::MissingAttribute,
            elem.line,
            "behavior reference is missing a name",
        );
        return None;
    };
    let frequency = match params_all.get("Frequency") {
        Some(param) => parse_frequency(&param.value.raw).unwrap_or_else(|| {
            ctx.at(
                Severity::Warning,
                DiagnosticKind::MissingAttribute,
                elem.line,
                format!(
                    "frequency is not a non-negative integer: {}",
                    param.value.raw
                ),
            );
            0
        }),
        None => {
            ctx.at(
                Severity::Warning,
                DiagnosticKind::MissingAttribute,
                elem.line,
                "behavior reference is missing a frequency",
            );
            0
        }
    };
    let mut conditions = ancestors.to_vec();
    if let Some(condition) = params_all.get("Condition") {
        conditions.push(condition.value.clone());
    }
    let params = strip_structural(ctx.dict, params_all);
    Some(BehaviorRef {
        name,
        frequency,
        conditions,
        params,
        line: elem.line,
    })
}

pub(super) fn load_information(ctx: &mut Ctx<'_>, elem: &Elem, info: &mut Information) {
    for child in &elem.children {
        match ctx.dict.tag(&child.name) {
            Tag::InfoName => {
                info.display_name = Some(child.text.trim().to_string());
            }
            Tag::PreviewImage => {
                info.preview_image = Some(child.text.trim().to_string());
            }
            Tag::SplashImage => {
                info.splash_image = Some(child.text.trim().to_string());
            }
            Tag::Artist => set_role(ctx, child, info, ContributorRole::Artist),
            Tag::Scripter => set_role(ctx, child, info, ContributorRole::Scripter),
            Tag::Commissioner => set_role(ctx, child, info, ContributorRole::Commissioner),
            Tag::Support => set_role(ctx, child, info, ContributorRole::Support),
            _ => {}
        }
    }
}

fn set_role(ctx: &mut Ctx<'_>, elem: &Elem, info: &mut Information, role: ContributorRole) {
    let Some(name) = raw_attr(ctx.dict, elem, CanonicalAttr::Name) else {
        return;
    };
    let url = raw_attr(ctx.dict, elem, CanonicalAttr::Url).map(str::to_string);
    info.set_contributor(Contributor {
        role,
        name: name.to_string(),
        url,
    });
}

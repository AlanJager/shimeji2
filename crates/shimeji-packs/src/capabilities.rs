//! Pose-name capabilities, and the behavior prune that follows from them.
//!
//! A behavior is removed when its action cannot run on the poses this pack
//! actually has. A sequence runs only when every step can. A select runs when
//! any branch can, so fall and throw keep the floor landing and lose the
//! wall-grab alternative. Next-list entries that named a removed behavior are
//! dropped; a list that becomes empty points at idle. A condition group that
//! loses every behavior gains one idle fallback, with that group's conditions.

use std::collections::{BTreeSet, HashMap, HashSet};

use shimeji_core::diagnostics::{Diagnostic, DiagnosticKind, Severity};
use shimeji_core::model::{
    Action, ActionKind, ActionNode, Animation, Behavior, BehaviorNode, BehaviorRef, Character,
    OrderedMap, Pack, Pose, Schema,
};

use crate::standard_atlas::{self, AtlasPose};

/// What the non-empty poses of a pack can support. The order matches the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PoseCapability {
    Core,
    Drag,
    Sit,
    Tumble,
    Wall,
    Ceiling,
    Window,
    Multiply,
    Look,
}

impl PoseCapability {
    pub fn label(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Drag => "drag",
            Self::Sit => "sit",
            Self::Tumble => "tumble",
            Self::Wall => "wall",
            Self::Ceiling => "ceiling",
            Self::Window => "window",
            Self::Multiply => "multiply",
            Self::Look => "look",
        }
    }
}

/// Capabilities of every non-empty pose in `pack`, in stable order.
pub fn pose_capabilities(pack: &Pack) -> Vec<PoseCapability> {
    let mut names = BTreeSet::new();
    for character in &pack.characters {
        names.extend(collect_pose_names(character));
    }
    capabilities_of(&names)
}

pub(crate) fn collect_pose_names(character: &Character) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut visit = |action: &Action| {
        for animation in &action.animations {
            for pose in &animation.poses {
                if pose.image.is_none() {
                    continue;
                }
                let Some(source) = pose.image_source.as_deref() else {
                    continue;
                };
                let Some(stem) = file_stem(source) else {
                    continue;
                };
                add_stem(&mut names, &stem);
            }
        }
    };
    for action in character.actions.values() {
        walk_shared(action, &mut visit);
    }
    names
}

pub(crate) fn missing_core(names: &BTreeSet<String>) -> Vec<&'static str> {
    ["stand", "walk-a", "walk-b", "fall"]
        .into_iter()
        .filter(|name| !names.contains(*name))
        .collect()
}

fn capabilities_of(names: &BTreeSet<String>) -> Vec<PoseCapability> {
    let mut out = Vec::new();
    if missing_core(names).is_empty() {
        out.push(PoseCapability::Core);
    }
    if family(names, "pinched") && family(names, "resist") {
        out.push(PoseCapability::Drag);
    }
    if family(names, "sit") && family(names, "dangle") && family(names, "spin") {
        out.push(PoseCapability::Sit);
    }
    if family(names, "bounce")
        && (family(names, "crawl") || family(names, "lie"))
        && family(names, "jump")
    {
        out.push(PoseCapability::Tumble);
    }
    if family(names, "wall-grab") && family(names, "climb") {
        out.push(PoseCapability::Wall);
    }
    if names.contains("ceiling-grab") && names.contains("ceiling-a") && names.contains("ceiling-b")
    {
        out.push(PoseCapability::Ceiling);
    }
    if family(names, "carry") && family(names, "throw") {
        out.push(PoseCapability::Window);
    }
    if family(names, "pullout") && family(names, "split") {
        out.push(PoseCapability::Multiply);
    }
    let looks = names
        .iter()
        .filter(|name| name.as_str() == "look" || name.starts_with("look-"))
        .count();
    if looks >= 16 {
        out.push(PoseCapability::Look);
    }
    out
}

fn family(names: &BTreeSet<String>, stem: &str) -> bool {
    let prefix = format!("{stem}-");
    names
        .iter()
        .any(|name| name == stem || name.starts_with(&prefix))
}

fn add_stem(names: &mut BTreeSet<String>, stem: &str) {
    if let Some(index) = shime_index(stem) {
        if let Some(pose) = standard_atlas::by_index(index) {
            insert_pose(names, pose);
            return;
        }
    }
    if let Some(pose) = standard_atlas::by_name(stem) {
        insert_pose(names, pose);
        return;
    }
    names.insert(stem.to_string());
}

fn insert_pose(names: &mut BTreeSet<String>, pose: &AtlasPose) {
    for name in standard_atlas::names_of(pose) {
        names.insert(name.to_string());
    }
}

fn shime_index(stem: &str) -> Option<u16> {
    let rest = stem.strip_prefix("shime")?;
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

pub(crate) fn file_stem(source: &str) -> Option<String> {
    let relative = source
        .trim()
        .trim_start_matches(['/', '\\'])
        .replace('\\', "/");
    let file = relative.rsplit('/').next().unwrap_or(relative.as_str());
    let stem = file.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(file);
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_string())
    }
}

/// Drop behaviors whose poses are missing. Returns the removed names, sorted.
pub(crate) fn prune_character(character: &mut Character) -> Vec<String> {
    let runnable = runnable_map(&character.actions);
    character
        .actions
        .for_each_value_mut(|action| strip_action(action, &runnable));

    let pruned: HashSet<String> = character
        .behaviors
        .iter()
        .filter(|(_, behavior)| !runnable.get(&behavior.action).copied().unwrap_or(false))
        .map(|(name, _)| name.to_string())
        .collect();

    let idle_name = ["立ってボーっとする", "Idle", "Stand"]
        .into_iter()
        .find(|name| character.behaviors.contains(name) && !pruned.contains(*name))
        .map(str::to_string);
    let idle_action = idle_name.as_ref().and_then(|name| {
        character
            .behaviors
            .get(name)
            .map(|behavior| behavior.action.clone())
    });

    let idle_for_lists = idle_name.clone();
    character.behaviors.for_each_value_mut(|behavior| {
        if pruned.contains(&behavior.name) {
            return;
        }
        for list in &mut behavior.next_lists {
            let had_refs = !list.refs.is_empty();
            list.refs
                .retain(|reference| !pruned.contains(&reference.name));
            filter_nodes(&mut list.nodes, &pruned);
            if had_refs && list.refs.is_empty() {
                if let Some(idle) = &idle_for_lists {
                    list.refs.push(BehaviorRef {
                        name: idle.clone(),
                        frequency: 1,
                        conditions: Vec::new(),
                        params: OrderedMap::new(),
                        line: 0,
                    });
                    list.nodes.push(BehaviorNode::Reference {
                        name: idle.clone(),
                        line: 0,
                    });
                }
            }
        }
    });

    if let Some(action) = idle_action.as_deref() {
        rewrite_tree(
            &mut character.behavior_tree,
            &mut character.behaviors,
            &pruned,
            action,
            character.behavior_schema,
            &[],
        );
    } else {
        filter_nodes(&mut character.behavior_tree, &pruned);
    }

    let pairs = std::mem::take(&mut character.behaviors).into_pairs();
    let mut kept = OrderedMap::new();
    for (name, behavior) in pairs {
        if !pruned.contains(&name) {
            kept.insert(name, behavior);
        }
    }
    character.behaviors = kept;

    let mut names: Vec<String> = pruned.into_iter().collect();
    names.sort();
    names
}

pub(crate) fn note_pruned(diags: &mut Vec<Diagnostic>, pack: &str, file: &str, pruned: &[String]) {
    if pruned.is_empty() {
        return;
    }
    diags.push(Diagnostic::new(
        Severity::Info,
        DiagnosticKind::Other,
        pack,
        file,
        None,
        format!(
            "pruned behaviors that need poses this pack does not have: {}",
            pruned.join(", ")
        ),
    ));
}

fn filter_nodes(nodes: &mut Vec<BehaviorNode>, pruned: &HashSet<String>) {
    let old = std::mem::take(nodes);
    for node in old {
        match node {
            BehaviorNode::Group {
                condition,
                mut children,
                line,
            } => {
                filter_nodes(&mut children, pruned);
                if !children.is_empty() {
                    nodes.push(BehaviorNode::Group {
                        condition,
                        children,
                        line,
                    });
                }
            }
            BehaviorNode::Behavior { name, line } if !pruned.contains(&name) => {
                nodes.push(BehaviorNode::Behavior { name, line });
            }
            BehaviorNode::Reference { name, line } if !pruned.contains(&name) => {
                nodes.push(BehaviorNode::Reference { name, line });
            }
            BehaviorNode::Behavior { .. } | BehaviorNode::Reference { .. } => {}
        }
    }
}

fn rewrite_tree(
    nodes: &mut Vec<BehaviorNode>,
    behaviors: &mut OrderedMap<Behavior>,
    pruned: &HashSet<String>,
    idle_action: &str,
    schema: Schema,
    ancestors: &[shimeji_core::model::ScriptSource],
) {
    let old = std::mem::take(nodes);
    let mut rebuilt = Vec::new();
    let mut had_direct = false;
    let mut fallback_conditions = None;
    let mut max_frequency = 0i32;
    for node in old {
        match node {
            BehaviorNode::Group {
                condition,
                mut children,
                line,
            } => {
                let mut next = ancestors.to_vec();
                if let Some(condition) = &condition {
                    next.push(condition.clone());
                }
                rewrite_tree(&mut children, behaviors, pruned, idle_action, schema, &next);
                rebuilt.push(BehaviorNode::Group {
                    condition,
                    children,
                    line,
                });
            }
            BehaviorNode::Behavior { name, line } => {
                had_direct = true;
                if pruned.contains(&name) {
                    if let Some(behavior) = behaviors.get(&name) {
                        if fallback_conditions.is_none() {
                            fallback_conditions = Some(behavior.conditions.clone());
                        }
                        max_frequency = max_frequency.max(behavior.frequency);
                    }
                } else {
                    rebuilt.push(BehaviorNode::Behavior { name, line });
                }
            }
            BehaviorNode::Reference { name, line } => {
                had_direct = true;
                if pruned.contains(&name) {
                    if let Some(behavior) = behaviors.get(&name) {
                        if fallback_conditions.is_none() {
                            fallback_conditions = Some(behavior.conditions.clone());
                        }
                        max_frequency = max_frequency.max(behavior.frequency);
                    }
                } else {
                    rebuilt.push(BehaviorNode::Reference { name, line });
                }
            }
        }
    }
    if had_direct && rebuilt.is_empty() {
        let base = if schema == Schema::Japanese {
            "立ち止まる"
        } else {
            "IdleFallback"
        };
        let name = fresh_name(behaviors, base);
        let frequency = if max_frequency >= 1 {
            max_frequency
        } else {
            100
        };
        let conditions = fallback_conditions.unwrap_or_else(|| ancestors.to_vec());
        behaviors.insert(
            name.clone(),
            Behavior {
                name: name.clone(),
                action: idle_action.to_string(),
                frequency,
                hidden: false,
                toggleable: false,
                conditions,
                params: OrderedMap::new(),
                next_lists: Vec::new(),
                next_additive: true,
                line: 0,
            },
        );
        rebuilt.push(BehaviorNode::Behavior { name, line: 0 });
    }
    *nodes = rebuilt;
}

fn fresh_name(behaviors: &OrderedMap<Behavior>, base: &str) -> String {
    if !behaviors.contains(base) {
        return base.to_string();
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}{n}");
        if !behaviors.contains(&candidate) {
            return candidate;
        }
        n = n.saturating_add(1);
        if n == u32::MAX {
            return format!("{base}-{n}");
        }
    }
}

fn runnable_map(actions: &OrderedMap<Action>) -> HashMap<String, bool> {
    let mut memo: HashMap<String, Option<bool>> = HashMap::new();
    for name in actions.keys() {
        eval_named(name, actions, &mut memo);
    }
    memo.into_iter()
        .map(|(name, value)| (name, value.unwrap_or(true)))
        .collect()
}

fn eval_named(
    name: &str,
    actions: &OrderedMap<Action>,
    memo: &mut HashMap<String, Option<bool>>,
) -> bool {
    match memo.get(name) {
        Some(Some(value)) => return *value,
        Some(None) => return true,
        None => {}
    }
    memo.insert(name.to_string(), None);
    let value = actions
        .get(name)
        .is_some_and(|action| eval_action(action, actions, memo));
    memo.insert(name.to_string(), Some(value));
    value
}

fn eval_action(
    action: &Action,
    actions: &OrderedMap<Action>,
    memo: &mut HashMap<String, Option<bool>>,
) -> bool {
    if action.animations.iter().any(animation_ok) {
        return true;
    }
    if children_can_run(action, actions, memo) {
        return true;
    }
    action.animations.is_empty() && action.children.is_empty()
}

fn node_runs(
    node: &ActionNode,
    actions: &OrderedMap<Action>,
    memo: &mut HashMap<String, Option<bool>>,
) -> bool {
    match node {
        ActionNode::Action(child) => eval_action(child, actions, memo),
        ActionNode::Reference(reference) => eval_named(&reference.name, actions, memo),
    }
}

/// Sequence needs every child. Select, and every other kind, needs any child.
fn children_can_run(
    action: &Action,
    actions: &OrderedMap<Action>,
    memo: &mut HashMap<String, Option<bool>>,
) -> bool {
    match action.kind {
        ActionKind::Sequence => {
            !action.children.is_empty()
                && action
                    .children
                    .iter()
                    .all(|child| node_runs(child, actions, memo))
        }
        _ => action
            .children
            .iter()
            .any(|child| node_runs(child, actions, memo)),
    }
}

fn children_kept(action: &Action, runnable: &HashMap<String, bool>) -> bool {
    match action.kind {
        ActionKind::Sequence => {
            !action.children.is_empty()
                && action
                    .children
                    .iter()
                    .all(|child| node_kept(child, runnable))
        }
        _ => action
            .children
            .iter()
            .any(|child| node_kept(child, runnable)),
    }
}

fn strip_action(action: &mut Action, runnable: &HashMap<String, bool>) {
    let ok = if let Some(name) = &action.name {
        runnable.get(name).copied().unwrap_or(false)
    } else {
        anonymous_ok(action, runnable)
    };
    if !ok {
        return;
    }
    action.animations.retain(animation_ok);
    let children = std::mem::take(&mut action.children);
    for child in children {
        match child {
            ActionNode::Reference(reference) => {
                if runnable.get(&reference.name).copied().unwrap_or(false) {
                    action.children.push(ActionNode::Reference(reference));
                }
            }
            ActionNode::Action(mut child) => {
                if anonymous_ok(&child, runnable) {
                    strip_action(&mut child, runnable);
                    action.children.push(ActionNode::Action(child));
                }
            }
        }
    }
}

fn node_kept(node: &ActionNode, runnable: &HashMap<String, bool>) -> bool {
    match node {
        ActionNode::Action(child) => anonymous_ok(child, runnable),
        ActionNode::Reference(reference) => runnable.get(&reference.name).copied().unwrap_or(false),
    }
}

fn anonymous_ok(action: &Action, runnable: &HashMap<String, bool>) -> bool {
    if action.animations.iter().any(animation_ok) {
        return true;
    }
    if children_kept(action, runnable) {
        return true;
    }
    action.animations.is_empty() && action.children.is_empty()
}

fn animation_ok(animation: &Animation) -> bool {
    !animation.poses.is_empty() && animation.poses.iter().all(pose_ok)
}

fn pose_ok(pose: &Pose) -> bool {
    pose.image_source.is_none() || pose.image.is_some()
}

fn walk_shared(action: &Action, visit: &mut impl FnMut(&Action)) {
    visit(action);
    for child in &action.children {
        if let ActionNode::Action(child) = child {
            walk_shared(child, visit);
        }
    }
}

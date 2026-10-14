//! Compile a character's actions once. Each instantiate builds a fresh [`VarMap`].
//!
//! `shimeji-expr` ties bytecode to one map, so the template stores source text
//! and slot names, not nodes. Action-reference attributes overlay the referenced
//! action, matching `ActionRef.buildAction` then `ActionBuilder.createVariables`.

use shimeji_expr::{VarKind, VarMap};

use crate::action::{ActionError, RunningAction, RunsAs};
use crate::diagnostics::{Diagnostic, DiagnosticKind, Severity};
use crate::model::{
    Action, ActionKind, ActionNode, ActionRef, Animation, Builtin, Character, OrderedMap, Param,
    Pose, Schema, ScriptKind, ScriptSource,
};

/// One variable slot. `slot` is the XML attribute name, which is the name Java looks up.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Binding {
    pub canonical: String,
    pub slot: String,
    pub value: SlotValue,
}

/// A constant, or a script body still wrapped the way the XML wrote it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SlotValue {
    Number(f64),
    Bool(bool),
    Str(String),
    Script { kind: VarKind, source: String },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnimTemplate {
    pub condition: Option<SlotValue>,
    pub is_turn: bool,
    pub poses: Vec<Pose>,
    pub duration: i32,
}

/// Lowered action. Built once per character; [`RunningAction::from_template`] per use.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionTemplate {
    pub name: Option<String>,
    pub class_name: &'static str,
    pub runs: RunsAs,
    pub schema: Schema,
    pub(crate) bindings: Vec<Binding>,
    pub(crate) animations: Vec<AnimTemplate>,
    pub(crate) children: Vec<ActionTemplate>,
}

/// Every top-level action of one character, in document order.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    actions: OrderedMap<ActionTemplate>,
    diagnostics: Vec<Diagnostic>,
}

impl Program {
    pub fn get(&self, name: &str) -> Option<&ActionTemplate> {
        self.actions.get(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.actions.keys()
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn instantiate(&self, name: &str) -> Result<RunningAction, ActionError> {
        self.instantiate_with(name, &OrderedMap::new())
    }

    /// Behavior parameters overlay the action. Incoming names win, as
    /// `BehaviorBuilder.buildBehavior(Map)` then `createVariables` do.
    pub fn instantiate_with(
        &self,
        name: &str,
        overlay: &OrderedMap<Param>,
    ) -> Result<RunningAction, ActionError> {
        let template = self
            .actions
            .get(name)
            .ok_or_else(|| ActionError::UnknownAction(name.to_string()))?;
        RunningAction::from_template(template, overlay)
    }
}

/// Lower `character` into templates. A bad script becomes a string constant and
/// a diagnostic, so the rest of the character still compiles.
pub fn compile_character(character: &Character) -> Program {
    let mut ctx = Ctx {
        character,
        stack: Vec::new(),
        diags: Vec::new(),
    };
    let mut actions = OrderedMap::new();
    for (name, action) in character.actions.iter() {
        let template = ctx.compile_action(action);
        actions.insert(name, template);
    }
    Program {
        actions,
        diagnostics: ctx.diags,
    }
}

struct Ctx<'a> {
    character: &'a Character,
    stack: Vec<String>,
    diags: Vec<Diagnostic>,
}

impl Ctx<'_> {
    fn compile_action(&mut self, action: &Action) -> ActionTemplate {
        let pushed = if let Some(name) = &action.name {
            if self.stack.iter().any(|item| item == name) {
                self.error(
                    DiagnosticKind::Other,
                    action.line,
                    format!("action reference cycle at \"{name}\""),
                );
                return duration_zero_stay(name);
            }
            self.stack.push(name.clone());
            true
        } else {
            false
        };
        let template = self.compile_action_body(action);
        if pushed {
            self.stack.pop();
        }
        template
    }

    fn compile_action_body(&mut self, action: &Action) -> ActionTemplate {
        let runs = runs_as(action.kind);
        let mut bindings = Vec::new();
        for (key, param) in action.params.iter() {
            push_binding(&mut bindings, key, param);
        }
        let mut animations = action.animations.iter().map(lower_anim).collect::<Vec<_>>();
        self.validate(action.schema, action.line, &mut bindings, &mut animations);

        let children = if matches!(runs, RunsAs::Sequence | RunsAs::Select) {
            if action.children.is_empty() {
                self.error(
                    DiagnosticKind::Other,
                    action.line,
                    "sequence or select has no children",
                );
            }
            action
                .children
                .iter()
                .map(|child| self.compile_child(child))
                .collect()
        } else {
            Vec::new()
        };

        ActionTemplate {
            name: action.name.clone(),
            class_name: class_name(action.kind),
            runs,
            schema: action.schema,
            bindings,
            animations,
            children,
        }
    }

    fn compile_child(&mut self, node: &ActionNode) -> ActionTemplate {
        match node {
            ActionNode::Action(action) => self.compile_action(action),
            ActionNode::Reference(reference) => self.compile_ref(reference),
        }
    }

    fn compile_ref(&mut self, reference: &ActionRef) -> ActionTemplate {
        if self.stack.iter().any(|item| item == &reference.name) {
            self.error(
                DiagnosticKind::Other,
                reference.line,
                format!("action reference cycle at \"{}\"", reference.name),
            );
            return duration_zero_stay(&reference.name);
        }
        let Some(target) = self.character.actions.get(&reference.name) else {
            self.error(
                DiagnosticKind::MissingReference,
                reference.line,
                format!("action \"{}\" is not defined", reference.name),
            );
            return duration_zero_stay(&reference.name);
        };
        self.stack.push(reference.name.clone());
        let mut template = self.compile_action_body(target);
        for (key, param) in reference.params.iter() {
            push_binding(&mut template.bindings, key, param);
        }
        self.validate(
            template.schema,
            reference.line,
            &mut template.bindings,
            &mut template.animations,
        );
        self.stack.pop();
        template
    }

    fn validate(
        &mut self,
        schema: Schema,
        line: u32,
        bindings: &mut [Binding],
        animations: &mut [AnimTemplate],
    ) {
        let mut probe = VarMap::new();
        for binding in bindings.iter() {
            probe.reserve(&binding.slot);
        }
        for name in engine_names(schema) {
            probe.reserve(name);
        }
        for index in 0..animations.len() {
            probe.reserve(&anim_slot(index));
        }
        for binding in bindings.iter_mut() {
            let SlotValue::Script { kind, source } = &binding.value else {
                continue;
            };
            let kind = *kind;
            let source = source.clone();
            if let Err(error) = probe.define(&binding.slot, kind, &source) {
                self.error(
                    DiagnosticKind::Expression,
                    line,
                    format!("parameter \"{}\": {error}", binding.slot),
                );
                binding.value = SlotValue::Str(source);
            }
        }
        for (index, anim) in animations.iter_mut().enumerate() {
            let Some(SlotValue::Script { kind, source }) = &anim.condition else {
                continue;
            };
            let kind = *kind;
            let source = source.clone();
            if let Err(error) = probe.define(&anim_slot(index), kind, &source) {
                self.error(
                    DiagnosticKind::Expression,
                    line,
                    format!("animation condition: {error}"),
                );
                anim.condition = Some(SlotValue::Str(source));
            }
        }
    }

    fn error(&mut self, kind: DiagnosticKind, line: u32, message: impl Into<String>) {
        self.diags.push(Diagnostic::new(
            Severity::Error,
            kind,
            self.character.name.clone(),
            self.character.actions_file.clone(),
            Some(line),
            message,
        ));
    }
}

fn push_binding(bindings: &mut Vec<Binding>, key: &str, param: &Param) {
    let value = lower_value(&param.value);
    let slot = param.source_name.clone();
    if let Some(existing) = bindings
        .iter_mut()
        .find(|binding| binding.canonical == key || binding.slot == slot)
    {
        existing.canonical = key.to_string();
        existing.slot = slot;
        existing.value = value;
    } else {
        bindings.push(Binding {
            canonical: key.to_string(),
            slot,
            value,
        });
    }
}

fn lower_value(source: &ScriptSource) -> SlotValue {
    match source.wrapper {
        Some(ScriptKind::Once) => SlotValue::Script {
            kind: VarKind::Dollar,
            source: source.raw.clone(),
        },
        Some(ScriptKind::EveryTick) => SlotValue::Script {
            kind: VarKind::Hash,
            source: source.raw.clone(),
        },
        None => lower_constant(&source.raw),
    }
}

fn lower_anim(animation: &Animation) -> AnimTemplate {
    let duration = animation
        .poses
        .iter()
        .fold(0i32, |sum, pose| sum.saturating_add(pose.duration_ticks));
    AnimTemplate {
        condition: animation.condition.as_ref().map(lower_value),
        is_turn: animation.is_turn,
        poses: animation.poses.clone(),
        duration,
    }
}

/// `Variable.parse` constant branch: exact `true`/`false`, else `Double.parseDouble`, else string.
fn lower_constant(raw: &str) -> SlotValue {
    if raw == "true" {
        return SlotValue::Bool(true);
    }
    if raw == "false" {
        return SlotValue::Bool(false);
    }
    if let Some(number) = java_double(raw) {
        return SlotValue::Number(number);
    }
    SlotValue::Str(raw.to_string())
}

fn java_double(raw: &str) -> Option<f64> {
    if raw.is_empty() {
        return None;
    }
    match raw {
        "NaN" => return Some(f64::NAN),
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if raw.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    raw.parse().ok()
}

pub(crate) fn engine_names(schema: Schema) -> [&'static str; 6] {
    let names = schema.engine_variables();
    [
        names.foot_x,
        names.foot_dx,
        names.velocity_x,
        names.velocity_y,
        names.target_x,
        names.target_y,
    ]
}

pub(crate) fn anim_slot(index: usize) -> String {
    format!("\0anim.{index}")
}

pub(crate) fn schema_word(schema: Schema, canonical: &str) -> &str {
    match (schema, canonical) {
        (Schema::Japanese, "Name") => "名前",
        (Schema::Japanese, "Type") => "種類",
        (Schema::Japanese, "Class") => "クラス",
        (Schema::Japanese, "BorderType") => "枠",
        (Schema::Japanese, "Condition") => "条件",
        (Schema::Japanese, "Duration") => "長さ",
        (Schema::Japanese, "Loop") => "繰り返し",
        (Schema::Japanese, "TargetX") => "目的地X",
        (Schema::Japanese, "TargetY") => "目的地Y",
        (Schema::Japanese, "VelocityParam") => "速度",
        (Schema::Japanese, "LookRight") => "右向き",
        (Schema::Japanese, "Ceiling") => "天井",
        (Schema::Japanese, "Wall") => "壁",
        (Schema::Japanese, "Floor") => "地面",
        _ => canonical,
    }
}

/// Fill `vars` from bindings plus engine names that are not already parameters.
pub(crate) fn load_vars(
    vars: &mut VarMap,
    schema: Schema,
    bindings: &[Binding],
    animations: &[AnimTemplate],
) -> Result<(), String> {
    for binding in bindings {
        vars.reserve(&binding.slot);
    }
    for name in engine_names(schema) {
        vars.reserve(name);
    }
    for index in 0..animations.len() {
        vars.reserve(&anim_slot(index));
    }
    for binding in bindings {
        write_slot(vars, &binding.slot, &binding.value)?;
    }
    for name in engine_names(schema) {
        if bindings.iter().any(|binding| binding.slot == name) {
            continue;
        }
        vars.define(name, VarKind::Dollar, "undefined")
            .map_err(|error| error.to_string())?;
    }
    for (index, anim) in animations.iter().enumerate() {
        if let Some(condition) = &anim.condition {
            write_slot(vars, &anim_slot(index), condition)?;
        }
    }
    Ok(())
}

fn write_slot(vars: &mut VarMap, name: &str, value: &SlotValue) -> Result<(), String> {
    match value {
        SlotValue::Number(number) => vars.bind_number(name, *number),
        SlotValue::Bool(flag) => vars.bind_bool(name, *flag),
        SlotValue::Str(text) => vars.bind_str(name, text),
        SlotValue::Script { kind, source } => {
            vars.define(name, *kind, source)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// Behavior-parameter overlay. The incoming attribute wins on a shared canonical key or slot.
pub(crate) fn merge_overlay(bindings: &[Binding], overlay: &OrderedMap<Param>) -> Vec<Binding> {
    let mut merged = bindings.to_vec();
    for (key, param) in overlay.iter() {
        push_binding(&mut merged, key, param);
    }
    merged
}

fn duration_zero_stay(name: &str) -> ActionTemplate {
    ActionTemplate {
        name: Some(name.to_string()),
        class_name: "Stay",
        runs: RunsAs::Stay,
        schema: Schema::English,
        bindings: vec![Binding {
            canonical: "Duration".to_string(),
            slot: "Duration".to_string(),
            value: SlotValue::Number(0.0),
        }],
        animations: Vec::new(),
        children: Vec::new(),
    }
}

fn class_name(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Move => "Move",
        ActionKind::Stay => "Stay",
        ActionKind::Animate => "Animate",
        ActionKind::Sequence => "Sequence",
        ActionKind::Select => "Select",
        ActionKind::Builtin(builtin) => builtin.java_name(),
        ActionKind::FastFollow(follow) => follow.java_name(),
    }
}

fn runs_as(kind: ActionKind) -> RunsAs {
    match kind {
        ActionKind::Move => RunsAs::Move,
        ActionKind::Animate => RunsAs::Animate,
        ActionKind::Sequence => RunsAs::Sequence,
        ActionKind::Select => RunsAs::Select,
        ActionKind::Stay | ActionKind::FastFollow(_) => RunsAs::Stay,
        ActionKind::Builtin(builtin) => match builtin {
            Builtin::Fall => RunsAs::Fall,
            Builtin::Jump => RunsAs::Jump,
            Builtin::Dragged => RunsAs::Dragged,
            Builtin::Regist => RunsAs::Regist,
            Builtin::Breed => RunsAs::Breed,
            Builtin::Look => RunsAs::Look,
            Builtin::Offset => RunsAs::Offset,
            Builtin::WalkWithIe => RunsAs::WalkWithIe,
            Builtin::FallWithIe => RunsAs::FallWithIe,
            Builtin::ThrowIe => RunsAs::ThrowIe,
        },
    }
}

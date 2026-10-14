//! Behavior selection, from `Configuration.buildNextBehavior` and `UserBehavior`.
//!
//! Each candidate re-evaluates its own condition chain, so a `Math.random()`
//! in a group condition is drawn once per behavior. Duplicate candidates stay.
//! The last next-list's `Add` flag is [`crate::model::Behavior::next_additive`].
//! A behavior reference is built from the named behavior's action plus the
//! reference's attributes. The next choice still uses the named behavior's list.
//!
//! Constants are bound only on this selection map. They are not copied into
//! the action.

use std::collections::{HashMap, HashSet};

use shimeji_expr::{
    CallSite, CompileError, EvalError, EvalErrorKind, JavaRandom, Limits, ParseError, Span, Value,
    VarMap, condition_bool, decide, decide_parse,
};

use crate::action::{ActionError, RunningAction};
use crate::caps::MAX_BEHAVIOR_CHAIN;
use crate::compile::Program;
use crate::diagnostics::{Diagnostic, DiagnosticKind, Severity};
use crate::env::{Area, Environment, Point};
use crate::mascot::Mascot;
use crate::model::{Behavior, Character, OrderedMap, Param, Schema, ScriptSource};

pub(crate) struct Catalog {
    schema: Schema,
    behaviors: Vec<Template>,
    by_name: HashMap<String, usize>,
    constants: Vec<(String, String)>,
    actions: Program,
    pack: String,
    file: String,
}

struct Template {
    name: String,
    action: String,
    frequency: i32,
    toggleable: bool,
    conditions: Vec<ScriptSource>,
    params: OrderedMap<Param>,
    next_refs: Vec<NextRef>,
    next_additive: bool,
    line: u32,
}

#[derive(Clone)]
struct NextRef {
    name: String,
    frequency: i32,
    conditions: Vec<ScriptSource>,
    params: OrderedMap<Param>,
    line: u32,
}

struct Candidate {
    index: usize,
    frequency: i32,
    overlay: OrderedMap<Param>,
}

struct Gate<'a> {
    frequency: i32,
    conditions: &'a [ScriptSource],
    line: u32,
}

/// The behavior a mascot is running. `name` is the plain behavior name.
pub(crate) struct ActiveBehavior {
    pub name: String,
    pub action: RunningAction,
    pub dragging: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Ran,
    Dispose,
}

impl Catalog {
    pub(crate) fn compile(character: &Character) -> Self {
        let mut behaviors = Vec::new();
        let mut by_name = HashMap::new();
        for (name, behavior) in character.behaviors.iter() {
            let index = behaviors.len();
            by_name.insert(name.to_string(), index);
            behaviors.push(lower(behavior));
        }
        let constants = character
            .constants
            .iter()
            .map(|(name, constant)| (name.to_string(), constant.value.clone()))
            .collect();
        Self {
            schema: character.behavior_schema,
            behaviors,
            by_name,
            constants,
            actions: crate::compile::compile_character(character),
            pack: character.name.clone(),
            file: character.behaviors_file.clone(),
        }
    }

    pub(crate) fn action_diagnostics(&self) -> &[Diagnostic] {
        self.actions.diagnostics()
    }

    pub(crate) fn behavior_name(&self, which: Known) -> &'static str {
        self.schema.required_behaviors()[which as usize]
    }

    pub(crate) fn contains_behavior(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// One action, not a behavior. When it finishes, selection treats the name
    /// as unknown and uses the normal candidate list.
    pub(crate) fn build_action_once(
        &self,
        name: &str,
        diags: &mut Vec<Diagnostic>,
    ) -> Result<ActiveBehavior, ()> {
        match self.actions.instantiate(name) {
            Ok(action) => Ok(ActiveBehavior {
                name: name.to_string(),
                action,
                dragging: false,
            }),
            Err(error) => {
                self.warn(diags, None, format!("no action \"{name}\": {error}"));
                Err(())
            }
        }
    }

    /// `buildNextBehavior`. Teleports when the total weight is 0.
    pub(crate) fn build_next(
        &self,
        previous: Option<&str>,
        mascot: &mut Mascot,
        world: &Environment,
        rng: &mut JavaRandom,
        disabled: &HashSet<String>,
        diags: &mut Vec<Diagnostic>,
    ) -> Result<ActiveBehavior, ()> {
        let candidates = self.candidates(previous, mascot, rng, disabled, diags);
        let total: i64 = candidates
            .iter()
            .map(|item| i64::from(item.frequency))
            .sum();
        if total > 0 {
            let mut random = rng.next_double() * total as f64;
            for candidate in &candidates {
                random -= f64::from(candidate.frequency);
                if random < 0.0 {
                    return self.instantiate(candidate.index, &candidate.overlay, diags);
                }
            }
        }
        self.teleport(mascot, world, rng);
        self.build_known(Known::Fall, diags)
    }

    /// `buildBehavior(name)`, no enabled check and no draw.
    pub(crate) fn build_known(
        &self,
        which: Known,
        diags: &mut Vec<Diagnostic>,
    ) -> Result<ActiveBehavior, ()> {
        let name = self.behavior_name(which);
        self.build_named(name, diags)
    }

    pub(crate) fn build_named(
        &self,
        name: &str,
        diags: &mut Vec<Diagnostic>,
    ) -> Result<ActiveBehavior, ()> {
        let Some(&index) = self.by_name.get(name) else {
            self.warn(diags, None, format!("no behavior \"{name}\""));
            return Err(());
        };
        self.instantiate(index, &self.behaviors[index].params, diags)
    }

    /// `buildBehavior(name, mascot)`. A disabled behavior teleports and falls.
    pub(crate) fn build_checked(
        &self,
        name: &str,
        mascot: &mut Mascot,
        world: &Environment,
        rng: &mut JavaRandom,
        disabled: &HashSet<String>,
        diags: &mut Vec<Diagnostic>,
    ) -> Result<ActiveBehavior, ()> {
        if self.enabled(name, disabled) {
            self.build_named(name, diags)
        } else if self.by_name.contains_key(name) {
            self.teleport(mascot, world, rng);
            self.build_known(Known::Fall, diags)
        } else {
            self.warn(diags, None, format!("no behavior \"{name}\""));
            Err(())
        }
    }

    /// Frequencies `buildNextBehavior` would weight, in candidate order.
    /// Evaluates conditions, so it draws when a condition draws.
    pub(crate) fn candidate_weights(
        &self,
        previous: Option<&str>,
        mascot: &Mascot,
        rng: &mut JavaRandom,
        disabled: &HashSet<String>,
        diags: &mut Vec<Diagnostic>,
    ) -> Vec<(String, i32)> {
        self.candidates(previous, mascot, rng, disabled, diags)
            .into_iter()
            .map(|candidate| {
                (
                    self.behaviors[candidate.index].name.clone(),
                    candidate.frequency,
                )
            })
            .collect()
    }

    /// `UserBehavior.init`, including the immediate next behavior when
    /// `hasNext` is already false.
    pub(crate) fn enter(
        &self,
        active: &mut ActiveBehavior,
        mascot: &mut Mascot,
        world: &Environment,
        rng: &mut JavaRandom,
        disabled: &HashSet<String>,
        diags: &mut Vec<Diagnostic>,
    ) -> Step {
        for _ in 0..MAX_BEHAVIOR_CHAIN {
            if let Err(error) = active.action.init(mascot, rng) {
                return self.on_action(error, diags);
            }
            match self.poll(active, mascot, rng, diags) {
                Poll::Yes => return Step::Ran,
                Poll::No => {
                    match self.build_next(Some(&active.name), mascot, world, rng, disabled, diags) {
                        Ok(next) => *active = next,
                        Err(()) => return Step::Dispose,
                    }
                }
                Poll::Dispose => return Step::Dispose,
            }
        }
        self.warn(
            diags,
            Some(active.action_line()),
            "behavior chain exceeded the init limit".to_string(),
        );
        Step::Dispose
    }

    /// `UserBehavior.next`. Hotspots are not scanned.
    pub(crate) fn next(
        &self,
        active: &mut ActiveBehavior,
        mascot: &mut Mascot,
        world: &Environment,
        rng: &mut JavaRandom,
        disabled: &HashSet<String>,
        diags: &mut Vec<Diagnostic>,
    ) -> Step {
        match self.poll(active, mascot, rng, diags) {
            Poll::Yes => {
                if active.action.class_name() == "Dragged" {
                    active.dragging = true;
                }
                match active.action.next(mascot, rng) {
                    Ok(()) => {}
                    Err(ActionError::LostGround) => {
                        active.dragging = false;
                        mascot.dragging = false;
                        return self.switch_fall(active, mascot, world, rng, disabled, diags);
                    }
                    Err(error) => return self.on_action(error, diags),
                }
            }
            Poll::No => {}
            Poll::Dispose => return Step::Dispose,
        }

        // Apply the opt-in throw boundary before off-screen recycling and before
        // polling the next action, so reaching a wall can select its grab action.
        if mascot.keep_on_screen && active.name == self.behavior_name(Known::Thrown) {
            retain_throw(mascot, world);
        }

        match self.poll(active, mascot, rng, diags) {
            Poll::Yes => {
                if off_screen(mascot, world.screen()) {
                    self.teleport(mascot, world, rng);
                    return self.switch_fall(active, mascot, world, rng, disabled, diags);
                }
                Step::Ran
            }
            Poll::No => {
                match self.build_next(Some(&active.name), mascot, world, rng, disabled, diags) {
                    Ok(next) => {
                        *active = next;
                        self.enter(active, mascot, world, rng, disabled, diags)
                    }
                    Err(()) => Step::Dispose,
                }
            }
            Poll::Dispose => Step::Dispose,
        }
    }

    fn switch_fall(
        &self,
        active: &mut ActiveBehavior,
        mascot: &mut Mascot,
        world: &Environment,
        rng: &mut JavaRandom,
        disabled: &HashSet<String>,
        diags: &mut Vec<Diagnostic>,
    ) -> Step {
        match self.build_known(Known::Fall, diags) {
            Ok(next) => {
                *active = next;
                self.enter(active, mascot, world, rng, disabled, diags)
            }
            Err(()) => Step::Dispose,
        }
    }

    fn candidates(
        &self,
        previous: Option<&str>,
        mascot: &Mascot,
        rng: &mut JavaRandom,
        disabled: &HashSet<String>,
        diags: &mut Vec<Diagnostic>,
    ) -> Vec<Candidate> {
        let mut scope = self.selection_map();
        let prev = previous.and_then(|name| self.by_name.get(name).copied());
        let mut candidates = Vec::new();
        let additive = match prev {
            Some(index) => self.behaviors[index].next_additive,
            None => true,
        };
        if additive {
            for index in 0..self.behaviors.len() {
                let template = &self.behaviors[index];
                if !self.effective(
                    Gate {
                        frequency: template.frequency,
                        conditions: &template.conditions,
                        line: template.line,
                    },
                    mascot,
                    &mut scope,
                    rng,
                    diags,
                ) {
                    continue;
                }
                if !self.enabled(&template.name, disabled) {
                    continue;
                }
                candidates.push(Candidate {
                    index,
                    frequency: template.frequency,
                    overlay: template.params.clone(),
                });
            }
        }
        if let Some(index) = prev {
            let refs = self.behaviors[index].next_refs.clone();
            for reference in refs {
                if !self.effective(
                    Gate {
                        frequency: reference.frequency,
                        conditions: &reference.conditions,
                        line: reference.line,
                    },
                    mascot,
                    &mut scope,
                    rng,
                    diags,
                ) {
                    continue;
                }
                let Some(&target) = self.by_name.get(&reference.name) else {
                    continue;
                };
                if !self.enabled(&reference.name, disabled) {
                    continue;
                }
                let mut overlay = self.behaviors[target].params.clone();
                for (key, param) in reference.params.iter() {
                    overlay.insert(key, param.clone());
                }
                candidates.push(Candidate {
                    index: target,
                    frequency: reference.frequency,
                    overlay,
                });
            }
        }
        candidates
    }

    fn effective(
        &self,
        gate: Gate<'_>,
        mascot: &Mascot,
        scope: &mut VarMap,
        rng: &mut JavaRandom,
        diags: &mut Vec<Diagnostic>,
    ) -> bool {
        if gate.frequency == 0 {
            return false;
        }
        for condition in gate.conditions {
            if !self.condition_holds(condition, gate.line, mascot, scope, rng, diags) {
                return false;
            }
        }
        true
    }

    fn condition_holds(
        &self,
        condition: &ScriptSource,
        line: u32,
        mascot: &Mascot,
        scope: &mut VarMap,
        rng: &mut JavaRandom,
        diags: &mut Vec<Diagnostic>,
    ) -> bool {
        if !mascot.allow_condition() {
            return false;
        }
        match eval_condition(condition, scope, mascot, rng) {
            Ok(value) => value,
            Err(failure) => {
                let (message, span_line) = match failure {
                    CondFail::Parse(error) => {
                        let decision = decide_parse(CallSite::BehaviorCondition, &error);
                        (decision.diagnostic.message, line)
                    }
                    CondFail::Eval(error) => {
                        let decision = decide(CallSite::BehaviorCondition, &error);
                        (
                            format!(
                                "{} at {}..{}",
                                decision.diagnostic.message, error.span.start, error.span.end
                            ),
                            line,
                        )
                    }
                };
                self.warn(diags, Some(span_line), message);
                false
            }
        }
    }

    fn enabled(&self, name: &str, disabled: &HashSet<String>) -> bool {
        let Some(&index) = self.by_name.get(name) else {
            return false;
        };
        let template = &self.behaviors[index];
        !(template.toggleable && disabled.contains(name))
    }

    fn instantiate(
        &self,
        index: usize,
        overlay: &OrderedMap<Param>,
        diags: &mut Vec<Diagnostic>,
    ) -> Result<ActiveBehavior, ()> {
        let template = &self.behaviors[index];
        match self.actions.instantiate_with(&template.action, overlay) {
            Ok(action) => Ok(ActiveBehavior {
                name: template.name.clone(),
                action,
                dragging: false,
            }),
            Err(error) => {
                self.warn(
                    diags,
                    Some(template.line),
                    format!("could not build \"{}\": {error}", template.action),
                );
                Err(())
            }
        }
    }

    fn poll(
        &self,
        active: &mut ActiveBehavior,
        mascot: &mut Mascot,
        rng: &mut JavaRandom,
        diags: &mut Vec<Diagnostic>,
    ) -> Poll {
        match active.action.has_next(mascot, rng) {
            Ok(value) => {
                if value {
                    Poll::Yes
                } else {
                    Poll::No
                }
            }
            Err(error) => self.on_poll(error, diags),
        }
    }

    fn on_poll(&self, error: ActionError, diags: &mut Vec<Diagnostic>) -> Poll {
        match &error {
            ActionError::Eval(inner) if inner.kind == EvalErrorKind::NonBoolean => {
                self.note_non_boolean(inner, diags);
                Poll::No
            }
            _ => {
                self.note_dispose(&error, diags);
                Poll::Dispose
            }
        }
    }

    fn on_action(&self, error: ActionError, diags: &mut Vec<Diagnostic>) -> Step {
        match &error {
            ActionError::Eval(inner) if inner.kind == EvalErrorKind::NonBoolean => {
                self.note_non_boolean(inner, diags);
                Step::Ran
            }
            ActionError::LostGround => Step::Ran,
            _ => {
                self.note_dispose(&error, diags);
                Step::Dispose
            }
        }
    }

    fn note_non_boolean(&self, error: &EvalError, diags: &mut Vec<Diagnostic>) {
        let decision = decide(CallSite::ActionCondition, error);
        self.warn(
            diags,
            None,
            format!(
                "{} at {}..{}",
                decision.diagnostic.message, error.span.start, error.span.end
            ),
        );
    }

    pub(crate) fn report(&self, error: &ActionError, diags: &mut Vec<Diagnostic>) {
        self.note_dispose(error, diags);
    }

    fn note_dispose(&self, error: &ActionError, diags: &mut Vec<Diagnostic>) {
        let message = match error {
            ActionError::Eval(inner) => {
                let decision = decide(CallSite::ActionParameter, inner);
                format!(
                    "{} at {}..{}",
                    decision.diagnostic.message, inner.span.start, inner.span.end
                )
            }
            other => other.to_string(),
        };
        diags.push(Diagnostic::new(
            Severity::Error,
            DiagnosticKind::Expression,
            &self.pack,
            &self.file,
            None,
            message,
        ));
    }

    fn selection_map(&self) -> VarMap {
        let mut scope = VarMap::new();
        for (name, value) in &self.constants {
            if name != "mascot" {
                scope.bind_str(name, value);
            }
        }
        scope
    }

    fn teleport(&self, mascot: &mut Mascot, world: &Environment, rng: &mut JavaRandom) {
        let area = if world.multiscreen() {
            *world.screen()
        } else {
            mascot.env.work_area()
        };
        place_above(mascot, area, rng);
    }

    fn warn(&self, diags: &mut Vec<Diagnostic>, line: Option<u32>, message: String) {
        diags.push(Diagnostic::new(
            Severity::Warning,
            DiagnosticKind::Expression,
            &self.pack,
            &self.file,
            line,
            message,
        ));
    }
}

/// Keep the thrown anchor at side walls and the floor, with body headroom at
/// the ceiling. Canvas padding below the feet must not stop a fall short of
/// the floor. The renderer clamps opaque overhang separately.
fn retain_throw(mascot: &mut Mascot, world: &Environment) {
    let area = if world.multiscreen() {
        *world.screen()
    } else {
        mascot.env.work_area()
    };
    if area.width() <= 0 || area.height() <= 0 {
        return;
    }
    let image_y = mascot.image_anchor.map_or(0, |point| point.y);
    let bottom = area.bottom;
    let top = area.top.saturating_add(image_y.max(0)).min(bottom);
    mascot.anchor.x = mascot.anchor.x.clamp(area.left, area.right);
    mascot.anchor.y = mascot.anchor.y.clamp(top, bottom);
}

impl ActiveBehavior {
    fn action_line(&self) -> u32 {
        0
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Known {
    #[allow(dead_code)]
    ChaseMouse = 0,
    Fall = 1,
    Dragged = 2,
    Thrown = 3,
}

enum Poll {
    Yes,
    No,
    Dispose,
}

enum CondFail {
    Parse(ParseError),
    Eval(EvalError),
}

fn lower(behavior: &Behavior) -> Template {
    let next_refs = behavior
        .next_refs()
        .map(|reference| NextRef {
            name: reference.name.clone(),
            frequency: reference.frequency,
            conditions: reference.conditions.clone(),
            params: reference.params.clone(),
            line: reference.line,
        })
        .collect();
    Template {
        name: behavior.name.clone(),
        action: behavior.action.clone(),
        frequency: behavior.frequency,
        toggleable: behavior.toggleable,
        conditions: behavior.conditions.clone(),
        params: behavior.params.clone(),
        next_refs,
        next_additive: behavior.next_additive,
        line: behavior.line,
    }
}

fn eval_condition(
    condition: &ScriptSource,
    scope: &mut VarMap,
    mascot: &Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, CondFail> {
    if let Some(inner) = script_body(&condition.raw) {
        let code = scope.compile(inner).map_err(|error| match error {
            CompileError::Parse(error) => CondFail::Parse(error),
        })?;
        let value = scope
            .eval(&code, mascot, rng, Limits::default())
            .map_err(CondFail::Eval)?;
        return condition_bool(value, code.span()).map_err(CondFail::Eval);
    }
    if let Some(value) = constant_bool(&condition.raw) {
        return Ok(value);
    }
    // Unwrapped text is `Variable.parse`'s constant. A number fails the Boolean
    // cast. Anything else is a non-boolean too; the reference aborts the tick,
    // and this engine records the failure and treats the condition as false.
    let value = match condition.raw.parse::<f64>() {
        Ok(number) => Value::Number(number),
        Err(_) => Value::Undefined,
    };
    condition_bool(value, Span::new(0, condition.raw.len())).map_err(CondFail::Eval)
}

fn script_body(raw: &str) -> Option<&str> {
    let wrapped =
        (raw.starts_with("${") || raw.starts_with("#{")) && raw.ends_with('}') && raw.len() >= 3;
    if wrapped {
        Some(&raw[2..raw.len() - 1])
    } else {
        None
    }
}

fn constant_bool(raw: &str) -> Option<bool> {
    match raw {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn place_above(mascot: &mut Mascot, area: Area, rng: &mut JavaRandom) {
    let span = area.width().wrapping_sub(2);
    let x = (rng.next_double() * f64::from(span)) as i32 + area.left + 1;
    let y = area.top.wrapping_sub(256);
    mascot.anchor = Point::new(x, y);
}

/// `Mascot.getBounds` and the off-screen test in `UserBehavior.next`.
/// Height is part of the rectangle and is not part of this test.
fn off_screen(mascot: &Mascot, screen: &Area) -> bool {
    let mut x = mascot.anchor.x;
    let mut y = mascot.anchor.y;
    if let Some(image_anchor) = mascot.image_anchor {
        x = x.wrapping_sub(image_anchor.x);
        y = y.wrapping_sub(image_anchor.y);
    }
    let width = mascot.image_width;
    x.wrapping_add(width) <= screen.left || screen.right <= x || screen.bottom <= y
}

#[cfg(test)]
mod tests {
    use super::off_screen;
    use crate::env::{Area, Point};
    use crate::mascot::Mascot;

    /// 128×128 sprite, anchor (64, 128), on a screen whose origin is (0, 0).
    fn sprite_at(x: i32) -> Mascot {
        let mut mascot = Mascot::new();
        mascot.anchor = Point::new(x, 400);
        mascot.image_anchor = Some(Point::new(64, 128));
        mascot.image_width = 128;
        mascot.image_height = 128;
        mascot
    }

    fn screen() -> Area {
        let mut area = Area::default();
        area.set(0, 0, 1280, 800);
        area
    }

    #[test]
    fn sprite_128_is_on_screen_at_anchor_zero_and_off_at_minus_64() {
        let screen = screen();
        assert!(
            !off_screen(&sprite_at(0), &screen),
            "anchor x 0 still overlaps the screen"
        );
        assert!(
            off_screen(&sprite_at(-64), &screen),
            "anchor x -64 puts the sprite fully past the left edge"
        );
    }
}

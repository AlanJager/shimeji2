//! Action lifecycle. `hasNext` is never memoized. `#{}` clears only in `next`.
//!
//! Fast-follow classes run as [`RunsAs::Stay`]. The ten built-ins have their
//! own arms. Children of those classes are not instantiated; only `Sequence`
//! and `Select` run their children. There is no `initFrame` in the vendored tree.

mod animate;
mod bordered;
mod builtins;
mod move_;
mod select;
mod sequence;
mod stay;

use shimeji_expr::{EvalError, JavaRandom, Limits, Span, Value, VarMap, condition_bool};

use crate::compile::{
    ActionTemplate, AnimTemplate, Binding, SlotValue, load_vars, merge_overlay, schema_word,
};
use crate::env::LiveBorder;
use crate::mascot::Mascot;
use crate::model::{OrderedMap, Param, Pose, Schema};

/// How a template actually ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunsAs {
    Move,
    Stay,
    Animate,
    Sequence,
    Select,
    Fall,
    Jump,
    Dragged,
    Regist,
    Breed,
    Look,
    Offset,
    WalkWithIe,
    FallWithIe,
    ThrowIe,
}

/// Per-action physics the Java classes keep in fields. Reset in `init`.
#[derive(Debug, Clone, Copy)]
pub(super) enum Extra {
    None,
    Fall {
        vx: f64,
        vy: f64,
        mod_x: f64,
        mod_y: f64,
        scaling: f64,
        deploy_at_apex: bool,
        deployed_at: Option<i32>,
    },
    Jump {
        scaling: f64,
    },
    Dragged {
        foot_x: f64,
        foot_dx: f64,
        time_to_resist: i32,
        scaling: f64,
    },
    Regist {
        scaling: f64,
    },
    Breed {
        scaling: f64,
    },
    ThrowIe {
        scaling: f64,
        window_id: u64,
    },
}

/// One instantiated action, including its own variable map and children.
#[derive(Debug)]
pub struct RunningAction {
    class_name: &'static str,
    runs: RunsAs,
    schema: Schema,
    bindings: Vec<Binding>,
    vars: VarMap,
    pub(super) animations: Vec<AnimTemplate>,
    pub(super) extra: Extra,
    children: Vec<RunningAction>,
    index: usize,
    border: Option<LiveBorder>,
    /// `None` until [`Self::init`]. `getTime()` is `mascot.time - start`.
    start_time: Option<i32>,
    turning: bool,
    saw_turn: Option<bool>,
    /// Plain `Name` constant, when the attribute is not a script.
    constant_name: Option<String>,
}

/// Failure of `init`, `has_next`, or `next`. Part B decides whether it disposes the mascot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    LostGround,
    Eval(EvalError),
    BadCast {
        expected: &'static str,
        got: &'static str,
    },
    NoPose,
    UnknownAction(String),
    Compile(String),
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LostGround => write!(formatter, "lost ground"),
            Self::Eval(error) => write!(formatter, "{error}"),
            Self::BadCast { expected, got } => {
                write!(formatter, "expected {expected}, got {got}")
            }
            Self::NoPose => write!(formatter, "no pose"),
            Self::UnknownAction(name) => write!(formatter, "unknown action \"{name}\""),
            Self::Compile(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for ActionError {}

impl RunningAction {
    pub(crate) fn from_template(
        template: &ActionTemplate,
        overlay: &OrderedMap<Param>,
    ) -> Result<Self, ActionError> {
        let bindings = merge_overlay(&template.bindings, overlay);
        let mut vars = VarMap::new();
        if bindings
            .iter()
            .any(|binding| binding.canonical == "DeployAtApex")
        {
            // Expressions are compiled before Fall.init supplies the deployment clock.
            vars.bind_number("DeployTime", -1.0);
        }
        load_vars(&mut vars, template.schema, &bindings, &template.animations)
            .map_err(ActionError::Compile)?;
        let constant_name = constant_name(template.schema, &bindings);
        let children = template
            .children
            .iter()
            .map(|child| Self::from_template(child, &OrderedMap::new()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            class_name: template.class_name,
            runs: template.runs,
            schema: template.schema,
            bindings,
            vars,
            animations: template.animations.clone(),
            extra: Extra::None,
            children,
            index: 0,
            border: None,
            start_time: None,
            turning: false,
            saw_turn: None,
            constant_name,
        })
    }

    /// Java simple class name (`Fall`, `Move`, `Sequence`).
    pub fn class_name(&self) -> &'static str {
        self.class_name
    }

    /// Fall, drag, and a window carry (including the active child of a sequence).
    pub(crate) fn blocks_pose_interject(&self) -> bool {
        if blocks_pose_class(self.class_name) {
            return true;
        }
        if matches!(self.runs, RunsAs::Sequence | RunsAs::Select) {
            if let Some(child) = self.children.get(self.index) {
                return child.blocks_pose_interject();
            }
        }
        false
    }

    /// Innermost action's constant `Name`, or its class name when `Name` is absent or scripted.
    pub fn active_name(&self) -> &str {
        if matches!(self.runs, RunsAs::Sequence | RunsAs::Select) {
            if let Some(child) = self.children.get(self.index) {
                return child.active_name();
            }
        }
        self.constant_name.as_deref().unwrap_or(self.class_name)
    }

    /// `Action.init`. Does not increment [`Mascot::time`].
    ///
    /// `Look` and `Offset` apply during `init`, which is why this takes
    /// `&mut Mascot`. A sequence seek calls it again for the next child.
    pub fn init(&mut self, mascot: &mut Mascot, rng: &mut JavaRandom) -> Result<(), ActionError> {
        self.start_time = Some(mascot.time);
        self.turning = false;
        self.extra = Extra::None;
        self.vars.init();
        if self.grabs_border() {
            bordered::grab(self, mascot, rng)?;
        }
        match self.runs {
            RunsAs::Fall | RunsAs::FallWithIe => builtins::fall::init(self, mascot, rng)?,
            RunsAs::Jump => builtins::jump::init(self, mascot)?,
            RunsAs::Dragged => builtins::dragged::init(self, mascot, rng)?,
            RunsAs::Regist => builtins::regist::init(self, mascot)?,
            RunsAs::Breed => builtins::breed::init(self, mascot, rng)?,
            RunsAs::Look => builtins::look::init(self, mascot, rng)?,
            RunsAs::Offset => builtins::offset::init(self, mascot, rng)?,
            RunsAs::ThrowIe => builtins::throw_ie::init(self, mascot)?,
            RunsAs::Sequence | RunsAs::Select => {
                if self.base_has_next(mascot, rng)? {
                    self.set_current(0, mascot, rng)?;
                    self.seek(mascot, rng)?;
                }
            }
            RunsAs::Move | RunsAs::Stay | RunsAs::Animate | RunsAs::WalkWithIe => {}
        }
        Ok(())
    }

    /// `Action.hasNext`. Callers must not cache the bool.
    pub fn has_next(
        &mut self,
        mascot: &mut Mascot,
        rng: &mut JavaRandom,
    ) -> Result<bool, ActionError> {
        match self.runs {
            RunsAs::Move => move_::has_next(self, mascot, rng),
            RunsAs::Animate | RunsAs::Breed => animate::has_next(self, mascot, rng),
            RunsAs::Sequence => sequence::has_next(self, mascot, rng),
            RunsAs::Select => select::has_next(self, mascot, rng),
            RunsAs::Stay => self.base_has_next(mascot, rng),
            RunsAs::Fall => builtins::fall::has_next(self, mascot, rng),
            RunsAs::Jump => builtins::jump::has_next(self, mascot, rng),
            RunsAs::Dragged => builtins::dragged::has_next(self, mascot, rng),
            RunsAs::Regist => builtins::regist::has_next(self, mascot, rng),
            RunsAs::Look | RunsAs::Offset => Ok(false),
            RunsAs::WalkWithIe => builtins::walk_with_ie::has_next(self, mascot, rng),
            RunsAs::FallWithIe => builtins::fall_with_ie::has_next(self, mascot, rng),
            RunsAs::ThrowIe => builtins::throw_ie::has_next(self, mascot, rng),
        }
    }

    /// `Action.next`: clear `#{}`, affordance, animation conditions, then `tick`.
    pub fn next(&mut self, mascot: &mut Mascot, rng: &mut JavaRandom) -> Result<(), ActionError> {
        if self.start_time.is_none() {
            return Ok(());
        }
        self.vars.next();
        self.apply_affordance(mascot, rng)?;
        // Dragged and Regist override `refreshHotspots` and do not call
        // `getAnimation` there, so the pose condition sees values `tick` writes.
        if !matches!(self.runs, RunsAs::Dragged | RunsAs::Regist) {
            self.refresh(mascot, rng);
        }
        match self.runs {
            RunsAs::Move => move_::tick(self, mascot, rng),
            RunsAs::WalkWithIe => builtins::walk_with_ie::tick(self, mascot, rng),
            RunsAs::Stay | RunsAs::Animate => stay::tick(self, mascot, rng),
            RunsAs::Breed => builtins::breed::tick(self, mascot, rng),
            RunsAs::Sequence | RunsAs::Select => self.complex_tick(mascot, rng),
            RunsAs::Fall => builtins::fall::tick(self, mascot, rng),
            RunsAs::FallWithIe => builtins::fall_with_ie::tick(self, mascot, rng),
            RunsAs::Jump => builtins::jump::tick(self, mascot, rng),
            RunsAs::Dragged => builtins::dragged::tick(self, mascot, rng),
            RunsAs::Regist => builtins::regist::tick(self, mascot, rng),
            RunsAs::Look | RunsAs::Offset => Ok(()),
            RunsAs::ThrowIe => builtins::throw_ie::tick(self, mascot, rng),
        }
    }

    /// Read one slot. Part A tests use this for `${}` / `#{}` caching.
    pub fn read(
        &mut self,
        name: &str,
        mascot: &Mascot,
        rng: &mut JavaRandom,
    ) -> Result<Value, ActionError> {
        self.vars
            .read(name, mascot, rng, Limits::default())
            .map_err(ActionError::Eval)
    }

    /// Engine write (`footX`, `VelocityX`, …). Part C calls this from the built-ins.
    pub fn put_number(&mut self, name: &str, value: f64) {
        self.vars.bind_number(name, value);
    }

    /// `ActionBase.isDraggable`. Absent means true.
    pub fn draggable(
        &mut self,
        mascot: &Mascot,
        rng: &mut JavaRandom,
    ) -> Result<bool, ActionError> {
        self.eval_bool("Draggable", true, mascot, rng)
    }

    fn grabs_border(&self) -> bool {
        matches!(
            self.runs,
            RunsAs::Move
                | RunsAs::Stay
                | RunsAs::Animate
                | RunsAs::Breed
                | RunsAs::WalkWithIe
                | RunsAs::ThrowIe
        )
    }

    fn set_current(
        &mut self,
        index: usize,
        mascot: &mut Mascot,
        rng: &mut JavaRandom,
    ) -> Result<(), ActionError> {
        if self.runs == RunsAs::Sequence {
            sequence::set_current(self, index, mascot, rng)
        } else {
            self.complex_set_current(index, mascot, rng)
        }
    }

    fn complex_set_current(
        &mut self,
        index: usize,
        mascot: &mut Mascot,
        rng: &mut JavaRandom,
    ) -> Result<(), ActionError> {
        self.index = index;
        if !self.base_has_next(mascot, rng)? {
            return Ok(());
        }
        if self.index < self.children.len() {
            self.children[self.index].init(mascot, rng)?;
        }
        Ok(())
    }

    fn seek(&mut self, mascot: &mut Mascot, rng: &mut JavaRandom) -> Result<(), ActionError> {
        if !self.base_has_next(mascot, rng)? {
            return Ok(());
        }
        // A looping sequence whose children can never run would spin in Java.
        // One extra step past the child count stops the seek instead.
        let limit = self.children.len().saturating_add(1);
        let mut steps = 0;
        while self.index < self.children.len() {
            if self.children[self.index].has_next(mascot, rng)? {
                break;
            }
            steps += 1;
            if steps > limit {
                break;
            }
            let next = self.index + 1;
            self.set_current(next, mascot, rng)?;
        }
        Ok(())
    }

    fn complex_has_next(
        &mut self,
        mascot: &mut Mascot,
        rng: &mut JavaRandom,
    ) -> Result<bool, ActionError> {
        if !self.base_has_next(mascot, rng)? {
            return Ok(false);
        }
        if self.index >= self.children.len() {
            return Ok(false);
        }
        self.children[self.index].has_next(mascot, rng)
    }

    fn complex_tick(
        &mut self,
        mascot: &mut Mascot,
        rng: &mut JavaRandom,
    ) -> Result<(), ActionError> {
        if self.index >= self.children.len() {
            return Ok(());
        }
        if self.children[self.index].has_next(mascot, rng)? {
            self.children[self.index].next(mascot, rng)?;
        }
        Ok(())
    }

    pub(super) fn animation_count(&self) -> usize {
        self.animations.len()
    }

    pub(super) fn velocity_names(&self) -> (&'static str, &'static str) {
        let names = self.schema.engine_variables();
        (names.velocity_x, names.velocity_y)
    }

    pub(super) fn foot_names(&self) -> (&'static str, &'static str) {
        let names = self.schema.engine_variables();
        (names.foot_x, names.foot_dx)
    }

    pub(super) fn value_text(&self, value: Value) -> Option<&str> {
        self.vars.text(value)
    }

    /// Pose for this frame. `NoPose` when every condition is false.
    pub(super) fn apply_current(
        &mut self,
        mascot: &mut Mascot,
        rng: &mut JavaRandom,
    ) -> Result<(), ActionError> {
        let Some(index) = self.pick(mascot, rng, None)? else {
            return Err(ActionError::NoPose);
        };
        let time = self.time_of(mascot);
        let pose = pose_at(&self.animations[index], time)?;
        apply_pose(mascot, pose);
        Ok(())
    }

    pub(super) fn current_duration(
        &mut self,
        mascot: &Mascot,
        rng: &mut JavaRandom,
    ) -> Result<i32, ActionError> {
        let Some(index) = self.pick(mascot, rng, None)? else {
            return Err(ActionError::NoPose);
        };
        Ok(self.animations[index].duration)
    }

    pub(super) fn set_time(&mut self, mascot: &Mascot, time: i32) {
        self.start_time = Some(mascot.time.wrapping_sub(time));
    }

    pub(super) fn base_has_next(
        &mut self,
        mascot: &Mascot,
        rng: &mut JavaRandom,
    ) -> Result<bool, ActionError> {
        if self.start_time.is_none() {
            return Ok(false);
        }
        let duration = self.eval_int("Duration", i32::MAX, mascot, rng)?;
        if self.time_of(mascot) >= duration {
            return Ok(false);
        }
        self.eval_bool("Condition", true, mascot, rng)
    }

    pub(super) fn time_of(&self, mascot: &Mascot) -> i32 {
        match self.start_time {
            Some(start) => mascot.time.wrapping_sub(start),
            None => 0,
        }
    }

    fn apply_affordance(
        &mut self,
        mascot: &mut Mascot,
        rng: &mut JavaRandom,
    ) -> Result<(), ActionError> {
        mascot.affordances.clear();
        let Some(value) = self.eval_raw("Affordance", mascot, rng)? else {
            mascot.affordances.clear();
            return Ok(());
        };
        let raw = match value {
            Value::Str(_) => self.vars.text(value).unwrap_or(""),
            other => {
                return Err(ActionError::BadCast {
                    expected: "string",
                    got: other.type_name(),
                });
            }
        };
        let trimmed = java_trim(raw);
        if trimmed.is_empty() {
            mascot.affordances.clear();
            return Ok(());
        }
        if mascot.affordances.len() == 1 && mascot.affordances[0] == trimmed {
            return Ok(());
        }
        let owned = trimmed.to_string();
        mascot.affordances.clear();
        mascot.affordances.push(owned);
        Ok(())
    }

    /// `refreshHotspots` swallows a condition error. `tick` evaluates again.
    fn refresh(&mut self, mascot: &Mascot, rng: &mut JavaRandom) {
        let turn = match self.runs {
            RunsAs::Move => Some(self.turning),
            _ => None,
        };
        let _ = self.pick(mascot, rng, turn);
    }

    pub(super) fn pick(
        &mut self,
        mascot: &Mascot,
        rng: &mut JavaRandom,
        turn: Option<bool>,
    ) -> Result<Option<usize>, ActionError> {
        for index in 0..self.animations.len() {
            if let Some(turning) = turn {
                if turning != self.animations[index].is_turn {
                    continue;
                }
            }
            if self.anim_effective(index, mascot, rng)? {
                return Ok(Some(index));
            }
        }
        Ok(None)
    }

    fn anim_effective(
        &mut self,
        index: usize,
        mascot: &Mascot,
        rng: &mut JavaRandom,
    ) -> Result<bool, ActionError> {
        if self.animations[index].condition.is_none() {
            return Ok(true);
        }
        if !mascot.allow_condition() {
            return Ok(false);
        }
        let value = {
            let mut buf = [0u8; 32];
            let name = anim_slot_name(index, &mut buf);
            self.vars
                .read(name, mascot, rng, Limits::default())
                .map_err(ActionError::Eval)?
        };
        condition_bool(value, Span::new(0, 0)).map_err(ActionError::Eval)
    }

    fn has_turning(&mut self) -> bool {
        if let Some(cached) = self.saw_turn {
            return cached;
        }
        let found = self.animations.iter().any(|anim| anim.is_turn);
        self.saw_turn = Some(found);
        found
    }

    fn copy_slot(&self, canonical: &str, buf: &mut [u8]) -> Option<usize> {
        let word = schema_word(self.schema, canonical);
        let slot = self
            .bindings
            .iter()
            .find(|binding| binding.canonical == canonical || binding.slot == word)?
            .slot
            .as_str();
        let bytes = slot.as_bytes();
        if bytes.len() > buf.len() {
            return None;
        }
        buf[..bytes.len()].copy_from_slice(bytes);
        Some(bytes.len())
    }

    pub(super) fn eval_raw(
        &mut self,
        canonical: &str,
        mascot: &Mascot,
        rng: &mut JavaRandom,
    ) -> Result<Option<Value>, ActionError> {
        let mut buf = [0u8; 192];
        if let Some(len) = self.copy_slot(canonical, &mut buf) {
            let name = std::str::from_utf8(&buf[..len]).unwrap_or("");
            return self
                .vars
                .read(name, mascot, rng, Limits::default())
                .map(Some)
                .map_err(ActionError::Eval);
        }
        let word = schema_word(self.schema, canonical);
        let Some(slot) = self
            .bindings
            .iter()
            .find(|binding| binding.canonical == canonical || binding.slot == word)
            .map(|binding| binding.slot.clone())
        else {
            return Ok(None);
        };
        self.vars
            .read(&slot, mascot, rng, Limits::default())
            .map(Some)
            .map_err(ActionError::Eval)
    }

    pub(super) fn eval_int(
        &mut self,
        canonical: &str,
        default: i32,
        mascot: &Mascot,
        rng: &mut JavaRandom,
    ) -> Result<i32, ActionError> {
        let Some(value) = self.eval_raw(canonical, mascot, rng)? else {
            return Ok(default);
        };
        match value {
            Value::Number(_) => Ok(self.vars.to_java_int(value)),
            other => Err(ActionError::BadCast {
                expected: "number",
                got: other.type_name(),
            }),
        }
    }

    pub(super) fn eval_f64(
        &mut self,
        canonical: &str,
        default: f64,
        mascot: &Mascot,
        rng: &mut JavaRandom,
    ) -> Result<f64, ActionError> {
        let Some(value) = self.eval_raw(canonical, mascot, rng)? else {
            return Ok(default);
        };
        match value {
            Value::Number(number) => Ok(number),
            other => Err(ActionError::BadCast {
                expected: "number",
                got: other.type_name(),
            }),
        }
    }

    pub(super) fn eval_bool(
        &mut self,
        canonical: &str,
        default: bool,
        mascot: &Mascot,
        rng: &mut JavaRandom,
    ) -> Result<bool, ActionError> {
        if canonical == "Condition" && self.has_slot(canonical) && !mascot.allow_condition() {
            return Ok(false);
        }
        let Some(value) = self.eval_raw(canonical, mascot, rng)? else {
            return Ok(default);
        };
        condition_bool(value, Span::new(0, 0)).map_err(ActionError::Eval)
    }

    fn has_slot(&self, canonical: &str) -> bool {
        let word = schema_word(self.schema, canonical);
        self.bindings
            .iter()
            .any(|binding| binding.canonical == canonical || binding.slot == word)
    }

    fn as_string(&self, value: Value) -> Result<String, ActionError> {
        match value {
            Value::Str(_) => Ok(self.vars.text(value).unwrap_or("").to_string()),
            other => Err(ActionError::BadCast {
                expected: "string",
                got: other.type_name(),
            }),
        }
    }
}

fn blocks_pose_class(class: &str) -> bool {
    matches!(
        class,
        "Fall" | "Dragged" | "WalkWithIE" | "FallWithIE" | "ThrowIE"
    )
}

fn constant_name(schema: Schema, bindings: &[Binding]) -> Option<String> {
    let word = schema_word(schema, "Name");
    let binding = bindings
        .iter()
        .find(|binding| binding.canonical == "Name" || binding.slot == word)?;
    match &binding.value {
        SlotValue::Str(text) => Some(text.clone()),
        _ => None,
    }
}

pub(super) fn pose_at(anim: &AnimTemplate, time: i32) -> Result<&Pose, ActionError> {
    if anim.duration <= 0 || anim.poses.is_empty() {
        return Err(ActionError::NoPose);
    }
    let mut cursor = time.wrapping_rem(anim.duration);
    for pose in &anim.poses {
        cursor -= pose.duration_ticks;
        if cursor < 0 {
            return Ok(pose);
        }
    }
    Err(ActionError::NoPose)
}

pub(super) fn apply_pose(mascot: &mut Mascot, pose: &Pose) {
    let dx = if mascot.look_right {
        -pose.velocity_x
    } else {
        pose.velocity_x
    };
    mascot.anchor.x = mascot.anchor.x.wrapping_add(dx);
    mascot.anchor.y = mascot.anchor.y.wrapping_add(pose.velocity_y);
    let image = pose
        .image_source
        .as_deref()
        .or_else(|| pose.image.as_ref().map(|image| image.source.as_str()));
    mascot.assign_image(image);
    // The reference ImagePair stores a facing-specific centre, including for
    // explicit right art. `setImage(null)` retains both prior size and centre.
    if image.is_some() {
        let facing_image = if mascot.look_right {
            pose.image_right
                .as_ref()
                .or(pose.right_sibling.as_ref())
                .or(pose.image.as_ref())
        } else {
            pose.image.as_ref()
        };
        let (width, height) = facing_image
            .map(|resolved| (resolved.width, resolved.height))
            .unwrap_or((0, 0));
        mascot.image_width = width;
        mascot.image_height = height;
        let anchor_x = if mascot.look_right {
            width.wrapping_sub(pose.anchor_x)
        } else {
            pose.anchor_x
        };
        mascot.image_anchor = Some(crate::env::Point::new(anchor_x, pose.anchor_y));
    }
    if let Some(sound) = &pose.sound {
        mascot.sounds.push((sound.clone(), pose.volume.clone()));
    }
}

/// `\0anim.{index}` into `buf`, matching [`anim_slot`] without a heap string.
fn anim_slot_name(index: usize, buf: &mut [u8; 32]) -> &str {
    buf[0] = 0;
    buf[1..6].copy_from_slice(b"anim.");
    let mut n = index;
    let mut digits = [b'0'; 16];
    let mut count = 0;
    loop {
        digits[count] = b'0' + (n % 10) as u8;
        count += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for (offset, digit) in digits[..count].iter().rev().enumerate() {
        buf[6 + offset] = *digit;
    }
    std::str::from_utf8(&buf[..6 + count]).unwrap_or("")
}

/// `Math.round(double)`: `floor(x + 0.5)`, half toward +∞. `NaN` is 0.
pub(super) fn java_round(value: f64) -> i32 {
    if !value.is_finite() {
        return if value.is_nan() {
            0
        } else if value.is_sign_positive() {
            i32::MAX
        } else {
            i32::MIN
        };
    }
    let shifted = value + 0.5;
    if !shifted.is_finite() {
        return if value.is_sign_positive() {
            i32::MAX
        } else {
            i32::MIN
        };
    }
    let rounded = shifted.floor();
    if rounded >= i32::MAX as f64 {
        i32::MAX
    } else if rounded <= i32::MIN as f64 {
        i32::MIN
    } else {
        rounded as i32
    }
}

/// Java `String.trim`: drop code points at or below U+0020.
fn java_trim(text: &str) -> &str {
    text.trim_matches(|ch: char| ch <= ' ')
}

#[cfg(test)]
mod tests {
    use super::apply_pose;
    use crate::env::Point;
    use crate::mascot::Mascot;
    use crate::model::{Pose, ResolvedImage};

    fn pose(image: Option<ResolvedImage>, anchor_x: i32, anchor_y: i32) -> Pose {
        Pose {
            image_source: image.as_ref().map(|image| image.source.clone()),
            image,
            image_right_source: None,
            image_right: None,
            right_sibling: None,
            anchor_x,
            anchor_y,
            velocity_x: 0,
            velocity_y: 0,
            duration_ticks: 1,
            sound: None,
            volume: None,
            line: 1,
        }
    }

    #[test]
    fn apply_pose_stores_pixel_size_and_a_blank_pose_keeps_it() {
        let image = ResolvedImage {
            source: "/shime.png".to_string(),
            relative: "shime.png".to_string(),
            width: 128,
            height: 128,
        };
        let mut mascot = Mascot::new();
        apply_pose(&mut mascot, &pose(Some(image), 64, 128));
        assert_eq!(mascot.image_anchor, Some(Point::new(64, 128)));
        assert_eq!((mascot.image_width, mascot.image_height), (128, 128));

        apply_pose(&mut mascot, &pose(None, 0, 0));
        assert_eq!((mascot.image_width, mascot.image_height), (128, 128));
        assert_eq!(mascot.image_anchor, Some(Point::new(64, 128)));
    }

    #[test]
    fn right_facing_bounds_use_the_displayed_images_mirrored_anchor() {
        let image = ResolvedImage {
            source: "/run.png".into(),
            relative: "run.png".into(),
            width: 97,
            height: 82,
        };
        let mut mascot = Mascot::new();
        mascot.look_right = true;
        let mut pose = pose(Some(image), 0, 81);
        apply_pose(&mut mascot, &pose);
        assert_eq!(mascot.image_anchor, Some(Point::new(97, 81)));
        pose.anchor_x = 7;
        pose.image_right = Some(ResolvedImage {
            source: "/run-right.png".into(),
            relative: "run-right.png".into(),
            width: 120,
            height: 90,
        });
        apply_pose(&mut mascot, &pose);
        assert_eq!(mascot.image_anchor, Some(Point::new(113, 81)));
        assert_eq!((mascot.image_width, mascot.image_height), (120, 90));
        pose.right_sibling = pose.image_right.take();
        apply_pose(&mut mascot, &pose);
        assert_eq!(mascot.image_anchor, Some(Point::new(113, 81)));
        mascot.look_right = false;
        apply_pose(&mut mascot, &super::tests::pose(None, 0, 0));
        assert_eq!(mascot.image_anchor, Some(Point::new(113, 81)));
    }
}

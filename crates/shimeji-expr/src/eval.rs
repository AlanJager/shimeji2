//! Recursive evaluator for the expression subset.
//!
//! Static host paths are already [`Accessor`]s or [`Border`]s when they reach
//! this module. `&&`, `||` and `?:` skip the untaken side, so a `Math.random`
//! there does not draw. A bare function value used as a number is `NaN`.

use crate::ast::{BinaryOp, Expr, ExprKind, Span, UnaryOp};
use crate::budget::{Budget, BudgetError, BudgetKind, Limits};
use crate::host::{Host, Static, resolve_prop, resolve_root};
use crate::java_random::JavaRandom;
use crate::parser::{MAX_EXPR_DEPTH, ParseError, parse, strip_wrapper};
use crate::value::{
    Accessor, Border, BuiltinFn, HostObject, Value, js_max, js_min, js_rem, js_round,
    number_to_string, to_bool, to_number,
};
use crate::vars::{VarKind, VarMap};

/// Total live evaluator frames, shared by AST recursion and variable re-entry.
///
/// `child` and `read_var` both increment this counter, so a chain of variables
/// cannot multiply with the depth of each expression. One step is one of those
/// calls. In a debug build the heaviest step is a nested call (`Math.abs`),
/// about 3 KiB of stack. On a thread started with `stack_size(256 * 1024)`
/// that pattern still returned at depth 82 and aborted by depth 85: the
/// pthread stack is 274,432 bytes and about 16 KiB of that is the guard page.
/// The check rejects the next frame after this cap, so the deepest frame is
/// 65. That is roughly 50 KiB under the probe, and still above one expression
/// (`MAX_EXPR_DEPTH` is 28) and every bundled variable chain.
pub const MAX_DEPTH: u32 = 64;

/// Where an expression sits in the engine. Unit 6 applies [`decide`]; this
/// crate only classifies the error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSite {
    BehaviorCondition,
    ActionCondition,
    ActionParameter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteEffect {
    /// The condition is false. The mascot stays.
    False,
    /// The mascot is disposed.
    Dispose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteDecision {
    pub effect: SiteEffect,
    pub diagnostic: Diagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalErrorKind {
    Reference,
    Type,
    Cycle,
    Budget,
    NonBoolean,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalError {
    pub kind: EvalErrorKind,
    pub span: Span,
    pub message: String,
    budget: Option<BudgetKind>,
}

impl EvalError {
    pub fn budget_kind(&self) -> Option<BudgetKind> {
        self.budget
    }

    fn new(kind: EvalErrorKind, span: Span, message: impl Into<String>) -> Self {
        Self {
            kind,
            span,
            message: message.into(),
            budget: None,
        }
    }

    fn reference(name: &str, span: Span) -> Self {
        Self::new(
            EvalErrorKind::Reference,
            span,
            format!("ReferenceError: \"{name}\" is not defined"),
        )
    }

    fn type_error(message: impl Into<String>, span: Span) -> Self {
        Self::new(EvalErrorKind::Type, span, message)
    }

    fn cycle(name: &str, span: Span) -> Self {
        Self::new(
            EvalErrorKind::Cycle,
            span,
            format!("cycle evaluating variable \"{name}\""),
        )
    }

    fn non_boolean(value: Value, span: Span) -> Self {
        Self::new(
            EvalErrorKind::NonBoolean,
            span,
            format!("condition is {}, expected boolean", value.type_name()),
        )
    }

    fn from_budget(error: BudgetError, message: &str) -> Self {
        Self {
            kind: EvalErrorKind::Budget,
            span: error.span,
            message: message.to_string(),
            budget: Some(error.kind),
        }
    }
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}..{}",
            self.message, self.span.start, self.span.end
        )
    }
}

impl std::error::Error for EvalError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    Parse(ParseError),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CompileError {}

impl From<ParseError> for CompileError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

/// Map an evaluation error the way the engine will.
///
/// A behavior-condition error is false. An action-condition or parameter
/// error disposes the mascot. A non-boolean condition is false at either
/// condition site: the reference throws `ClassCastException`, and that
/// difference is intentional.
pub fn decide(site: CallSite, error: &EvalError) -> SiteDecision {
    let effect = match (site, error.kind) {
        (CallSite::BehaviorCondition, _) => SiteEffect::False,
        (CallSite::ActionCondition, EvalErrorKind::NonBoolean) => SiteEffect::False,
        (CallSite::ActionCondition | CallSite::ActionParameter, _) => SiteEffect::Dispose,
    };
    SiteDecision {
        effect,
        diagnostic: Diagnostic {
            message: error.message.clone(),
            span: error.span,
        },
    }
}

pub fn decide_parse(site: CallSite, error: &ParseError) -> SiteDecision {
    let effect = match site {
        CallSite::BehaviorCondition => SiteEffect::False,
        CallSite::ActionCondition | CallSite::ActionParameter => SiteEffect::Dispose,
    };
    SiteDecision {
        effect,
        diagnostic: Diagnostic {
            message: error.message.clone(),
            span: error.span,
        },
    }
}

/// Conditions are Java `Boolean` casts, not JavaScript truthiness.
/// `1` is an error here; `!0` is the boolean `true`.
pub fn condition_bool(value: Value, span: Span) -> Result<bool, EvalError> {
    match value {
        Value::Bool(value) => Ok(value),
        other => Err(EvalError::non_boolean(other, span)),
    }
}

/// A compiled expression. Host paths inside it are accessors, not strings.
#[derive(Debug)]
pub struct Code {
    node: Node,
}

impl Code {
    pub fn span(&self) -> Span {
        self.node.span
    }
}

#[derive(Debug)]
pub(crate) struct Node {
    span: Span,
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    Number(f64),
    Bool(bool),
    Str(u32),
    Null,
    Undefined,
    Host(HostObject),
    Border(Border),
    Func(BuiltinFn),
    Math,
    IsOn(Border),
    Load(Accessor),
    ReadVar(u32),
    Unresolved(String),
    Unary {
        op: UnaryOp,
        expr: Box<Node>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Node>,
        right: Box<Node>,
    },
    Ternary {
        cond: Box<Node>,
        then_b: Box<Node>,
        else_b: Box<Node>,
    },
    Call {
        callee: Box<Node>,
        args: Vec<Node>,
    },
    Index {
        object: Box<Node>,
        index: Box<Node>,
    },
    GetProp {
        object: Box<Node>,
        name: String,
    },
}

impl VarMap {
    pub fn compile(&mut self, src: &str) -> Result<Code, CompileError> {
        Ok(Code {
            node: compile_node(src, self)?,
        })
    }

    pub fn define(&mut self, name: &str, kind: VarKind, src: &str) -> Result<(), CompileError> {
        self.reserve(name);
        let node = compile_node(src, self)?;
        let slot = self.slot_index(name).expect("reserved") as usize;
        self.set_script(slot, kind, node);
        Ok(())
    }

    /// Reserve every name, then compile. A script may refer to a sibling
    /// defined later in `defs`.
    pub fn define_all(&mut self, defs: &[(&str, VarKind, &str)]) -> Result<(), CompileError> {
        for (name, _, _) in defs {
            self.reserve(name);
        }
        for (name, kind, src) in defs {
            self.define(name, *kind, src)?;
        }
        Ok(())
    }

    pub fn eval<H: Host>(
        &mut self,
        code: &Code,
        host: &H,
        rng: &mut JavaRandom,
        limits: Limits,
    ) -> Result<Value, EvalError> {
        let mut budget = Budget::new(limits);
        let mut frame = Frame {
            scope: self,
            host,
            rng,
            budget: &mut budget,
            depth: 0,
        };
        frame.eval(&code.node)
    }

    pub fn read<H: Host>(
        &mut self,
        name: &str,
        host: &H,
        rng: &mut JavaRandom,
        limits: Limits,
    ) -> Result<Value, EvalError> {
        let Some(slot) = self.slot_index(name) else {
            return Err(EvalError::reference(name, Span::new(0, 0)));
        };
        let mut budget = Budget::new(limits);
        let mut frame = Frame {
            scope: self,
            host,
            rng,
            budget: &mut budget,
            depth: 0,
        };
        frame.read_var(slot, Span::new(0, name.len()))
    }
}

fn compile_node(src: &str, scope: &mut VarMap) -> Result<Node, CompileError> {
    let expr = parse(strip_wrapper(src))?;
    lower(&expr, scope).map_err(Into::into)
}

fn lower(expr: &Expr, scope: &mut VarMap) -> Result<Node, ParseError> {
    lower_at(expr, scope, 0)
}

fn lower_at(expr: &Expr, scope: &mut VarMap, depth: u32) -> Result<Node, ParseError> {
    if depth > MAX_EXPR_DEPTH {
        return Err(ParseError::new("expression is too deep", expr.span));
    }
    let span = expr.span;
    let next = depth + 1;
    let kind = match &expr.kind {
        ExprKind::Number(n) => Kind::Number(*n),
        ExprKind::Bool(b) => Kind::Bool(*b),
        ExprKind::Null => Kind::Null,
        ExprKind::String(text) => Kind::Str(scope.intern(text)),
        ExprKind::Ident(name) => lower_ident(name, scope),
        ExprKind::Unary { op, expr } => Kind::Unary {
            op: *op,
            expr: Box::new(lower_at(expr, scope, next)?),
        },
        ExprKind::Binary { op, left, right } => Kind::Binary {
            op: *op,
            left: Box::new(lower_at(left, scope, next)?),
            right: Box::new(lower_at(right, scope, next)?),
        },
        ExprKind::Ternary {
            cond,
            then_branch,
            else_branch,
        } => Kind::Ternary {
            cond: Box::new(lower_at(cond, scope, next)?),
            then_b: Box::new(lower_at(then_branch, scope, next)?),
            else_b: Box::new(lower_at(else_branch, scope, next)?),
        },
        ExprKind::Member { object, name } => {
            return lower_member(object, name, span, scope, next);
        }
        ExprKind::Call { callee, args } => {
            let mut lowered = Vec::with_capacity(args.len());
            for arg in args {
                lowered.push(lower_at(arg, scope, next)?);
            }
            Kind::Call {
                callee: Box::new(lower_at(callee, scope, next)?),
                args: lowered,
            }
        }
        ExprKind::Index { object, index } => Kind::Index {
            object: Box::new(lower_at(object, scope, next)?),
            index: Box::new(lower_at(index, scope, next)?),
        },
    };
    Ok(Node { span, kind })
}

fn lower_ident(name: &str, scope: &VarMap) -> Kind {
    if let Some(slot) = scope.slot_index(name) {
        return Kind::ReadVar(slot);
    }
    match resolve_root(name) {
        Some(value) => static_kind(value),
        None => Kind::Unresolved(name.to_string()),
    }
}

fn lower_member(
    object: &Expr,
    name: &str,
    span: Span,
    scope: &mut VarMap,
    depth: u32,
) -> Result<Node, ParseError> {
    let base = lower_at(object, scope, depth)?;
    if let Some(static_base) = kind_static(&base.kind) {
        if let Some(prop) = resolve_prop(&static_base, name) {
            return Ok(Node {
                span,
                kind: static_kind(prop),
            });
        }
    }
    Ok(Node {
        span,
        kind: Kind::GetProp {
            object: Box::new(base),
            name: name.to_string(),
        },
    })
}

fn kind_static(kind: &Kind) -> Option<Static> {
    match kind {
        Kind::Host(object) => Some(Static::Host(*object)),
        Kind::Border(border) => Some(Static::Border(*border)),
        Kind::Math => Some(Static::Math),
        _ => None,
    }
}

fn static_kind(value: Static) -> Kind {
    match value {
        Static::Number(n) => Kind::Number(n),
        Static::Undefined => Kind::Undefined,
        Static::Host(object) => Kind::Host(object),
        Static::Border(border) => Kind::Border(border),
        Static::Load(accessor) => Kind::Load(accessor),
        Static::Func(func) => Kind::Func(func),
        Static::Math => Kind::Math,
        Static::IsOn(border) => Kind::IsOn(border),
    }
}

struct Frame<'a, H: Host> {
    scope: &'a mut VarMap,
    host: &'a H,
    rng: &'a mut JavaRandom,
    budget: &'a mut Budget,
    depth: u32,
}

impl<'a, H: Host> Frame<'a, H> {
    fn eval(&mut self, node: &Node) -> Result<Value, EvalError> {
        if self.depth > MAX_DEPTH {
            return Err(EvalError::from_budget(
                BudgetError {
                    kind: BudgetKind::Steps,
                    span: node.span,
                },
                "expression is too deep",
            ));
        }
        self.budget
            .step(node.span)
            .map_err(|error| EvalError::from_budget(error, "step budget exceeded"))?;
        match &node.kind {
            Kind::Number(n) => Ok(Value::Number(*n)),
            Kind::Bool(b) => Ok(Value::Bool(*b)),
            Kind::Str(id) => Ok(Value::Str(*id)),
            Kind::Null => Ok(Value::Null),
            Kind::Undefined => Ok(Value::Undefined),
            Kind::Host(object) => Ok(Value::Host(*object)),
            Kind::Border(border) => Ok(Value::Border(*border)),
            Kind::Func(func) => Ok(Value::Func(*func)),
            Kind::Math => Ok(Value::Math),
            Kind::IsOn(border) => Ok(Value::IsOn(*border)),
            Kind::Load(accessor) => Ok(self.host.get(*accessor).into()),
            Kind::ReadVar(slot) => self.read_var(*slot, node.span),
            Kind::Unresolved(name) => Err(EvalError::reference(name, node.span)),
            Kind::Unary { op, expr } => self.eval_unary(*op, expr, node.span),
            Kind::Binary { op, left, right } => self.eval_binary(*op, left, right, node.span),
            Kind::Ternary {
                cond,
                then_b,
                else_b,
            } => {
                let cond = self.child(cond)?;
                if to_bool(cond, self.scope.pool()) {
                    self.child(then_b)
                } else {
                    self.child(else_b)
                }
            }
            Kind::Call { callee, args } => self.eval_call(callee, args, node.span),
            Kind::Index { object, index } => self.eval_index(object, index, node.span),
            Kind::GetProp { object, name } => self.eval_prop(object, name, node.span),
        }
    }

    fn child(&mut self, node: &Node) -> Result<Value, EvalError> {
        self.depth += 1;
        let result = self.eval(node);
        self.depth -= 1;
        result
    }

    fn read_var(&mut self, slot: u32, span: Span) -> Result<Value, EvalError> {
        let slot = slot as usize;
        if self.scope.is_busy(slot) {
            let name = self.scope.slot_name(slot).to_string();
            return Err(EvalError::cycle(&name, span));
        }
        if let Some(value) = self.scope.cached(slot) {
            return Ok(value);
        }
        // Same counter as `child`. A cycle still reports [`EvalErrorKind::Cycle`]
        // when it closes inside [`MAX_DEPTH`]; past it the read stops with a
        // budget error.
        self.depth += 1;
        let result = if self.depth > MAX_DEPTH {
            Err(EvalError::from_budget(
                BudgetError {
                    kind: BudgetKind::Steps,
                    span,
                },
                "expression is too deep",
            ))
        } else if let Some(node) = self.scope.take_script(slot) {
            self.scope.set_busy(slot, true);
            let result = self.eval(&node);
            self.scope.set_busy(slot, false);
            self.scope.put_script(slot, node);
            result
        } else {
            let name = self.scope.slot_name(slot).to_string();
            Err(EvalError::reference(&name, span))
        };
        self.depth -= 1;
        match result {
            Ok(value) => {
                // `Script.needsReevaluation`: a null or undefined result is cached.
                self.scope.store_cache(slot, value);
                Ok(value)
            }
            Err(error) => Err(error),
        }
    }

    fn eval_unary(&mut self, op: UnaryOp, expr: &Node, _span: Span) -> Result<Value, EvalError> {
        let value = self.child(expr)?;
        Ok(match op {
            UnaryOp::Not => Value::Bool(!to_bool(value, self.scope.pool())),
            UnaryOp::Neg => Value::Number(-to_number(value, self.scope.pool())),
            UnaryOp::Plus => Value::Number(to_number(value, self.scope.pool())),
        })
    }

    fn eval_binary(
        &mut self,
        op: BinaryOp,
        left: &Node,
        right: &Node,
        span: Span,
    ) -> Result<Value, EvalError> {
        if op == BinaryOp::Or || op == BinaryOp::And {
            let left = self.child(left)?;
            let truthy = to_bool(left, self.scope.pool());
            if (op == BinaryOp::Or && truthy) || (op == BinaryOp::And && !truthy) {
                return Ok(left);
            }
            return self.child(right);
        }
        let left = self.child(left)?;
        let right = self.child(right)?;
        match op {
            BinaryOp::Or | BinaryOp::And => unreachable!("short-circuit handled above"),
            BinaryOp::Add => self.add(left, right, span),
            BinaryOp::Sub => Ok(Value::Number(
                to_number(left, self.scope.pool()) - to_number(right, self.scope.pool()),
            )),
            BinaryOp::Mul => Ok(Value::Number(
                to_number(left, self.scope.pool()) * to_number(right, self.scope.pool()),
            )),
            BinaryOp::Div => Ok(Value::Number(
                to_number(left, self.scope.pool()) / to_number(right, self.scope.pool()),
            )),
            BinaryOp::Rem => Ok(Value::Number(js_rem(
                to_number(left, self.scope.pool()),
                to_number(right, self.scope.pool()),
            ))),
            BinaryOp::Eq => Ok(Value::Bool(abstract_eq(left, right, self.scope.pool()))),
            BinaryOp::NotEq => Ok(Value::Bool(!abstract_eq(left, right, self.scope.pool()))),
            BinaryOp::StrictEq => Ok(Value::Bool(strict_eq(left, right, self.scope.pool()))),
            BinaryOp::StrictNotEq => Ok(Value::Bool(!strict_eq(left, right, self.scope.pool()))),
            BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                Ok(Value::Bool(relational(op, left, right, self.scope.pool())))
            }
        }
    }

    fn add(&mut self, left: Value, right: Value, span: Span) -> Result<Value, EvalError> {
        let left = self.primitive(left, span)?;
        let right = self.primitive(right, span)?;
        if matches!(left, Value::Str(_)) || matches!(right, Value::Str(_)) {
            self.concat(left, right, span)
        } else {
            Ok(Value::Number(
                to_number(left, self.scope.pool()) + to_number(right, self.scope.pool()),
            ))
        }
    }

    fn primitive(&mut self, value: Value, span: Span) -> Result<Value, EvalError> {
        match value {
            Value::Host(_) | Value::Border(_) | Value::Func(_) | Value::Math | Value::IsOn(_) => {
                let text = "[object Object]";
                self.budget
                    .reserve(text.len(), span)
                    .map_err(|error| EvalError::from_budget(error, "allocation budget exceeded"))?;
                let id = self.scope.intern(text);
                Ok(Value::Str(id))
            }
            other => Ok(other),
        }
    }

    fn concat(&mut self, left: Value, right: Value, span: Span) -> Result<Value, EvalError> {
        if let (Value::Str(a), Value::Str(b)) = (left, right) {
            let len_a = self.scope.pool().text(a).len();
            let len_b = self.scope.pool().text(b).len();
            let len = len_a + len_b;
            self.budget
                .reserve(len, span)
                .map_err(|error| EvalError::from_budget(error, "allocation budget exceeded"))?;
            let mut out = String::with_capacity(len);
            out.push_str(self.scope.pool().text(a));
            out.push_str(self.scope.pool().text(b));
            let id = self.scope.intern_owned(out);
            return Ok(Value::Str(id));
        }
        let left = self.render(left, span)?;
        let right = self.render(right, span)?;
        let len = left.len() + right.len();
        self.budget
            .reserve(len, span)
            .map_err(|error| EvalError::from_budget(error, "allocation budget exceeded"))?;
        let mut out = String::with_capacity(len);
        out.push_str(&left);
        out.push_str(&right);
        let id = self.scope.intern_owned(out);
        Ok(Value::Str(id))
    }

    fn render(&mut self, value: Value, span: Span) -> Result<String, EvalError> {
        let text = match value {
            Value::Str(id) => self.scope.pool().text(id).to_string(),
            Value::Number(n) => number_to_string(n),
            Value::Bool(true) => "true".to_string(),
            Value::Bool(false) => "false".to_string(),
            Value::Null => "null".to_string(),
            Value::Undefined => "undefined".to_string(),
            Value::Host(_) | Value::Border(_) | Value::Func(_) | Value::Math | Value::IsOn(_) => {
                "[object Object]".to_string()
            }
        };
        self.budget
            .reserve(text.len(), span)
            .map_err(|error| EvalError::from_budget(error, "allocation budget exceeded"))?;
        Ok(text)
    }

    fn eval_call(&mut self, callee: &Node, args: &[Node], span: Span) -> Result<Value, EvalError> {
        let func = self.child(callee)?;
        const INLINE: usize = 8;
        if args.len() <= INLINE {
            let mut inline = [Value::Undefined; INLINE];
            for (index, arg) in args.iter().enumerate() {
                inline[index] = self.child(arg)?;
            }
            self.call(func, &inline[..args.len()], span)
        } else {
            let bytes = args.len().saturating_mul(std::mem::size_of::<Value>());
            self.budget
                .reserve(bytes, span)
                .map_err(|error| EvalError::from_budget(error, "allocation budget exceeded"))?;
            let mut heap = Vec::with_capacity(args.len());
            for arg in args {
                heap.push(self.child(arg)?);
            }
            self.call(func, &heap, span)
        }
    }

    fn call(&mut self, func: Value, args: &[Value], span: Span) -> Result<Value, EvalError> {
        match func {
            Value::Func(BuiltinFn::Random) => Ok(Value::Number(self.rng.next_double())),
            Value::Func(BuiltinFn::Abs) => Ok(Value::Number(self.arg_number(args, 0).abs())),
            Value::Func(BuiltinFn::Floor) => Ok(Value::Number(self.arg_number(args, 0).floor())),
            Value::Func(BuiltinFn::Ceil) => Ok(Value::Number(self.arg_number(args, 0).ceil())),
            Value::Func(BuiltinFn::Round) => Ok(Value::Number(js_round(self.arg_number(args, 0)))),
            Value::Func(BuiltinFn::Min) => Ok(Value::Number(self.fold_min(args))),
            Value::Func(BuiltinFn::Max) => Ok(Value::Number(self.fold_max(args))),
            Value::IsOn(border) => {
                let point = args.first().copied().unwrap_or(Value::Undefined);
                Ok(Value::Bool(self.host.is_on(border, point)))
            }
            _ => Err(EvalError::type_error("value is not a function", span)),
        }
    }

    fn arg_number(&self, args: &[Value], index: usize) -> f64 {
        args.get(index)
            .map(|value| to_number(*value, self.scope.pool()))
            .unwrap_or(f64::NAN)
    }

    fn fold_min(&self, args: &[Value]) -> f64 {
        if args.is_empty() {
            return f64::INFINITY;
        }
        let mut acc = f64::INFINITY;
        for arg in args {
            acc = js_min(acc, to_number(*arg, self.scope.pool()));
        }
        acc
    }

    fn fold_max(&self, args: &[Value]) -> f64 {
        if args.is_empty() {
            return f64::NEG_INFINITY;
        }
        let mut acc = f64::NEG_INFINITY;
        for arg in args {
            acc = js_max(acc, to_number(*arg, self.scope.pool()));
        }
        acc
    }

    fn eval_index(&mut self, object: &Node, index: &Node, span: Span) -> Result<Value, EvalError> {
        let object = self.child(object)?;
        let index = self.child(index)?;
        let Value::Str(id) = object else {
            return Ok(Value::Undefined);
        };
        let n = to_number(index, self.scope.pool());
        if !n.is_finite() || n < 0.0 || n.fract() != 0.0 {
            return Ok(Value::Undefined);
        }
        let ch = self.scope.pool().text(id).chars().nth(n as usize);
        let Some(ch) = ch else {
            return Ok(Value::Undefined);
        };
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf);
        self.budget
            .reserve(encoded.len(), span)
            .map_err(|error| EvalError::from_budget(error, "allocation budget exceeded"))?;
        let owned = encoded.to_string();
        let id = self.scope.intern_owned(owned);
        Ok(Value::Str(id))
    }

    fn eval_prop(&mut self, object: &Node, name: &str, span: Span) -> Result<Value, EvalError> {
        let base = self.child(object)?;
        let static_base = match base {
            Value::Host(object) => Static::Host(object),
            Value::Border(border) => Static::Border(border),
            Value::Math => Static::Math,
            Value::Null | Value::Undefined => {
                return Err(EvalError::type_error(
                    "cannot read property of null or undefined",
                    span,
                ));
            }
            _ => return Ok(Value::Undefined),
        };
        match resolve_prop(&static_base, name) {
            Some(prop) => Ok(self.static_value(prop)),
            None => Ok(Value::Undefined),
        }
    }

    fn static_value(&self, value: Static) -> Value {
        match value {
            Static::Number(n) => Value::Number(n),
            Static::Undefined => Value::Undefined,
            Static::Host(object) => Value::Host(object),
            Static::Border(border) => Value::Border(border),
            Static::Load(accessor) => self.host.get(accessor).into(),
            Static::Func(func) => Value::Func(func),
            Static::Math => Value::Math,
            Static::IsOn(border) => Value::IsOn(border),
        }
    }
}

fn abstract_eq(left: Value, right: Value, pool: &crate::value::Pool) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => pool.text(a) == pool.text(b),
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
        (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null) => true,
        (Value::Host(a), Value::Host(b)) => a == b,
        (Value::Border(a), Value::Border(b)) => a == b,
        (Value::Func(a), Value::Func(b)) => a == b,
        (Value::IsOn(a), Value::IsOn(b)) => a == b,
        (Value::Math, Value::Math) => true,
        (Value::Number(a), Value::Str(_)) => a == to_number(right, pool),
        (Value::Str(_), Value::Number(b)) => to_number(left, pool) == b,
        (Value::Bool(value), _) => abstract_eq(Value::Number(bool_number(value)), right, pool),
        (_, Value::Bool(value)) => abstract_eq(left, Value::Number(bool_number(value)), pool),
        (Value::Host(_) | Value::Border(_) | Value::Func(_) | Value::Math | Value::IsOn(_), _) => {
            abstract_eq(Value::Number(f64::NAN), right, pool)
        }
        (_, Value::Host(_) | Value::Border(_) | Value::Func(_) | Value::Math | Value::IsOn(_)) => {
            abstract_eq(left, Value::Number(f64::NAN), pool)
        }
        _ => false,
    }
}

fn strict_eq(left: Value, right: Value, pool: &crate::value::Pool) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => pool.text(a) == pool.text(b),
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
        (Value::Host(a), Value::Host(b)) => a == b,
        (Value::Border(a), Value::Border(b)) => a == b,
        (Value::Func(a), Value::Func(b)) => a == b,
        (Value::IsOn(a), Value::IsOn(b)) => a == b,
        (Value::Math, Value::Math) => true,
        _ => false,
    }
}

fn bool_number(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

fn relational(op: BinaryOp, left: Value, right: Value, pool: &crate::value::Pool) -> bool {
    let order = if let (Value::Str(a), Value::Str(b)) = (left, right) {
        Some(pool.text(a).cmp(pool.text(b)))
    } else {
        let a = to_number(left, pool);
        let b = to_number(right, pool);
        if a.is_nan() || b.is_nan() {
            None
        } else {
            a.partial_cmp(&b)
        }
    };
    matches!(
        (op, order),
        (BinaryOp::Lt, Some(std::cmp::Ordering::Less))
            | (BinaryOp::Gt, Some(std::cmp::Ordering::Greater))
            | (
                BinaryOp::LtEq,
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            )
            | (
                BinaryOp::GtEq,
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{BorderKind, Root, Side};

    #[test]
    fn static_path_folds_to_a_border() {
        let mut scope = VarMap::new();
        let code = scope
            .compile("mascot.environment.activeIE.topBorder")
            .expect("compile");
        match &code.node.kind {
            Kind::Border(border) => {
                assert_eq!(border.root, Root::Mascot);
                assert_eq!(border.kind, BorderKind::ActiveIe(Side::Top));
            }
            other => panic!("expected a border accessor, got {other:?}"),
        }
    }

    #[test]
    fn bare_math_random_is_a_function_value() {
        let mut scope = VarMap::new();
        let code = scope.compile("Math.random").expect("compile");
        assert!(matches!(code.node.kind, Kind::Func(BuiltinFn::Random)));
    }

    #[test]
    fn anchor_x_is_a_load() {
        let mut scope = VarMap::new();
        let code = scope.compile("mascot.anchor.x").expect("compile");
        assert!(matches!(code.node.kind, Kind::Load(_)));
    }
}

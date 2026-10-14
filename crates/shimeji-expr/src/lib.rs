//! Sandboxed Shimeji expression evaluator and shared RNG.
//!
//! The parser and [`JavaRandom`] come from Unit 4 part A. This half evaluates
//! that AST: JavaScript coercion, host accessors resolved at compile time,
//! lazy `${}` / `#{}` cells, and a step and allocation budget.

pub mod ast;
pub mod budget;
pub mod eval;
pub mod host;
pub mod java_random;
pub mod lexer;
pub mod parser;
pub mod value;
pub mod vars;

pub use ast::{BinaryOp, Expr, ExprKind, Span, UnaryOp};
pub use budget::{BudgetKind, Limits};
pub use eval::{
    CallSite, Code, CompileError, Diagnostic, EvalError, EvalErrorKind, MAX_DEPTH, SiteDecision,
    SiteEffect, condition_bool, decide, decide_parse,
};
pub use host::{AreaFields, Host, MascotFields, StubHost};
pub use java_random::JavaRandom;
pub use parser::{MAX_EXPR_DEPTH, MAX_SOURCE_LEN, ParseError, parse, parse_script, strip_wrapper};
pub use value::{
    Accessor, Border, BorderKind, BuiltinFn, Field, Geom, HostObject, HostVal, Root, Side, Value,
    to_i32,
};
pub use vars::{VarKind, VarMap};

/// Crate name. Dependents call this so the workspace link is real.
pub fn crate_name() -> &'static str {
    "shimeji-expr"
}

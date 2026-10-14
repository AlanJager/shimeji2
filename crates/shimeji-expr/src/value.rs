//! JavaScript values for the expression subset, plus the Java `int` sink.
//!
//! String payloads are interned in the [`crate::VarMap`] that produced them.
//! [`Value::Str`] is an index into that pool, so a steady-state read does not
//! allocate.

/// Interned strings for one [`crate::VarMap`]. `ToNumber` is cached beside the text.
#[derive(Debug, Default)]
pub(crate) struct Pool {
    strings: Vec<String>,
    numbers: Vec<f64>,
}

impl Pool {
    pub(crate) fn intern(&mut self, text: &str) -> u32 {
        if let Some(index) = self.strings.iter().position(|existing| existing == text) {
            return index as u32;
        }
        self.push(text.to_string())
    }

    pub(crate) fn intern_owned(&mut self, text: String) -> u32 {
        if let Some(index) = self.strings.iter().position(|existing| existing == &text) {
            return index as u32;
        }
        self.push(text)
    }

    fn push(&mut self, text: String) -> u32 {
        let number = number_from_str(&text);
        let id = self.strings.len() as u32;
        self.strings.push(text);
        self.numbers.push(number);
        id
    }

    pub(crate) fn text(&self, id: u32) -> &str {
        &self.strings[id as usize]
    }

    pub(crate) fn number(&self, id: u32) -> f64 {
        self.numbers[id as usize]
    }
}

/// Which mascot a host path belongs to. `target` is the other mascot a few
/// corpus expressions read (`target.anchor.x`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Root {
    Mascot,
    Target,
}

/// A host object reached by a static path. Property chains are folded away
/// at compile time; evaluation only sees this when the object itself is a
/// value (for example the argument of `isOn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostObject {
    Mascot(Root),
    Anchor(Root),
    Environment(Root),
    Cursor(Root),
    Screen(Root),
    WorkArea(Root),
    ActiveIe(Root),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BorderKind {
    Floor,
    Ceiling,
    Wall,
    Screen(Side),
    WorkArea(Side),
    ActiveIe(Side),
}

/// A border object. `isOn` is its only method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Border {
    pub root: Root,
    pub kind: BorderKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Geom {
    Left,
    Top,
    Right,
    Bottom,
    Width,
    Height,
    Visible,
}

/// One numeric or boolean field of the host, after a path such as
/// `mascot.environment.workArea.bottom` has been folded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    AnchorX,
    AnchorY,
    LookRight,
    TotalCount,
    CursorX,
    CursorY,
    CursorDx,
    CursorDy,
    Screen(Geom),
    WorkArea(Geom),
    ActiveIe(Geom),
}

/// Compile-time accessor. Evaluation passes it back to [`crate::Host::get`]
/// and does not look at the original property names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Accessor {
    pub root: Root,
    pub field: Field,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinFn {
    Random,
    Min,
    Max,
    Abs,
    Floor,
    Ceil,
    Round,
}

/// What [`crate::Host::get`] returns. Host fields in this subset are numbers
/// and booleans. Strings live in the value pool instead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HostVal {
    Number(f64),
    Bool(bool),
    Null,
    Undefined,
}

/// A runtime value. `Copy` on purpose: numbers, bools, host refs and interned
/// strings move without allocating.
#[derive(Debug, Clone, Copy)]
pub enum Value {
    Number(f64),
    Bool(bool),
    /// Index into the owning [`crate::VarMap`] string pool.
    Str(u32),
    Null,
    Undefined,
    Host(HostObject),
    Border(Border),
    Func(BuiltinFn),
    /// The `Math` object, not a function. A bare `Math.random` is [`Value::Func`].
    Math,
    /// Bound `isOn` that has not been called. Arithmetic on it is `NaN`.
    IsOn(Border),
}

impl Value {
    pub fn type_name(self) -> &'static str {
        match self {
            Self::Number(_) => "number",
            Self::Bool(_) => "boolean",
            Self::Str(_) => "string",
            Self::Null => "null",
            Self::Undefined => "undefined",
            Self::Host(_) | Self::Border(_) | Self::Math => "object",
            Self::Func(_) | Self::IsOn(_) => "function",
        }
    }

    pub fn is_null(self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn as_number(self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_bool(self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(b),
            _ => None,
        }
    }
}

impl From<HostVal> for Value {
    fn from(value: HostVal) -> Self {
        match value {
            HostVal::Number(n) => Self::Number(n),
            HostVal::Bool(b) => Self::Bool(b),
            HostVal::Null => Self::Null,
            HostVal::Undefined => Self::Undefined,
        }
    }
}

/// Java `Double.intValue` / narrowing conversion: truncate toward zero,
/// saturate at the `int` bounds, and map `NaN` to 0.
pub fn to_i32(n: f64) -> i32 {
    if n.is_nan() {
        return 0;
    }
    if n >= i32::MAX as f64 {
        return i32::MAX;
    }
    if n <= i32::MIN as f64 {
        return i32::MIN;
    }
    n.trunc() as i32
}

pub(crate) fn to_number(value: Value, pool: &Pool) -> f64 {
    match value {
        Value::Number(n) => n,
        Value::Bool(b) => {
            if b {
                1.0
            } else {
                0.0
            }
        }
        Value::Null => 0.0,
        Value::Str(id) => pool.number(id),
        Value::Undefined
        | Value::Host(_)
        | Value::Border(_)
        | Value::Func(_)
        | Value::Math
        | Value::IsOn(_) => f64::NAN,
    }
}

pub(crate) fn to_bool(value: Value, pool: &Pool) -> bool {
    match value {
        Value::Bool(b) => b,
        Value::Number(n) => n != 0.0 && !n.is_nan(),
        Value::Null | Value::Undefined => false,
        Value::Str(id) => !pool.text(id).is_empty(),
        Value::Host(_) | Value::Border(_) | Value::Func(_) | Value::Math | Value::IsOn(_) => true,
    }
}

/// ES5 `ToNumber` on a string. Computed once when the string is interned.
pub(crate) fn number_from_str(src: &str) -> f64 {
    let s = trim_js(src);
    if s.is_empty() {
        return 0.0;
    }
    if s == "Infinity" || s == "+Infinity" {
        return f64::INFINITY;
    }
    if s == "-Infinity" {
        return f64::NEG_INFINITY;
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return hex_to_f64(hex);
    }
    if !is_js_decimal(s) {
        return f64::NAN;
    }
    s.parse().unwrap_or(f64::NAN)
}

pub(crate) fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n == 0.0 {
        return "0".to_string();
    }
    if n.is_infinite() {
        return if n.is_sign_positive() {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    format!("{n}")
}

fn trim_js(src: &str) -> &str {
    src.trim_matches(is_js_ws)
}

fn is_js_ws(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{000B}'
            | '\u{000C}'
            | '\r'
            | ' '
            | '\u{00A0}'
            | '\u{FEFF}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{1680}'
            | '\u{2000}'
            | '\u{2001}'
            | '\u{2002}'
            | '\u{2003}'
            | '\u{2004}'
            | '\u{2005}'
            | '\u{2006}'
            | '\u{2007}'
            | '\u{2008}'
            | '\u{2009}'
            | '\u{200A}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

fn is_js_decimal(src: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let mut saw_digit = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        saw_digit = true;
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            saw_digit = true;
            i += 1;
        }
    }
    if !saw_digit {
        return false;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let exp = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp {
            return false;
        }
    }
    i == bytes.len()
}

fn hex_to_f64(src: &str) -> f64 {
    if src.is_empty() || !src.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return f64::NAN;
    }
    u64::from_str_radix(src, 16)
        .map(|n| n as f64)
        .unwrap_or(f64::NAN)
}

/// `f64::min` returns the other operand when one is `NaN`. JavaScript and
/// Java `Math.min` return `NaN` instead. Same for signed zero.
pub(crate) fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b { a } else { b }
}

pub(crate) fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_positive() { a } else { b };
    }
    if a > b { a } else { b }
}

pub(crate) fn js_round(n: f64) -> f64 {
    if !n.is_finite() || n == 0.0 {
        return n;
    }
    if (-0.5..0.0).contains(&n) {
        return -0.0;
    }
    (n + 0.5).floor()
}

pub(crate) fn js_rem(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() || a.is_infinite() || b == 0.0 {
        f64::NAN
    } else if a == 0.0 || b.is_infinite() {
        a
    } else {
        a % b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_sink_truncates_saturates_and_maps_nan() {
        assert_eq!(to_i32(1.9), 1);
        assert_eq!(to_i32(-1.9), -1);
        assert_eq!(to_i32(-0.9), 0);
        assert_eq!(to_i32(f64::NAN), 0);
        assert_eq!(to_i32(f64::INFINITY), i32::MAX);
        assert_eq!(to_i32(f64::NEG_INFINITY), i32::MIN);
        assert_eq!(to_i32(1e20), i32::MAX);
        assert_eq!(to_i32(-1e20), i32::MIN);
        assert_eq!(to_i32(i32::MAX as f64), i32::MAX);
        assert_eq!(to_i32(i32::MIN as f64), i32::MIN);
    }

    #[test]
    fn string_to_number_matches_js() {
        assert_eq!(number_from_str("50"), 50.0);
        assert_eq!(number_from_str("  50 "), 50.0);
        assert_eq!(number_from_str(""), 0.0);
        assert_eq!(number_from_str("   "), 0.0);
        assert!(number_from_str("foo").is_nan());
        assert_eq!(number_from_str("Infinity"), f64::INFINITY);
        assert_eq!(number_from_str("-Infinity"), f64::NEG_INFINITY);
        assert_eq!(number_from_str("0x10"), 16.0);
        assert_eq!(number_from_str(".5"), 0.5);
        assert_eq!(number_from_str("5."), 5.0);
        assert_eq!(number_from_str("+5e1"), 50.0);
    }
}

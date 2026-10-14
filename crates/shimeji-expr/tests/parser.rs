//! Parse tests for every grammar production, errors, Unicode, wrappers, and rejects.

use shimeji_expr::{
    BinaryOp, Expr, ExprKind, ParseError, UnaryOp, parse, parse_script, strip_wrapper,
};

fn dump(expr: &Expr) -> String {
    dump_kind(&expr.kind)
}

fn dump_kind(kind: &ExprKind) -> String {
    match kind {
        ExprKind::Number(n) => format!("num({n})"),
        ExprKind::String(s) => format!("str({s:?})"),
        ExprKind::Bool(b) => format!("bool({b})"),
        ExprKind::Null => "null".into(),
        ExprKind::Ident(name) => format!("id({name})"),
        ExprKind::Unary { op, expr } => {
            let name = match op {
                UnaryOp::Not => "not",
                UnaryOp::Neg => "neg",
                UnaryOp::Plus => "plus",
            };
            format!("{name}({})", dump(expr))
        }
        ExprKind::Binary { op, left, right } => {
            let name = match op {
                BinaryOp::Or => "or",
                BinaryOp::And => "and",
                BinaryOp::Eq => "eq",
                BinaryOp::NotEq => "ne",
                BinaryOp::StrictEq => "seq",
                BinaryOp::StrictNotEq => "sne",
                BinaryOp::Lt => "lt",
                BinaryOp::LtEq => "le",
                BinaryOp::Gt => "gt",
                BinaryOp::GtEq => "ge",
                BinaryOp::Add => "add",
                BinaryOp::Sub => "sub",
                BinaryOp::Mul => "mul",
                BinaryOp::Div => "div",
                BinaryOp::Rem => "rem",
            };
            format!("{name}({}, {})", dump(left), dump(right))
        }
        ExprKind::Ternary {
            cond,
            then_branch,
            else_branch,
        } => format!(
            "ternary({}, {}, {})",
            dump(cond),
            dump(then_branch),
            dump(else_branch)
        ),
        ExprKind::Member { object, name } => format!("member({}, {name})", dump(object)),
        ExprKind::Call { callee, args } => {
            let inner = args.iter().map(dump).collect::<Vec<_>>().join(", ");
            format!("call({}, [{inner}])", dump(callee))
        }
        ExprKind::Index { object, index } => format!("index({}, {})", dump(object), dump(index)),
    }
}

fn assert_dump(src: &str, expected: &str) {
    let expr = parse(src).unwrap_or_else(|error| panic!("parse {src:?}: {error}"));
    assert_eq!(dump(&expr), expected, "src={src:?}");
}

fn assert_reject(src: &str, needle: &str) -> ParseError {
    match parse(src) {
        Ok(expr) => panic!("expected reject {src:?}, got {}", dump(&expr)),
        Err(error) => {
            assert!(
                error.message.contains(needle),
                "src={src:?} message={:?} missing {needle:?}",
                error.message
            );
            error
        }
    }
}

#[test]
fn number_integer() {
    assert_dump("100", "num(100)");
}

#[test]
fn number_float() {
    assert_dump("0.5", "num(0.5)");
}

#[test]
fn number_leading_dot() {
    assert_dump(".5", "num(0.5)");
}

#[test]
fn number_exponent() {
    assert_dump("1e-3", "num(0.001)");
}

#[test]
fn string_double_quote() {
    assert_dump("\"5\"", "str(\"5\")");
}

#[test]
fn string_single_quote() {
    assert_dump("'hello'", "str(\"hello\")");
}

#[test]
fn string_escape() {
    assert_dump("\"a\\nb\"", "str(\"a\\nb\")");
}

#[test]
fn literals_true_false_null() {
    assert_dump("true", "bool(true)");
    assert_dump("false", "bool(false)");
    assert_dump("null", "null");
}

#[test]
fn ident_ascii() {
    assert_dump("maxCount", "id(maxCount)");
}

#[test]
fn ident_unicode_japanese() {
    assert_dump("目的地Y", "id(目的地Y)");
    assert_dump("ずれ", "id(ずれ)");
    assert_dump("目的地Y + ずれ", "add(id(目的地Y), id(ずれ))");
}

#[test]
fn grouping() {
    assert_dump("(1+2)*3", "mul(add(num(1), num(2)), num(3))");
}

#[test]
fn unary_not_neg_plus() {
    assert_dump("!false", "not(bool(false))");
    assert_dump("-15", "neg(num(15))");
    assert_dump("+\"5\"", "plus(str(\"5\"))");
    assert_dump("!!true", "not(not(bool(true)))");
}

#[test]
fn postfix_member() {
    assert_dump("mascot.anchor.x", "member(member(id(mascot), anchor), x)");
}

#[test]
fn postfix_call() {
    assert_dump("Math.random()", "call(member(id(Math), random), [])");
    assert_dump(
        "Math.min(2, 5)",
        "call(member(id(Math), min), [num(2), num(5)])",
    );
}

#[test]
fn postfix_index() {
    assert_dump(
        "mascot.variables['k']",
        "index(member(id(mascot), variables), str(\"k\"))",
    );
}

#[test]
fn postfix_call_on_member() {
    assert_dump(
        "mascot.environment.floor.isOn(mascot.anchor)",
        "call(member(member(member(id(mascot), environment), floor), isOn), [member(id(mascot), anchor)])",
    );
}

#[test]
fn mul_div_rem() {
    assert_dump("2*3/4%5", "rem(div(mul(num(2), num(3)), num(4)), num(5))");
}

#[test]
fn add_sub() {
    assert_dump("1+2-3", "sub(add(num(1), num(2)), num(3))");
}

#[test]
fn string_concat_shape() {
    assert_dump("\"5\" + 1", "add(str(\"5\"), num(1))");
}

#[test]
fn rel_ops() {
    assert_dump("a<b", "lt(id(a), id(b))");
    assert_dump("a<=b", "le(id(a), id(b))");
    assert_dump("a>b", "gt(id(a), id(b))");
    assert_dump("a>=b", "ge(id(a), id(b))");
}

#[test]
fn eq_ops() {
    assert_dump("1==\"1\"", "eq(num(1), str(\"1\"))");
    assert_dump("1!=\"1\"", "ne(num(1), str(\"1\"))");
    assert_dump("1===\"1\"", "seq(num(1), str(\"1\"))");
    assert_dump("1!==\"1\"", "sne(num(1), str(\"1\"))");
}

#[test]
fn and_or() {
    assert_dump("a&&b||c", "or(and(id(a), id(b)), id(c))");
}

#[test]
fn ternary() {
    assert_dump(
        "mascot.lookRight ? 1 : 0",
        "ternary(member(id(mascot), lookRight), num(1), num(0))",
    );
}

#[test]
fn ternary_right_assoc() {
    assert_dump(
        "a ? b : c ? d : e",
        "ternary(id(a), id(b), ternary(id(c), id(d), id(e)))",
    );
}

#[test]
fn precedence_add_vs_mul() {
    assert_dump("1+2*3", "add(num(1), mul(num(2), num(3)))");
}

#[test]
fn bare_math_random_times() {
    assert_dump("Math.random*100", "mul(member(id(Math), random), num(100))");
}

#[test]
fn happy_path_cursor_lerp() {
    assert_dump(
        "mascot.anchor.x+(mascot.environment.cursor.x-mascot.anchor.x)*Math.random()",
        "add(member(member(id(mascot), anchor), x), mul(sub(member(member(member(id(mascot), environment), cursor), x), member(member(id(mascot), anchor), x)), call(member(id(Math), random), [])))",
    );
}

#[test]
fn wrapper_strip_dollar() {
    assert_eq!(strip_wrapper("  ${1+2}  "), "1+2");
    assert_dump(
        &{
            let inner = strip_wrapper("${1+2}");
            inner.to_string()
        },
        "add(num(1), num(2))",
    );
    let expr = parse_script("${1+2}").unwrap();
    assert_eq!(dump(&expr), "add(num(1), num(2))");
}

#[test]
fn wrapper_strip_hash() {
    assert_eq!(strip_wrapper("#{mascot.x}"), "mascot.x");
    let expr = parse_script("#{mascot.x}").unwrap();
    assert_eq!(dump(&expr), "member(id(mascot), x)");
}

#[test]
fn wrapper_strip_leaves_bare() {
    assert_eq!(strip_wrapper("1+2"), "1+2");
}

#[test]
fn error_span_assignment() {
    let error = assert_reject("foo = 1", "assignment");
    assert_eq!(error.span.start, 4);
    assert_eq!(error.span.end, 5);
}

#[test]
fn error_span_unclosed_paren() {
    let error = assert_reject("(1+2", "expected ')'");
    assert_eq!(error.span.start, 4);
    assert_eq!(error.span.end, 4);
}

#[test]
fn error_span_unclosed_string() {
    let error = assert_reject("\"hello", "unterminated string");
    assert_eq!(error.span.start, 0);
}

#[test]
fn reject_new() {
    assert_reject("new Foo()", "new is not in the subset");
}

#[test]
fn reject_function() {
    assert_reject("function () { return 1; }", "function is not in the subset");
}

#[test]
fn reject_arrow_function() {
    assert_reject("() => 1", "arrow functions");
}

#[test]
fn reject_statements_semicolon() {
    assert_reject("1; 2", "statements");
}

#[test]
fn reject_var() {
    assert_reject("var x = 1", "var is not in the subset");
}

#[test]
fn reject_assignment_compound() {
    assert_reject("a += 1", "assignment");
}

#[test]
fn reject_increment() {
    assert_reject("a++", "increment");
}

#[test]
fn reject_empty() {
    assert_reject("   ", "empty expression");
}

#[test]
fn reject_hex() {
    assert_reject("0x10", "base prefixes");
}

#[test]
fn whitespace_and_crlf() {
    let expr = parse("mascot.anchor.x\r\n+\t1").unwrap();
    assert_eq!(
        dump(&expr),
        "add(member(member(id(mascot), anchor), x), num(1))"
    );
}

//! Hostile expressions return a typed error instead of overflowing the stack.

use shimeji_expr::{
    EvalErrorKind, Expr, ExprKind, JavaRandom, Limits, MAX_DEPTH, MAX_EXPR_DEPTH, MAX_SOURCE_LEN,
    StubHost, Value, VarKind, VarMap, parse, parse_script,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

fn beyond_limit() -> usize {
    (MAX_EXPR_DEPTH as usize).saturating_mul(128)
}

fn assert_rejected(src: &str) {
    assert!(parse(src).is_err(), "parse accepted {} bytes", src.len());
    let mut scope = VarMap::new();
    assert!(
        scope.compile(src).is_err(),
        "compile accepted {} bytes",
        src.len()
    );
    let mut scope = VarMap::new();
    assert!(
        evaluate(&mut scope, src).is_err(),
        "eval accepted {} bytes",
        src.len()
    );
}

fn evaluate(scope: &mut VarMap, src: &str) -> Result<Value, String> {
    let code = scope.compile(src).map_err(|error| error.to_string())?;
    scope
        .eval(
            &code,
            &StubHost::default(),
            &mut JavaRandom::new(1),
            Limits::default(),
        )
        .map_err(|error| error.to_string())
}

#[test]
fn deep_shapes_error_at_parse_compile_and_eval() {
    let n = beyond_limit();
    let addition = format!("{}1", "1+".repeat(n));
    let parens = format!("{}1{}", "(".repeat(n), ")".repeat(n));
    let bangs = format!("{}true", "!".repeat(n));
    let ternary = format!("{}1{}", "a?".repeat(n), ":0".repeat(n));
    let minuses = format!("{}1", "-".repeat(n));
    for src in [&addition, &parens, &bangs, &ternary, &minuses] {
        assert!(
            src.len() < MAX_SOURCE_LEN,
            "fixture length {} hit the byte cap; depth was not tested",
            src.len()
        );
        assert_rejected(src);
    }
    assert!(
        parse(&addition).unwrap_err().message.contains("too deep"),
        "addition chain"
    );
    assert!(
        parse(&parens).unwrap_err().message.contains("too deep"),
        "parentheses"
    );
    assert!(
        parse(&bangs).unwrap_err().message.contains("too deep"),
        "prefix !"
    );
    assert!(
        parse(&ternary).unwrap_err().message.contains("too deep"),
        "ternary"
    );
    assert!(
        parse(&minuses).unwrap_err().message.contains("decrement"),
        "adjacent minuses are a decrement, which the lexer rejects"
    );

    // Per-level caps would still accept this: each parenthesis holds a legal
    // run of prefixes, and the runs stack. The path cap rejects the product.
    let open = format!("{}(", "!".repeat((MAX_EXPR_DEPTH - 1) as usize));
    let layered = format!(
        "{}1{}",
        open.repeat((MAX_EXPR_DEPTH - 1) as usize),
        ")".repeat((MAX_EXPR_DEPTH - 1) as usize)
    );
    assert!(layered.len() < MAX_SOURCE_LEN);
    assert_rejected(&layered);
    assert!(
        parse(&layered).unwrap_err().message.contains("too deep"),
        "prefixes nested through parentheses"
    );
}

#[test]
fn source_over_the_length_cap_is_too_long() {
    let src = "1".repeat(MAX_SOURCE_LEN + 1);
    let error = parse(&src).expect_err("length cap");
    assert!(
        error.message.contains("too long"),
        "message was {:?}",
        error.message
    );
}

#[test]
fn just_under_the_limit_parses_on_a_small_stack() {
    let nesting = (MAX_EXPR_DEPTH - 1) as usize;
    let src = format!("{}1{}", "(".repeat(nesting), ")".repeat(nesting));
    let handle = std::thread::Builder::new()
        .name("expr-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            parse(&src).expect("parenthesis nest just under the limit");
        })
        .expect("spawn 256 KiB thread");
    handle.join().expect("small-stack parser panicked");
}

#[test]
fn chain_at_the_limit_compiles_and_evaluates() {
    // A chain of N `+` nodes has N + 1 nodes on its path (the operators and
    // the leftmost leaf). N = MAX - 1 is the longest chain the cap accepts.
    let operators = (MAX_EXPR_DEPTH - 1) as usize;
    let src = format!("{}1", "1+".repeat(operators));
    let mut scope = VarMap::new();
    let code = scope.compile(&src).expect("chain at the limit compiles");
    let value = scope
        .eval(
            &code,
            &StubHost::default(),
            &mut JavaRandom::new(1),
            Limits::default(),
        )
        .expect("chain at the limit evaluates");
    assert_eq!(value.as_number(), Some((operators as f64) + 1.0));
}

/// `v0` ..= `v300`: each earlier variable is `Math.abs` nested as deep as the
/// compiler accepts, around the next variable, and `v300` is `"1"`.
#[test]
fn deep_variable_chain_returns_a_typed_error_on_a_small_stack() {
    let nesting = largest_abs_nesting();
    assert_eq!(nesting, 26, "largest Math.abs nest that still compiles");
    let links = 300;
    let mut names = Vec::with_capacity(links + 1);
    let mut sources = Vec::with_capacity(links + 1);
    for index in 0..=links {
        names.push(format!("v{index}"));
        if index == links {
            sources.push("\"1\"".to_string());
        } else {
            sources.push(abs_around(&format!("v{}", index + 1), nesting));
        }
    }
    let defs: Vec<(&str, VarKind, &str)> = names
        .iter()
        .zip(&sources)
        .map(|(name, source)| (name.as_str(), VarKind::Dollar, source.as_str()))
        .collect();
    let mut scope = VarMap::new();
    scope.define_all(&defs).expect("define the variable chain");
    scope.init();
    let handle = std::thread::Builder::new()
        .name("expr-chain-stack".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            scope.read(
                "v0",
                &StubHost::default(),
                &mut JavaRandom::new(1),
                Limits::new(u32::MAX, usize::MAX),
            )
        })
        .expect("spawn 256 KiB thread");
    let error = handle
        .join()
        .expect("small-stack evaluation panicked")
        .expect_err("deep variable chain must fail");
    assert_eq!(error.kind, EvalErrorKind::Budget);
    assert!(
        error.message.contains("too deep"),
        "message was {:?}",
        error.message
    );
}

/// A chain of [`MAX_DEPTH`] bare references is the longest one the cap still
/// runs. One more read would be rejected. The value shows the cap is not zero.
#[test]
fn variable_chain_just_under_the_cap_evaluates_on_a_small_stack() {
    let count = MAX_DEPTH as usize;
    let mut names = Vec::with_capacity(count);
    let mut sources = Vec::with_capacity(count);
    for index in 0..count {
        names.push(format!("v{index}"));
        if index + 1 == count {
            sources.push("1".to_string());
        } else {
            sources.push(format!("v{}", index + 1));
        }
    }
    let defs: Vec<(&str, VarKind, &str)> = names
        .iter()
        .zip(&sources)
        .map(|(name, source)| (name.as_str(), VarKind::Dollar, source.as_str()))
        .collect();
    let mut scope = VarMap::new();
    scope
        .define_all(&defs)
        .expect("define the chain at the cap");
    scope.init();
    let handle = std::thread::Builder::new()
        .name("expr-chain-under".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            scope.read(
                "v0",
                &StubHost::default(),
                &mut JavaRandom::new(1),
                Limits::new(u32::MAX, usize::MAX),
            )
        })
        .expect("spawn 256 KiB thread");
    let value = handle
        .join()
        .expect("chain at the cap panicked")
        .expect("chain at the cap evaluates");
    assert_eq!(value.as_number(), Some(1.0));
}

fn largest_abs_nesting() -> usize {
    let mut nesting = 0;
    loop {
        let candidate = nesting + 1;
        let src = abs_around("1", candidate);
        let mut scope = VarMap::new();
        if scope.compile(&src).is_err() {
            assert!(nesting > 0, "Math.abs did not compile at any depth");
            return nesting;
        }
        nesting = candidate;
        assert!(
            nesting < MAX_EXPR_DEPTH as usize,
            "abs nesting reached {nesting} without hitting the compiler cap"
        );
    }
}

fn abs_around(inner: &str, nesting: usize) -> String {
    let mut src = inner.to_string();
    for _ in 0..nesting {
        src = format!("Math.abs({src})");
    }
    src
}

#[derive(Debug)]
struct Measured {
    expressions: usize,
    source_len: usize,
    paren: u32,
    ast: u32,
    unary: u32,
    binary: u32,
    postfix: u32,
    /// Longest chain of sibling attributes whose scripts reference each other.
    var_chain: u32,
}

#[test]
fn bundled_and_corpus_expressions_are_under_the_limit() {
    let mut sources = shime_scripts();
    let corpus = corpus_scripts();
    let corpus_len = corpus.len();
    sources.extend(corpus);
    assert!(
        sources.len() > 80,
        "expected the bundled shime scripts, got {}",
        sources.len()
    );
    let mut measured = Measured {
        expressions: 0,
        source_len: 0,
        paren: 0,
        ast: 0,
        unary: 0,
        binary: 0,
        postfix: 0,
        var_chain: shime_variable_chain(),
    };
    for src in &sources {
        measured.expressions += 1;
        measured.source_len = measured.source_len.max(src.len());
        measured.paren = measured.paren.max(paren_depth(src));
        let expr = parse_script(src).unwrap_or_else(|error| panic!("parse {error} :: {src}"));
        let info = walk(&expr);
        measured.ast = measured.ast.max(info.ast);
        measured.unary = measured.unary.max(info.unary);
        measured.binary = measured.binary.max(info.binary);
        measured.postfix = measured.postfix.max(info.postfix);
    }
    assert!(
        corpus_len == 78 || corpus_len == 0,
        "corpus length {corpus_len}"
    );
    assert!(
        measured.source_len < MAX_SOURCE_LEN
            && measured.ast <= MAX_EXPR_DEPTH
            && measured.unary <= MAX_EXPR_DEPTH
            && measured.binary <= MAX_EXPR_DEPTH
            && measured.postfix <= MAX_EXPR_DEPTH
            && measured.paren < MAX_EXPR_DEPTH
            && measured.var_chain < MAX_DEPTH,
        "real expressions must sit under the caps: {measured:?}"
    );
    assert_eq!(
        measured.var_chain, 1,
        "bundled shime sibling-variable chain changed: {measured:?}"
    );
    eprintln!("measured nesting {measured:?} corpus={corpus_len}");
}

struct Walk {
    /// Nodes on the deepest path, counting this node.
    ast: u32,
    /// Longest run of prefix operators in this subtree.
    unary: u32,
    /// Longest run of binary operators in this subtree.
    binary: u32,
    /// Longest run of `.` / call / index in this subtree.
    postfix: u32,
    unary_here: u32,
    binary_here: u32,
    postfix_here: u32,
}

fn leaf() -> Walk {
    Walk {
        ast: 1,
        unary: 0,
        binary: 0,
        postfix: 0,
        unary_here: 0,
        binary_here: 0,
        postfix_here: 0,
    }
}

fn walk(expr: &Expr) -> Walk {
    match &expr.kind {
        ExprKind::Number(_)
        | ExprKind::String(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Ident(_) => leaf(),
        ExprKind::Unary { expr, .. } => {
            let inner = walk(expr);
            let unary_here = 1 + inner.unary_here;
            Walk {
                ast: 1 + inner.ast,
                unary: unary_here.max(inner.unary),
                binary: inner.binary,
                postfix: inner.postfix,
                unary_here,
                binary_here: 0,
                postfix_here: 0,
            }
        }
        ExprKind::Binary { left, right, .. } => {
            let left = walk(left);
            let right = walk(right);
            let binary_here = 1 + left.binary_here.max(right.binary_here);
            Walk {
                ast: 1 + left.ast.max(right.ast),
                unary: left.unary.max(right.unary),
                binary: binary_here.max(left.binary).max(right.binary),
                postfix: left.postfix.max(right.postfix),
                unary_here: 0,
                binary_here,
                postfix_here: 0,
            }
        }
        ExprKind::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let parts = [walk(cond), walk(then_branch), walk(else_branch)];
            Walk {
                ast: 1 + parts.iter().map(|part| part.ast).max().unwrap_or(0),
                unary: parts.iter().map(|part| part.unary).max().unwrap_or(0),
                binary: parts.iter().map(|part| part.binary).max().unwrap_or(0),
                postfix: parts.iter().map(|part| part.postfix).max().unwrap_or(0),
                unary_here: 0,
                binary_here: 0,
                postfix_here: 0,
            }
        }
        ExprKind::Member { object, .. } => extend_postfix(walk(object)),
        ExprKind::Call { callee, args } => {
            let mut here = extend_postfix(walk(callee));
            for arg in args {
                here = max_walk(here, walk(arg));
            }
            here
        }
        ExprKind::Index { object, index } => {
            let here = extend_postfix(walk(object));
            max_walk(here, walk(index))
        }
    }
}

fn extend_postfix(inner: Walk) -> Walk {
    let postfix_here = 1 + inner.postfix_here;
    Walk {
        ast: 1 + inner.ast,
        unary: inner.unary,
        binary: inner.binary,
        postfix: postfix_here.max(inner.postfix),
        unary_here: 0,
        binary_here: 0,
        postfix_here,
    }
}

fn max_walk(left: Walk, right: Walk) -> Walk {
    Walk {
        ast: left.ast.max(right.ast),
        unary: left.unary.max(right.unary),
        binary: left.binary.max(right.binary),
        postfix: left.postfix.max(right.postfix),
        unary_here: left.unary_here.max(right.unary_here),
        binary_here: left.binary_here.max(right.binary_here),
        postfix_here: left.postfix_here.max(right.postfix_here),
    }
}

fn paren_depth(src: &str) -> u32 {
    let mut depth = 0u32;
    let mut max = 0u32;
    let mut chars = src.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '(' | '[' => {
                depth += 1;
                max = max.max(depth);
            }
            ')' | ']' => depth = depth.saturating_sub(1),
            '\'' | '"' => {
                while let Some(inner) = chars.next() {
                    if inner == '\\' {
                        chars.next();
                    } else if inner == ch {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    max
}

fn shime_scripts() -> Vec<String> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime/conf");
    let mut out = Vec::new();
    for name in ["動作.xml", "行動.xml"] {
        let text = fs::read_to_string(dir.join(name)).expect("shime xml");
        out.extend(extract_scripts(&text));
    }
    out
}

fn corpus_scripts() -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/expressions.json");
    if !path.is_file() {
        return Vec::new();
    }
    let text = fs::read_to_string(&path).expect("corpus");
    let items: Vec<serde_json::Value> = serde_json::from_str(&text).expect("corpus json");
    items
        .iter()
        .filter_map(|item| item.get("expr").and_then(|value| value.as_str()))
        .map(str::to_string)
        .collect()
}

fn extract_scripts(xml: &str) -> Vec<String> {
    let xml = strip_comments(xml);
    let mut found = Vec::new();
    let mut index = 0;
    while index < xml.len() {
        let rest = &xml[index..];
        let dollar = rest.find("${").map(|at| index + at);
        let hash = rest.find("#{").map(|at| index + at);
        let Some(start) = dollar.into_iter().chain(hash).min() else {
            break;
        };
        let mut cursor = start + 2;
        let mut depth = 1i32;
        let bytes = xml.as_bytes();
        while cursor < bytes.len() && depth > 0 {
            match bytes[cursor] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            cursor += 1;
        }
        if depth != 0 {
            break;
        }
        let interior = unescape_xml(&xml[start + 2..cursor - 1]);
        let trimmed = interior.trim();
        if !trimmed.is_empty() {
            found.push(trimmed.to_string());
        }
        index = cursor;
    }
    found
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if let Some(rel) = text[index..].find("<!--") {
            out.push_str(&text[index..index + rel]);
            let after = index + rel + 4;
            match text[after..].find("-->") {
                Some(end) => index = after + end + 3,
                None => break,
            }
        } else {
            out.push_str(&text[index..]);
            break;
        }
    }
    out
}

/// Longest run of sibling attributes on one element whose scripts reference
/// each other. A hop is one variable re-entry: `目的地X` reading `ずれ` is 1.
/// Names that are not themselves scripts (plain attributes, host roots) do
/// not continue the chain, because reading them does not re-enter evaluation.
fn shime_variable_chain() -> u32 {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime/conf");
    let mut longest = 0;
    for name in ["動作.xml", "行動.xml"] {
        let text = fs::read_to_string(dir.join(name)).expect("shime xml");
        longest = longest.max(variable_chain(&text));
    }
    longest
}

fn variable_chain(xml: &str) -> u32 {
    let xml = strip_comments(xml);
    let mut longest = 0;
    for tag in opening_tags(&xml) {
        let attrs = tag_attributes(tag);
        let mut scripts: HashMap<String, String> = HashMap::new();
        for (name, value) in &attrs {
            let trimmed = value.trim();
            if trimmed.starts_with("${") || trimmed.starts_with("#{") {
                scripts.insert(name.clone(), value.clone());
            }
        }
        if scripts.len() < 2 {
            continue;
        }
        let mut edges: HashMap<String, Vec<String>> = HashMap::new();
        for (name, value) in &scripts {
            let expr = parse_script(value)
                .unwrap_or_else(|error| panic!("parse {error} :: {name}={value}"));
            let mut refs = Vec::new();
            idents(&expr, &mut refs);
            let mut next = Vec::new();
            for ident in refs {
                if ident != *name && scripts.contains_key(&ident) {
                    next.push(ident);
                }
            }
            next.sort();
            next.dedup();
            edges.insert(name.clone(), next);
        }
        longest = longest.max(longest_hops(&edges));
    }
    longest
}

fn longest_hops(edges: &HashMap<String, Vec<String>>) -> u32 {
    fn walk(name: &str, edges: &HashMap<String, Vec<String>>, stack: &mut HashSet<String>) -> u32 {
        if !stack.insert(name.to_string()) {
            return 0;
        }
        let mut best = 0;
        if let Some(next) = edges.get(name) {
            for child in next {
                best = best.max(1 + walk(child, edges, stack));
            }
        }
        stack.remove(name);
        best
    }
    edges
        .keys()
        .map(|name| walk(name, edges, &mut HashSet::new()))
        .max()
        .unwrap_or(0)
}

fn idents(expr: &Expr, out: &mut Vec<String>) {
    match &expr.kind {
        ExprKind::Ident(name) => out.push(name.clone()),
        ExprKind::Unary { expr, .. } => idents(expr, out),
        ExprKind::Binary { left, right, .. } => {
            idents(left, out);
            idents(right, out);
        }
        ExprKind::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            idents(cond, out);
            idents(then_branch, out);
            idents(else_branch, out);
        }
        ExprKind::Member { object, .. } => idents(object, out),
        ExprKind::Call { callee, args } => {
            idents(callee, out);
            for arg in args {
                idents(arg, out);
            }
        }
        ExprKind::Index { object, index } => {
            idents(object, out);
            idents(index, out);
        }
        ExprKind::Number(_) | ExprKind::String(_) | ExprKind::Bool(_) | ExprKind::Null => {}
    }
}

fn opening_tags(xml: &str) -> Vec<&str> {
    let bytes = xml.as_bytes();
    let mut tags = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'<'
            || xml[index..].starts_with("</")
            || xml[index..].starts_with("<?")
            || xml[index..].starts_with("<!")
        {
            index += 1;
            continue;
        }
        let start = index + 1;
        let mut cursor = start;
        let mut quote = 0u8;
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            if quote != 0 {
                if byte == quote {
                    quote = 0;
                }
            } else if byte == b'"' || byte == b'\'' {
                quote = byte;
            } else if byte == b'>' {
                break;
            }
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }
        tags.push(&xml[start..cursor]);
        index = cursor + 1;
    }
    tags
}

fn tag_attributes(tag: &str) -> Vec<(String, String)> {
    let mut rest = tag.trim().trim_end_matches('/').trim();
    if let Some(split) = rest.find(char::is_whitespace) {
        rest = rest[split..].trim_start();
    } else {
        return Vec::new();
    }
    let mut attrs = Vec::new();
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else {
            break;
        };
        let name = rest[..eq].trim();
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            break;
        }
        rest = rest[eq + 1..].trim_start();
        let Some(quote) = rest.chars().next() else {
            break;
        };
        if quote != '"' && quote != '\'' {
            break;
        }
        rest = &rest[quote.len_utf8()..];
        let Some(end) = rest.find(quote) else {
            break;
        };
        let value = unescape_xml(&rest[..end]);
        attrs.push((name.to_string(), value));
        rest = rest[end + quote.len_utf8()..].trim_start();
    }
    attrs
}

fn unescape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if let Some(rel) = text[index..].find('&') {
            out.push_str(&text[index..index + rel]);
            let amp = index + rel;
            match text[amp + 1..].find(';') {
                None => {
                    out.push_str(&text[amp..]);
                    break;
                }
                Some(semi_rel) => {
                    let entity = &text[amp + 1..amp + 1 + semi_rel];
                    match entity {
                        "lt" => out.push('<'),
                        "gt" => out.push('>'),
                        "amp" => out.push('&'),
                        "quot" => out.push('"'),
                        "apos" => out.push('\''),
                        _ => {
                            out.push('&');
                            out.push_str(entity);
                            out.push(';');
                        }
                    }
                    index = amp + 1 + semi_rel + 1;
                }
            }
        } else {
            out.push_str(&text[index..]);
            break;
        }
    }
    out
}

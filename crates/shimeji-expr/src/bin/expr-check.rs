//! Parse-only checker for pack expressions.

use shimeji_expr::{ParseError, parse_script};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut total = 0usize;
    let mut in_subset = 0usize;
    let mut io_failed = false;

    if args.is_empty() {
        let mut buf = String::new();
        if let Err(error) = io::stdin().read_to_string(&mut buf) {
            eprintln!("failed to read stdin: {error}");
            return ExitCode::from(2);
        }
        check_source(&buf, &mut total, &mut in_subset);
    } else {
        for path in &args {
            match fs::read_to_string(path) {
                Ok(text) => check_source(&text, &mut total, &mut in_subset),
                Err(error) => {
                    eprintln!("failed to read {path}: {error}");
                    io_failed = true;
                }
            }
        }
    }

    if io_failed {
        return ExitCode::from(2);
    }

    let out_of_subset = total - in_subset;
    let percent = if total == 0 {
        0.0
    } else {
        100.0 * out_of_subset as f64 / total as f64
    };
    println!("---");
    println!("total: {total}");
    println!("in-subset: {in_subset}");
    println!("out-of-subset: {out_of_subset}");
    println!("out-of-subset-percent: {percent:.2}");
    if out_of_subset == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn check_source(text: &str, total: &mut usize, in_subset: &mut usize) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if trimmed.starts_with('[') {
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(serde_json::Value::Array(items)) => {
                for item in items {
                    let expr = match item.get("expr").and_then(|value| value.as_str()) {
                        Some(expr) => expr,
                        None => {
                            *total += 1;
                            println!("REJECT missing string field \"expr\" | {item}");
                            continue;
                        }
                    };
                    check_one(expr, total, in_subset);
                }
            }
            Ok(_) => {
                *total += 1;
                println!("REJECT JSON root must be an array of {{\"expr\": ...}} objects");
            }
            Err(error) => {
                *total += 1;
                println!("REJECT invalid JSON: {error}");
            }
        }
        return;
    }
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        check_one(line, total, in_subset);
    }
}

fn check_one(expr: &str, total: &mut usize, in_subset: &mut usize) {
    *total += 1;
    let shown = display_expr(expr);
    match parse_script(expr) {
        Ok(_) => {
            *in_subset += 1;
            println!("OK {shown}");
        }
        Err(ParseError { message, span }) => {
            println!("REJECT {message} at {}..{} | {shown}", span.start, span.end);
        }
    }
}

fn display_expr(src: &str) -> String {
    src.split_whitespace().collect::<Vec<_>>().join(" ")
}

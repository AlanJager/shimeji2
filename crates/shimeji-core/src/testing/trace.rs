//! Trace lines from `tests/scenarios/schema.md`.

use crate::delta::MascotId;
use crate::diagnostics::Diagnostic;
use crate::engine::{Engine, MouseButton};
use crate::model::Character;

use super::scenario::{MouseButtonName, MouseKind, Scenario};

/// The first `T` or `# abort` line where `actual` and `expected` differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    pub tick: u32,
    pub actual: String,
    pub expected: String,
}

/// Trace text and the diagnostics the engine collected while producing it.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub trace: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Replay `scenario` on a fresh engine and return the trace text.
pub fn run(character: &Character, scenario: &Scenario) -> String {
    run_output(character, scenario).trace
}

/// Replay `scenario` and keep the engine diagnostics, including budget trips.
pub fn run_output(character: &Character, scenario: &Scenario) -> RunOutput {
    let mut engine = Engine::new(character, scenario.seed);
    engine.apply(&scenario.snapshot(0));
    engine.spawn();
    let mut out = String::new();
    header(&mut out, scenario);
    for tick in 0..scenario.ticks {
        for event in scenario.mouse_at(i64::from(tick)) {
            let mascot = event.mascot.map(MascotId);
            let button = match event.button {
                MouseButtonName::Left => MouseButton::Left,
                MouseButtonName::Right | MouseButtonName::Other => MouseButton::Right,
            };
            match event.kind {
                MouseKind::Press => engine.press(mascot, button),
                MouseKind::Release => engine.release(mascot, button),
            }
        }
        engine.tick(&scenario.snapshot(i64::from(tick)));
        let draws = engine.rng_draws();
        for frame in engine.frames() {
            push_tick(&mut out, tick, &frame, draws);
        }
    }
    RunOutput {
        trace: out,
        diagnostics: engine.diagnostics().to_vec(),
    }
}

/// Compare data lines. Header lines other than `# abort` are ignored.
pub fn first_divergence(actual: &str, expected: &str) -> Option<Divergence> {
    let actual_lines = data_lines(actual);
    let expected_lines = data_lines(expected);
    let count = actual_lines.len().max(expected_lines.len());
    for index in 0..count {
        let actual_line = actual_lines.get(index).copied();
        let expected_line = expected_lines.get(index).copied();
        if actual_line == expected_line {
            continue;
        }
        let sample = actual_line.or(expected_line).unwrap_or("");
        return Some(Divergence {
            tick: line_tick(sample),
            actual: actual_line.unwrap_or("<eof>").to_string(),
            expected: expected_line.unwrap_or("<eof>").to_string(),
        });
    }
    None
}

fn data_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.starts_with("T\t") || line.starts_with("# abort\t"))
        .collect()
}

fn line_tick(line: &str) -> u32 {
    line.split('\t')
        .nth(1)
        .and_then(|text| text.parse().ok())
        .unwrap_or(0)
}

fn header(out: &mut String, scenario: &Scenario) {
    line(out, &format!("# scenario\t{}", escape(&scenario.name)));
    line(out, &format!("# seed\t{}", scenario.seed));
    line(out, &format!("# ticks\t{}", scenario.ticks));
    line(out, "# engine\tshimeji-core");
    line(out, "# commit\t-");
    line(out, "# script-engine\tshimeji-expr");
    line(out, "# nashorn\t-");
    line(out, "# jdk\t-");
    line(out, "# rng-counter\tshared-after-tick");
}

fn push_tick(out: &mut String, tick: u32, frame: &crate::engine::MascotFrame, draws: u64) {
    let image = frame.image.as_deref().unwrap_or("-");
    let look = if frame.look_right { "true" } else { "false" };
    line(
        out,
        &format!(
            "T\t{tick}\t{}\t{}\t{}\t{}\t{}\t{}\t{look}\t{draws}",
            escape(&frame.id.label()),
            escape(&frame.behavior),
            escape(&frame.action),
            frame.anchor.x,
            frame.anchor.y,
            escape(image),
        ),
    );
}

fn line(out: &mut String, text: &str) {
    out.push_str(text);
    out.push('\n');
}

fn escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out
}

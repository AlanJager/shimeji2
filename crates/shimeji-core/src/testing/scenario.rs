//! Scenario JSON from `tests/scenarios/schema.md`.

use crate::env::{Point, ScreenSample, Snapshot, WindowSample, WorkRect};

#[derive(Debug)]
pub struct ScenarioError {
    pub message: String,
}

impl std::fmt::Display for ScenarioError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ScenarioError {}

#[derive(Debug, Clone)]
pub struct Scenario {
    pub name: String,
    pub seed: i64,
    pub ticks: u32,
    pub image_set: String,
    screens: Vec<ScreenSample>,
    cursor: Vec<CursorFrame>,
    window: Vec<WindowFrame>,
    mouse: Vec<MouseFrame>,
}

#[derive(Debug, Clone, Copy)]
struct CursorFrame {
    tick: i64,
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Copy)]
struct WindowFrame {
    tick: i64,
    visible: bool,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Debug, Clone)]
pub(crate) struct MouseFrame {
    pub tick: i64,
    pub kind: MouseKind,
    pub button: MouseButtonName,
    pub mascot: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MouseKind {
    Press,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MouseButtonName {
    Left,
    Right,
    Other,
}

impl Scenario {
    pub fn parse(text: &str) -> Result<Self, ScenarioError> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|error| err(error.to_string()))?;
        let name = string(&value, "name")?;
        let seed = i64_field(&value, "seed")?;
        let ticks = u32_field(&value, "ticks")?;
        let image_set = value
            .get("imageSet")
            .and_then(|item| item.as_str())
            .unwrap_or("shime")
            .to_string();
        let screens = screens_of(&value)?;
        if screens.is_empty() {
            return Err(err("scenario has no screens"));
        }
        Ok(Self {
            name,
            seed,
            ticks,
            image_set,
            screens,
            cursor: cursors_of(&value)?,
            window: windows_of(&value)?,
            mouse: mice_of(&value)?,
        })
    }

    pub fn snapshot(&self, tick: i64) -> Snapshot {
        Snapshot {
            screens: self.screens.clone(),
            cursor: self.cursor_at(tick),
            active_window: self.window_at(tick),
            multiscreen: true,
        }
    }

    pub(crate) fn mouse_at(&self, tick: i64) -> impl Iterator<Item = &MouseFrame> {
        self.mouse.iter().filter(move |event| event.tick == tick)
    }

    fn cursor_at(&self, tick: i64) -> Point {
        match bracket(&self.cursor, tick, |frame| frame.tick) {
            Bracket::Empty => Point::new(0, 0),
            Bracket::Exact(frame) | Bracket::One(frame) | Bracket::Hold(frame) => {
                Point::new(frame.x, frame.y)
            }
            Bracket::Between(start, end) => Point::new(
                lerp(start.x, end.x, tick, start.tick, end.tick),
                lerp(start.y, end.y, tick, start.tick, end.tick),
            ),
        }
    }

    fn window_at(&self, tick: i64) -> Option<WindowSample> {
        let frame = match bracket(&self.window, tick, |frame| frame.tick) {
            Bracket::Empty => return None,
            Bracket::Exact(frame) | Bracket::One(frame) | Bracket::Hold(frame) => *frame,
            Bracket::Between(start, end) => {
                if !start.visible || start.visible != end.visible {
                    *start
                } else {
                    WindowFrame {
                        tick,
                        visible: true,
                        x: lerp(start.x, end.x, tick, start.tick, end.tick),
                        y: lerp(start.y, end.y, tick, start.tick, end.tick),
                        width: lerp(start.width, end.width, tick, start.tick, end.tick),
                        height: lerp(start.height, end.height, tick, start.tick, end.tick),
                    }
                }
            }
        };
        Some(WindowSample {
            visible: frame.visible,
            id: 0,
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
        })
    }
}

enum Bracket<'a, T> {
    Empty,
    One(&'a T),
    Exact(&'a T),
    Hold(&'a T),
    Between(&'a T, &'a T),
}

fn bracket<'a, T>(frames: &'a [T], tick: i64, at: impl Fn(&T) -> i64) -> Bracket<'a, T> {
    if frames.is_empty() {
        return Bracket::Empty;
    }
    if frames.len() == 1 {
        return Bracket::One(&frames[0]);
    }
    if tick <= at(&frames[0]) {
        return Bracket::Hold(&frames[0]);
    }
    if tick >= at(frames.last().expect("non-empty")) {
        return Bracket::Hold(frames.last().expect("non-empty"));
    }
    for pair in frames.windows(2) {
        let start_tick = at(&pair[0]);
        let end_tick = at(&pair[1]);
        if tick == start_tick {
            return Bracket::Exact(&pair[0]);
        }
        if tick == end_tick {
            return Bracket::Exact(&pair[1]);
        }
        if start_tick < tick && tick < end_tick {
            return Bracket::Between(&pair[0], &pair[1]);
        }
    }
    Bracket::Hold(frames.last().expect("non-empty"))
}

fn lerp(start: i32, end: i32, tick: i64, tick_start: i64, tick_end: i64) -> i32 {
    let span = tick_end - tick_start;
    if span == 0 {
        return start;
    }
    let delta = i64::from(end) - i64::from(start);
    let numer = delta * (tick - tick_start);
    (i64::from(start) + numer / span) as i32
}

fn screens_of(value: &serde_json::Value) -> Result<Vec<ScreenSample>, ScenarioError> {
    let Some(items) = value.get("screens").and_then(|item| item.as_array()) else {
        return Err(err("screens must be an array"));
    };
    items.iter().map(screen_of).collect()
}

fn screen_of(value: &serde_json::Value) -> Result<ScreenSample, ScenarioError> {
    let id = string(value, "id")?;
    let x = i32_field(value, "x")?;
    let y = i32_field(value, "y")?;
    let width = i32_field(value, "width")?;
    let height = i32_field(value, "height")?;
    let work = match value.get("work") {
        Some(work) => Some(WorkRect {
            x: i32_field(work, "x")?,
            y: i32_field(work, "y")?,
            width: i32_field(work, "width")?,
            height: i32_field(work, "height")?,
        }),
        None => None,
    };
    Ok(ScreenSample {
        id,
        x,
        y,
        width,
        height,
        work,
    })
}

fn cursors_of(value: &serde_json::Value) -> Result<Vec<CursorFrame>, ScenarioError> {
    let Some(items) = value.get("cursor").and_then(|item| item.as_array()) else {
        return Ok(Vec::new());
    };
    let mut frames = Vec::new();
    for item in items {
        frames.push(CursorFrame {
            tick: i64_field(item, "tick")?,
            x: i32_field(item, "x")?,
            y: i32_field(item, "y")?,
        });
    }
    frames.sort_by_key(|frame| frame.tick);
    Ok(frames)
}

fn windows_of(value: &serde_json::Value) -> Result<Vec<WindowFrame>, ScenarioError> {
    let Some(items) = value.get("activeWindow").and_then(|item| item.as_array()) else {
        return Ok(Vec::new());
    };
    let mut frames = Vec::new();
    for item in items {
        let visible = item
            .get("visible")
            .and_then(|flag| flag.as_bool())
            .unwrap_or(true);
        frames.push(WindowFrame {
            tick: i64_field(item, "tick")?,
            visible,
            x: item
                .get("x")
                .map(|_| i32_field(item, "x"))
                .transpose()?
                .unwrap_or(0),
            y: item
                .get("y")
                .map(|_| i32_field(item, "y"))
                .transpose()?
                .unwrap_or(0),
            width: item
                .get("width")
                .map(|_| i32_field(item, "width"))
                .transpose()?
                .unwrap_or(0),
            height: item
                .get("height")
                .map(|_| i32_field(item, "height"))
                .transpose()?
                .unwrap_or(0),
        });
    }
    frames.sort_by_key(|frame| frame.tick);
    Ok(frames)
}

fn mice_of(value: &serde_json::Value) -> Result<Vec<MouseFrame>, ScenarioError> {
    let Some(items) = value.get("mouse").and_then(|item| item.as_array()) else {
        return Ok(Vec::new());
    };
    let mut events = Vec::new();
    for item in items {
        let kind = match string(item, "type")?.as_str() {
            "press" => MouseKind::Press,
            "release" => MouseKind::Release,
            other => return Err(err(format!("unknown mouse type {other}"))),
        };
        let button = match string(item, "button")?.as_str() {
            "left" => MouseButtonName::Left,
            "right" => MouseButtonName::Right,
            _ => MouseButtonName::Other,
        };
        let mascot = match item.get("mascot").and_then(|value| value.as_str()) {
            Some(label) => Some(parse_mascot(label)?),
            None => None,
        };
        events.push(MouseFrame {
            tick: i64_field(item, "tick")?,
            kind,
            button,
            mascot,
        });
    }
    Ok(events)
}

fn parse_mascot(label: &str) -> Result<u64, ScenarioError> {
    let Some(rest) = label.strip_prefix("mascot") else {
        return Err(err(format!("mascot id {label} is not mascotN")));
    };
    rest.parse::<u64>()
        .map_err(|_| err(format!("mascot id {label} is not mascotN")))
}

fn string(value: &serde_json::Value, key: &str) -> Result<String, ScenarioError> {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .map(str::to_string)
        .ok_or_else(|| err(format!("{key} must be a string")))
}

fn i32_field(value: &serde_json::Value, key: &str) -> Result<i32, ScenarioError> {
    let number = value
        .get(key)
        .and_then(|item| item.as_i64())
        .ok_or_else(|| err(format!("{key} must be an integer")))?;
    i32::try_from(number).map_err(|_| err(format!("{key} does not fit in i32")))
}

fn i64_field(value: &serde_json::Value, key: &str) -> Result<i64, ScenarioError> {
    value
        .get(key)
        .and_then(|item| item.as_i64())
        .ok_or_else(|| err(format!("{key} must be an integer")))
}

fn u32_field(value: &serde_json::Value, key: &str) -> Result<u32, ScenarioError> {
    let number = i64_field(value, key)?;
    u32::try_from(number).map_err(|_| err(format!("{key} must be positive")))
}

fn err(message: impl Into<String>) -> ScenarioError {
    ScenarioError {
        message: message.into(),
    }
}

//! Shared character fixtures for the engine tests.

#![allow(dead_code)]

use shimeji_core::env::{Point, ScreenSample, Snapshot, WindowSample};
use shimeji_core::model::{
    Action, ActionKind, Animation, Behavior, BehaviorRef, Character, Constant, Information,
    NextList, OrderedMap, Param, Pose, Schema, ScriptSource,
};
use shimeji_core::{Engine, MascotId};

pub fn param(key: &str, raw: &str) -> Param {
    let (value, error) = ScriptSource::parse_attribute(raw);
    assert!(error.is_none(), "{raw}");
    Param {
        key: key.to_string(),
        source_name: key.to_string(),
        value,
        line: 1,
    }
}

pub fn script(raw: &str) -> ScriptSource {
    let (value, error) = ScriptSource::parse_attribute(raw);
    assert!(error.is_none(), "{raw}");
    value
}

pub fn params(pairs: &[(&str, &str)]) -> OrderedMap<Param> {
    let mut map = OrderedMap::new();
    for (key, raw) in pairs {
        map.insert(*key, param(key, raw));
    }
    map
}

fn pose(image: &str) -> Pose {
    Pose {
        image_source: Some(image.to_string()),
        image: None,
        image_right_source: None,
        image_right: None,
        right_sibling: None,
        anchor_x: 0,
        anchor_y: 0,
        velocity_x: 0,
        velocity_y: 0,
        duration_ticks: 8,
        sound: None,
        volume: None,
        line: 1,
    }
}

pub fn stay(name: &str, kind: ActionKind, pairs: &[(&str, &str)]) -> Action {
    Action {
        name: Some(name.to_string()),
        schema: Schema::English,
        kind,
        degraded_from: None,
        params: params(pairs),
        animations: vec![Animation {
            condition: None,
            is_turn: false,
            poses: vec![pose("/body.png")],
            hotspots: Vec::new(),
            line: 1,
        }],
        children: Vec::new(),
        line: 1,
    }
}

pub fn behavior(
    name: &str,
    action: &str,
    frequency: i32,
    conditions: &[&str],
    next: Option<(bool, Vec<BehaviorRef>)>,
) -> Behavior {
    let next_lists = next
        .as_ref()
        .map(|(add, refs)| {
            vec![NextList {
                add: *add,
                nodes: Vec::new(),
                refs: refs.clone(),
                line: 1,
            }]
        })
        .unwrap_or_default();
    let next_additive = next.as_ref().map(|(add, _)| *add).unwrap_or(true);
    Behavior {
        name: name.to_string(),
        action: action.to_string(),
        frequency,
        hidden: false,
        toggleable: false,
        conditions: conditions.iter().copied().map(script).collect(),
        params: OrderedMap::new(),
        next_lists,
        next_additive,
        line: 1,
    }
}

pub fn behavior_lists(name: &str, action: &str, frequency: i32, lists: Vec<NextList>) -> Behavior {
    let next_additive = lists.last().map(|list| list.add).unwrap_or(true);
    Behavior {
        name: name.to_string(),
        action: action.to_string(),
        frequency,
        hidden: false,
        toggleable: false,
        conditions: Vec::new(),
        params: OrderedMap::new(),
        next_lists: lists,
        next_additive,
        line: 1,
    }
}

pub fn reference(name: &str, frequency: i32, conditions: &[&str]) -> BehaviorRef {
    BehaviorRef {
        name: name.to_string(),
        frequency,
        conditions: conditions.iter().copied().map(script).collect(),
        params: OrderedMap::new(),
        line: 1,
    }
}

pub fn list(add: bool, refs: Vec<BehaviorRef>) -> NextList {
    NextList {
        add,
        nodes: Vec::new(),
        refs,
        line: 1,
    }
}

pub fn program(
    actions: Vec<(&str, Action)>,
    behaviors: Vec<Behavior>,
    constants: &[(&str, &str)],
) -> Character {
    let mut action_map = OrderedMap::new();
    for (name, action) in actions {
        action_map.insert(name, action);
    }
    let mut behavior_map = OrderedMap::new();
    for behavior in behaviors {
        behavior_map.insert(behavior.name.clone(), behavior);
    }
    let mut constant_map = OrderedMap::new();
    for (name, value) in constants {
        constant_map.insert(
            *name,
            Constant {
                value: (*value).to_string(),
                line: 1,
            },
        );
    }
    Character {
        name: "test".to_string(),
        schema: Schema::English,
        behavior_schema: Schema::English,
        image_root: String::new(),
        actions_file: "actions.xml".to_string(),
        behaviors_file: "behaviors.xml".to_string(),
        info_file: None,
        constants: constant_map,
        actions: action_map,
        behaviors: behavior_map,
        behavior_tree: Vec::new(),
        info: Information::default(),
    }
}

/// Screen that contains the spawn anchor `(-4000, -4000)`.
pub fn wide_screen() -> Snapshot {
    Snapshot {
        screens: vec![ScreenSample {
            id: "main".to_string(),
            x: -4500,
            y: 0,
            width: 6000,
            height: 800,
            work: None,
        }],
        cursor: Point::new(0, 0),
        active_window: None,
        multiscreen: true,
    }
}

pub fn desktop() -> Snapshot {
    Snapshot {
        screens: vec![ScreenSample {
            id: "main".to_string(),
            x: 0,
            y: 0,
            width: 1280,
            height: 800,
            work: None,
        }],
        cursor: Point::new(0, 0),
        active_window: None,
        multiscreen: true,
    }
}

pub fn window_at(x: i32, y: i32) -> Snapshot {
    Snapshot {
        screens: vec![ScreenSample {
            id: "main".to_string(),
            x: 0,
            y: 0,
            width: 800,
            height: 600,
            work: None,
        }],
        cursor: Point::new(0, 0),
        active_window: Some(WindowSample {
            visible: true,
            id: 0,
            x,
            y,
            width: 200,
            height: 150,
        }),
        multiscreen: true,
    }
}

pub fn start(character: &Character) -> Engine {
    Engine::new(character, 1)
}

pub fn behavior_name(engine: &Engine, id: MascotId) -> String {
    engine
        .frames()
        .into_iter()
        .find(|frame| frame.id == id)
        .map(|frame| frame.behavior)
        .unwrap_or_else(|| panic!("no {id}"))
}

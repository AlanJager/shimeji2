//! Lazy memo cells for `${…}` and `#{…}`.
//!
//! A cell is cleared by bumping a generation, not by wiping the slot. `${}`
//! watches the `init` generation. `#{}` also watches the `next` generation.
//! A script result is stored whatever it is, including null and undefined, so
//! a variable runs at most once until the next clear. Reading a variable that
//! is already on the stack returns a cycle error.

use std::collections::HashMap;

use crate::eval::Node;
use crate::value::Pool;
use crate::value::{Value, to_i32, to_number};

/// How long a script cell keeps its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    /// Cleared at [`VarMap::init`]. Evaluated on the first read after that.
    Dollar,
    /// Cleared at [`VarMap::init`] and again at [`VarMap::next`].
    Hash,
}

#[derive(Debug)]
pub struct VarMap {
    slots: Vec<Slot>,
    names: HashMap<String, u32>,
    pool: Pool,
    busy: Vec<bool>,
    init_gen: u32,
    next_gen: u32,
}

#[derive(Debug)]
struct Slot {
    name: String,
    body: Body,
}

#[derive(Debug)]
enum Body {
    Empty,
    Const(Value),
    Script {
        kind: VarKind,
        node: Option<Node>,
        init_seen: u32,
        next_seen: u32,
        cached: Option<Value>,
    },
}

impl Default for VarMap {
    fn default() -> Self {
        Self::new()
    }
}

impl VarMap {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            names: HashMap::new(),
            pool: Pool::default(),
            busy: Vec::new(),
            init_gen: 0,
            next_gen: 0,
        }
    }

    pub fn bind_number(&mut self, name: &str, value: f64) {
        self.set_const(name, Value::Number(value));
    }

    pub fn bind_bool(&mut self, name: &str, value: bool) {
        self.set_const(name, Value::Bool(value));
    }

    pub fn bind_str(&mut self, name: &str, value: &str) {
        let id = self.pool.intern(value);
        self.set_const(name, Value::Str(id));
    }

    pub fn bind_null(&mut self, name: &str) {
        self.set_const(name, Value::Null);
    }

    /// Reserve a slot so a script compiled later can refer to this name.
    /// Existing slots are left alone.
    pub fn reserve(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.names.get(name) {
            return id;
        }
        let id = self.slots.len() as u32;
        self.names.insert(name.to_string(), id);
        self.slots.push(Slot {
            name: name.to_string(),
            body: Body::Empty,
        });
        self.busy.push(false);
        id
    }

    pub fn slot_index(&self, name: &str) -> Option<u32> {
        self.names.get(name).copied()
    }

    /// Drop every `${}` and `#{}` cache. Does not evaluate anything.
    pub fn init(&mut self) {
        self.init_gen = self.init_gen.wrapping_add(1);
    }

    /// Drop `#{}` caches for this action. `${}` cells stay cached.
    pub fn next(&mut self) {
        self.next_gen = self.next_gen.wrapping_add(1);
    }

    pub fn text(&self, value: Value) -> Option<&str> {
        match value {
            Value::Str(id) => Some(self.pool.text(id)),
            _ => None,
        }
    }

    /// `ToNumber`, then Java's `int` narrowing conversion.
    pub fn to_java_int(&self, value: Value) -> i32 {
        to_i32(to_number(value, &self.pool))
    }

    pub(crate) fn intern(&mut self, text: &str) -> u32 {
        self.pool.intern(text)
    }

    pub(crate) fn intern_owned(&mut self, text: String) -> u32 {
        self.pool.intern_owned(text)
    }

    pub(crate) fn pool(&self) -> &Pool {
        &self.pool
    }

    pub(crate) fn slot_name(&self, slot: usize) -> &str {
        &self.slots[slot].name
    }

    pub(crate) fn is_busy(&self, slot: usize) -> bool {
        self.busy[slot]
    }

    pub(crate) fn set_busy(&mut self, slot: usize, busy: bool) {
        self.busy[slot] = busy;
    }

    /// `Some` when the cell can be returned without running its script.
    pub(crate) fn cached(&self, slot: usize) -> Option<Value> {
        match &self.slots[slot].body {
            Body::Const(value) => Some(*value),
            Body::Script {
                kind,
                init_seen,
                next_seen,
                cached,
                ..
            } => {
                let init_ok = *init_seen == self.init_gen;
                let next_ok = *kind == VarKind::Dollar || *next_seen == self.next_gen;
                if init_ok && next_ok { *cached } else { None }
            }
            Body::Empty => None,
        }
    }

    pub(crate) fn take_script(&mut self, slot: usize) -> Option<Node> {
        match &mut self.slots[slot].body {
            Body::Script { node, .. } => node.take(),
            _ => None,
        }
    }

    pub(crate) fn put_script(&mut self, slot: usize, node: Node) {
        if let Body::Script {
            node: slot_node, ..
        } = &mut self.slots[slot].body
        {
            *slot_node = Some(node);
        }
    }

    /// Remember a script result for the current generations, null included.
    pub(crate) fn store_cache(&mut self, slot: usize, value: Value) {
        let init_gen = self.init_gen;
        let next_gen = self.next_gen;
        if let Body::Script {
            init_seen,
            next_seen,
            cached,
            ..
        } = &mut self.slots[slot].body
        {
            *init_seen = init_gen;
            *next_seen = next_gen;
            *cached = Some(value);
        }
    }

    pub(crate) fn set_script(&mut self, slot: usize, kind: VarKind, node: Node) {
        self.slots[slot].body = Body::Script {
            kind,
            node: Some(node),
            init_seen: 0,
            next_seen: 0,
            cached: None,
        };
        self.busy[slot] = false;
    }

    fn set_const(&mut self, name: &str, value: Value) {
        let slot = self.ensure(name);
        self.slots[slot].body = Body::Const(value);
        self.busy[slot] = false;
    }

    fn ensure(&mut self, name: &str) -> usize {
        if let Some(id) = self.slot_index(name) {
            return id as usize;
        }
        self.reserve(name) as usize
    }
}

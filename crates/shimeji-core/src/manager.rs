//! Live mascots and the spawn/removal queues.
//!
//! `flush` matches `Manager.tick`: admit every queued spawn, then drop every
//! queued removal. A dispose during `mousePressed` is therefore gone before
//! that tick's trace. A dispose during `behavior.next` is still listed for
//! the current trace and gone on the next tick.

use std::collections::HashSet;

use crate::behavior::ActiveBehavior;
use crate::delta::MascotId;
use crate::mascot::Mascot;

pub(crate) struct Slot {
    pub id: MascotId,
    pub mascot: Mascot,
    pub behavior: ActiveBehavior,
}

pub(crate) struct Manager {
    live: Vec<Slot>,
    pending_add: Vec<Slot>,
    pending_remove: HashSet<u64>,
}

impl Manager {
    pub(crate) fn new() -> Self {
        Self {
            live: Vec::new(),
            pending_add: Vec::new(),
            pending_remove: HashSet::new(),
        }
    }

    pub(crate) fn population(&self) -> usize {
        self.live.len() + self.pending_add.len()
    }

    pub(crate) fn live_len(&self) -> usize {
        self.live.len()
    }

    pub(crate) fn enqueue(&mut self, slot: Slot) {
        self.pending_add.push(slot);
    }

    /// Queue a removal. A mascot that has not been admitted yet never appears.
    pub(crate) fn dispose(&mut self, id: MascotId) {
        self.pending_add.retain(|slot| slot.id != id);
        if self.live.iter().any(|slot| slot.id == id) {
            self.pending_remove.insert(id.0);
        }
    }

    pub(crate) fn flush(&mut self) {
        self.live.append(&mut self.pending_add);
        if !self.pending_remove.is_empty() {
            let doomed = std::mem::take(&mut self.pending_remove);
            self.live.retain(|slot| !doomed.contains(&slot.id.0));
        }
    }

    pub(crate) fn live(&self) -> &[Slot] {
        &self.live
    }

    pub(crate) fn live_mut(&mut self) -> &mut [Slot] {
        &mut self.live
    }

    pub(crate) fn contains_live(&self, id: MascotId) -> bool {
        self.live.iter().any(|slot| slot.id == id)
    }

    pub(crate) fn first_live(&self) -> Option<MascotId> {
        self.live.first().map(|slot| slot.id)
    }

    pub(crate) fn slot_mut(&mut self, id: MascotId) -> Option<&mut Slot> {
        self.live.iter_mut().find(|slot| slot.id == id)
    }

    pub(crate) fn slot_any_mut(&mut self, id: MascotId) -> Option<&mut Slot> {
        if let Some(slot) = self.live.iter_mut().find(|slot| slot.id == id) {
            return Some(slot);
        }
        self.pending_add.iter_mut().find(|slot| slot.id == id)
    }

    pub(crate) fn behavior_name(&self, id: MascotId) -> Option<&str> {
        self.live
            .iter()
            .chain(self.pending_add.iter())
            .find(|slot| slot.id == id)
            .map(|slot| slot.behavior.name.as_str())
    }
}

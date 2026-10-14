//! What changed for the shell's dirty-only commit.
//!
//! One record per mascot that ticked, in manager-list order. `move_frozen`
//! is outside the tick and returns its own record.

/// Stable id. Display form is `mascot{id}`, matching `Mascot.toString`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MascotId(pub u64);

impl MascotId {
    pub fn label(self) -> String {
        format!("mascot{}", self.0)
    }
}

impl std::fmt::Display for MascotId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "mascot{}", self.0)
    }
}

/// Anchor and image-key changes for one mascot over one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MascotDelta {
    pub id: MascotId,
    pub anchor_changed: bool,
    pub image_changed: bool,
}

/// Deltas for the mascots that ticked, in list order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickDelta {
    pub mascots: Vec<MascotDelta>,
}

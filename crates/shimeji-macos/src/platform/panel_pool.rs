//! Pre-warmed slack panels. A panel is shown only after its first commit.

use objc2::MainThreadMarker;

use super::input::InputQueue;
use super::panel::Panel;
use shimeji_core::delta::MascotId;

pub struct PanelPool {
    idle: Vec<Panel>,
}

impl PanelPool {
    pub fn prewarm(
        mtm: MainThreadMarker,
        count: usize,
        size: f64,
        queue: &InputQueue,
        scale: f64,
    ) -> Self {
        let mut idle = Vec::with_capacity(count);
        for _ in 0..count {
            let panel = Panel::create(mtm, size, MascotId(0), queue.clone(), scale);
            panel.hide();
            idle.push(panel);
        }
        Self { idle }
    }

    pub fn take(
        &mut self,
        mtm: MainThreadMarker,
        size: f64,
        mascot: MascotId,
        queue: InputQueue,
        scale: f64,
    ) -> Panel {
        self.idle
            .pop()
            .unwrap_or_else(|| Panel::create(mtm, size, mascot, queue, scale))
    }

    pub fn give(&mut self, panel: Panel) {
        self.idle.push(panel);
    }
}

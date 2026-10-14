//! Mouse down/drag/up on a pet. Right-click and Control-click open the menu.
//!
//! While the clock is stopped, `deliver_now` wakes the shell on the pushing
//! thread (the main thread) so a frozen pet can still be dragged.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSView};
use objc2_foundation::NSRect;
use shimeji_core::delta::MascotId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    LeftDown(MascotId),
    LeftUp(MascotId),
    RightDown(MascotId),
    /// Coalesced to one pending event per mascot. The shell reads the cursor
    /// when it drains, not when the view posted the drag.
    Drag(MascotId),
}

type InputWake = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone, Default)]
pub struct InputQueue {
    inner: Arc<Mutex<Vec<InputEvent>>>,
    deliver_now: Arc<AtomicBool>,
    wake: Arc<Mutex<Option<InputWake>>>,
}

impl InputQueue {
    pub fn set_deliver_now(&self, on: bool) {
        self.deliver_now.store(on, Ordering::Release);
    }

    pub fn set_wake(&self, wake: InputWake) {
        *self.wake.lock().expect("input wake") = Some(wake);
    }

    pub fn push(&self, event: InputEvent) {
        let wake = {
            let mut guard = self.inner.lock().expect("input queue");
            match event {
                InputEvent::Drag(id) => {
                    let queued = guard
                        .iter()
                        .any(|item| matches!(item, InputEvent::Drag(existing) if *existing == id));
                    if !queued {
                        guard.push(event);
                    }
                }
                other => guard.push(other),
            }
            if self.deliver_now.load(Ordering::Acquire) {
                self.wake.lock().ok().and_then(|slot| slot.clone())
            } else {
                None
            }
        };
        if let Some(wake) = wake {
            wake();
        }
    }

    pub fn drain(&self) -> Vec<InputEvent> {
        let mut guard = self.inner.lock().expect("input queue");
        std::mem::take(&mut *guard)
    }
}

pub struct PetIvars {
    pub mascot: Cell<u64>,
    pub pointer_hit: Cell<bool>,
    pub dragging: Cell<bool>,
    pub queue: InputQueue,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PetIvars]
    pub struct PetView;

    impl PetView {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if !self.ivars().pointer_hit.get() { return; }
            if event.modifierFlags().contains(NSEventModifierFlags::Control) {
                self.emit(InputEvent::RightDown(self.mascot()));
                return;
            }
            self.ivars().dragging.set(true);
            self.emit(InputEvent::LeftDown(self.mascot()));
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, _event: &NSEvent) {
            self.emit(InputEvent::Drag(self.mascot()));
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            if !self.ivars().dragging.replace(false) { return; }
            self.emit(InputEvent::LeftUp(self.mascot()));
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _event: &NSEvent) {
            if !self.ivars().pointer_hit.get() { return; }
            self.emit(InputEvent::RightDown(self.mascot()));
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn mouse_down_can_move_window(&self) -> bool {
            false
        }
    }

    unsafe impl NSObjectProtocol for PetView {}
);

impl PetView {
    pub fn new(
        mtm: objc2::MainThreadMarker,
        frame: NSRect,
        mascot: MascotId,
        queue: InputQueue,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PetIvars {
            mascot: Cell::new(mascot.0),
            pointer_hit: Cell::new(false),
            dragging: Cell::new(false),
            queue,
        });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    pub fn set_mascot(&self, mascot: MascotId) {
        self.ivars().mascot.set(mascot.0);
        self.ivars().dragging.set(false);
    }

    pub fn set_pointer_hit(&self, hit: bool) {
        self.ivars().pointer_hit.set(hit);
    }

    pub fn dragging(&self) -> bool {
        self.ivars().dragging.get()
    }

    fn mascot(&self) -> MascotId {
        MascotId(self.ivars().mascot.get())
    }

    fn emit(&self, event: InputEvent) {
        self.ivars().queue.push(event);
    }
}

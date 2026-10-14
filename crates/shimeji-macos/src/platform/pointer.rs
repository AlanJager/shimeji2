//! Event-driven cursor updates. No timer is needed for a paused or still pet.

use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask};

pub struct PointerMonitors {
    tokens: Vec<Retained<AnyObject>>,
}

impl PointerMonitors {
    pub fn new(update: impl Fn() + 'static) -> Self {
        let update: Rc<dyn Fn()> = Rc::new(update);
        let mask = NSEventMask::MouseMoved
            | NSEventMask::LeftMouseDragged
            | NSEventMask::RightMouseDragged
            | NSEventMask::OtherMouseDragged
            | NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseUp
            | NSEventMask::RightMouseDown
            | NSEventMask::RightMouseUp;
        let global_update = Rc::clone(&update);
        let global = RcBlock::new(move |_event: NonNull<NSEvent>| global_update());
        let local = RcBlock::new(move |event: NonNull<NSEvent>| {
            update();
            event.as_ptr()
        });
        let mut tokens = Vec::new();
        if let Some(token) = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &global) {
            tokens.push(token);
        }
        // The original NSEvent pointer is passed through unchanged.
        if let Some(token) =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &local) }
        {
            tokens.push(token);
        }
        Self { tokens }
    }
}

impl Drop for PointerMonitors {
    fn drop(&mut self) {
        for token in &self.tokens {
            unsafe { NSEvent::removeMonitor(token) };
        }
    }
}

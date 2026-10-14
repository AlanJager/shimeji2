//! One GCD timer, 40 ms interval, 1 ms leeway. Recreated on main-screen change.

use std::ffi::c_void;

use objc2::runtime::AnyObject;

const INTERVAL_NS: u64 = 40_000_000;
const LEEWAY_NS: u64 = 1_000_000;

pub struct GcdTimer {
    source: *mut c_void,
}

impl GcdTimer {
    /// `target` must outlive the timer. The C handler receives that pointer.
    pub fn start(target: *const AnyObject, handler: unsafe extern "C" fn(*mut c_void)) -> Self {
        unsafe extern "C" {
            static _dispatch_source_type_timer: u8;
            static _dispatch_main_q: u8;
            fn dispatch_source_create(
                type_: *const u8,
                handle: usize,
                mask: usize,
                queue: *mut c_void,
            ) -> *mut c_void;
            fn dispatch_source_set_timer(
                source: *mut c_void,
                start: u64,
                interval: u64,
                leeway: u64,
            );
            fn dispatch_source_set_event_handler_f(
                source: *mut c_void,
                handler: Option<unsafe extern "C" fn(*mut c_void)>,
            );
            fn dispatch_set_context(object: *mut c_void, context: *mut c_void);
            fn dispatch_resume(object: *mut c_void);
        }
        let source = unsafe {
            dispatch_source_create(
                &raw const _dispatch_source_type_timer,
                0,
                0,
                &raw const _dispatch_main_q as *mut c_void,
            )
        };
        assert!(!source.is_null(), "dispatch_source_create failed");
        unsafe {
            dispatch_set_context(source, target.cast_mut().cast());
            dispatch_source_set_timer(source, 0, INTERVAL_NS, LEEWAY_NS);
            dispatch_source_set_event_handler_f(source, Some(handler));
            dispatch_resume(source);
        }
        Self { source }
    }
}

impl Drop for GcdTimer {
    fn drop(&mut self) {
        unsafe {
            dispatch_source_cancel(self.source);
            dispatch_release(self.source);
        }
    }
}

unsafe extern "C" {
    fn dispatch_source_cancel(source: *mut c_void);
    fn dispatch_release(object: *mut c_void);
}

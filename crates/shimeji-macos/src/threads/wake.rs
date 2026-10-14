//! Wake the main queue from a worker. The clock already runs on main; this is
//! for notifications that arrive while the clock is stopped (part B).

use std::ffi::c_void;

/// Run `job` on the main GCD queue.
pub fn on_main<F>(job: F)
where
    F: FnOnce() + Send + 'static,
{
    let boxed = Box::into_raw(Box::new(job));
    unsafe {
        dispatch_async_f(
            std::ptr::from_ref(&_dispatch_main_q).cast_mut().cast(),
            boxed.cast(),
            Some(run::<F>),
        );
    }
}

unsafe extern "C" fn run<F: FnOnce()>(context: *mut c_void) {
    let job = unsafe { Box::from_raw(context.cast::<F>()) };
    job();
}

unsafe extern "C" {
    static _dispatch_main_q: u8;
    fn dispatch_async_f(
        queue: *mut c_void,
        context: *mut c_void,
        work: Option<unsafe extern "C" fn(*mut c_void)>,
    );
}

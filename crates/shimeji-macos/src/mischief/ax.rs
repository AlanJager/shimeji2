//! Accessibility calls. This module is the only place that touches AXUIElement.
//! The worker thread is the only caller.

use std::cell::RefCell;
use std::ptr::NonNull;

use objc2_app_kit::NSRunningApplication;
use objc2_application_services::{AXError, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{CFArray, CFRetained, CFString, CFType, CGFloat, CGPoint, CGSize};
use shimeji_app::journal::{AX_STANDARD_WINDOW, AxWindow, Bounds};

use super::ax_worker::{AxBackend, MoveMiss};
use crate::platform::window_env;

const MESSAGING_TIMEOUT: f32 = 0.1;

// The `kAX*Attribute` names are `CFSTR` macros, not exported symbols.
fn attr(name: &str) -> CFRetained<CFString> {
    CFString::from_str(name)
}

unsafe extern "C" {
    fn AXUIElementIsAttributeSettable(
        element: &AXUIElement,
        attribute: &CFString,
        settable: *mut u8,
    ) -> AXError;
}

pub struct MacAx {
    system: RefCell<Option<CFRetained<AXUIElement>>>,
    elements: RefCell<Vec<CFRetained<AXUIElement>>>,
}

impl MacAx {
    pub fn new() -> Self {
        Self {
            system: RefCell::new(None),
            elements: RefCell::new(Vec::new()),
        }
    }
}

// `AXUIElement` is reference-counted and not marked `Send`. This value is
// moved onto the worker and never shared: `prepare` and every later call run
// on that thread.
unsafe impl Send for MacAx {}

impl AxBackend for MacAx {
    fn prepare(&self) {
        let element = unsafe { AXUIElement::new_system_wide() };
        let _ = unsafe { element.set_messaging_timeout(MESSAGING_TIMEOUT) };
        *self.system.borrow_mut() = Some(element);
    }

    fn windows(&self, pid: i32, with_title: bool) -> Vec<AxWindow> {
        let mut cached = self.elements.borrow_mut();
        cached.clear();
        if pid <= 0 {
            return Vec::new();
        }
        let app = unsafe { AXUIElement::new_application(pid) };
        let _ = unsafe { app.set_messaging_timeout(MESSAGING_TIMEOUT) };
        let windows_attr = attr("AXWindows");
        let Some(array) = copy_array(&app, &windows_attr) else {
            return Vec::new();
        };
        let count = usize::try_from(array.count()).unwrap_or(0);
        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            let ptr = unsafe { array.value_at_index(index as isize) };
            let Some(ptr) = NonNull::new(ptr as *mut AXUIElement) else {
                continue;
            };
            let element = unsafe { CFRetained::retain(ptr) };
            let Some(window) = describe(&element, with_title) else {
                continue;
            };
            cached.push(element);
            out.push(window);
        }
        out
    }

    fn app_name(&self, pid: i32) -> Option<String> {
        if pid <= 0 {
            return None;
        }
        let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
        let name = app.localizedName()?;
        let text = ns_to_string(&name);
        if text.is_empty() { None } else { Some(text) }
    }

    fn pid_alive(&self, pid: i32) -> bool {
        if pid <= 0 {
            return false;
        }
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
            .is_some_and(|app| !app.isTerminated())
    }

    fn cg_bounds(&self, pid: i32, window: u32) -> Option<Bounds> {
        let id = u64::from(window);
        window_env::full_list().into_iter().find_map(|record| {
            if record.owner_pid == pid && record.id == id {
                Some(Bounds {
                    x: record.bounds.x,
                    y: record.bounds.y,
                    width: record.bounds.width,
                    height: record.bounds.height,
                })
            } else {
                None
            }
        })
    }

    fn move_matched(&self, index: usize, bounds: Bounds) -> Result<Bounds, MoveMiss> {
        let elements = self.elements.borrow();
        let element = elements.get(index).ok_or(MoveMiss)?;
        if !set_point(element, bounds) {
            return Err(MoveMiss);
        }
        Ok(bounds)
    }
}

fn describe(element: &AXUIElement, with_title: bool) -> Option<AxWindow> {
    let position = attr("AXPosition");
    let size_name = attr("AXSize");
    let subrole_name = attr("AXSubrole");
    let origin = point_attr(element, &position)?;
    let size = size_attr(element, &size_name)?;
    let subrole =
        string_attr(element, &subrole_name).unwrap_or_else(|| AX_STANDARD_WINDOW.to_string());
    let position_settable = is_settable(element, &position);
    let title = if with_title {
        let title_name = attr("AXTitle");
        string_attr(element, &title_name)
    } else {
        None
    };
    Some(AxWindow {
        bounds: Bounds {
            x: origin.x.round() as i32,
            y: origin.y.round() as i32,
            width: size.width.round() as i32,
            height: size.height.round() as i32,
        },
        subrole,
        position_settable,
        title,
    })
}

fn is_settable(element: &AXUIElement, attribute: &CFString) -> bool {
    let mut flag = 0u8;
    let err = unsafe { AXUIElementIsAttributeSettable(element, attribute, &mut flag) };
    err == AXError::Success && flag != 0
}

fn set_point(element: &AXUIElement, bounds: Bounds) -> bool {
    let mut point = CGPoint {
        x: bounds.x as CGFloat,
        y: bounds.y as CGFloat,
    };
    let Some(ptr) = NonNull::new((&mut point) as *mut CGPoint) else {
        return false;
    };
    let Some(value) = (unsafe { AXValue::new(AXValueType::CGPoint, ptr.cast()) }) else {
        return false;
    };
    let typed = unsafe { CFRetained::cast_unchecked::<CFType>(value) };
    let position = attr("AXPosition");
    let err = unsafe { element.set_attribute_value(&position, &typed) };
    err == AXError::Success
}

fn copy_type(element: &AXUIElement, attribute: &CFString) -> Option<CFRetained<CFType>> {
    let mut raw: *const CFType = std::ptr::null();
    let slot = NonNull::new(&mut raw)?;
    let err = unsafe { element.copy_attribute_value(attribute, slot) };
    if err != AXError::Success {
        return None;
    }
    let raw = NonNull::new(raw as *mut CFType)?;
    Some(unsafe { CFRetained::from_raw(raw) })
}

fn copy_array(element: &AXUIElement, attribute: &CFString) -> Option<CFRetained<CFArray>> {
    let value = copy_type(element, attribute)?;
    Some(unsafe { CFRetained::cast_unchecked::<CFArray>(value) })
}

fn point_attr(element: &AXUIElement, attribute: &CFString) -> Option<CGPoint> {
    let value = copy_type(element, attribute)?;
    let value = unsafe { CFRetained::cast_unchecked::<AXValue>(value) };
    let mut point = CGPoint::default();
    let ptr = NonNull::new(&mut point)?;
    let ok = unsafe { value.value(AXValueType::CGPoint, ptr.cast()) };
    if ok { Some(point) } else { None }
}

fn size_attr(element: &AXUIElement, attribute: &CFString) -> Option<CGSize> {
    let value = copy_type(element, attribute)?;
    let value = unsafe { CFRetained::cast_unchecked::<AXValue>(value) };
    let mut size = CGSize::default();
    let ptr = NonNull::new(&mut size)?;
    let ok = unsafe { value.value(AXValueType::CGSize, ptr.cast()) };
    if ok { Some(size) } else { None }
}

fn string_attr(element: &AXUIElement, attribute: &CFString) -> Option<String> {
    let value = copy_type(element, attribute)?;
    let value = unsafe { CFRetained::cast_unchecked::<CFString>(value) };
    let text = value.to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn ns_to_string(text: &objc2_foundation::NSString) -> String {
    use std::ffi::CStr;
    unsafe {
        let ptr = text.UTF8String();
        if ptr.is_null() {
            String::new()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

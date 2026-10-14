//! Frontmost-window selection for the engine snapshot.
//!
//! Parsing the window-info records is pure logic and is tested headlessly.
//! Talking to `CGWindowListCopyWindowInfo` and `NSWorkspace` stays behind
//! `cfg(target_os = "macos")`.

use std::collections::HashSet;

use shimeji_core::MascotFrame;
use shimeji_core::env::WindowSample;
use shimeji_core::model::{ActionKind, ActionNode, Builtin, Character, OrderedMap, Param};

use super::coords::Rect;

/// One on-screen window, already in engine (CG) coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowRecord {
    pub id: u64,
    pub owner_pid: i32,
    pub layer: i32,
    pub alpha: f64,
    pub bounds: Rect,
}

/// What [`select_active_ie`] needs besides the z-ordered window list.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectInput<'a> {
    pub windows: &'a [WindowRecord],
    pub frontmost_pid: i32,
    pub our_pid: i32,
    pub screens: &'a [Rect],
}

/// Active window plus whether its top border is a standable floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveIe {
    pub sample: WindowSample,
    pub top_standable: bool,
}

/// First layer-0, non-zero-alpha window of the frontmost PID, skipping our
/// own windows. A window whose bounds match a screen is still published, but
/// `top_standable` is false so it does not count as a floor.
pub fn select_active_ie(input: SelectInput<'_>) -> Option<ActiveIe> {
    let record = input.windows.iter().find(|window| {
        window.owner_pid != input.our_pid
            && window.owner_pid == input.frontmost_pid
            && window.layer == 0
            && window.alpha > 0.0
            && window.id > 0
            && window.bounds.width > 0
            && window.bounds.height > 0
    })?;
    let full_screen = input.screens.iter().any(|screen| {
        screen.x == record.bounds.x
            && screen.y == record.bounds.y
            && screen.width == record.bounds.width
            && screen.height == record.bounds.height
    });
    Some(ActiveIe {
        sample: WindowSample {
            visible: true,
            id: record.id,
            x: record.bounds.x,
            y: record.bounds.y,
            width: record.bounds.width,
            height: record.bounds.height,
        },
        top_standable: !full_screen,
    })
}

/// Snapshot form of [`ActiveIe`]. A full-screen window stays in the sample
/// with `visible: false`, which clears `isOn` for every border (the top is
/// not a floor).
pub fn to_window_sample(active: &ActiveIe) -> WindowSample {
    let mut sample = active.sample;
    if !active.top_standable {
        sample.visible = false;
    }
    sample
}

/// How often the poller should query, from the plan's adaptive rates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollPace {
    /// Clock stopped.
    Stopped,
    /// A mascot rides the window, or a carry is active: every tick (25 Hz).
    Ride,
    /// A mascot is within 80 px of the window: 10 Hz.
    Near,
    /// Otherwise: 2 Hz.
    Far,
}

impl PollPace {
    /// All characters share one poller. The most demanding one wins.
    pub fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Ride, _) | (_, Self::Ride) => Self::Ride,
            (Self::Near, _) | (_, Self::Near) => Self::Near,
            (Self::Far, _) | (_, Self::Far) => Self::Far,
            _ => Self::Stopped,
        }
    }

    pub fn period_ticks(self) -> u32 {
        match self {
            Self::Stopped => u32::MAX,
            Self::Ride => 1,
            Self::Near => 3,
            Self::Far => 13,
        }
    }
}

/// Resolve the engine's innermost action names once at character load time.
/// A carry needs Ride even on a pose with no move request; a falling pet
/// needs Near before reaching the 80 px band, including when no window is
/// currently known. This does not evaluate any pack expressions.
pub struct PollingActions {
    falling: HashSet<String>,
    carrying: HashSet<String>,
}

impl PollingActions {
    pub fn new(character: &Character) -> Self {
        let mut actions = Self {
            falling: HashSet::from(["Fall".to_string()]),
            carrying: ["WalkWithIE", "FallWithIE", "ThrowIE"]
                .map(str::to_string)
                .into_iter()
                .collect(),
        };
        character.visit_actions(&mut |action| {
            actions.record(action.kind, &action.params);
            for child in &action.children {
                if let ActionNode::Reference(reference) = child {
                    if let Some(target) = character.actions.get(&reference.name) {
                        actions.record(target.kind, &reference.params);
                    }
                }
            }
        });
        for behavior in character.behaviors.values() {
            if let Some(target) = character.actions.get(&behavior.action) {
                actions.record(target.kind, &behavior.params);
            }
            for reference in behavior.next_refs() {
                if let Some(target) = character
                    .behaviors
                    .get(&reference.name)
                    .and_then(|behavior| character.actions.get(&behavior.action))
                {
                    actions.record(target.kind, &reference.params);
                }
            }
        }
        actions
    }

    fn record(&mut self, kind: ActionKind, params: &OrderedMap<Param>) {
        let names = match kind {
            ActionKind::Builtin(Builtin::Fall) => &mut self.falling,
            ActionKind::Builtin(Builtin::WalkWithIe | Builtin::FallWithIe | Builtin::ThrowIe) => {
                &mut self.carrying
            }
            _ => return,
        };
        if let Some(name) = params.get("Name") {
            if name.value.wrapper.is_none() {
                names.insert(name.value.raw.clone());
            }
        }
    }

    /// Current carry actions keep the active window open even on a still pose.
    /// Every IE builtin operates on this runtime's current active window.
    pub fn carry_windows(&self, frames: &[MascotFrame], window: Option<&WindowSample>) -> Vec<u64> {
        window
            .filter(|window| window.visible && window.id != 0)
            .filter(|_| {
                frames
                    .iter()
                    .any(|frame| self.carrying.contains(&frame.action))
            })
            .map(|window| vec![window.id])
            .unwrap_or_default()
    }

    pub fn pace(
        &self,
        running: bool,
        frames: &[MascotFrame],
        window: Option<&WindowSample>,
    ) -> PollPace {
        if !running {
            return PollPace::Stopped;
        }
        if frames
            .iter()
            .any(|frame| self.carrying.contains(&frame.action))
        {
            return PollPace::Ride;
        }
        let anchors: Vec<_> = frames.iter().map(|frame| frame.anchor).collect();
        let pace = pace_for(&anchors, window);
        if frames
            .iter()
            .any(|frame| self.falling.contains(&frame.action))
        {
            pace.merge(PollPace::Near)
        } else {
            pace
        }
    }
}

/// Ride / near / far from mascot anchors and the current window.
pub fn pace_for(anchors: &[shimeji_core::env::Point], window: Option<&WindowSample>) -> PollPace {
    let Some(window) = window else {
        return PollPace::Far;
    };
    if !window.visible {
        return PollPace::Far;
    }
    let right = window.x.wrapping_add(window.width);
    let bottom = window.y.wrapping_add(window.height);
    let mut near = false;
    for anchor in anchors {
        let on_top = anchor.y == window.y && window.x <= anchor.x && anchor.x <= right;
        let on_bottom = anchor.y == bottom && window.x <= anchor.x && anchor.x <= right;
        let on_left = anchor.x == window.x && window.y <= anchor.y && anchor.y <= bottom;
        let on_right = anchor.x == right && window.y <= anchor.y && anchor.y <= bottom;
        if on_top || on_bottom || on_left || on_right {
            return PollPace::Ride;
        }
        let dx = if anchor.x < window.x {
            window.x.wrapping_sub(anchor.x)
        } else if anchor.x > right {
            anchor.x.wrapping_sub(right)
        } else {
            0
        };
        let dy = if anchor.y < window.y {
            window.y.wrapping_sub(anchor.y)
        } else if anchor.y > bottom {
            anchor.y.wrapping_sub(bottom)
        } else {
            0
        };
        if dx <= 80 && dy <= 80 {
            near = true;
        }
    }
    if near { PollPace::Near } else { PollPace::Far }
}

#[cfg(target_os = "macos")]
mod live {
    use super::{ActiveIe, SelectInput, WindowRecord, select_active_ie};
    use crate::platform::coords::Rect;
    use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFNumberType, CGRect};
    use objc2_core_graphics::{
        CGRectMakeWithDictionaryRepresentation, CGWindowID, CGWindowListCopyWindowInfo,
        CGWindowListOption, kCGNullWindowID, kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer,
        kCGWindowNumber, kCGWindowOwnerPID,
    };
    use std::ffi::c_void;

    pub fn list_windows(option: CGWindowListOption, relative: CGWindowID) -> Vec<WindowRecord> {
        let Some(array) = CGWindowListCopyWindowInfo(option, relative) else {
            return Vec::new();
        };
        records(&array)
    }

    pub fn full_list() -> Vec<WindowRecord> {
        list_windows(
            CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
            kCGNullWindowID,
        )
    }

    pub fn one(id: u64) -> Vec<WindowRecord> {
        let Ok(window) = u32::try_from(id) else {
            return Vec::new();
        };
        list_windows(CGWindowListOption::OptionIncludingWindow, window)
    }

    pub fn pick(
        windows: &[WindowRecord],
        frontmost_pid: i32,
        our_pid: i32,
        screens: &[Rect],
    ) -> Option<ActiveIe> {
        select_active_ie(SelectInput {
            windows,
            frontmost_pid,
            our_pid,
            screens,
        })
    }

    fn records(array: &CFArray) -> Vec<WindowRecord> {
        let count = usize::try_from(array.count()).unwrap_or(0);
        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            let dict = unsafe { array.value_at_index(index as isize) };
            if dict.is_null() {
                continue;
            }
            let dict = dict.cast::<CFDictionary>();
            let dict = unsafe { &*dict };
            let Some(id) = dict_i64(dict, unsafe {
                std::ptr::from_ref(kCGWindowNumber).cast::<c_void>()
            }) else {
                continue;
            };
            if id <= 0 {
                continue;
            }
            let owner = dict_i64(dict, unsafe {
                std::ptr::from_ref(kCGWindowOwnerPID).cast::<c_void>()
            })
            .unwrap_or(-1);
            let layer = dict_i64(dict, unsafe {
                std::ptr::from_ref(kCGWindowLayer).cast::<c_void>()
            })
            .unwrap_or(-1);
            let alpha = dict_f64(dict, unsafe {
                std::ptr::from_ref(kCGWindowAlpha).cast::<c_void>()
            })
            .unwrap_or(1.0);
            let Some(bounds) = dict_rect(dict) else {
                continue;
            };
            out.push(WindowRecord {
                id: id as u64,
                owner_pid: owner as i32,
                layer: layer as i32,
                alpha,
                bounds: Rect::new(
                    bounds.origin.x.round() as i32,
                    bounds.origin.y.round() as i32,
                    bounds.size.width.round() as i32,
                    bounds.size.height.round() as i32,
                ),
            });
        }
        out
    }

    fn dict_i64(dict: &CFDictionary, key: *const c_void) -> Option<i64> {
        let value = unsafe { dict.value(key) };
        if value.is_null() {
            return None;
        }
        let number = unsafe { &*value.cast::<CFNumber>() };
        let mut out = 0i64;
        let ok = unsafe { number.value(CFNumberType::SInt64Type, (&raw mut out).cast::<c_void>()) };
        if ok { Some(out) } else { None }
    }

    fn dict_f64(dict: &CFDictionary, key: *const c_void) -> Option<f64> {
        let value = unsafe { dict.value(key) };
        if value.is_null() {
            return None;
        }
        let number = unsafe { &*value.cast::<CFNumber>() };
        let mut out = 0f64;
        let ok =
            unsafe { number.value(CFNumberType::Float64Type, (&raw mut out).cast::<c_void>()) };
        if ok {
            Some(out)
        } else {
            dict_i64(dict, key).map(|n| n as f64)
        }
    }

    fn dict_rect(dict: &CFDictionary) -> Option<CGRect> {
        let key = unsafe { std::ptr::from_ref(kCGWindowBounds).cast::<c_void>() };
        let value = unsafe { dict.value(key) };
        if value.is_null() {
            return None;
        }
        let bounds = unsafe { &*value.cast::<CFDictionary>() };
        let mut raw = CGRect::default();
        let ok = unsafe { CGRectMakeWithDictionaryRepresentation(Some(bounds), &mut raw) };
        if ok { Some(raw) } else { None }
    }
}

#[cfg(target_os = "macos")]
pub use live::{full_list, one, pick};

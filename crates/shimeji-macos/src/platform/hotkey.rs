//! Global hide/show hotkey.
//!
//! Carbon `RegisterEventHotKey` needs no Accessibility permission. macOS 15
//! rejects a chord that is only Option, or only Option+Shift, so the default
//! includes Control.
//!
//! Default: Control+Shift+S (`ctrl+shift+s`). Control satisfies the macOS 15
//! rule, Shift keeps it off the plain Control-letter editing keys, and S is
//! on every keyboard (the U2 spike used F19, which laptops lack). The settings
//! window stores the spec. `SHIMEJI_HOTKEY` overrides it for this process.

use std::fmt;

/// Env var read at launch. Same grammar as [`parse`].
pub const HOTKEY_ENV: &str = "SHIMEJI_HOTKEY";

/// Control+Shift+S.
pub const DEFAULT_SPEC: &str = "ctrl+shift+s";

const BIT_CMD: u32 = 1 << 8;
const BIT_SHIFT: u32 = 1 << 9;
const BIT_OPTION: u32 = 1 << 11;
const BIT_CONTROL: u32 = 1 << 12;

/// A Carbon hotkey: virtual key code plus modifier bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub key_code: u32,
    pub modifiers: u32,
}

impl fmt::Display for Chord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.modifiers & BIT_CONTROL != 0 {
            parts.push("ctrl");
        }
        if self.modifiers & BIT_OPTION != 0 {
            parts.push("opt");
        }
        if self.modifiers & BIT_SHIFT != 0 {
            parts.push("shift");
        }
        if self.modifiers & BIT_CMD != 0 {
            parts.push("cmd");
        }
        parts.push(key_name(self.key_code).unwrap_or("?"));
        write!(formatter, "{}", parts.join("+"))
    }
}

impl Chord {
    /// macOS 15 refuses Option-only and Option+Shift-only chords.
    pub fn rejected_by_macos15(self) -> bool {
        if self.modifiers & (BIT_CMD | BIT_CONTROL) != 0 {
            return false;
        }
        let option = self.modifiers & BIT_OPTION != 0;
        let extra = self.modifiers & !(BIT_OPTION | BIT_SHIFT);
        option && extra == 0
    }

    pub fn uses_cmd_or_ctrl(self) -> bool {
        self.modifiers & (BIT_CMD | BIT_CONTROL) != 0
    }
}

/// `ctrl+shift+s`, `cmd+opt+h`, `f19`. Order of modifiers does not matter.
pub fn parse(spec: &str) -> Result<Chord, String> {
    let text = spec.trim().to_ascii_lowercase();
    if text.is_empty() {
        return Err("empty hotkey".to_string());
    }
    let mut modifiers = 0u32;
    let mut key: Option<u32> = None;
    for part in text.split('+') {
        let part = part.trim();
        if part.is_empty() {
            return Err(format!("bad hotkey \"{spec}\""));
        }
        match part {
            "cmd" | "command" | "meta" => modifiers |= BIT_CMD,
            "shift" => modifiers |= BIT_SHIFT,
            "opt" | "option" | "alt" => modifiers |= BIT_OPTION,
            "ctrl" | "control" => modifiers |= BIT_CONTROL,
            other => {
                if key.is_some() {
                    return Err(format!("two keys in \"{spec}\""));
                }
                key = Some(key_code(other).ok_or_else(|| format!("unknown key \"{other}\""))?);
            }
        }
    }
    let key_code = key.ok_or_else(|| format!("no key in \"{spec}\""))?;
    let chord = Chord {
        key_code,
        modifiers,
    };
    if chord.rejected_by_macos15() {
        return Err(format!(
            "\"{spec}\" is Option-only; macOS 15 requires Cmd or Control"
        ));
    }
    if !chord.uses_cmd_or_ctrl() {
        return Err(format!("\"{spec}\" needs Cmd or Control"));
    }
    Ok(chord)
}

pub fn default_chord() -> Chord {
    parse(DEFAULT_SPEC).expect("default hotkey")
}

fn key_code(name: &str) -> Option<u32> {
    let bytes = name.as_bytes();
    if bytes.len() == 1 {
        let byte = bytes[0];
        if byte.is_ascii_digit() {
            return digit_code(byte - b'0');
        }
        if byte.is_ascii_lowercase() {
            return letter_code(byte);
        }
    }
    match name {
        "space" => Some(0x31),
        "return" | "enter" => Some(0x24),
        "tab" => Some(0x30),
        "escape" | "esc" => Some(0x35),
        "left" => Some(0x7B),
        "right" => Some(0x7C),
        "down" => Some(0x7D),
        "up" => Some(0x7E),
        "f1" => Some(0x7A),
        "f2" => Some(0x78),
        "f3" => Some(0x63),
        "f4" => Some(0x76),
        "f5" => Some(0x60),
        "f6" => Some(0x61),
        "f7" => Some(0x62),
        "f8" => Some(0x64),
        "f9" => Some(0x65),
        "f10" => Some(0x6D),
        "f11" => Some(0x67),
        "f12" => Some(0x6F),
        "f13" => Some(0x69),
        "f14" => Some(0x6B),
        "f15" => Some(0x71),
        "f16" => Some(0x6A),
        "f17" => Some(0x40),
        "f18" => Some(0x4F),
        "f19" => Some(0x50),
        _ => None,
    }
}

fn key_name(code: u32) -> Option<&'static str> {
    Some(match code {
        0x00 => "a",
        0x01 => "s",
        0x02 => "d",
        0x03 => "f",
        0x04 => "h",
        0x05 => "g",
        0x06 => "z",
        0x07 => "x",
        0x08 => "c",
        0x09 => "v",
        0x0B => "b",
        0x0C => "q",
        0x0D => "w",
        0x0E => "e",
        0x0F => "r",
        0x10 => "y",
        0x11 => "t",
        0x12 => "1",
        0x13 => "2",
        0x14 => "3",
        0x15 => "4",
        0x16 => "6",
        0x17 => "5",
        0x18 => "=",
        0x19 => "9",
        0x1A => "7",
        0x1B => "-",
        0x1C => "8",
        0x1D => "0",
        0x1F => "o",
        0x20 => "u",
        0x22 => "i",
        0x23 => "p",
        0x25 => "l",
        0x26 => "j",
        0x28 => "k",
        0x24 => "return",
        0x2D => "n",
        0x2E => "m",
        0x30 => "tab",
        0x31 => "space",
        0x35 => "escape",
        0x40 => "f17",
        0x4F => "f18",
        0x50 => "f19",
        0x60 => "f5",
        0x61 => "f6",
        0x62 => "f7",
        0x63 => "f3",
        0x64 => "f8",
        0x65 => "f9",
        0x67 => "f11",
        0x69 => "f13",
        0x6A => "f16",
        0x6B => "f14",
        0x6D => "f10",
        0x6F => "f12",
        0x71 => "f15",
        0x76 => "f4",
        0x78 => "f2",
        0x7A => "f1",
        0x7B => "left",
        0x7C => "right",
        0x7D => "down",
        0x7E => "up",
        _ => return None,
    })
}

fn letter_code(byte: u8) -> Option<u32> {
    Some(match byte {
        b'a' => 0x00,
        b's' => 0x01,
        b'd' => 0x02,
        b'f' => 0x03,
        b'h' => 0x04,
        b'g' => 0x05,
        b'z' => 0x06,
        b'x' => 0x07,
        b'c' => 0x08,
        b'v' => 0x09,
        b'b' => 0x0B,
        b'q' => 0x0C,
        b'w' => 0x0D,
        b'e' => 0x0E,
        b'r' => 0x0F,
        b'y' => 0x10,
        b't' => 0x11,
        b'o' => 0x1F,
        b'u' => 0x20,
        b'i' => 0x22,
        b'p' => 0x23,
        b'l' => 0x25,
        b'j' => 0x26,
        b'k' => 0x28,
        b'n' => 0x2D,
        b'm' => 0x2E,
        _ => return None,
    })
}

fn digit_code(digit: u8) -> Option<u32> {
    Some(match digit {
        0 => 0x1D,
        1 => 0x12,
        2 => 0x13,
        3 => 0x14,
        4 => 0x15,
        5 => 0x17,
        6 => 0x16,
        7 => 0x1A,
        8 => 0x1C,
        9 => 0x19,
        _ => return None,
    })
}

/// Registered Carbon hotkey. Drop unregisters it.
#[cfg(target_os = "macos")]
pub struct Registration {
    hotkey: *mut std::ffi::c_void,
    handler: *mut std::ffi::c_void,
    thunk: *mut Thunk,
}

#[cfg(target_os = "macos")]
struct Thunk {
    user: *mut std::ffi::c_void,
    callback: unsafe extern "C" fn(*mut std::ffi::c_void),
}

#[cfg(target_os = "macos")]
impl Registration {
    /// `callback` runs on the main thread from the Carbon handler. It must
    /// not assume the clock's borrow is free; the shell hops to the next
    /// main-queue turn.
    pub fn install(
        chord: Chord,
        callback: unsafe extern "C" fn(*mut std::ffi::c_void),
        user: *mut std::ffi::c_void,
    ) -> Result<Self, i32> {
        let thunk = Box::into_raw(Box::new(Thunk { user, callback }));
        let mut handler = std::ptr::null_mut();
        let spec = EventTypeSpec {
            event_class: u32::from_be_bytes(*b"keyb"),
            event_kind: 5, // kEventHotKeyPressed
        };
        let target = unsafe { GetApplicationEventTarget() };
        if target.is_null() {
            drop(unsafe { Box::from_raw(thunk) });
            return Err(-1);
        }
        let installed = unsafe {
            InstallEventHandler(target, trampoline, 1, &spec, thunk.cast(), &mut handler)
        };
        if installed != 0 {
            drop(unsafe { Box::from_raw(thunk) });
            return Err(installed);
        }
        let mut hotkey = std::ptr::null_mut();
        let status = unsafe {
            RegisterEventHotKey(
                chord.key_code,
                chord.modifiers,
                EventHotKeyId {
                    signature: u32::from_be_bytes(*b"shj2"),
                    id: 1,
                },
                target,
                0,
                &mut hotkey,
            )
        };
        if status != 0 || hotkey.is_null() {
            unsafe { RemoveEventHandler(handler) };
            drop(unsafe { Box::from_raw(thunk) });
            return Err(if status == 0 { -2 } else { status });
        }
        Ok(Self {
            hotkey,
            handler,
            thunk,
        })
    }

    pub fn status_ok(&self) -> bool {
        !self.hotkey.is_null()
    }
}

#[cfg(target_os = "macos")]
impl Drop for Registration {
    fn drop(&mut self) {
        unsafe {
            if !self.hotkey.is_null() {
                UnregisterEventHotKey(self.hotkey);
            }
            if !self.handler.is_null() {
                RemoveEventHandler(self.handler);
            }
            if !self.thunk.is_null() {
                drop(Box::from_raw(self.thunk));
            }
        }
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn trampoline(
    _call: *mut std::ffi::c_void,
    _event: *mut std::ffi::c_void,
    user: *mut std::ffi::c_void,
) -> i32 {
    if !user.is_null() {
        let thunk = unsafe { &*user.cast::<Thunk>() };
        unsafe { (thunk.callback)(thunk.user) };
    }
    0
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

#[cfg(target_os = "macos")]
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut std::ffi::c_void;
    fn InstallEventHandler(
        target: *mut std::ffi::c_void,
        handler: unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *mut std::ffi::c_void,
            *mut std::ffi::c_void,
        ) -> i32,
        num_types: u32,
        types: *const EventTypeSpec,
        user: *mut std::ffi::c_void,
        out_handler: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn RemoveEventHandler(handler: *mut std::ffi::c_void) -> i32;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        hotkey_id: EventHotKeyId,
        target: *mut std::ffi::c_void,
        options: u32,
        out_ref: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hotkey: *mut std::ffi::c_void) -> i32;
}

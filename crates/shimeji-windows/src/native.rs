//! All Win32 resources and calls live on this thread. The window procedure only
//! queues input events. Timer callbacks drain only when no App borrow is held,
//! including the nested native menu loop; synchronous render callbacks defer.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::ffi::{OsString, c_void};
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::Instant;

use shimeji_app::pomodoro::{Interruption, Pomodoro, Status};
use shimeji_app::settings::{self, Settings};
use shimeji_app::{Command, Runtime};
use shimeji_core::env::{Point, ScreenSample, Snapshot, WindowSample, WorkRect};
use shimeji_core::{MascotFrame, MascotId, MouseButton};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use windows_sys::core::BOOL;

use crate::focus;
use crate::launch::{HELP, Loaded, Options, bundled_pack};
use crate::sprites::{Atlas, Drawing};

#[path = "native_focus.rs"]
mod focus_window;

const TRAY_MESSAGE: u32 = WM_APP + 1;
const TIMER: usize = 1;
const FOCUS_TIMER: usize = 2;
const DEADLINE_TIMER: usize = 3;
const OPEN: usize = 1;
const ADD: usize = 2;
const PAUSE: usize = 3;
const HIDE: usize = 4;
const RESCUE: usize = 5;
const REMOVE: usize = 6;
const QUIT: usize = 7;

enum Event {
    Tick,
    FocusTick,
    FocusCommand(usize),
    Command(usize, Option<MascotId>),
    Interrupted(Interruption, bool),
    Press(MascotId),
    Release,
    Menu(Option<MascotId>),
    Open(PathBuf),
    ChoosePack,
    Error(String),
    ToggleHidden,
    Quit,
}

thread_local! {
    static EVENTS: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static FAILURE: RefCell<Option<String>> = const { RefCell::new(None) };
    static OPEN_MENU: Cell<HMENU> = const { Cell::new(null_mut()) };
}

fn queue(event: Event) {
    EVENTS.with_borrow_mut(|events| {
        if !matches!(event, Event::Tick) || !events.iter().any(|event| matches!(event, Event::Tick))
        {
            events.push_back(event);
        }
    });
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

pub fn show_error(message: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(message).as_ptr(),
            wide("shimeji2").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn win_error(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let id = MascotId(unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as u64);
    match message {
        WM_TIMER => {
            queue(if wp == FOCUS_TIMER {
                Event::FocusTick
            } else {
                Event::Tick
            });
            // A native popup runs a nested message loop. The popup is displayed
            // without an App borrow, so focus deadlines still run while open.
            dispatch_pending();
        }
        WM_WTSSESSION_CHANGE => {
            match wp as u32 {
                WTS_SESSION_LOCK => queue(Event::Interrupted(Interruption::Locked, true)),
                WTS_SESSION_UNLOCK => queue(Event::Interrupted(Interruption::Locked, false)),
                WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => {
                    queue(Event::Interrupted(Interruption::Session, true))
                }
                WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => {
                    queue(Event::Interrupted(Interruption::Session, false))
                }
                _ => {}
            }
            dispatch_pending();
        }
        WM_POWERBROADCAST => {
            match wp as u32 {
                PBT_APMSUSPEND => queue(Event::Interrupted(Interruption::Sleep, true)),
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND | PBT_APMRESUMECRITICAL => {
                    queue(Event::Interrupted(Interruption::Sleep, false))
                }
                _ => {}
            }
            dispatch_pending();
            return 1;
        }
        WM_LBUTTONDOWN if id.0 != 0 => {
            unsafe {
                SetCapture(hwnd);
            }
            queue(Event::Press(id));
        }
        WM_LBUTTONUP => {
            unsafe {
                ReleaseCapture();
            }
            queue(Event::Release);
        }
        WM_CAPTURECHANGED | WM_CANCELMODE => queue(Event::Release),
        WM_MOUSEMOVE => {
            if unsafe { GetCapture() } == hwnd {
                queue(Event::Tick);
            }
        }
        WM_RBUTTONUP => queue(Event::Menu((id.0 != 0).then_some(id))),
        WM_HOTKEY => queue(Event::ToggleHidden),
        TRAY_MESSAGE => match lp as u32 {
            WM_RBUTTONUP | WM_CONTEXTMENU => queue(Event::Menu(None)),
            WM_LBUTTONDBLCLK => queue(Event::ToggleHidden),
            _ => {}
        },
        WM_DROPFILES => {
            let drop = wp as HDROP;
            // A drop owns its HDROP until DragFinish. Copy its UTF-16 path first.
            let length = unsafe { DragQueryFileW(drop, 0, null_mut(), 0) };
            let mut path = vec![0; length as usize + 1];
            if length > 0 && unsafe { DragQueryFileW(drop, 0, path.as_mut_ptr(), length + 1) } > 0 {
                queue(Event::Open(PathBuf::from(OsString::from_wide(
                    &path[..length as usize],
                ))));
            }
            unsafe {
                DragFinish(drop);
            }
        }
        WM_CLOSE => queue(Event::Quit),
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        _ => return unsafe { DefWindowProcW(hwnd, message, wp, lp) },
    }
    0
}

struct Window(HWND);

impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0);
        }
    }
}

fn window(layered: bool, id: Option<MascotId>) -> Result<Window, String> {
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE
                | if layered {
                    WS_EX_LAYERED | WS_EX_TOPMOST
                } else {
                    0
                },
            wide("Shimeji2Windows").as_ptr(),
            wide("shimeji2").as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    if hwnd.is_null() {
        return Err(win_error("CreateWindowExW"));
    }
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, id.map_or(0, |id| id.0 as isize));
        DragAcceptFiles(hwnd, 1);
    }
    Ok(Window(hwnd))
}

struct Bitmap {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    width: i32,
    height: i32,
}

impl Bitmap {
    fn new(drawing: &Drawing) -> Result<Self, String> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: drawing.width,
                biHeight: -drawing.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = null_mut();
        let dc = unsafe { CreateCompatibleDC(null_mut()) };
        if dc.is_null() {
            return Err(win_error("CreateCompatibleDC"));
        }
        let bitmap =
            unsafe { CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0) };
        if bitmap.is_null() || bits.is_null() {
            unsafe {
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                DeleteDC(dc);
            }
            return Err(win_error("CreateDIBSection"));
        }
        // The DIB was allocated for exactly width * height * 4 bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(
                drawing.bgra.as_ptr(),
                bits.cast::<u8>(),
                drawing.bgra.len(),
            );
        }
        let previous = unsafe { SelectObject(dc, bitmap) };
        Ok(Self {
            dc,
            bitmap,
            previous,
            width: drawing.width,
            height: drawing.height,
        })
    }

    fn present(&self, hwnd: HWND, origin: Point) -> Result<(), String> {
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let result = unsafe {
            UpdateLayeredWindow(
                hwnd,
                null_mut(),
                &POINT {
                    x: origin.x,
                    y: origin.y,
                },
                &SIZE {
                    cx: self.width,
                    cy: self.height,
                },
                self.dc,
                &POINT { x: 0, y: 0 },
                0,
                &blend,
                ULW_ALPHA,
            )
        };
        if result == 0 {
            return Err(win_error("UpdateLayeredWindow"));
        }
        Ok(())
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

struct Pet {
    window: Window,
    bitmap: Option<Bitmap>,
    last: Option<MascotFrame>,
}

impl Pet {
    fn update(&mut self, atlas: &Atlas, frame: &MascotFrame, hidden: bool) -> Result<(), String> {
        let image_changed = self.last.as_ref().is_none_or(|last| {
            last.image != frame.image
                || last.look_right != frame.look_right
                || last.image_anchor != frame.image_anchor
        });
        let moved = self
            .last
            .as_ref()
            .is_none_or(|last| last.anchor != frame.anchor);
        if image_changed {
            if let Some(drawing) = atlas.draw(frame)? {
                let bitmap = Bitmap::new(&drawing)?;
                bitmap.present(self.window.0, drawing.origin)?;
                self.bitmap = Some(bitmap);
            } else {
                self.bitmap = None;
            }
        } else if moved {
            // Position-only updates preserve the system's existing alpha surface.
            let previous = self.last.as_ref().expect("existing frame");
            let mut rect = RECT::default();
            unsafe {
                GetWindowRect(self.window.0, &mut rect);
                SetWindowPos(
                    self.window.0,
                    HWND_TOPMOST,
                    rect.left + frame.anchor.x - previous.anchor.x,
                    rect.top + frame.anchor.y - previous.anchor.y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
        unsafe {
            ShowWindow(
                self.window.0,
                if hidden || self.bitmap.is_none() {
                    SW_HIDE
                } else {
                    SW_SHOWNOACTIVATE
                },
            );
        }
        self.last = Some(frame.clone());
        Ok(())
    }
}

struct Tray {
    data: NOTIFYICONDATAW,
    last_tip: String,
}

impl Tray {
    fn new(hwnd: HWND) -> Result<Self, String> {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY_MESSAGE,
            hIcon: unsafe { LoadIconW(null_mut(), IDI_APPLICATION) },
            ..Default::default()
        };
        let tip = wide("shimeji2 — 右键打开菜单");
        data.szTip[..tip.len()].copy_from_slice(&tip);
        let tray = Self {
            data,
            last_tip: String::new(),
        };
        tray.add()?;
        Ok(tray)
    }

    fn add(&self) -> Result<(), String> {
        if unsafe { Shell_NotifyIconW(NIM_ADD, &self.data) } == 0 {
            return Err(win_error("Shell_NotifyIconW"));
        }
        Ok(())
    }
    fn update(&mut self, label: &str) {
        if self.last_tip == label {
            return;
        }
        self.data.uFlags = NIF_TIP;
        copy_wide(&mut self.data.szTip, &format!("shimeji2 · {label}"));
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &self.data);
        }
        self.data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        self.last_tip = label.to_string();
    }

    fn notify(&self, title: &str, body: &str) {
        let mut data = self.data;
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_INFO | NIIF_NOSOUND | NIIF_RESPECT_QUIET_TIME;
        copy_wide(&mut data.szInfoTitle, title);
        copy_wide(&mut data.szInfo, body);
        if unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) } == 0 {
            eprintln!(
                "pomodoro_notification=failed {}",
                std::io::Error::last_os_error()
            );
        }
    }
}

fn copy_wide<const N: usize>(buffer: &mut [u16; N], text: &str) {
    buffer.fill(0);
    for (slot, value) in buffer
        .iter_mut()
        .take(N.saturating_sub(1))
        .zip(text.encode_utf16())
    {
        *slot = value;
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &self.data);
        }
    }
}

unsafe extern "system" fn monitor_sample(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
        // EnumDisplayMonitors calls back synchronously while this vector is alive.
        let screens = unsafe { &mut *(data as *mut Vec<ScreenSample>) };
        screens.push(ScreenSample {
            id: format!("{:p}", monitor),
            x: info.rcMonitor.left,
            y: info.rcMonitor.top,
            width: info.rcMonitor.right - info.rcMonitor.left,
            height: info.rcMonitor.bottom - info.rcMonitor.top,
            work: Some(WorkRect {
                x: info.rcWork.left,
                y: info.rcWork.top,
                width: info.rcWork.right - info.rcWork.left,
                height: info.rcWork.bottom - info.rcWork.top,
            }),
        });
    }
    1
}

fn snapshot() -> Snapshot {
    snapshot_with_windows(true)
}

fn snapshot_with_windows(read_window: bool) -> Snapshot {
    let mut screens = Vec::<ScreenSample>::new();
    let mut cursor = POINT::default();
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(monitor_sample),
            std::ptr::from_mut(&mut screens) as LPARAM,
        );
        GetCursorPos(&mut cursor);
    }
    Snapshot {
        screens,
        cursor: Point::new(cursor.x, cursor.y),
        active_window: read_window.then(active_window).flatten(),
        multiscreen: true,
    }
}

fn active_window() -> Option<WindowSample> {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if hwnd.is_null()
            || pid == std::process::id()
            || IsIconic(hwnd) != 0
            || IsWindowVisible(hwnd) == 0
        {
            return None;
        }
        let mut class = [0; 128];
        let length = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        let class = String::from_utf16_lossy(&class[..length.max(0) as usize]);
        if matches!(
            class.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return None;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return None;
        }
        Some(WindowSample {
            id: hwnd as u64,
            visible: true,
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        })
    }
}

struct Drag {
    id: MascotId,
    frozen_offset: Option<Point>,
}

struct App {
    runtime: Runtime,
    atlas: Atlas,
    fall: String,
    pets: HashMap<MascotId, Pet>,
    // Drop the tray before its owner window.
    tray: Tray,
    controller: Window,
    origin: Instant,
    seconds: Option<f64>,
    scale_override: Option<f64>,
    paused: bool,
    hidden: bool,
    drag: Option<Drag>,
    pomodoro: Pomodoro,
    settings: Settings,
    profile: PathBuf,
    focus_ui: Option<focus_window::FocusWindow>,
    badge: Option<focus_window::Badge>,
    animation_timer: bool,
    focus_timer: bool,
    interrupted: bool,
    sleep: bool,
    locked: bool,
    session_inactive: bool,
}

impl App {
    fn new(loaded: Loaded, options: &Options) -> Result<Self, String> {
        let controller = window(false, None)?;
        let tray = Tray::new(controller.0)?;
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let profile = focus::profile_directory(options.profile.as_deref(), local.as_deref())?;
        let settings = settings::load(&profile).settings;
        let mut app = Self {
            runtime: loaded.runtime(42),
            atlas: Atlas::new(&loaded.character, loaded.images, loaded.scale),
            fall: loaded.character.behavior_schema.required_behaviors()[1].to_string(),
            pets: HashMap::new(),
            tray,
            controller,
            origin: Instant::now(),
            seconds: options.seconds,
            scale_override: options.scale,
            paused: false,
            hidden: false,
            drag: None,
            pomodoro: Pomodoro::default(),
            settings,
            profile,
            focus_ui: None,
            badge: None,
            animation_timer: false,
            focus_timer: false,
            interrupted: false,
            sleep: false,
            locked: false,
            session_inactive: false,
        };
        let world = snapshot();
        if world.screens.is_empty() {
            return Err("No desktop monitor was found".to_string());
        }
        // Apply deferred settings before creating a pet, without replaying real time.
        app.runtime.on_frame(-40.0, &world);
        app.runtime.on_frame(0.0, &world);
        app.spawn(&world);
        app.tick()?;
        unsafe {
            if WTSRegisterSessionNotification(app.controller.0, NOTIFY_FOR_THIS_SESSION) == 0 {
                return Err(win_error("WTSRegisterSessionNotification"));
            }
            RegisterHotKey(
                app.controller.0,
                1,
                MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
                u32::from(b'S'),
            );
        }
        app.sync_timers()?;
        Ok(app)
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn sync_timers(&mut self) -> Result<(), String> {
        let animation = !self.interrupted
            && ((!self.paused && !self.hidden && !self.pomodoro.quiet()) || self.drag.is_some());
        for (id, wanted, current, interval) in [
            (TIMER, animation, &mut self.animation_timer, 40),
            (
                FOCUS_TIMER,
                self.pomodoro.running(),
                &mut self.focus_timer,
                1_000,
            ),
        ] {
            if wanted == *current {
                continue;
            }
            unsafe {
                if wanted {
                    if SetTimer(self.controller.0, id, interval, None) == 0 {
                        return Err(win_error("SetTimer"));
                    }
                } else {
                    KillTimer(self.controller.0, id);
                }
            }
            *current = wanted;
        }
        Ok(())
    }

    fn sync_pomodoro(&mut self) -> Result<(), String> {
        let quiet = self.pomodoro.quiet();
        if quiet != self.runtime.companion_mode() {
            self.release();
        }
        let changed = quiet != self.runtime.companion_mode();
        let world = snapshot_with_windows(false);
        self.runtime.set_companion_mode(quiet, &world);
        self.set_frozen();
        self.sync_timers()?;
        let now = self.now_ms();
        let label = self.pomodoro.label(now);
        self.tray.update(&label);
        if let Some(ui) = self.focus_ui.as_mut() {
            ui.update(&self.pomodoro, now);
        }
        OPEN_MENU.with(|menu| {
            if !menu.get().is_null() {
                unsafe {
                    ModifyMenuW(
                        menu.get(),
                        0,
                        MF_BYPOSITION | MF_STRING | MF_GRAYED,
                        0,
                        wide(&label).as_ptr(),
                    );
                    ModifyMenuW(
                        menu.get(),
                        focus_window::CANCEL as u32,
                        MF_BYCOMMAND
                            | MF_STRING
                            | if matches!(self.pomodoro.status(), Status::Running | Status::Paused)
                            {
                                MF_ENABLED
                            } else {
                                MF_GRAYED
                            },
                        focus_window::CANCEL,
                        wide("结束本轮").as_ptr(),
                    );
                    ModifyMenuW(
                        menu.get(),
                        focus_window::ACTION as u32,
                        MF_BYCOMMAND
                            | MF_STRING
                            | if focus::action_enabled(&self.pomodoro) {
                                MF_ENABLED
                            } else {
                                MF_GRAYED
                            },
                        focus_window::ACTION,
                        wide(focus::action_title(&self.pomodoro)).as_ptr(),
                    );
                }
            }
        });
        self.update_badge(&world)?;
        if changed {
            self.tick()?;
        }
        Ok(())
    }

    fn update_badge(&mut self, world: &Snapshot) -> Result<(), String> {
        let presentation = if self.settings.pomodoro.show_badge && !self.hidden && !self.interrupted
        {
            self.pomodoro.patience(self.now_ms()).and_then(|patience| {
                self.runtime
                    .frames()
                    .first()
                    .and_then(|frame| self.pets.get(&frame.id))
                    .and_then(|pet| {
                        let mut rect = RECT::default();
                        if unsafe { GetWindowRect(pet.window.0, &mut rect) } == 0 {
                            return None;
                        }
                        let pet_bounds = WorkRect {
                            x: rect.left,
                            y: rect.top,
                            width: rect.right.saturating_sub(rect.left),
                            height: rect.bottom.saturating_sub(rect.top),
                        };
                        let card = focus::patience_card(pet_bounds, &world.screens, unsafe {
                            GetDpiForWindow(pet.window.0)
                        });
                        Some((card, patience))
                    })
            })
        } else {
            None
        };
        if presentation.is_some() && self.badge.is_none() {
            self.badge = Some(focus_window::Badge::new()?);
        }
        if let Some(badge) = self.badge.as_mut() {
            badge.update(
                presentation
                    .as_ref()
                    .map(|(card, patience)| (*card, patience)),
            );
        }
        Ok(())
    }

    fn focus_tick(&mut self) -> Result<(), String> {
        if let Some(done) = (!self.pomodoro.interrupted())
            .then(|| self.pomodoro.update(self.now_ms()))
            .flatten()
        {
            let (title, body) = focus::completion_message(done.phase);
            if self.settings.pomodoro.notifications {
                self.tray.notify(title, body);
            }
            if self.settings.pomodoro.sound {
                unsafe {
                    MessageBeep(MB_OK);
                }
            }
        }
        self.sync_pomodoro()
    }

    fn focus_command(&mut self, command: usize) -> Result<(), String> {
        let was_running = self.pomodoro.running();
        self.focus_tick()?;
        // A click on the old Pause action at the deadline must not start a
        // different stage merely because the state settled during dispatch.
        if command == focus_window::ACTION
            && was_running
            && self.pomodoro.status() == Status::Completed
        {
            return Ok(());
        }
        match command {
            focus_window::ACTION => match self.pomodoro.status() {
                Status::Running => self.pomodoro.pause(self.now_ms()),
                Status::Paused => {
                    self.pomodoro.resume(self.now_ms());
                }
                Status::Idle | Status::Completed => {
                    self.pomodoro
                        .start_next(self.now_ms(), &self.settings.pomodoro);
                }
            },
            focus_window::CANCEL => self.pomodoro.cancel(),
            focus_window::SETTINGS => {
                if self.focus_ui.is_none() {
                    self.focus_ui = Some(focus_window::FocusWindow::new(
                        self.controller.0,
                        &self.settings.pomodoro,
                    )?);
                }
                if let Some(ui) = self.focus_ui.as_ref() {
                    ui.show();
                }
            }
            _ if focus_window::FocusWindow::is_save(command) => {
                if let Some(ui) = self.focus_ui.as_ref() {
                    match ui.read(&self.settings.pomodoro) {
                        Ok(preferences) => {
                            let mut updated = self.settings.clone();
                            updated.pomodoro = preferences;
                            match settings::save(&self.profile, &updated) {
                                Ok(()) => self.settings = updated,
                                Err(error) => queue(Event::Error(format!("设置未保存：{error}"))),
                            }
                        }
                        Err(error) => queue(Event::Error(error)),
                    }
                }
            }
            _ => {}
        }
        self.sync_pomodoro()?;
        self.tick()
    }

    fn spawn(&mut self, world: &Snapshot) {
        if let Some(screen) = world
            .screens
            .iter()
            .find(|screen| {
                world.cursor.x >= screen.x
                    && world.cursor.x < screen.x + screen.width
                    && world.cursor.y >= screen.y
                    && world.cursor.y < screen.y + screen.height
            })
            .or(world.screens.first())
        {
            let work = screen.work.unwrap_or(WorkRect {
                x: screen.x,
                y: screen.y,
                width: screen.width,
                height: screen.height,
            });
            self.runtime.push(Command::SpawnAt(Point::new(
                work.x + work.width / 2,
                work.y + work.height / 2,
            )));
        }
    }

    fn release(&mut self) {
        if let Some(drag) = self.drag.take() {
            unsafe {
                ReleaseCapture();
            }
            if drag.frozen_offset.is_none() {
                self.runtime.push(Command::Release {
                    mascot: Some(drag.id),
                    button: MouseButton::Left,
                });
            }
        }
    }

    fn tick(&mut self) -> Result<(), String> {
        let world = snapshot_with_windows(!self.runtime.paused() && !self.runtime.companion_mode());
        if let Some(Drag {
            id,
            frozen_offset: Some(offset),
        }) = &self.drag
        {
            self.runtime.push(Command::MoveFrozen {
                id: *id,
                anchor: Point::new(world.cursor.x + offset.x, world.cursor.y + offset.y),
            });
        }
        let now = self.origin.elapsed().as_secs_f64() * 1000.0;
        self.runtime.on_frame(now, &world);
        let frames = self.runtime.frames();
        self.pets
            .retain(|id, _| frames.iter().any(|frame| frame.id == *id));
        for frame in &frames {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.pets.entry(frame.id) {
                entry.insert(Pet {
                    window: window(true, Some(frame.id))?,
                    bitmap: None,
                    last: None,
                });
            }
            self.pets.get_mut(&frame.id).expect("attached pet").update(
                &self.atlas,
                frame,
                self.hidden,
            )?;
        }
        self.update_badge(&world)?;
        if self
            .seconds
            .is_some_and(|seconds| self.origin.elapsed().as_secs_f64() >= seconds)
        {
            queue(Event::Quit);
        }
        Ok(())
    }

    fn set_frozen(&mut self) {
        self.runtime
            .set_paused(self.paused || self.hidden || self.interrupted);
    }

    fn open(&mut self, path: &Path) {
        match Loaded::open(path, None, self.scale_override) {
            Ok(loaded) => {
                self.release();
                unsafe {
                    ReleaseCapture();
                }
                self.pets.clear();
                self.runtime = loaded.runtime(42);
                let world = snapshot();
                let now = self.origin.elapsed().as_secs_f64() * 1000.0;
                self.runtime.on_frame(now - 40.0, &world);
                self.runtime.on_frame(now, &world);
                self.atlas = Atlas::new(&loaded.character, loaded.images, loaded.scale);
                self.fall = loaded.character.behavior_schema.required_behaviors()[1].to_string();
                self.set_frozen();
                self.runtime
                    .set_companion_mode(self.pomodoro.quiet(), &world);
                self.spawn(&world);
            }
            Err(error) => queue(Event::Error(error)),
        }
    }

    fn event(&mut self, event: Event) -> Result<bool, String> {
        match event {
            Event::Tick => self.tick()?,
            Event::FocusTick => self.focus_tick()?,
            Event::FocusCommand(command) => self.focus_command(command)?,
            Event::Interrupted(reason, active) => {
                self.pomodoro
                    .set_interruption(reason, active, self.now_ms());
                match reason {
                    Interruption::Sleep => self.sleep = active,
                    Interruption::Locked => self.locked = active,
                    Interruption::Session => self.session_inactive = active,
                };
                self.interrupted = self.sleep || self.locked || self.session_inactive;
                self.release();
                self.focus_tick()?;
                self.tick()?;
            }
            Event::Quit => return Ok(false),
            Event::Press(id) => {
                let world = snapshot();
                let offset = if self.runtime.paused() || self.runtime.companion_mode() {
                    self.runtime
                        .frames()
                        .iter()
                        .find(|frame| frame.id == id)
                        .map(|frame| {
                            Point::new(
                                frame.anchor.x - world.cursor.x,
                                frame.anchor.y - world.cursor.y,
                            )
                        })
                } else {
                    self.runtime.push(Command::Press {
                        mascot: Some(id),
                        button: MouseButton::Left,
                    });
                    None
                };
                self.drag = Some(Drag {
                    id,
                    frozen_offset: offset,
                });
            }
            Event::Release => self.release(),
            Event::ToggleHidden => {
                self.release();
                self.hidden = !self.hidden;
                self.set_frozen();
                self.tick()?;
            }
            Event::Open(path) => {
                self.open(&path);
                self.tick()?;
            }
            Event::Menu(_) | Event::ChoosePack | Event::Error(_) => {
                unreachable!("native modal UI is dispatched without an App borrow")
            }
            Event::Command(command, id) => {
                self.release();
                match command {
                    focus_window::ACTION | focus_window::CANCEL | focus_window::SETTINGS => {
                        self.focus_command(command)?
                    }
                    OPEN => queue(Event::ChoosePack),
                    ADD => self.spawn(&snapshot()),
                    PAUSE => {
                        self.paused = !self.paused;
                        self.set_frozen();
                    }
                    HIDE => {
                        self.hidden = !self.hidden;
                        self.set_frozen();
                    }
                    RESCUE => {
                        self.hidden = false;
                        self.set_frozen();
                        let world = snapshot();
                        if let Some(screen) = world.screens.first() {
                            let work = screen.work.unwrap_or(WorkRect {
                                x: screen.x,
                                y: screen.y,
                                width: screen.width,
                                height: screen.height,
                            });
                            let frames = self.runtime.frames();
                            if frames.is_empty() {
                                self.spawn(&world);
                            }
                            for frame in frames {
                                self.runtime.push(Command::MoveFrozen {
                                    id: frame.id,
                                    anchor: Point::new(
                                        work.x + work.width / 2,
                                        work.y + work.height / 2,
                                    ),
                                });
                                self.runtime.push(Command::SetBehavior {
                                    id: Some(frame.id),
                                    name: self.fall.clone(),
                                });
                            }
                        }
                    }
                    REMOVE => {
                        if let Some(id) = id {
                            self.runtime.push(Command::Dismiss(id));
                        }
                    }
                    QUIT => return Ok(false),
                    _ => {}
                }
                self.tick()?;
            }
        }
        self.sync_timers()?;
        Ok(true)
    }
}

// Dispatching a menu releases the App borrow before TrackPopupMenu enters its
// nested message loop. Reentrant timer messages can then update the countdown,
// complete phases, and issue reminders without aliased mutable references.
fn dispatch_pending() {
    loop {
        let available = APP.with(|slot| slot.try_borrow_mut().is_ok_and(|slot| slot.is_some()));
        if !available {
            return;
        }
        let Some(event) = EVENTS.with_borrow_mut(VecDeque::pop_front) else {
            return;
        };
        if let Event::Error(message) = event {
            show_error(&message);
            continue;
        }
        if let Event::ChoosePack = event {
            let owner = APP.with_borrow(|slot| slot.as_ref().expect("running app").controller.0);
            if let Some(path) = choose_pack(owner) {
                queue(Event::Open(path));
            }
            continue;
        }
        if let Event::Menu(id) = event {
            APP.with_borrow_mut(|slot| slot.as_mut().expect("running app").release());
            let state = APP.with_borrow(|app| {
                let app = app.as_ref().expect("running app");
                (
                    app.controller.0,
                    app.paused,
                    app.hidden,
                    app.pomodoro.label(app.now_ms()),
                    focus::action_title(&app.pomodoro),
                    focus::action_enabled(&app.pomodoro),
                    matches!(app.pomodoro.status(), Status::Running | Status::Paused),
                )
            });
            let command = popup(
                state.0, id, state.1, state.2, &state.3, state.4, state.5, state.6,
            );
            queue(Event::Command(command, id));
            continue;
        }
        let result = APP.with_borrow_mut(|app| app.as_mut().expect("running app").event(event));
        match result {
            Ok(true) => {}
            Ok(false) => {
                unsafe {
                    PostQuitMessage(0);
                }
                return;
            }
            Err(error) => {
                FAILURE.with_borrow_mut(|failure| *failure = Some(error));
                unsafe {
                    PostQuitMessage(1);
                }
                return;
            }
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        unsafe {
            KillTimer(self.controller.0, TIMER);
            KillTimer(self.controller.0, FOCUS_TIMER);
            KillTimer(self.controller.0, DEADLINE_TIMER);
            WTSUnRegisterSessionNotification(self.controller.0);
            UnregisterHotKey(self.controller.0, 1);
            ReleaseCapture();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn popup(
    hwnd: HWND,
    mascot: Option<MascotId>,
    paused: bool,
    hidden: bool,
    label: &str,
    action: &str,
    enabled: bool,
    cancellable: bool,
) -> usize {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return 0;
        }
        OPEN_MENU.with(|open| open.set(menu));
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide(label).as_ptr());
        AppendMenuW(
            menu,
            MF_STRING | if enabled { MF_ENABLED } else { MF_GRAYED },
            focus_window::ACTION,
            wide(action).as_ptr(),
        );
        AppendMenuW(
            menu,
            MF_STRING | if cancellable { MF_ENABLED } else { MF_GRAYED },
            focus_window::CANCEL,
            wide("结束本轮").as_ptr(),
        );
        AppendMenuW(
            menu,
            MF_STRING,
            focus_window::SETTINGS,
            wide("番茄钟设置…").as_ptr(),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        for (id, text) in [
            (OPEN, "打开角色 ZIP…"),
            (ADD, "再添加一只"),
            (PAUSE, if paused { "继续" } else { "暂停" }),
            (
                HIDE,
                if hidden {
                    "显示桌宠"
                } else {
                    "隐藏桌宠"
                },
            ),
            (RESCUE, "找回桌宠"),
        ] {
            AppendMenuW(menu, MF_STRING, id, wide(text).as_ptr());
        }
        if mascot.is_some() {
            AppendMenuW(menu, MF_STRING, REMOVE, wide("移除这只").as_ptr());
        }
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, QUIT, wide("退出 shimeji2").as_ptr());
        let mut point = POINT::default();
        GetCursorPos(&mut point);
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            null(),
        ) as usize;
        PostMessageW(hwnd, WM_NULL, 0, 0);
        OPEN_MENU.with(|open| open.set(null_mut()));
        DestroyMenu(menu);
        command
    }
}

fn choose_pack(hwnd: HWND) -> Option<PathBuf> {
    let mut path = vec![0u16; 32768];
    let filter = wide("Shimeji ZIP\0*.zip\0All files\0*.*\0");
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: filter.as_ptr(),
        lpstrFile: path.as_mut_ptr(),
        nMaxFile: path.len() as u32,
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        return None;
    }
    let length = path.iter().position(|ch| *ch == 0)?;
    Some(PathBuf::from(OsString::from_wide(&path[..length])))
}

pub fn run() -> Result<(), String> {
    let options = Options::parse(std::env::args_os().skip(1))?;
    if options.help {
        unsafe {
            MessageBoxW(
                null_mut(),
                wide(HELP).as_ptr(),
                wide("shimeji2").as_ptr(),
                MB_OK,
            );
        }
        return Ok(());
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let source = options
        .pack
        .clone()
        .unwrap_or_else(|| bundled_pack(&executable));
    let loaded = Loaded::open(&source, options.character.as_deref(), options.scale)?;
    if options.check_pack {
        // Smoke-check shared runtime + rendering without opening windows.
        let mut runtime = loaded.runtime(42);
        let world = Snapshot {
            screens: vec![ScreenSample {
                id: "check".into(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                work: None,
            }],
            cursor: Point::new(500, 500),
            active_window: None,
            multiscreen: true,
        };
        let atlas = Atlas::new(&loaded.character, loaded.images, loaded.scale);
        runtime.on_frame(-40.0, &world);
        runtime.on_frame(0.0, &world);
        runtime.push(Command::SpawnAt(Point::new(800, 300)));
        let mut rendered = false;
        for tick in 0..250 {
            runtime.on_frame(f64::from(tick * 40), &world);
            for frame in runtime.frames() {
                rendered |= atlas.draw(&frame)?.is_some();
            }
        }
        return if rendered {
            Ok(())
        } else {
            Err("No visible pose in the first 250 ticks".into())
        };
    }
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let class_name = wide("Shimeji2Windows");
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: GetModuleHandleW(null()),
            lpszClassName: class_name.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(win_error("RegisterClassW"));
        }
    }
    let taskbar_created = unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) };
    let mut app = App::new(loaded, &options)?;
    if let Some(seconds) = options.seconds {
        // Bounded smoke tests still exit with all periodic sources stopped.
        unsafe {
            if SetTimer(
                app.controller.0,
                DEADLINE_TIMER,
                (seconds * 1000.0).ceil().clamp(1.0, f64::from(u32::MAX)) as u32,
                None,
            ) == 0
            {
                return Err(win_error("SetTimer(smoke deadline)"));
            }
        }
    }
    app.sync_pomodoro()?;
    APP.with_borrow_mut(|slot| *slot = Some(app));
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if result == -1 {
            return Err(win_error("GetMessageW"));
        }
        if result == 0 {
            break;
        }
        if taskbar_created != 0 && message.message == taskbar_created {
            APP.with_borrow(|slot| slot.as_ref().expect("running app").tray.add())?;
        }
        let dialog_handled = APP.with_borrow(|slot| {
            slot.as_ref()
                .and_then(|app| app.focus_ui.as_ref())
                .is_some_and(|ui| ui.is_dialog_message(&message))
        });
        if !dialog_handled {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        dispatch_pending();
    }
    APP.with_borrow_mut(|slot| {
        slot.take();
    });
    if let Some(error) = FAILURE.with_borrow_mut(Option::take) {
        Err(error)
    } else {
        Ok(())
    }
}

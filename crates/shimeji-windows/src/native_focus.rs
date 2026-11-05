//! Modeless native settings and an opt-in, non-activating companion label.

use super::*;
use crate::focus;
use shimeji_app::pomodoro::{Patience, Preferences};
use std::rc::Rc;
use windows_sys::Win32::UI::Controls::EM_SETLIMITTEXT;

pub(super) const ACTION: usize = 100;
pub(super) const CANCEL: usize = 101;
pub(super) const SETTINGS: usize = 102;
const SAVE: usize = 103;
const FOCUS_MINUTES: usize = 201;
const SHORT_MINUTES: usize = 202;
const LONG_MINUTES: usize = 203;
const NOTIFICATIONS: usize = 204;
const SOUND: usize = 205;
const BADGE: usize = 206;

unsafe extern "system" fn settings_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            if (wp >> 16) == BN_CLICKED as usize {
                queue(Event::FocusCommand(wp & 0xffff));
            }
            0
        }
        WM_CLOSE => {
            unsafe {
                ShowWindow(hwnd, SW_HIDE);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wp, lp) },
    }
}

fn control(
    parent: HWND,
    class: &str,
    title: &str,
    style: u32,
    rect: [i32; 4],
    id: usize,
) -> Result<HWND, String> {
    let scale = unsafe { GetDpiForWindow(parent) }.max(96) as f64 / 96.0;
    let [x, y, width, height] = rect.map(|value| (f64::from(value) * scale).round() as i32);
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(title).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            x,
            y,
            width,
            height,
            parent,
            id as HMENU,
            GetModuleHandleW(null()),
            null(),
        )
    };
    if hwnd.is_null() {
        return Err(win_error("CreateWindowExW(control)"));
    }
    unsafe {
        SendMessageW(
            hwnd,
            WM_SETFONT,
            GetStockObject(DEFAULT_GUI_FONT) as usize,
            1,
        );
    }
    Ok(hwnd)
}

pub(super) struct FocusWindow {
    window: Window,
    status: HWND,
    action: HWND,
    cancel: HWND,
    fields: [HWND; 3],
    notifications: HWND,
    sound: HWND,
    badge: HWND,
    last_label: String,
}

impl FocusWindow {
    pub(super) fn new(owner: HWND, preferences: &Preferences) -> Result<Self, String> {
        let class_name = wide("Shimeji2FocusSettings");
        let class = WNDCLASSW {
            lpfnWndProc: Some(settings_proc),
            hInstance: unsafe { GetModuleHandleW(null()) },
            lpszClassName: class_name.as_ptr(),
            hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
            hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0
            && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
        {
            return Err(win_error("RegisterClassW(focus settings)"));
        }
        let scale = f64::from(unsafe { GetDpiForSystem() }.max(96)) / 96.0;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: (460.0 * scale).round() as i32,
            bottom: (470.0 * scale).round() as i32,
        };
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        unsafe {
            AdjustWindowRectEx(&mut rect, style, 0, WS_EX_CONTROLPARENT);
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_CONTROLPARENT,
                class_name.as_ptr(),
                wide("番茄钟 · 专注陪伴").as_ptr(),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                rect.right - rect.left,
                rect.bottom - rect.top,
                owner,
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        if hwnd.is_null() {
            return Err(win_error("CreateWindowExW(focus settings)"));
        }
        let window = Window(hwnd);
        control(hwnd, "STATIC", "专注陪伴", 0, [24, 20, 400, 30], 0)?;
        let status = control(
            hwnd,
            "STATIC",
            "准备好后，开始一轮专注。",
            0,
            [24, 58, 410, 28],
            0,
        )?;
        let action = control(
            hwnd,
            "BUTTON",
            "开始专注",
            WS_TABSTOP | BS_PUSHBUTTON as u32,
            [24, 94, 180, 34],
            ACTION,
        )?;
        let cancel = control(
            hwnd,
            "BUTTON",
            "结束本轮",
            WS_TABSTOP | BS_PUSHBUTTON as u32,
            [220, 94, 180, 34],
            CANCEL,
        )?;
        let mut fields = [null_mut(); 3];
        for (index, (title, id)) in [
            ("专注时长（分钟）", FOCUS_MINUTES),
            ("短休息（分钟）", SHORT_MINUTES),
            ("长休息（分钟）", LONG_MINUTES),
        ]
        .into_iter()
        .enumerate()
        {
            let y = 152 + index as i32 * 42;
            control(hwnd, "STATIC", title, 0, [24, y, 238, 26], 0)?;
            fields[index] = control(
                hwnd,
                "EDIT",
                "",
                WS_TABSTOP | WS_BORDER | ES_NUMBER as u32 | ES_AUTOHSCROLL as u32,
                [280, y - 2, 120, 28],
                id,
            )?;
            unsafe {
                SendMessageW(fields[index], EM_SETLIMITTEXT, 3, 0);
            }
        }
        let notifications = control(
            hwnd,
            "BUTTON",
            "到点显示系统提醒",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            [24, 284, 400, 26],
            NOTIFICATIONS,
        )?;
        let sound = control(
            hwnd,
            "BUTTON",
            "到点播放提示音",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            [24, 316, 400, 26],
            SOUND,
        )?;
        let badge = control(
            hwnd,
            "BUTTON",
            "头顶耐心条与倒计时",
            WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            [24, 348, 400, 26],
            BADGE,
        )?;
        control(
            hwnd,
            "STATIC",
            "每完成 4 轮专注提供长休息；下一阶段手动开始。\n时长修改从下一阶段生效。锁屏或休眠后请手动继续。",
            0,
            [24, 382, 412, 42],
            0,
        )?;
        control(
            hwnd,
            "BUTTON",
            "保存设置",
            WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            [280, 430, 120, 30],
            SAVE,
        )?;
        let result = Self {
            window,
            status,
            action,
            cancel,
            fields,
            notifications,
            sound,
            badge,
            last_label: String::new(),
        };
        result.set_preferences(preferences);
        Ok(result)
    }

    fn set_preferences(&self, preferences: &Preferences) {
        for (field, value) in self.fields.into_iter().zip([
            preferences.focus_minutes,
            preferences.short_break_minutes,
            preferences.long_break_minutes,
        ]) {
            unsafe {
                SetWindowTextW(field, wide(&value.to_string()).as_ptr());
            }
        }
        for (field, checked) in [
            (self.notifications, preferences.notifications),
            (self.sound, preferences.sound),
            (self.badge, preferences.show_badge),
        ] {
            unsafe {
                SendMessageW(field, BM_SETCHECK, usize::from(checked), 0);
            }
        }
    }

    pub(super) fn show(&self) {
        unsafe {
            ShowWindow(self.window.0, SW_SHOWNORMAL);
            SetForegroundWindow(self.window.0);
        }
    }

    pub(super) fn update(&mut self, timer: &Pomodoro, now: u64) {
        let label = format!(
            "{} · 已完成 {} 轮",
            timer.label(now),
            timer.completed_focus()
        );
        if self.last_label != label {
            unsafe {
                SetWindowTextW(self.status, wide(&label).as_ptr());
            }
            self.last_label = label;
        }
        unsafe {
            SetWindowTextW(self.action, wide(focus::action_title(timer)).as_ptr());
            EnableWindow(self.action, i32::from(focus::action_enabled(timer)));
            EnableWindow(
                self.cancel,
                i32::from(matches!(timer.status(), Status::Running | Status::Paused)),
            );
        }
    }

    pub(super) fn read(&self, previous: &Preferences) -> Result<Preferences, String> {
        let values = self.fields.map(|field| {
            let mut text = [0; 16];
            let len = unsafe { GetWindowTextW(field, text.as_mut_ptr(), text.len() as i32) };
            String::from_utf16_lossy(&text[..len.max(0) as usize])
        });
        let mut result = focus::parse_minutes([&values[0], &values[1], &values[2]], previous)?;
        let checked = |field| unsafe { SendMessageW(field, BM_GETCHECK, 0, 0) == 1 };
        result.notifications = checked(self.notifications);
        result.sound = checked(self.sound);
        result.show_badge = checked(self.badge);
        Ok(result)
    }

    pub(super) fn is_dialog_message(&self, message: &MSG) -> bool {
        unsafe {
            IsWindowVisible(self.window.0) != 0 && IsDialogMessageW(self.window.0, message) != 0
        }
    }

    pub(super) fn is_save(command: usize) -> bool {
        command == SAVE
    }
}

// The HWND owns one Rc reference from WM_NCCREATE to WM_NCDESTROY. The
// creator/Badge owns another. Callbacks only copy this Cell's value; they never
// dereference a raw pointer into Badge while Badge::update holds &mut self.
#[derive(Debug, Clone, Copy)]
struct BadgePaint {
    dpi: u32,
    per_mille: u16,
    color: [u8; 3],
}

unsafe extern "system" fn badge_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let creation = unsafe { &*(lp as *const CREATESTRUCTW) };
            let state = creation.lpCreateParams.cast::<Cell<BadgePaint>>();
            if state.is_null() {
                return 0;
            }
            unsafe {
                Rc::increment_strong_count(state);
                SetLastError(0);
                if SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize) == 0
                    && GetLastError() != 0
                {
                    drop(Rc::from_raw(state));
                    return 0;
                }
            }
        }
        WM_NCDESTROY => {
            let state =
                unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Cell<BadgePaint> };
            if !state.is_null() {
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    drop(Rc::from_raw(state));
                }
            }
        }
        WM_PAINT => {
            let state =
                unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Cell<BadgePaint> };
            let presentation = if state.is_null() {
                BadgePaint {
                    dpi: 96,
                    per_mille: 0,
                    color: [0, 0, 0],
                }
            } else {
                // Copy before any GDI/User32 operation can reenter this proc.
                unsafe { (*state).get() }
            };
            paint_badge(hwnd, presentation);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        WM_NCHITTEST => return HTTRANSPARENT as LRESULT,
        WM_DPICHANGED => {
            let state =
                unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Cell<BadgePaint> };
            if !state.is_null() {
                let mut presentation = unsafe { (*state).get() };
                presentation.dpi = (wp as u32 & 0xffff).clamp(96, 768);
                unsafe {
                    (*state).set(presentation);
                }
            }
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}

fn color_ref([red, green, blue]: [u8; 3]) -> COLORREF {
    u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16)
}

fn fill_badge_rect(dc: HDC, rect: &RECT, color: [u8; 3]) {
    let brush = unsafe { CreateSolidBrush(color_ref(color)) };
    if !brush.is_null() {
        unsafe {
            FillRect(dc, rect, brush);
            DeleteObject(brush);
        }
    }
}

fn paint_badge(hwnd: HWND, presentation: BadgePaint) {
    let mut paint = PAINTSTRUCT::default();
    let dc = unsafe { BeginPaint(hwnd, &mut paint) };
    if !dc.is_null() {
        let mut bounds = RECT::default();
        unsafe {
            GetClientRect(hwnd, &mut bounds);
        }
        fill_badge_rect(dc, &bounds, [250, 250, 252]);
        let px =
            |logical: i32| ((i64::from(logical) * i64::from(presentation.dpi) + 48) / 96) as i32;
        let mut text = [0; 160];
        let length = unsafe { GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32) };
        let font = unsafe {
            CreateFontW(
                -px(12),
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                u32::from(DEFAULT_CHARSET),
                0,
                0,
                u32::from(CLEARTYPE_QUALITY),
                u32::from(DEFAULT_PITCH),
                wide("Microsoft YaHei UI").as_ptr(),
            )
        };
        let selected_font = if font.is_null() {
            unsafe { GetStockObject(DEFAULT_GUI_FONT) }
        } else {
            font
        };
        let previous = unsafe { SelectObject(dc, selected_font) };
        let mut header = RECT {
            left: px(8),
            top: px(3),
            right: (bounds.right - px(8)).max(px(8)),
            bottom: px(27).min(bounds.bottom),
        };
        unsafe {
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, color_ref([35, 38, 44]));
            DrawTextW(
                dc,
                text.as_ptr(),
                length,
                &mut header,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
            SelectObject(dc, previous);
            if !font.is_null() {
                DeleteObject(font);
            }
        }
        let track = RECT {
            left: px(8),
            top: px(32).min(bounds.bottom),
            right: (bounds.right - px(8)).max(px(8)),
            bottom: px(38).min(bounds.bottom),
        };
        fill_badge_rect(dc, &track, [221, 225, 231]);
        let fill = RECT {
            right: track.left
                + focus::patience_fill_width(track.right - track.left, presentation.per_mille),
            ..track
        };
        if fill.right > fill.left {
            fill_badge_rect(dc, &fill, presentation.color);
        }
    }
    unsafe {
        EndPaint(hwnd, &paint);
    }
}

pub(super) struct Badge {
    window: Window,
    paint: Rc<Cell<BadgePaint>>,
    last: Option<(focus::PatienceCard, Patience)>,
}

impl Badge {
    pub(super) fn new() -> Result<Self, String> {
        let class_name = wide("Shimeji2PatienceBadge");
        let class = WNDCLASSW {
            lpfnWndProc: Some(badge_proc),
            hInstance: unsafe { GetModuleHandleW(null()) },
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0
            && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
        {
            return Err(win_error("RegisterClassW(patience badge)"));
        }
        let paint = Rc::new(Cell::new(BadgePaint {
            dpi: 96,
            per_mille: 0,
            color: [0, 0, 0],
        }));
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_LAYERED
                    | WS_EX_TRANSPARENT,
                class_name.as_ptr(),
                wide("").as_ptr(),
                WS_POPUP,
                0,
                0,
                224,
                44,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                Rc::as_ptr(&paint).cast(),
            )
        };
        if hwnd.is_null() {
            return Err(win_error("CreateWindowExW(patience badge)"));
        }
        let window = Window(hwnd);
        // Layered + WS_EX_TRANSPARENT forwards mouse events even to windows in
        // other processes; HTTRANSPARENT alone only covers same-thread windows.
        if unsafe { SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA) } == 0 {
            return Err(win_error("SetLayeredWindowAttributes(patience badge)"));
        }
        Ok(Self {
            window,
            paint,
            last: None,
        })
    }

    pub(super) fn update(&mut self, presentation: Option<(focus::PatienceCard, &Patience)>) {
        let Some((card, patience)) = presentation else {
            if self.last.take().is_some() {
                unsafe {
                    ShowWindow(self.window.0, SW_HIDE);
                }
            }
            return;
        };
        if self
            .last
            .as_ref()
            .is_some_and(|(last_card, last_patience)| {
                *last_card == card && last_patience == patience
            })
        {
            return;
        }
        let repaint = self.last.as_ref().is_none_or(|(last_card, last_patience)| {
            last_card.dpi != card.dpi
                || last_card.width != card.width
                || last_card.height != card.height
                || last_patience != patience
        });
        let relabel = self
            .last
            .as_ref()
            .is_none_or(|(_, last_patience)| last_patience.label != patience.label);
        self.paint.set(BadgePaint {
            dpi: card.dpi,
            per_mille: patience.per_mille,
            color: patience.color.rgb(),
        });
        unsafe {
            if relabel {
                SetWindowTextW(self.window.0, wide(&patience.label).as_ptr());
            }
            SetWindowPos(
                self.window.0,
                HWND_TOPMOST,
                card.position.x,
                card.position.y,
                card.width,
                card.height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            if repaint {
                InvalidateRect(self.window.0, null(), 0);
            }
        }
        self.last = Some((card, patience.clone()));
    }
}

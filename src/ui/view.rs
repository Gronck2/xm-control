//! The Win32 view: one fixed-size, per-monitor-DPI-aware window made of native controls (comctl32 v6 look comes from the manifest, see `build.rs`).
//! It draws `ViewState` from `model` and forwards clicks; the device worker lives in its own thread and wakes the window with a private message,
//! so the UI thread never waits on Bluetooth (the one exception: the paired-device list is read synchronously, it is a local registry-backed query).

use super::model::Model;
use crate::bt::enumerate::{Device, paired_devices};
use crate::device::{DeviceHandle, Event};
use std::ptr::{null, null_mut};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Posted by the worker thread's wake callback: "events are waiting".
const WM_DEVICE_EVENT: u32 = WM_APP + 1;
/// How long closing the window waits for the worker to end after the disconnect request.
const CLOSE_WAIT: Duration = Duration::from_millis(1500);

const ID_REFRESH: usize = 102;
const ID_CONNECT: usize = 103;
const ID_MAC: usize = 104;
const CLIENT: (i32, i32) = (420, 338);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

struct App {
    hwnd: HWND,
    model: Model,
    devices: Vec<Device>,
    handle: Option<DeviceHandle>,
    events: Option<Receiver<Event>>,
    dpi: u32,
    font: HFONT,
    /// Every child with its rectangle in 96-dpi units (x, y, w, h); scaled by `relayout`.
    children: Vec<(HWND, [i32; 4])>,
    combo: HWND,
    refresh: HWND,
    connect: HWND,
    status: HWND,
    model_value: HWND,
    firmware_value: HWND,
    mac_value: HWND,
    mac_button: HWND,
    battery_label: HWND,
    battery_value: HWND,
}

impl App {
    fn new(hwnd: HWND) -> App {
        let mut children = Vec::new();
        let mut add = |class: &str, text: &str, style: u32, id: usize, rect: [i32; 4]| {
            // SAFETY: plain child-window creation with valid, NUL-terminated wide strings; the parent is a live window.
            let h = unsafe {
                CreateWindowExW(0, wide(class).as_ptr(), wide(text).as_ptr(), WS_CHILD | WS_VISIBLE | style, 0, 0, 0, 0, hwnd, id as _, null_mut(), null())
            };
            children.push((h, rect));
            h
        };
        let tab = WS_TABSTOP;
        let button = BS_PUSHBUTTON as u32 | tab;
        add("STATIC", "Headset", 0, 0, [12, 16, 60, 18]);
        let combo = add("COMBOBOX", "", CBS_DROPDOWNLIST as u32 | WS_VSCROLL | tab, 101, [76, 12, 250, 200]);
        let refresh = add("BUTTON", "Refresh", button, ID_REFRESH, [336, 11, 72, 24]);
        let connect = add("BUTTON", "Connect", button, ID_CONNECT, [12, 44, 100, 26]);
        let status = add("STATIC", "", 0, 0, [124, 49, 284, 18]);
        add("BUTTON", "Device", BS_GROUPBOX as u32, 0, [12, 84, 396, 112]);
        add("STATIC", "Model", 0, 0, [24, 106, 70, 18]);
        let model_value = add("STATIC", "", 0, 0, [100, 106, 290, 18]);
        add("STATIC", "Firmware", 0, 0, [24, 128, 70, 18]);
        let firmware_value = add("STATIC", "", 0, 0, [100, 128, 290, 18]);
        add("STATIC", "MAC", 0, 0, [24, 150, 70, 18]);
        let mac_value = add("STATIC", "", 0, 0, [100, 150, 210, 18]);
        let mac_button = add("BUTTON", "Show", button, ID_MAC, [320, 147, 70, 22]);
        let battery_label = add("STATIC", "Battery", 0, 0, [24, 172, 70, 18]);
        let battery_value = add("STATIC", "", 0, 0, [100, 172, 290, 18]);
        // Empty sections for the feature panels (tasks 0013 and 0014).
        add("BUTTON", "Sound", BS_GROUPBOX as u32, 0, [12, 206, 396, 56]);
        add("BUTTON", "Controls", BS_GROUPBOX as u32, 0, [12, 270, 396, 56]);
        let mut app = App {
            hwnd,
            model: Model::new(),
            devices: Vec::new(),
            handle: None,
            events: None,
            // SAFETY: `hwnd` is a live window.
            dpi: unsafe { GetDpiForWindow(hwnd) },
            font: null_mut(),
            children,
            combo,
            refresh,
            connect,
            status,
            model_value,
            firmware_value,
            mac_value,
            mac_button,
            battery_label,
            battery_value,
        };
        app.refresh_devices();
        app.relayout();
        app.fit_window();
        app.render();
        app
    }

    fn scale(&self, v: i32) -> i32 {
        v * self.dpi as i32 / 96
    }

    /// New font and child positions for the current DPI.
    fn relayout(&mut self) {
        // SAFETY: the old font is deleted only after every child got the new one; all handles are ours and live.
        unsafe {
            let new = CreateFontW(
                -(self.dpi as i32 * 9 / 72),
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as _,
                0,
                0,
                CLEARTYPE_QUALITY as _,
                0,
                wide("Segoe UI").as_ptr(),
            );
            for &(h, [x, y, w, hh]) in &self.children {
                MoveWindow(h, self.scale(x), self.scale(y), self.scale(w), self.scale(hh), 1);
                SendMessageW(h, WM_SETFONT, new as WPARAM, 1);
            }
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
            self.font = new;
        }
    }

    /// Fixed client size, scaled by DPI.
    fn fit_window(&self) {
        let mut r = RECT { left: 0, top: 0, right: self.scale(CLIENT.0), bottom: self.scale(CLIENT.1) };
        // SAFETY: `r` is valid; style matches the one used at creation.
        unsafe {
            AdjustWindowRectExForDpi(&mut r, WINDOW_STYLE, 0, 0, self.dpi);
            SetWindowPos(self.hwnd, null_mut(), 0, 0, r.right - r.left, r.bottom - r.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    fn refresh_devices(&mut self) {
        // shortcut: synchronous call on the UI thread; it queries the local device cache (no inquiry), move it to a thread if it ever stalls.
        let mut placeholder = "(no paired devices)".to_string();
        self.devices = paired_devices().unwrap_or_else(|e| {
            placeholder = format!("({e})");
            Vec::new()
        });
        let names: Vec<String> = if self.devices.is_empty() { vec![placeholder] } else { self.devices.iter().map(|d| d.name.clone()).collect() };
        // SAFETY: `combo` is a live combo box; the strings outlive the calls.
        unsafe {
            SendMessageW(self.combo, CB_RESETCONTENT, 0, 0);
            for n in names {
                SendMessageW(self.combo, CB_ADDSTRING, 0, wide(&n).as_ptr() as LPARAM);
            }
            let pick = self.devices.iter().position(|d| d.name.contains("WH-1000XM")).unwrap_or(0);
            SendMessageW(self.combo, CB_SETCURSEL, pick, 0);
        }
    }

    fn render(&self) {
        let v = self.model.view();
        let set = |h: HWND, s: &str| {
            // SAFETY: `h` is a live control, the string is NUL-terminated.
            unsafe { SetWindowTextW(h, wide(s).as_ptr()) };
        };
        set(self.status, &v.status);
        set(self.connect, v.button_label);
        set(self.model_value, &v.model);
        set(self.firmware_value, &v.firmware);
        set(self.mac_value, &v.mac);
        set(self.mac_button, v.mac_button_label);
        set(self.battery_value, v.battery.as_deref().unwrap_or(""));
        // SAFETY: live controls.
        unsafe {
            EnableWindow(self.combo, v.picker_enabled as i32);
            EnableWindow(self.refresh, v.picker_enabled as i32);
            EnableWindow(self.connect, v.button_enabled as i32);
            let show = if v.battery.is_some() { SW_SHOW } else { SW_HIDE };
            ShowWindow(self.battery_label, show);
            ShowWindow(self.battery_value, show);
        }
    }

    fn on_connect_button(&mut self) {
        if self.model.active() {
            // Dropping the handle stops the worker; the events stay open until it reports `Disconnected`.
            self.handle = None;
            self.model.begin_disconnect();
        } else {
            // SAFETY: `combo` is a live combo box.
            let sel = unsafe { SendMessageW(self.combo, CB_GETCURSEL, 0, 0) };
            let Some(d) = usize::try_from(sel).ok().and_then(|i| self.devices.get(i)) else { return };
            let target = self.hwnd as isize;
            // SAFETY (in the closure): posting to a window that is gone just fails.
            let wake = Box::new(move || unsafe {
                PostMessageW(target as HWND, WM_DEVICE_EVENT, 0, 0);
            });
            let (handle, events) = DeviceHandle::connect(d.mac, Some(wake));
            self.model.begin_connect(d.name.clone(), d.mac);
            (self.handle, self.events) = (Some(handle), Some(events));
        }
        self.render();
    }

    /// Applies every queued event; once the worker is gone the handle and receiver are released.
    fn pump(&mut self) {
        while let Some(rx) = &self.events {
            match rx.try_recv() {
                Ok(e) => self.model.apply(e),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.model.active() {
                        self.model.apply(Event::Disconnected("the device worker ended".into()));
                    }
                }
            }
            if !self.model.active() {
                (self.handle, self.events) = (None, None);
            }
        }
        self.render();
    }

    /// Disconnects and waits briefly for the worker so the RFCOMM socket is closed before the process exits.
    fn shutdown(&mut self) {
        self.handle = None;
        if let Some(rx) = self.events.take() {
            let end = Instant::now() + CLOSE_WAIT;
            while let Ok(e) = rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
                if matches!(e, Event::Disconnected(_)) {
                    break;
                }
            }
        }
    }
}

/// The style used for the main window (fixed size: no sizing border, no maximize).
const WINDOW_STYLE: u32 = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Only these messages touch the App. Everything else (WM_SIZE, WM_CTLCOLOR*, CBN_* notifications, ...) can arrive re-entrantly from our
    // own calls and must not create a second `&mut App`.
    if msg == WM_DESTROY {
        unsafe { PostQuitMessage(0) };
        return 0;
    }
    let clicked = msg == WM_COMMAND && (wparam >> 16) as u32 == BN_CLICKED;
    if !(clicked || matches!(msg, WM_DEVICE_EVENT | WM_DPICHANGED | WM_CLOSE | WM_NCDESTROY)) {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    // SAFETY: the pointer is set once in `run` to a leaked `App` and cleared in WM_NCDESTROY; only the UI thread gets here.
    let app = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App).as_mut() };
    let Some(app) = app else { return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) } };
    match msg {
        WM_COMMAND => match wparam & 0xFFFF {
            ID_CONNECT => app.on_connect_button(),
            ID_REFRESH => app.refresh_devices(),
            ID_MAC => {
                app.model.toggle_mac();
                app.render();
            }
            _ => {}
        },
        WM_DEVICE_EVENT => app.pump(),
        WM_DPICHANGED => {
            // SAFETY: for WM_DPICHANGED lparam points to the suggested RECT.
            let r = unsafe { *(lparam as *const RECT) };
            // SetWindowPos re-enters wndproc; every message it sends is passed to DefWindowProcW above without touching `app`.
            unsafe { SetWindowPos(hwnd, null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE) };
            app.dpi = (wparam >> 16) as u32 & 0xFFFF;
            app.relayout();
        }
        WM_CLOSE => {
            app.shutdown();
            // `app` is dead from here: DestroyWindow re-enters with WM_NCDESTROY, which frees it.
            unsafe { DestroyWindow(hwnd) };
        }
        WM_NCDESTROY => {
            // SAFETY: last message of the window; reclaim the leaked `App` (deleting its font) and make later messages harmless.
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let app = Box::from_raw(app as *mut App);
                DeleteObject(app.font);
            }
        }
        _ => return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
    0
}

/// Opens the window and runs the message loop until it is closed.
pub fn run() -> Result<(), String> {
    let class = wide("XmControlWindow");
    // SAFETY: standard window-class registration and creation; all pointers are valid for the calls.
    unsafe {
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_BTNFACE + 1) as usize as HBRUSH,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            return Err("cannot register the window class".into());
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("XM Control").as_ptr(),
            WINDOW_STYLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            100,
            100,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return Err("cannot create the window".into());
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(Box::new(App::new(hwnd))) as isize);
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        let mut msg = std::mem::zeroed::<MSG>();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    Ok(())
}

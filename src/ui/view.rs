//! The Win32 view: one fixed-size, per-monitor-DPI-aware window made of native controls (comctl32 v6 look comes from the manifest, see `build.rs`).
//! It draws `ViewState` from `model` and forwards clicks; the device worker lives in its own thread and wakes the window with a private message,
//! so the UI thread never waits on Bluetooth (the one exception: the paired-device list is read synchronously, it is a local registry-backed query).

use super::model::{Model, SoundView, SystemView};
use crate::bt::enumerate::{Device, paired_devices};
use crate::device::Command;
use crate::device::ncasm::Mode;
use crate::device::{DeviceHandle, Event};
use std::cell::Cell;
use std::ptr::{null, null_mut};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    ICC_BAR_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, TB_ENDTRACK, TB_THUMBPOSITION, TB_THUMBTRACK, TBM_SETPOS, TBM_SETRANGE, TBS_AUTOTICKS,
    TBS_NOTICKS, TBS_VERT,
};
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Posted by the worker thread's wake callback: "events are waiting".
const WM_DEVICE_EVENT: u32 = WM_APP + 1;
/// Posted by a multipoint click: show the confirmation (wparam: 1 = turn on). Handled with no `&mut App` live, because the dialog runs its own message loop.
const WM_CONFIRM: u32 = WM_APP + 2;
/// Posted with the dialog's answer (wparam: 1 = yes); applied in a fresh handler.
const WM_CONFIRM_ANSWER: u32 = WM_APP + 3;
/// A repeating timer: flushes a paced slider step and drops edits the headset never answered.
const TICK_ID: usize = 1;
const TICK_MS: u32 = 150;
/// How long closing the window waits for the worker to end after the disconnect request.
const CLOSE_WAIT: Duration = Duration::from_millis(1500);

const ID_REFRESH: usize = 102;
const ID_CONNECT: usize = 103;
const ID_MAC: usize = 104;
const ID_NC_MODE: usize = 110; // 110..=112: Off, Noise cancelling, Ambient
const ID_VOICE: usize = 113;
const ID_AUTO: usize = 114;
const ID_SENS: usize = 115;
const ID_EQ_PRESET: usize = 116;
const ID_DSEE: usize = 117;
const ID_STC: usize = 118;
const ID_STC_SENS: usize = 119;
const ID_STC_TIMEOUT: usize = 120;
const ID_POWER: usize = 121;
const ID_AUTOPAUSE: usize = 122;
const ID_MULTIPOINT: usize = 123;
/// `TBM_GETPOS` is `WM_USER`.
const TBM_GETPOS: u32 = WM_USER;
/// Static text style "centered" (not in windows-sys).
const SS_CENTER: u32 = 1;
const CLIENT: (i32, i32) = (828, 556);

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
    nc_ctrls: Vec<HWND>,
    nc_radios: [HWND; 3],
    level_slider: HWND,
    level_value: HWND,
    voice: HWND,
    auto: HWND,
    sens: HWND,
    eq_ctrls: Vec<HWND>,
    eq_preset: HWND,
    eq_sliders: [HWND; 10],
    eq_db: [HWND; 10],
    eq_freq: [HWND; 10],
    /// The preset names now in the combo, so it is refilled only when they change.
    eq_names: Vec<String>,
    dsee: HWND,
    stc: HWND,
    stc_sens: HWND,
    stc_timeout: HWND,
    /// The sensitivity and timeout labels and combos (shown only when both values are known).
    stc_levels: Vec<HWND>,
    power: HWND,
    power_ctrls: Vec<HWND>,
    power_names: Vec<String>,
    autopause: HWND,
    wearing_ctrls: Vec<HWND>,
    wearing_value: HWND,
    multipoint: HWND,
    dev_ctrls: Vec<HWND>,
    dev_list: HWND,
    /// The lines now in the list box, so it is refilled only when they change.
    dev_lines: Vec<String>,
    /// The slider whose last step was a final code other than ENDTRACK: the timer flushes its value if no ENDTRACK follows (wheel).
    step_slider: Option<HWND>,
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
        add("BUTTON", "Sound", BS_GROUPBOX as u32, 0, [12, 206, 396, 336]);
        // noise control; `nc_ctrls` and `eq_ctrls` are hidden together when the headset does not advertise the feature
        let radio = BS_AUTORADIOBUTTON as u32 | tab;
        let check = BS_AUTOCHECKBOX as u32 | tab;
        let mut nc_ctrls = vec![
            add("BUTTON", "Off", radio | WS_GROUP, ID_NC_MODE, [24, 228, 56, 20]),
            add("BUTTON", "Noise cancelling", radio, ID_NC_MODE + 1, [84, 228, 120, 20]),
            add("BUTTON", "Ambient", radio, ID_NC_MODE + 2, [210, 228, 80, 20]),
            add("STATIC", "Ambient level", 0, 0, [24, 258, 86, 18]),
        ];
        let nc_radios = [nc_ctrls[0], nc_ctrls[1], nc_ctrls[2]];
        let level_slider = add("msctls_trackbar32", "", TBS_AUTOTICKS | tab, 0, [112, 252, 246, 28]);
        let level_value = add("STATIC", "", 0, 0, [364, 258, 32, 18]);
        let voice = add("BUTTON", "Focus on voice", check, ID_VOICE, [24, 284, 150, 20]);
        let auto = add("BUTTON", "Auto ambient", check, ID_AUTO, [180, 284, 150, 20]);
        let sens_label = add("STATIC", "Adaptive sensitivity", 0, 0, [24, 314, 110, 18]);
        let sens = add("COMBOBOX", "", CBS_DROPDOWNLIST as u32 | tab, ID_SENS, [140, 310, 150, 100]);
        nc_ctrls.extend([level_slider, level_value, voice, auto, sens_label, sens]);
        // equalizer
        let mut eq_ctrls = vec![add("STATIC", "Equalizer", 0, 0, [24, 346, 70, 18])];
        let eq_preset = add("COMBOBOX", "", CBS_DROPDOWNLIST as u32 | WS_VSCROLL | tab, ID_EQ_PRESET, [100, 342, 170, 200]);
        eq_ctrls.push(eq_preset);
        let mut eq_sliders = [null_mut(); 10];
        let mut eq_db = [null_mut(); 10];
        let mut eq_freq = [null_mut(); 10];
        for i in 0..10 {
            let x = 24 + i as i32 * 37;
            eq_sliders[i] = add("msctls_trackbar32", "", TBS_VERT | TBS_NOTICKS | tab, 0, [x, 370, 36, 96]);
            eq_db[i] = add("STATIC", "", SS_CENTER, 0, [x, 468, 36, 16]);
            eq_freq[i] = add("STATIC", "", SS_CENTER, 0, [x, 484, 36, 16]);
            eq_ctrls.extend([eq_sliders[i], eq_db[i], eq_freq[i]]);
        }
        let dsee = add("BUTTON", "DSEE (auto)", check, ID_DSEE, [24, 510, 250, 20]);
        // second column: Speak-to-Chat, power, wearing, multipoint and the device list
        add("BUTTON", "Controls", BS_GROUPBOX as u32, 0, [420, 12, 396, 420]);
        let stc = add("BUTTON", "Speak-to-Chat", check, ID_STC, [432, 36, 130, 20]);
        let stc_sens_label = add("STATIC", "Sensitivity", 0, 0, [572, 38, 62, 18]);
        let stc_sens = add("COMBOBOX", "", CBS_DROPDOWNLIST as u32 | tab, ID_STC_SENS, [638, 34, 100, 100]);
        let stc_timeout_label = add("STATIC", "Timeout", 0, 0, [432, 70, 62, 18]);
        let stc_timeout = add("COMBOBOX", "", CBS_DROPDOWNLIST as u32 | tab, ID_STC_TIMEOUT, [500, 66, 130, 100]);
        let stc_levels = vec![stc_sens_label, stc_sens, stc_timeout_label, stc_timeout];
        let power_label = add("STATIC", "Auto power-off", 0, 0, [432, 108, 90, 18]);
        let power = add("COMBOBOX", "", CBS_DROPDOWNLIST as u32 | tab, ID_POWER, [528, 104, 200, 100]);
        let autopause = add("BUTTON", "Pause when removed from ears", check, ID_AUTOPAUSE, [432, 140, 300, 20]);
        let wearing_label = add("STATIC", "Wearing", 0, 0, [432, 172, 70, 18]);
        let wearing_value = add("STATIC", "", 0, 0, [508, 172, 290, 18]);
        let multipoint = add("BUTTON", "Multipoint", check, ID_MULTIPOINT, [432, 206, 300, 20]);
        let dev_label = add("STATIC", "Paired devices", 0, 0, [432, 238, 200, 18]);
        // LBS_NOSEL without LBS_NOTIFY: read-only, no selection, no notifications
        let dev_list = add("LISTBOX", "", LBS_NOSEL as u32 | WS_VSCROLL | WS_BORDER, 0, [432, 258, 372, 160]);
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
            nc_ctrls,
            nc_radios,
            level_slider,
            level_value,
            voice,
            auto,
            sens,
            eq_ctrls,
            eq_preset,
            eq_sliders,
            eq_db,
            eq_freq,
            eq_names: Vec::new(),
            dsee,
            stc,
            stc_sens,
            stc_timeout,
            stc_levels,
            power,
            power_ctrls: vec![power_label, power],
            power_names: Vec::new(),
            autopause,
            wearing_ctrls: vec![wearing_label, wearing_value],
            wearing_value,
            multipoint,
            dev_ctrls: vec![dev_label, dev_list],
            dev_list,
            dev_lines: Vec::new(),
            step_slider: None,
        };
        // SAFETY: live controls; the strings outlive the calls. None of these messages notifies the parent (TBM_SETRANGE, CB_ADDSTRING).
        unsafe {
            SendMessageW(app.level_slider, TBM_SETRANGE, 1, (20 << 16) | 1);
            for s in app.eq_sliders {
                SendMessageW(s, TBM_SETRANGE, 1, 12 << 16);
            }
            for n in ["Standard", "High", "Low"] {
                SendMessageW(app.sens, CB_ADDSTRING, 0, wide(n).as_ptr() as LPARAM);
            }
            for n in ["Auto", "High", "Low"] {
                SendMessageW(app.stc_sens, CB_ADDSTRING, 0, wide(n).as_ptr() as LPARAM);
            }
            for n in ["Fast", "Mid", "Slow", "Manual off"] {
                SendMessageW(app.stc_timeout, CB_ADDSTRING, 0, wide(n).as_ptr() as LPARAM);
            }
        }
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

    fn render(&mut self) {
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
        self.render_sound(&v.sound);
        self.render_system(&v.system);
    }

    /// Draws the "Controls" rows; as in `render_sound`, every message sent here is silent (BM_SETCHECK, CB_SETCURSEL, CB_*/LB_* string messages).
    fn render_system(&mut self, s: &SystemView) {
        let send = |h: HWND, m: u32, w: WPARAM, l: LPARAM| {
            // SAFETY: `h` is a live control; every call here passes plain values or a string that outlives it.
            unsafe { SendMessageW(h, m, w, l) };
        };
        let show = |hs: &[HWND], on: bool| {
            for &h in hs {
                // SAFETY: live controls.
                unsafe { ShowWindow(h, if on { SW_SHOW } else { SW_HIDE }) };
            }
        };
        show(&[self.stc], s.stc.enabled.is_some());
        send(self.stc, BM_SETCHECK, s.stc.enabled.unwrap_or(false) as WPARAM, 0);
        show(&self.stc_levels, s.stc.levels.is_some());
        if let Some((sens, timeout)) = s.stc.levels {
            send(self.stc_sens, CB_SETCURSEL, sens as WPARAM, 0);
            send(self.stc_timeout, CB_SETCURSEL, timeout as WPARAM, 0);
        }
        show(&self.power_ctrls, s.power.is_some());
        if let Some(p) = &s.power {
            if self.power_names != p.options {
                send(self.power, CB_RESETCONTENT, 0, 0);
                for n in &p.options {
                    send(self.power, CB_ADDSTRING, 0, wide(n).as_ptr() as LPARAM);
                }
                self.power_names = p.options.clone();
            }
            send(self.power, CB_SETCURSEL, p.selected.unwrap_or(usize::MAX), 0);
        }
        show(&[self.autopause], s.autopause.is_some());
        send(self.autopause, BM_SETCHECK, s.autopause.unwrap_or(false) as WPARAM, 0);
        show(&self.wearing_ctrls, s.wearing.is_some());
        // SAFETY: live control, NUL-terminated string.
        unsafe { SetWindowTextW(self.wearing_value, wide(s.wearing.as_deref().unwrap_or("")).as_ptr()) };
        show(&[self.multipoint], s.multipoint.is_some());
        send(self.multipoint, BM_SETCHECK, s.multipoint.unwrap_or(false) as WPARAM, 0);
        show(&self.dev_ctrls, s.devices.is_some());
        let lines = s.devices.as_deref().unwrap_or_default();
        if self.dev_lines != lines {
            send(self.dev_list, LB_RESETCONTENT, 0, 0);
            for l in lines {
                send(self.dev_list, LB_ADDSTRING, 0, wide(l).as_ptr() as LPARAM);
            }
            self.dev_lines = lines.to_vec();
        }
    }

    /// Draws the sound panels. Every message sent here (BM_SETCHECK, CB_SETCURSEL, TBM_SETPOS, CB_*STRING) is silent: none of them notifies the
    /// parent, so drawing a headset value can never look like a user gesture.
    fn render_sound(&mut self, s: &SoundView) {
        let text = |h: HWND, t: &str| {
            // SAFETY: `h` is a live control, the string is NUL-terminated.
            unsafe { SetWindowTextW(h, wide(t).as_ptr()) };
        };
        let show = |hs: &[HWND], on: bool| {
            for &h in hs {
                // SAFETY: live controls.
                unsafe { ShowWindow(h, if on { SW_SHOW } else { SW_HIDE }) };
            }
        };
        let send = |h: HWND, m: u32, w: WPARAM, l: LPARAM| {
            // SAFETY: `h` is a live control; every call here passes plain values or a string that outlives it.
            unsafe { SendMessageW(h, m, w, l) };
        };
        let enable = |h: HWND, on: bool| {
            // SAFETY: live control.
            unsafe { EnableWindow(h, on as i32) };
        };
        show(&self.nc_ctrls, s.nc.is_some());
        if let Some(n) = &s.nc {
            for (r, m) in self.nc_radios.iter().zip([Mode::Off, Mode::Nc, Mode::Ambient]) {
                send(*r, BM_SETCHECK, (n.mode == m) as WPARAM, 0);
            }
            send(self.level_slider, TBM_SETPOS, 1, n.level as LPARAM);
            text(self.level_value, &n.level.to_string());
            send(self.voice, BM_SETCHECK, n.voice as WPARAM, 0);
            send(self.auto, BM_SETCHECK, n.auto as WPARAM, 0);
            send(self.sens, CB_SETCURSEL, n.sens as WPARAM, 0);
            for h in [self.level_slider, self.level_value, self.voice] {
                enable(h, n.ambient_enabled);
            }
        }
        show(&self.eq_ctrls, s.eq.is_some());
        if let Some(e) = &s.eq {
            if self.eq_names != e.presets {
                send(self.eq_preset, CB_RESETCONTENT, 0, 0);
                for n in &e.presets {
                    send(self.eq_preset, CB_ADDSTRING, 0, wide(n).as_ptr() as LPARAM);
                }
                self.eq_names = e.presets.clone();
            }
            send(self.eq_preset, CB_SETCURSEL, e.selected.unwrap_or(usize::MAX), 0); // usize::MAX = -1: no selection
            enable(self.eq_preset, e.enabled);
            for (i, (&slider, &db_label)) in self.eq_sliders.iter().zip(&self.eq_db).enumerate() {
                let db = e.bands.get(i).copied().unwrap_or(0);
                send(slider, TBM_SETPOS, 1, (6 - db) as LPARAM); // vertical: the top is the minimum, so +6 dB = position 0
                text(db_label, &format!("{db:+}"));
                enable(slider, e.sliders_enabled);
                text(self.eq_freq[i], e.labels.get(i).map_or("", String::as_str));
            }
        }
        show(&[self.dsee], s.dsee.is_some());
        if let Some(d) = &s.dsee {
            send(self.dsee, BM_SETCHECK, d.auto as WPARAM, 0);
            enable(self.dsee, d.enabled);
        }
    }

    fn send(&self, command: Option<Command>) {
        if let (Some(c), Some(h)) = (command, &self.handle) {
            // an error means the worker ended; its Disconnected event follows
            let _ = h.send(c);
        }
    }

    /// A user gesture on a button, checkbox or dropdown of the sound panels.
    fn on_sound_command(&mut self, id: usize) {
        // read the picks before the flush below re-renders the combos
        let [sens, preset, stc_sens, stc_timeout, power] = [self.sens, self.eq_preset, self.stc_sens, self.stc_timeout, self.power].map(|c| self.selection(c));
        self.flush_step();
        let c = match id {
            ID_STC => self.model.stc_toggle(),
            ID_STC_SENS => self.model.stc_sens(stc_sens),
            ID_STC_TIMEOUT => self.model.stc_timeout(stc_timeout),
            ID_POWER => self.model.power_select(power),
            ID_AUTOPAUSE => self.model.toggle_autopause(),
            ID_MULTIPOINT => {
                // only asks: the dialog is shown by WM_CONFIRM once this handler has returned (it runs a message loop of its own)
                if let Some(on) = self.model.multipoint_ask() {
                    // SAFETY: posting to our own window only fails if it is gone.
                    unsafe { PostMessageW(self.hwnd, WM_CONFIRM, on as WPARAM, 0) };
                }
                None
            }
            ID_NC_MODE..=112 => self.model.nc_mode([Mode::Off, Mode::Nc, Mode::Ambient][id - ID_NC_MODE]),
            ID_VOICE => self.model.nc_toggle_voice(),
            ID_AUTO => self.model.nc_toggle_auto(),
            ID_DSEE => self.model.toggle_dsee(),
            ID_SENS => self.model.nc_sens(sens),
            ID_EQ_PRESET => self.model.eq_select_preset(preset),
            _ => return,
        };
        self.send(c);
        self.render(); // puts the control back to the headset's value until the headset confirms
    }

    /// A paced slider step still waiting goes out first, so it cannot land after (and undo) another command.
    fn flush_step(&mut self) {
        if let Some(s) = self.step_slider {
            self.on_scroll(TB_ENDTRACK, s);
        }
    }

    /// The answer of the multipoint dialog.
    fn on_confirm_answer(&mut self, yes: bool) {
        self.flush_step();
        let c = self.model.multipoint_answer(yes);
        self.send(c);
        self.render();
    }

    fn selection(&self, combo: HWND) -> usize {
        // SAFETY: live combo box; CB_ERR (-1) becomes usize::MAX, which no intent accepts.
        unsafe { SendMessageW(combo, CB_GETCURSEL, 0, 0) as usize }
    }

    /// A trackbar notification (WM_HSCROLL / WM_VSCROLL): only the ambient-level and EQ sliders exist.
    fn on_scroll(&mut self, code: u32, slider: HWND) {
        // SAFETY: `slider` is the control handle the notification came with.
        let pos = unsafe { SendMessageW(slider, TBM_GETPOS, 0, 0) } as i32;
        // TB_THUMBTRACK is a drag; ENDTRACK / THUMBPOSITION release it; every other code (keys, wheel, page, capture loss) is a final value
        // that auto-repeats, so the model paces it and the next ENDTRACK or the timer flushes the last one. The model drops duplicate SETs.
        let now = Instant::now();
        // a paced step on another slider would otherwise never be flushed: release it first
        if let Some(prev) = self.step_slider.filter(|&p| p != slider) {
            self.on_scroll(TB_ENDTRACK, prev);
        }
        let (drag, end) = (code == TB_THUMBTRACK, matches!(code, TB_ENDTRACK | TB_THUMBPOSITION));
        self.step_slider = (!drag && !end).then_some(slider);
        if slider == self.level_slider {
            let c = match (drag, end) {
                (true, _) => self.model.nc_level_moved(pos as u8, now),
                (_, true) => self.model.nc_level_released(pos as u8),
                _ => self.model.nc_level_stepped(pos as u8, now),
            };
            self.send(c);
        } else if let Some(i) = self.eq_sliders.iter().position(|&s| s == slider) {
            let db = (6 - pos) as i16;
            let c = match (drag, end) {
                (true, _) => {
                    self.model.eq_band_moved(i, db);
                    None
                }
                (_, true) => self.model.eq_band_released(i, db),
                _ => self.model.eq_band_stepped(i, db, now),
            };
            self.send(c);
        }
        self.render();
    }

    /// The timer: flush the value of a paced step that no ENDTRACK followed, and drop edits the headset never answered.
    fn tick(&mut self) {
        let now = Instant::now();
        // the flush is a SET too: keep it inside the pace so a held key stays at one SET per STEP_PACE
        if let Some(s) = self.step_slider
            && self.model.pace(now)
        {
            self.on_scroll(TB_ENDTRACK, s);
        }
        let c = self.model.nc_drag_poll(now);
        self.send(c);
        if self.model.expire(now) {
            self.render();
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
    // Only these messages touch the App: user gestures (BN_CLICKED, CBN_SELCHANGE, trackbar WM_HSCROLL/WM_VSCROLL) and our own posts. Everything else
    // (WM_SIZE, WM_CTLCOLOR*, other notifications, ...) can arrive re-entrantly from our own calls and must not create a second `&mut App`. The render calls
    // are silent by design: TBM_SETPOS, BM_SETCHECK and CB_SETCURSEL send no WM_HSCROLL, BN_CLICKED or CBN_SELCHANGE, so a headset value never loops back.
    // EnableWindow/ShowWindow can still make a control notify us synchronously; `IN_HANDLER` defers such a message by posting it back.
    if msg == WM_DESTROY {
        unsafe { PostQuitMessage(0) };
        return 0;
    }
    let gesture = msg == WM_COMMAND && matches!((wparam >> 16) as u32, BN_CLICKED | CBN_SELCHANGE);
    if !(gesture
        || matches!(msg, WM_HSCROLL | WM_VSCROLL | WM_TIMER | WM_DEVICE_EVENT | WM_CONFIRM | WM_CONFIRM_ANSWER | WM_DPICHANGED | WM_CLOSE | WM_NCDESTROY))
    {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    // SAFETY: the pointer is set once in `run` to a leaked `App` and cleared in WM_NCDESTROY; only the UI thread gets here.
    // A handler is running (`&mut App` is live) and one of our own calls re-entered with a gesture: e.g. render() disabling a trackbar the user drags
    // sends WM_CANCELMODE, and the trackbar can answer with TB_ENDTRACK. Handle it after the handler returns.
    if msg != WM_NCDESTROY && IN_HANDLER.get() {
        if msg != WM_DPICHANGED {
            // posting to our own window only fails if it is gone
            unsafe { PostMessageW(hwnd, msg, wparam, lparam) };
            return 0;
        }
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    if msg == WM_CONFIRM {
        // No `&mut App` exists here (IN_HANDLER is false): the dialog's own message loop dispatches WM_TIMER and WM_DEVICE_EVENT back into this
        // function, each in a fresh handler. The answer is posted and applied in another fresh handler.
        let yes = confirm_multipoint(hwnd, wparam != 0);
        unsafe { PostMessageW(hwnd, WM_CONFIRM_ANSWER, yes as WPARAM, 0) };
        return 0;
    }
    let app = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App).as_mut() };
    let Some(app) = app else { return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) } };
    IN_HANDLER.set(msg != WM_NCDESTROY);
    match msg {
        WM_COMMAND => match wparam & 0xFFFF {
            ID_CONNECT => app.on_connect_button(),
            ID_REFRESH => app.refresh_devices(),
            ID_MAC => {
                app.model.toggle_mac();
                app.render();
            }
            id => app.on_sound_command(id),
        },
        WM_HSCROLL | WM_VSCROLL => app.on_scroll((wparam & 0xFFFF) as u32, lparam as HWND),
        WM_DEVICE_EVENT => app.pump(),
        WM_CONFIRM_ANSWER => app.on_confirm_answer(wparam != 0),
        WM_TIMER if wparam == TICK_ID => app.tick(),
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
        _ => {
            IN_HANDLER.set(false);
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }
    }
    IN_HANDLER.set(false);
    0
}

/// Asks whether to turn multipoint on or off (default: No). Turning it on or off can make the headset drop a connection, maybe the one to this PC.
fn confirm_multipoint(hwnd: HWND, on: bool) -> bool {
    let text = format!(
        "Turn multipoint {}?

The headset may drop a connected device, possibly this PC.",
        if on { "on" } else { "off" }
    );
    // SAFETY: valid NUL-terminated strings and a live owner window; this returns after the user answers.
    unsafe { MessageBoxW(hwnd, wide(&text).as_ptr(), wide("Multipoint").as_ptr(), MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2) == IDYES }
}

thread_local! {
    /// true while a wndproc arm holds `&mut App`.
    static IN_HANDLER: Cell<bool> = const { Cell::new(false) };
}

/// Opens the window and runs the message loop until it is closed.
pub fn run() -> Result<(), String> {
    let class = wide("XmControlWindow");
    // SAFETY: standard window-class registration and creation; all pointers are valid for the calls.
    unsafe {
        // the trackbar class lives in comctl32
        InitCommonControlsEx(&INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_BAR_CLASSES });
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
        SetTimer(hwnd, TICK_ID, TICK_MS, None);
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

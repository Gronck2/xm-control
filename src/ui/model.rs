//! UI-independent view model: `Event`s from the device worker update the `Model`, `Model::view` gives the text and enabled flags the view draws.
//! User intents (`nc_*`, `eq_*`, `toggle_dsee`) return the `Command` to send; the view calls them only from real user gestures, never from a value it was told to show.

use crate::bt::format_mac;
use crate::device::battery::{Battery, Charge};
use crate::device::handshake::DeviceInfo;
use crate::device::ncasm::{Drag, Mode};
use crate::device::state::DeviceState;
use crate::device::{Command, Event};
use crate::mdr::guard::EQ_PRESETS;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Idle,
    Connecting,
    Connected,
    /// The user asked to disconnect; waiting for the worker to end.
    Disconnecting,
    Disconnected(String),
}

#[derive(Debug)]
pub struct Model {
    phase: Phase,
    /// The device being connected to: (name, MAC) from the OS device list.
    target: Option<(String, u64)>,
    info: Option<DeviceInfo>,
    state: DeviceState,
    synced: bool,
    notice: Option<String>,
    show_mac: bool,
    /// Rate limiter for the ambient-level drag (the core's own `Drag`).
    drag: Drag,
    /// The slider values the user is editing: `(value, still held)`. Kept after release until the headset answers, so the slider does not snap back.
    level_edit: Option<(u8, bool)>,
    eq_edit: Option<(Vec<i16>, bool)>,
    /// The dropdown picks (EQ preset id, sensitivity index) shown until the headset answers, so arrow keys on a combo do not snap back each step.
    preset_edit: Option<u8>,
    sens_edit: Option<u8>,
}

/// The noise-control rows. `level` is the displayed slider value (1..=20).
#[derive(Debug, PartialEq, Eq)]
pub struct NcView {
    pub mode: Mode,
    pub level: u8,
    /// Level slider and focus-on-voice apply to ambient mode only.
    pub ambient_enabled: bool,
    pub voice: bool,
    pub auto: bool,
    /// 0 standard, 1 high, 2 low.
    pub sens: u8,
}

/// The equalizer rows: `presets` are the names the XM6 accepts, `bands` the displayed dB values, `labels` the band frequencies.
#[derive(Debug, PartialEq, Eq)]
pub struct EqView {
    pub presets: Vec<String>,
    pub selected: Option<usize>,
    pub bands: Vec<i16>,
    pub labels: Vec<String>,
    /// false: the headset says the EQ is unavailable (grey the panel).
    pub enabled: bool,
    /// Only CUSTOM and the user slots take band edits.
    pub sliders_enabled: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DseeView {
    pub auto: bool,
    pub enabled: bool,
}

/// `None` = the headset does not advertise the feature (or it is not read yet): hide the controls.
#[derive(Debug, PartialEq, Eq)]
pub struct SoundView {
    pub nc: Option<NcView>,
    pub eq: Option<EqView>,
    pub dsee: Option<DseeView>,
}

/// What the view shows. `battery` is `None` when the headset does not advertise it (or it is not read yet): hide the row.
#[derive(Debug, PartialEq, Eq)]
pub struct ViewState {
    pub status: String,
    pub button_label: &'static str,
    pub button_enabled: bool,
    pub picker_enabled: bool,
    pub model: String,
    pub firmware: String,
    pub mac: String,
    pub mac_button_label: &'static str,
    pub battery: Option<String>,
    pub sound: SoundView,
}

impl Model {
    pub fn new() -> Self {
        Model {
            phase: Phase::Idle,
            target: None,
            info: None,
            state: DeviceState::default(),
            synced: false,
            notice: None,
            show_mac: false,
            drag: Drag::default(),
            level_edit: None,
            eq_edit: None,
            preset_edit: None,
            sens_edit: None,
        }
    }

    /// A worker exists (connecting, connected or going down): the picker stays locked and the event channel needs draining.
    pub fn active(&self) -> bool {
        matches!(self.phase, Phase::Connecting | Phase::Connected | Phase::Disconnecting)
    }

    pub fn begin_connect(&mut self, name: String, mac: u64) {
        *self = Model { phase: Phase::Connecting, target: Some((name, mac)), show_mac: self.show_mac, ..Model::new() };
    }

    pub fn begin_disconnect(&mut self) {
        if self.active() {
            self.phase = Phase::Disconnecting;
        }
    }

    pub fn toggle_mac(&mut self) {
        self.show_mac = !self.show_mac;
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Connected(info) => {
                if self.phase == Phase::Connecting {
                    self.phase = Phase::Connected;
                }
                self.info = Some(info);
            }
            Event::Synced(state) => {
                self.synced = true;
                self.state = state;
                self.clear_edits();
            }
            Event::StateChanged { feature, state } => {
                self.state = state;
                // the headset answered: show its value again. A held slider stays; a released one stays until the answer matches it (or the
                // slider is off), so a late notification for an earlier step does not make it jump back.
                if feature == "nc" {
                    self.sens_edit = None;
                    let level = self.state.nc.map(|n| (n.level, n.mode() != Mode::Ambient));
                    if self.level_edit.is_some_and(|(l, held)| !held && level.is_none_or(|(nl, off)| nl == l || off)) {
                        self.level_edit = None;
                    }
                }
                if feature == "eq" {
                    self.preset_edit = None;
                    let eq = self.state.eq.as_ref();
                    if self.eq_edit.as_ref().is_some_and(|(b, held)| !held && eq.is_none_or(|e| e.bands == *b || !is_custom(e.preset))) {
                        self.eq_edit = None;
                    }
                }
            }
            Event::Error(e) => {
                self.notice = Some(format!("{e:?}"));
                self.clear_edits();
            }
            Event::Unhandled { .. } => {}
            Event::Disconnected(reason) => {
                self.phase = if self.phase == Phase::Disconnecting { Phase::Idle } else { Phase::Disconnected(reason) };
                self.info = None;
                self.state = DeviceState::default();
                self.synced = false;
                self.notice = None;
                self.clear_edits();
            }
        }
    }

    fn clear_edits(&mut self) {
        (self.level_edit, self.eq_edit, self.preset_edit, self.sens_edit) = (None, None, None, None);
    }

    pub fn view(&self) -> ViewState {
        let name = self.target.as_ref().map_or("", |t| t.0.as_str());
        let mut status = match &self.phase {
            Phase::Idle => "Not connected".to_string(),
            Phase::Connecting => format!("Connecting to {name} ..."),
            Phase::Connected if self.synced => format!("Connected to {name}"),
            Phase::Connected => format!("Connected to {name}, reading settings ..."),
            Phase::Disconnecting => "Disconnecting ...".to_string(),
            Phase::Disconnected(reason) => format!("Disconnected: {reason}"),
        };
        if let Some(n) = &self.notice {
            status = format!("{status} ({n})");
        }
        ViewState {
            status,
            button_label: if self.active() { "Disconnect" } else { "Connect" },
            button_enabled: self.phase != Phase::Disconnecting,
            picker_enabled: !self.active(),
            model: self.info.as_ref().map_or("-", |i| i.model_name.as_str()).to_string(),
            firmware: self.info.as_ref().map_or("-", |i| i.firmware.as_str()).to_string(),
            mac: self.target.as_ref().map_or("-".to_string(), |t| mac_text(t.1, self.show_mac)),
            mac_button_label: if self.show_mac { "Hide" } else { "Show" },
            battery: self.state.battery.map(battery_text),
            sound: self.sound_view(),
        }
    }
}

const SENS: [&str; 3] = ["standard", "high", "low"];

fn mode_text(m: Mode) -> &'static str {
    match m {
        Mode::Off => "off",
        Mode::Nc => "nc",
        Mode::Ambient => "ambient",
    }
}

/// CUSTOM and the user slots (the XM6 accepts 0xA0..=0xA5).
fn is_custom(preset: u8) -> bool {
    (0xA0..=0xA5).contains(&preset)
}

/// "31", "500", "1k", "16k".
fn freq_label(hz: u16) -> String {
    if hz >= 1000 && hz.is_multiple_of(1000) {
        format!("{}k", hz / 1000)
    } else if hz >= 1000 {
        format!("{:.1}k", hz as f32 / 1000.0)
    } else {
        hz.to_string()
    }
}

/// The sound panels and their intents. Every intent returns the command for one user gesture; a value that came from the headset never reaches them.
impl Model {
    fn sound_view(&self) -> SoundView {
        let nc = self.state.nc.map(|n| NcView {
            mode: n.mode(),
            level: self.level_edit.as_ref().map_or(n.level, |e| e.0).clamp(1, 20),
            ambient_enabled: n.mode() == Mode::Ambient,
            voice: n.voice,
            auto: n.auto,
            sens: self.sens_edit.unwrap_or(n.sens).min(2),
        });
        let eq = self.eq_presets().and_then(|presets| {
            let eq = self.state.eq.as_ref()?;
            let bands = self.eq_edit.as_ref().map_or(&eq.bands, |e| &e.0);
            Some(EqView {
                selected: presets.iter().position(|p| p.0 == self.preset_edit.unwrap_or(eq.preset)),
                presets: presets.into_iter().map(|p| p.1).collect(),
                bands: bands.iter().map(|b| (*b).clamp(-6, 6)).collect(),
                labels: self.state.eq_freqs.iter().flatten().map(|&f| freq_label(f)).collect(),
                enabled: !self.state.eq_disabled,
                sliders_enabled: !self.state.eq_disabled && is_custom(eq.preset),
            })
        });
        let dsee = self.state.dsee.map(|auto| DseeView { auto, enabled: !self.state.dsee_disabled });
        SoundView { nc, eq, dsee }
    }

    /// The presets to offer: those the headset lists and the XM6 accepts. `None` while the EQ is unknown or not the 10 x 13 layout (read-only, hidden).
    fn eq_presets(&self) -> Option<Vec<(u8, String)>> {
        let cap = self.state.eq_cap.as_ref().filter(|c| c.bands == 10 && c.steps == 13)?;
        Some(cap.presets.iter().filter(|p| EQ_PRESETS.contains(&p.0)).cloned().collect())
    }

    fn set(feature: &str, value: String) -> Option<Command> {
        Some(Command::Set { feature: feature.into(), value })
    }

    /// `<mode> <extra>`: the NC command that keeps the current mode.
    fn nc_set(&self, extra: &str) -> Option<Command> {
        Self::set("nc", format!("{} {extra}", mode_text(self.state.nc?.mode())))
    }

    pub fn nc_mode(&mut self, m: Mode) -> Option<Command> {
        let nc = self.state.nc?;
        if nc.mode() == m { None } else { Self::set("nc", mode_text(m).into()) }
    }

    pub fn nc_toggle_voice(&mut self) -> Option<Command> {
        let v = self.state.nc?.voice;
        self.nc_set(&format!("--voice {}", if v { "off" } else { "on" }))
    }

    pub fn nc_toggle_auto(&mut self) -> Option<Command> {
        let v = self.state.nc?.auto;
        self.nc_set(&format!("--auto {}", if v { "off" } else { "on" }))
    }

    pub fn nc_sens(&mut self, index: usize) -> Option<Command> {
        let name = SENS.get(index)?;
        if self.sens_edit.unwrap_or(self.state.nc?.sens) as usize == index {
            return None;
        }
        self.sens_edit = Some(index as u8);
        self.nc_set(&format!("--sens {name}"))
    }

    /// The ambient slider moved to `level` (still held): an UNDER_CHANGING SET at most every 100 ms.
    pub fn nc_level_moved(&mut self, level: u8, now: Instant) -> Option<Command> {
        let nc = self.state.nc.filter(|n| n.mode() == Mode::Ambient && (1..=20).contains(&level))?;
        self.level_edit = Some((level, true));
        self.drag.update(now, &crate::device::ncasm::NcAsm { level, ..nc })?;
        self.nc_set(&format!("--level {level} --changing"))
    }

    /// The ambient slider was let go at `level`: the final CHANGED SET, always sent.
    pub fn nc_level_released(&mut self, level: u8) -> Option<Command> {
        let Some(nc) = self.state.nc.filter(|n| n.mode() == Mode::Ambient && (1..=20).contains(&level)) else {
            self.level_edit = None; // refused: show the headset's level again
            return None;
        };
        if self.level_edit == Some((level, false)) {
            return None; // already sent (the ENDTRACK after a key or page step)
        }
        self.level_edit = Some((level, false));
        self.drag.release(&crate::device::ncasm::NcAsm { level, ..nc });
        self.nc_set(&format!("--level {level}"))
    }

    pub fn eq_select_preset(&mut self, index: usize) -> Option<Command> {
        let eq = self.state.eq.as_ref()?;
        let id = self.eq_presets()?.get(index)?.0;
        if self.state.eq_disabled || id == self.preset_edit.unwrap_or(eq.preset) {
            return None;
        }
        self.preset_edit = Some(id);
        Self::set("eq", format!("preset {id:#04X}"))
    }

    /// Band `index` is being dragged to `db`: only shown, nothing is sent until release.
    pub fn eq_band_moved(&mut self, index: usize, db: i16) {
        if let Some(bands) = self.eq_bands_with(index, db) {
            self.eq_edit = Some((bands, true));
        }
    }

    /// Band `index` was let go at `db`: one SET with all ten bands.
    pub fn eq_band_released(&mut self, index: usize, db: i16) -> Option<Command> {
        let Some(bands) = self.eq_bands_with(index, db) else {
            self.eq_edit = None; // refused: show the headset's bands again
            return None;
        };
        // compared with what was last sent (a released edit), else with the headset's bands
        let sent = self.eq_edit.as_ref().filter(|e| !e.1).map(|e| e.0.clone());
        if sent.or_else(|| self.state.eq.as_ref().map(|e| e.bands.clone())).as_ref() == Some(&bands) {
            if self.eq_edit.as_ref().is_some_and(|e| e.1) {
                self.eq_edit = None;
            }
            return None;
        }
        let text = bands.iter().map(i16::to_string).collect::<Vec<_>>().join(" ");
        self.eq_edit = Some((bands, false));
        Self::set("eq", format!("bands {text}"))
    }

    /// The bands as shown with band `index` replaced; `None` unless the sliders are enabled and the value is in range.
    fn eq_bands_with(&self, index: usize, db: i16) -> Option<Vec<i16>> {
        let eq = self.state.eq.as_ref()?;
        self.eq_presets()?;
        if self.state.eq_disabled || !is_custom(eq.preset) || !(-6..=6).contains(&db) || index >= eq.bands.len() {
            return None;
        }
        let mut bands = self.eq_edit.as_ref().map_or(&eq.bands, |e| &e.0).clone();
        bands[index] = db;
        Some(bands)
    }

    pub fn toggle_dsee(&mut self) -> Option<Command> {
        let auto = self.state.dsee?;
        if self.state.dsee_disabled { None } else { Self::set("dsee", if auto { "off" } else { "auto" }.into()) }
    }
}

/// The MAC is hidden by default: only the last two bytes show.
fn mac_text(mac: u64, show: bool) -> String {
    let full = format_mac(mac);
    if show { full } else { format!("**:**:**:**:{}", &full[12..]) }
}

fn battery_text(b: Battery) -> String {
    match b.charging {
        Charge::Charging => format!("{}% (charging)", b.level),
        Charge::Charged => format!("{}% (charged)", b.level),
        Charge::NotCharging | Charge::Unknown => format!("{}%", b.level),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> DeviceInfo {
        DeviceInfo {
            protocol_version: 1,
            table2_enabled: false,
            unique_id: String::new(),
            firmware: "1.2.3".into(),
            model_name: "WH-1000XM6".into(),
            series: 0,
            color: 0,
            t1_functions: vec![],
            t2_functions: vec![],
        }
    }

    fn connecting() -> Model {
        let mut m = Model::new();
        m.begin_connect("WH-1000XM6".into(), 0xAABBCCDDEE01);
        m
    }

    #[test]
    fn idle_view() {
        let v = Model::new().view();
        assert_eq!((v.status.as_str(), v.button_label, v.picker_enabled, v.model.as_str(), v.mac.as_str()), ("Not connected", "Connect", true, "-", "-"));
        assert_eq!(v.battery, None);
    }

    #[test]
    fn connect_flow_and_formatting() {
        let mut m = connecting();
        let v = m.view();
        assert_eq!((v.status.as_str(), v.button_label, v.picker_enabled), ("Connecting to WH-1000XM6 ...", "Disconnect", false));
        assert_eq!(v.mac, "**:**:**:**:EE:01");
        m.apply(Event::Connected(info()));
        let v = m.view();
        assert_eq!((v.model.as_str(), v.firmware.as_str()), ("WH-1000XM6", "1.2.3"));
        assert!(v.status.ends_with("reading settings ..."));
        m.apply(Event::Synced(DeviceState::default()));
        assert_eq!(m.view().status, "Connected to WH-1000XM6");
        assert_eq!(m.view().battery, None); // not advertised: hidden
        m.toggle_mac();
        assert_eq!((m.view().mac.as_str(), m.view().mac_button_label), ("AA:BB:CC:DD:EE:01", "Hide"));
    }

    #[test]
    fn battery_shown_and_updated() {
        let mut m = connecting();
        m.apply(Event::Connected(info()));
        let state = DeviceState { battery: Some(Battery { level: 80, charging: Charge::Charging }), ..Default::default() };
        m.apply(Event::StateChanged { feature: "battery", state });
        assert_eq!(m.view().battery.as_deref(), Some("80% (charging)"));
        let state = DeviceState { battery: Some(Battery { level: 79, charging: Charge::NotCharging }), ..Default::default() };
        m.apply(Event::StateChanged { feature: "battery", state });
        assert_eq!(m.view().battery.as_deref(), Some("79%"));
    }

    #[test]
    fn lost_link_shows_reason_and_clears_info() {
        let mut m = connecting();
        m.apply(Event::Connected(info()));
        m.apply(Event::Disconnected("link lost".into()));
        let v = m.view();
        assert_eq!((v.status.as_str(), v.button_label, v.model.as_str(), v.picker_enabled), ("Disconnected: link lost", "Connect", "-", true));
        assert!(!m.active());
    }

    #[test]
    fn user_disconnect_waits_for_worker() {
        let mut m = connecting();
        m.begin_disconnect();
        let v = m.view();
        assert_eq!((v.status.as_str(), v.button_enabled, v.picker_enabled), ("Disconnecting ...", false, false));
        assert!(m.active());
        m.apply(Event::Disconnected("closed".into()));
        assert_eq!((m.view().status.as_str(), m.view().button_enabled), ("Not connected", true));
    }

    #[test]
    fn command_error_is_a_notice_cleared_on_disconnect() {
        use crate::device::CommandError;
        let mut m = connecting();
        m.apply(Event::Connected(info()));
        m.apply(Event::Error(CommandError::NotSupported("x".into())));
        assert!(m.view().status.contains("NotSupported"));
        assert!(m.active());
        m.apply(Event::Disconnected("gone".into()));
        assert_eq!(m.view().status, "Disconnected: gone");
    }

    #[test]
    fn late_connected_does_not_undo_disconnecting_and_reconnect_resets() {
        let mut m = connecting();
        m.begin_disconnect();
        m.apply(Event::Connected(info()));
        assert_eq!(m.view().status, "Disconnecting ...");
        m.apply(Event::Disconnected("closed".into()));
        m.toggle_mac();
        let state = DeviceState { battery: Some(Battery { level: 50, charging: Charge::Charged }), ..Default::default() };
        m.apply(Event::Synced(state));
        m.begin_connect("Other".into(), 0x010203040506);
        let v = m.view();
        assert_eq!((v.battery, v.model.as_str(), v.mac.as_str()), (None, "-", "01:02:03:04:05:06")); // old state gone, MAC choice kept
        assert_eq!(v.status, "Connecting to Other ...");
    }

    #[test]
    fn begin_disconnect_when_idle_is_a_noop_and_charged_text() {
        let mut m = Model::new();
        m.begin_disconnect();
        assert_eq!(m.view().status, "Not connected");
        assert_eq!(battery_text(Battery { level: 100, charging: Charge::Charged }), "100% (charged)");
    }

    #[test]
    fn connect_failure_is_reported() {
        let mut m = connecting();
        m.apply(Event::Disconnected("connect failed: timed out".into()));
        assert_eq!(m.view().status, "Disconnected: connect failed: timed out");
    }

    use crate::device::eq::{Cap, Eq};
    use crate::device::ncasm::NcAsm;
    use std::time::Duration;

    fn cmd(c: Option<Command>) -> (String, String) {
        match c.expect("a command") {
            Command::Set { feature, value } => (feature, value),
            other => panic!("{other:?}"),
        }
    }

    fn sound_state() -> DeviceState {
        let presets = [(0x30, "HEAVY"), (0x32, "HARD"), (0xA0, "CUSTOM"), (0x01, "ROCK"), (0xA1, "USER1")];
        DeviceState {
            nc: Some(NcAsm { effect: true, asm: true, voice: false, level: 10, auto: false, sens: 0 }),
            eq: Some(Eq { preset: 0xA0, bands: vec![0, 1, 2, 3, 4, 5, 6, -1, -2, -3] }),
            eq_cap: Some(Cap { bands: 10, steps: 13, presets: presets.iter().map(|p| (p.0, p.1.to_string())).collect() }),
            eq_freqs: Some(vec![31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]),
            dsee: Some(false),
            ..Default::default()
        }
    }

    fn with(state: DeviceState) -> Model {
        let mut m = connecting();
        m.apply(Event::Synced(state));
        m
    }

    #[test]
    fn nothing_advertised_hides_every_sound_panel() {
        let s = with(DeviceState::default()).view().sound;
        assert_eq!((s.nc, s.eq, s.dsee), (None, None, None));
        // an EQ without a usable capability (read-only) is hidden too
        let mut st = sound_state();
        st.eq_cap = None;
        assert_eq!(with(st).view().sound.eq, None);
    }

    #[test]
    fn panels_show_the_state_and_follow_events() {
        let mut m = with(sound_state());
        let s = m.view().sound;
        let nc = s.nc.unwrap();
        assert_eq!((nc.mode, nc.level, nc.ambient_enabled, nc.voice, nc.auto, nc.sens), (Mode::Ambient, 10, true, false, false, 0));
        let eq = s.eq.unwrap();
        // ROCK is listed by the headset but not accepted: not offered
        assert_eq!(eq.presets, ["HEAVY", "HARD", "CUSTOM", "USER1"]);
        assert_eq!((eq.selected, eq.enabled, eq.sliders_enabled), (Some(2), true, true));
        assert_eq!((eq.bands[6], eq.labels[0].as_str(), eq.labels[5].as_str(), eq.labels[9].as_str()), (6, "31", "1k", "16k"));
        assert_eq!(s.dsee, Some(DseeView { auto: false, enabled: true }));
        // the headset changes everything (button press, other device): the view follows; `apply` has no way to return a command
        let mut st = sound_state();
        st.nc = Some(NcAsm { effect: false, asm: true, voice: true, level: 3, auto: true, sens: 2 });
        st.eq.as_mut().unwrap().preset = 0x32;
        st.dsee = Some(true);
        m.apply(Event::StateChanged { feature: "nc", state: st });
        let s = m.view().sound;
        let nc = s.nc.unwrap();
        assert_eq!((nc.mode, nc.level, nc.ambient_enabled, nc.voice, nc.auto, nc.sens), (Mode::Off, 3, false, true, true, 2));
        let eq = s.eq.unwrap();
        assert_eq!((eq.selected, eq.sliders_enabled), (Some(1), false)); // factory preset: sliders off
        assert!(s.dsee.unwrap().auto);
    }

    #[test]
    fn eq_unavailable_greys_the_panel_and_blocks_intents() {
        let mut st = sound_state();
        st.eq_disabled = true;
        st.dsee_disabled = true;
        let mut m = with(st);
        let s = m.view().sound;
        let eq = s.eq.unwrap();
        assert_eq!((eq.enabled, eq.sliders_enabled), (false, false));
        assert!(!s.dsee.unwrap().enabled);
        assert!(m.eq_select_preset(0).is_none() && m.eq_band_released(0, 3).is_none() && m.toggle_dsee().is_none());
    }

    #[test]
    fn nc_intents_become_set_commands() {
        let mut m = with(sound_state());
        assert_eq!(cmd(m.nc_mode(Mode::Nc)), ("nc".into(), "nc".into()));
        assert!(m.nc_mode(Mode::Ambient).is_none()); // already ambient: no SET
        assert_eq!(cmd(m.nc_toggle_voice()).1, "ambient --voice on");
        assert_eq!(cmd(m.nc_toggle_auto()).1, "ambient --auto on");
        assert_eq!(cmd(m.nc_sens(2)).1, "ambient --sens low");
        m.apply(Event::StateChanged { feature: "nc", state: sound_state() });
        assert!(m.nc_sens(0).is_none() && m.nc_sens(3).is_none());
        // without NC state nothing is sent
        let mut none = with(DeviceState::default());
        assert!(none.nc_mode(Mode::Nc).is_none() && none.nc_toggle_voice().is_none() && none.nc_level_released(5).is_none());
    }

    #[test]
    fn the_level_slider_is_rate_limited_and_ends_with_changed() {
        let mut m = with(sound_state());
        let t0 = Instant::now();
        assert_eq!(cmd(m.nc_level_moved(11, t0)).1, "ambient --level 11 --changing");
        assert!(m.nc_level_moved(12, t0 + Duration::from_millis(30)).is_none());
        assert_eq!(m.view().sound.nc.unwrap().level, 12); // shown even when the SET was held back
        // a headset update while the slider is held does not move it
        m.apply(Event::StateChanged { feature: "nc", state: sound_state() });
        assert_eq!(m.view().sound.nc.unwrap().level, 12);
        assert_eq!(cmd(m.nc_level_released(13)).1, "ambient --level 13");
        assert_eq!(m.view().sound.nc.unwrap().level, 13); // stays until the headset answers
        assert!(m.nc_level_released(13).is_none()); // the ENDTRACK after a step: no duplicate SET
        m.apply(Event::StateChanged { feature: "nc", state: sound_state() }); // late answer to an earlier step
        assert_eq!(m.view().sound.nc.unwrap().level, 13);
        let mut st = sound_state();
        st.nc.as_mut().unwrap().level = 13;
        m.apply(Event::StateChanged { feature: "nc", state: st });
        assert_eq!(m.view().sound.nc.unwrap().level, 13);
        m.apply(Event::StateChanged { feature: "nc", state: sound_state() });
        assert_eq!(m.view().sound.nc.unwrap().level, 10);
        // bounds, and not outside ambient mode
        assert!(m.nc_level_moved(0, t0).is_none() && m.nc_level_released(21).is_none());
        let mut st = sound_state();
        st.nc.as_mut().unwrap().asm = false;
        assert!(with(st).nc_level_released(5).is_none());
    }

    #[test]
    fn eq_presets_and_bands() {
        let mut m = with(sound_state());
        assert_eq!(cmd(m.eq_select_preset(1)), ("eq".into(), "preset 0x32".into()));
        m.apply(Event::StateChanged { feature: "eq", state: sound_state() });
        assert!(m.eq_select_preset(2).is_none()); // already CUSTOM
        assert!(m.eq_select_preset(9).is_none());
        // dragging sends nothing and shows the value; release sends all ten bands once
        m.eq_band_moved(0, 5);
        assert_eq!(m.view().sound.eq.unwrap().bands[0], 5);
        assert_eq!(cmd(m.eq_band_released(0, 5)), ("eq".into(), "bands 5 1 2 3 4 5 6 -1 -2 -3".into()));
        assert_eq!(m.view().sound.eq.unwrap().bands[0], 5);
        assert!(m.eq_band_released(0, 5).is_none()); // duplicate ENDTRACK
        let mut st = sound_state();
        st.eq.as_mut().unwrap().bands[0] = 5;
        m.apply(Event::StateChanged { feature: "eq", state: st });
        m.apply(Event::StateChanged { feature: "eq", state: sound_state() });
        assert_eq!(m.view().sound.eq.unwrap().bands[0], 0);
        // no change, out of range, bad index
        assert!(m.eq_band_released(1, 1).is_none() && m.eq_band_released(0, 7).is_none() && m.eq_band_released(10, 0).is_none());
        // factory presets: sliders are off, no SET
        let mut st = sound_state();
        st.eq.as_mut().unwrap().preset = 0x30;
        let mut f = with(st);
        f.eq_band_moved(0, 3);
        assert!(f.eq_band_released(0, 3).is_none());
        assert_eq!(f.view().sound.eq.unwrap().bands[0], 0);
    }

    #[test]
    fn a_refused_release_drops_the_held_edit() {
        let mut m = with(sound_state());
        m.eq_band_moved(0, 4);
        let mut st = sound_state();
        st.eq.as_mut().unwrap().preset = 0x30; // HEAVY: sliders off
        m.apply(Event::StateChanged { feature: "eq", state: st });
        assert!(m.eq_band_released(0, 4).is_none());
        assert_eq!(m.view().sound.eq.unwrap().bands[0], 0);
        // the same for the ambient level
        let mut m = with(sound_state());
        m.nc_level_moved(15, Instant::now());
        let mut st = sound_state();
        st.nc.as_mut().unwrap().asm = false;
        m.apply(Event::StateChanged { feature: "nc", state: st });
        assert!(m.nc_level_released(15).is_none());
        assert_eq!(m.view().sound.nc.unwrap().level, 10);
    }

    #[test]
    fn dropdown_picks_stay_until_the_headset_answers() {
        let mut m = with(sound_state());
        assert_eq!(cmd(m.eq_select_preset(0)).1, "preset 0x30");
        assert_eq!(m.view().sound.eq.unwrap().selected, Some(0));
        assert_eq!(cmd(m.eq_select_preset(1)).1, "preset 0x32"); // arrow key: next step
        assert!(m.eq_select_preset(1).is_none());
        assert_eq!(cmd(m.nc_sens(1)).1, "ambient --sens high");
        assert_eq!(m.view().sound.nc.unwrap().sens, 1);
        assert!(m.nc_sens(1).is_none());
        m.apply(Event::StateChanged { feature: "nc", state: sound_state() });
        assert_eq!(m.view().sound.nc.unwrap().sens, 0);
        m.apply(Event::StateChanged { feature: "eq", state: sound_state() });
        assert_eq!(m.view().sound.eq.unwrap().selected, Some(2));
        assert!(m.eq_select_preset(0).is_some());
        m.apply(Event::Error(crate::device::CommandError::NotSupported("x".into())));
        assert_eq!(m.view().sound.eq.unwrap().selected, Some(2));
    }

    #[test]
    fn dsee_toggle_and_errors_reset_edits() {
        let mut m = with(sound_state());
        assert_eq!(cmd(m.toggle_dsee()), ("dsee".into(), "auto".into()));
        m.apply(Event::StateChanged { feature: "dsee", state: DeviceState { dsee: Some(true), ..sound_state() } });
        assert_eq!(cmd(m.toggle_dsee()).1, "off");
        m.eq_band_moved(0, 4);
        m.apply(Event::Error(crate::device::CommandError::NotSupported("x".into())));
        assert_eq!(m.view().sound.eq.unwrap().bands[0], 0);
    }

    /// Every command the model emits must encode and pass the guard in normal mode.
    #[test]
    fn every_emitted_command_encodes_and_passes_the_guard() {
        use crate::device::FEATURES;
        use crate::mdr::guard::{Mode as GMode, check};
        let mut m = with(sound_state());
        let t0 = Instant::now();
        let mut cmds = vec![m.nc_mode(Mode::Nc), m.nc_toggle_voice(), m.nc_toggle_auto(), m.nc_sens(1), m.nc_sens(2), m.nc_level_moved(11, t0)];
        cmds.extend([m.nc_level_released(1), m.nc_level_released(20), m.eq_select_preset(0), m.eq_select_preset(1)]);
        cmds.extend([m.eq_band_released(0, -6), m.eq_band_released(9, 6), m.toggle_dsee()]);
        let mut off = sound_state();
        off.nc = Some(NcAsm { effect: false, asm: false, voice: false, level: 10, auto: false, sens: 0 });
        cmds.push(with(off).nc_mode(Mode::Ambient));
        assert!(cmds.iter().all(Option::is_some));
        for c in cmds.into_iter().flatten() {
            let Command::Set { feature, value } = c else { panic!("{c:?}") };
            let f = FEATURES.iter().find(|f| f.name() == feature).expect("known feature");
            let (table, payload) = f.encode_set(&sound_state(), &value).unwrap_or_else(|e| panic!("{feature} {value}: {e}"));
            check(table, &payload, GMode::Normal).unwrap_or_else(|e| panic!("{feature} {value}: {e:?}"));
        }
    }

    #[test]
    fn frequency_labels() {
        assert_eq!([31, 500, 1000, 1500, 16000].map(freq_label), ["31", "500", "1k", "1.5k", "16k"]);
    }
}

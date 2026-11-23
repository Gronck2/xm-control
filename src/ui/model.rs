//! UI-independent view model: `Event`s from the device worker update the `Model`, `Model::view` gives the text and enabled flags the view draws.
//! The UI in this task sends no commands; Connect/Disconnect are handled by the view through `DeviceHandle`.

use crate::bt::format_mac;
use crate::device::Event;
use crate::device::battery::{Battery, Charge};
use crate::device::handshake::DeviceInfo;
use crate::device::state::DeviceState;

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
}

impl Model {
    pub fn new() -> Self {
        Model { phase: Phase::Idle, target: None, info: None, state: DeviceState::default(), synced: false, notice: None, show_mac: false }
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
            }
            Event::StateChanged { state, .. } => self.state = state,
            Event::Error(e) => self.notice = Some(format!("{e:?}")),
            Event::Unhandled { .. } => {}
            Event::Disconnected(reason) => {
                self.phase = if self.phase == Phase::Disconnecting { Phase::Idle } else { Phase::Disconnected(reason) };
                self.info = None;
                self.state = DeviceState::default();
                self.synced = false;
                self.notice = None;
            }
        }
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
        }
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
}

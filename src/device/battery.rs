//! Headset battery (T1 function 0x20): GET `22 00`, RET `23 00 <level> <charging>`, NTFY `25 00 <level> <charging>`.
//! Read-only. The threshold variant (`22 08`, function 0x28) is not advertised on the XM6 and is never sent.

use super::feature::{Feature, Get, Parsed};
use super::handshake::DeviceInfo;
use super::state::DeviceState;
use crate::mdr::guard::Table;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charge {
    NotCharging,
    Charging,
    Unknown,
    Charged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    /// 0-100.
    pub level: u8,
    pub charging: Charge,
}

const RET: u8 = 0x23;
const NTFY: u8 = 0x25;

/// `<level> <charging>` after the two id bytes; a level above 100 or an unknown charging value is an error, not clamped.
fn parse(args: &[u8]) -> Result<Battery, String> {
    let &[level, charging] = args else { return Err(format!("battery payload has {} value bytes, expected 2", args.len())) };
    if level > 100 {
        return Err(format!("battery level {level} is above 100"));
    }
    let charging = match charging {
        0 => Charge::NotCharging,
        1 => Charge::Charging,
        2 => Charge::Unknown,
        3 => Charge::Charged,
        c => return Err(format!("unknown charging value {c}")),
    };
    Ok(Battery { level, charging })
}

pub struct BatteryFeature;

impl Feature for BatteryFeature {
    fn name(&self) -> &'static str {
        "battery"
    }

    fn required(&self) -> &'static [(Table, u8)] {
        &[(Table::T1, 0x20)]
    }

    fn initial_gets(&self, _: &DeviceInfo) -> Vec<Get> {
        vec![Get { table: Table::T1, payload: vec![0x22, 0x00], reply_id: RET }]
    }

    fn on_frame(&self, state: &mut DeviceState, table: Table, payload: &[u8]) -> Parsed {
        let (Table::T1, [id @ (RET | NTFY), 0x00, args @ ..]) = (table, payload) else { return Parsed::NotMine };
        // a malformed frame is reported as unhandled
        let Ok(b) = parse(args) else { return Parsed::NotMine };
        let seen = *id == NTFY || state.battery_notified;
        if state.battery == Some(b) && state.battery_notified == seen {
            return Parsed::Unchanged;
        }
        state.battery = Some(b);
        state.battery_notified = seen;
        Parsed::Changed
    }

    fn encode_set(&self, _: &DeviceState, _: &str) -> Result<(Table, Vec<u8>), String> {
        Err("battery is read-only".into())
    }

    /// Fallback polling only until the headset has sent one battery NTFY.
    fn wants_refresh(&self, state: &DeviceState) -> bool {
        !state.battery_notified
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::mock::FakeHeadset;
    use crate::device::{Command, CommandError, DeviceHandle, Event, FEATURES};
    use crate::mdr::frame::{TYPE_ACK, unpack};
    use crate::mdr::guard::{Mode, check};
    use crate::mdr::session::Timeouts;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    static ONLY: &[&dyn Feature] = &[&BatteryFeature];
    const FAST: Timeouts = Timeouts { ack: Duration::from_millis(15), response: Duration::from_millis(60), tries: 3 };
    const SLOW: Duration = Duration::from_secs(3600);

    fn start(h: FakeHeadset, refresh: Duration) -> (DeviceHandle, Receiver<Event>) {
        DeviceHandle::spawn(move || Ok(h), ONLY, FAST, refresh, None)
    }

    fn next(rx: &Receiver<Event>) -> Event {
        rx.recv_timeout(Duration::from_secs(5)).expect("an event")
    }

    fn synced(rx: &Receiver<Event>) -> DeviceState {
        loop {
            if let Event::Synced(s) = next(rx) {
                return s;
            }
        }
    }

    /// How many `22 00` commands the headset has received so far.
    fn gets(h: &FakeHeadset) -> impl Fn() -> usize + use<> {
        let w = h.writes.clone();
        move || w.lock().unwrap().iter().filter_map(|b| unpack(b).ok()).filter(|f| f.data_type != TYPE_ACK && f.payload == [0x22, 0x00]).count()
    }

    #[test]
    fn parses_all_charging_values_and_rejects_bad_ones() {
        // 0x61 = 97, from the XM6 capture
        for (c, want) in [(0, Charge::NotCharging), (1, Charge::Charging), (2, Charge::Unknown), (3, Charge::Charged)] {
            assert_eq!(parse(&[0x61, c]), Ok(Battery { level: 97, charging: want }));
        }
        assert_eq!(parse(&[100, 0]).unwrap().level, 100);
        assert!(parse(&[101, 0]).is_err());
        assert!(parse(&[50, 4]).is_err());
        assert!(parse(&[50]).is_err());
        assert!(parse(&[50, 0, 0]).is_err());
    }

    #[test]
    fn frames_update_state() {
        let mut s = DeviceState::default();
        let f = BatteryFeature;
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0x23, 0x00, 0x61, 0x00]), Parsed::Changed);
        assert_eq!(s.battery, Some(Battery { level: 97, charging: Charge::NotCharging }));
        assert!(f.wants_refresh(&s));
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0x23, 0x00, 0x61, 0x00]), Parsed::Unchanged);
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0x25, 0x00, 0x60, 0x01]), Parsed::Changed);
        assert_eq!(s.battery, Some(Battery { level: 96, charging: Charge::Charging }));
        assert!(!f.wants_refresh(&s));
        // a RET after an NTFY keeps the polling off
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0x23, 0x00, 0x5F, 0x01]), Parsed::Changed);
        assert!(!f.wants_refresh(&s));
        // not mine: other ids, the 0x28 variant, the other table, malformed values (state untouched)
        let before = s.clone();
        for (t, p) in [
            (Table::T1, &[0x23, 0x08, 0x50, 0x00][..]),
            (Table::T1, &[0x27, 0x00, 0x50, 0x00]),
            (Table::T2, &[0x23, 0x00, 0x50, 0x00]),
            (Table::T1, &[0x23, 0x00, 0x65, 0x00]),
        ] {
            assert_eq!(f.on_frame(&mut s, t, p), Parsed::NotMine);
        }
        assert_eq!(s, before);
    }

    #[test]
    fn gating_and_guard_on_the_real_support_list() {
        let (_handle, rx) = start(FakeHeadset::from_xm6_capture(), SLOW);
        let Event::Connected(info) = next(&rx) else { panic!() };
        assert!(info.has_t1(0x20) && !info.has_t1(0x28));
        assert!(crate::device::feature::supported(&BatteryFeature, &info));
        let g = &BatteryFeature.initial_gets(&info)[0];
        assert!(check(g.table, &g.payload, Mode::ReadOnly).is_ok());
        assert!(FEATURES.iter().any(|f| f.name() == "battery"));
        // no function 0x20: not supported
        let mut none = info.clone();
        none.t1_functions.retain(|&(f, _)| f != 0x20);
        assert!(!crate::device::feature::supported(&BatteryFeature, &none));
        assert!(BatteryFeature.encode_set(&DeviceState::default(), "50").is_err());
    }

    #[test]
    fn syncs_97_percent_from_the_capture_and_never_sends_the_threshold_variant() {
        let h = FakeHeadset::from_xm6_capture();
        let writes = h.writes.clone();
        let (_handle, rx) = start(h, SLOW);
        let s = synced(&rx);
        assert_eq!(s.battery, Some(Battery { level: 97, charging: Charge::NotCharging }));
        assert!(!writes.lock().unwrap().iter().filter_map(|b| unpack(b).ok()).any(|f| f.payload == [0x22, 0x08]));
    }

    #[test]
    fn without_function_0x20_state_is_none_and_nothing_is_sent() {
        let h = FakeHeadset::xm6(&[0x99], &[], true);
        let count = gets(&h);
        let (handle, rx) = start(h, Duration::from_millis(30));
        assert_eq!(synced(&rx).battery, None);
        handle.send(Command::Get { feature: "battery".into() }).unwrap();
        loop {
            if let Event::Error(e) = next(&rx) {
                assert!(matches!(e, CommandError::NotSupported(_)));
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(count(), 0);
    }

    #[test]
    fn fallback_refresh_stops_after_the_first_ntfy() {
        let mut h = FakeHeadset::from_xm6_capture();
        // the headset sends an NTFY along with the answer to the sync GET
        h.ntfy_on = Some((vec![0x22, 0x00], vec![0x25, 0x00, 0x50, 0x00]));
        let count = gets(&h);
        let (_handle, rx) = start(h, Duration::from_millis(30));
        let s = synced(&rx);
        assert_eq!(s.battery, Some(Battery { level: 80, charging: Charge::NotCharging }));
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(count(), 1);
    }

    #[test]
    fn fallback_refresh_polls_while_there_is_no_ntfy() {
        let h = FakeHeadset::from_xm6_capture();
        let count = gets(&h);
        let (_handle, rx) = start(h, Duration::from_millis(30));
        synced(&rx);
        let t = std::time::Instant::now();
        while count() < 3 {
            assert!(t.elapsed() < Duration::from_secs(5), "no periodic GET");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

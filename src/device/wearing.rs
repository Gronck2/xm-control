//! Wearing status (T2, data type 0x0E), read-only: GET `F2 00` -> RET `F3 00 <code>`, NTFY `F5 00 <code>`.
//! The reference queries it when Table 2 exists and any of T2 0xF0, T1 0xF6 or T1 0xF1 is listed (0xF0 alone is not reliable); the XM6 passes through 0xF1.

use super::feature::{Feature, Get, Parsed};
use super::handshake::DeviceInfo;
use super::state::DeviceState;
use crate::mdr::guard::Table;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wearing {
    Normal,
    Illegal,
    LeftNotWear,
    RightNotWear,
    BothNotWear,
    /// A code this version does not know, kept as sent.
    Unknown(u8),
}

fn parse(code: u8) -> Wearing {
    match code {
        0 => Wearing::Normal,
        1 => Wearing::Illegal,
        2 => Wearing::LeftNotWear,
        3 => Wearing::RightNotWear,
        4 => Wearing::BothNotWear,
        c => Wearing::Unknown(c),
    }
}

pub struct WearingFeature;

impl Feature for WearingFeature {
    fn name(&self) -> &'static str {
        "wearing"
    }

    /// Not a plain AND of functions: see `available`.
    fn required(&self) -> &'static [(Table, u8)] {
        &[]
    }

    fn available(&self, info: &DeviceInfo) -> bool {
        info.table2_enabled && (info.has_t2(0xF0) || info.has_t1(0xF6) || info.has_t1(0xF1))
    }

    fn initial_gets(&self, _: &DeviceInfo) -> Vec<Get> {
        vec![Get { table: Table::T2, payload: vec![0xF2, 0x00], reply_id: 0xF3 }]
    }

    fn on_frame(&self, state: &mut DeviceState, table: Table, payload: &[u8]) -> Parsed {
        let (Table::T2, [0xF3 | 0xF5, 0x00, a @ ..]) = (table, payload) else { return Parsed::NotMine };
        // loose: extra bytes are ignored, a missing code forgets the field
        let v = a.first().map(|&c| parse(c));
        if state.wearing == v {
            return Parsed::Unchanged;
        }
        state.wearing = v;
        Parsed::Changed
    }

    fn encode_set(&self, _: &DeviceState, _: &str) -> Result<(Table, Vec<u8>), String> {
        Err("the wearing status is read-only".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::mock::FakeHeadset;
    use crate::device::{Command, CommandError, DeviceHandle, Event, FEATURES};
    use crate::mdr::frame::{TYPE_ACK, TYPE_T2, unpack};
    use crate::mdr::guard::{GuardError, Mode, check};
    use crate::mdr::session::Timeouts;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    static ONLY: &[&dyn Feature] = &[&WearingFeature];
    const FAST: Timeouts = Timeouts { ack: Duration::from_millis(15), response: Duration::from_millis(60), tries: 3 };

    fn start(h: FakeHeadset) -> (DeviceHandle, Receiver<Event>) {
        DeviceHandle::spawn(move || Ok(h), ONLY, FAST, Duration::from_secs(3600), None)
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

    fn info(table2: bool, t1: &[u8], t2: &[u8]) -> DeviceInfo {
        DeviceInfo {
            protocol_version: 0,
            table2_enabled: table2,
            unique_id: String::new(),
            firmware: String::new(),
            model_name: String::new(),
            series: 0,
            color: 0,
            t1_functions: t1.iter().map(|&f| (f, 0)).collect(),
            t2_functions: t2.iter().map(|&f| (f, 0)).collect(),
        }
    }

    #[test]
    fn codes_parse_and_an_unknown_one_is_kept() {
        let mut s = DeviceState::default();
        let all = [Wearing::Normal, Wearing::Illegal, Wearing::LeftNotWear, Wearing::RightNotWear, Wearing::BothNotWear];
        for (code, want) in all.into_iter().enumerate() {
            // RET and NTFY alternate; each code differs from the previous one
            let id = if code % 2 == 0 { 0xF3 } else { 0xF5 };
            assert_eq!(WearingFeature.on_frame(&mut s, Table::T2, &[id, 0x00, code as u8]), Parsed::Changed);
            assert_eq!(s.wearing, Some(want));
        }
        assert_eq!(WearingFeature.on_frame(&mut s, Table::T2, &[0xF5, 0x00, 4]), Parsed::Unchanged);
        assert_eq!(WearingFeature.on_frame(&mut s, Table::T2, &[0xF5, 0x00, 9, 1]), Parsed::Changed);
        assert_eq!(s.wearing, Some(Wearing::Unknown(9)));
        assert_eq!(WearingFeature.on_frame(&mut s, Table::T2, &[0xF5, 0x00]), Parsed::Changed);
        assert_eq!(s.wearing, None);
        let before = s.clone();
        for (t, p) in [(Table::T1, &[0xF3, 0x00, 0][..]), (Table::T2, &[0xF3, 0x01, 0]), (Table::T2, &[0xF2, 0x00]), (Table::T2, &[0xF4, 0x00, 0])] {
            assert_eq!(WearingFeature.on_frame(&mut s, t, p), Parsed::NotMine, "{p:02X?}");
        }
        assert_eq!(s, before);
    }

    #[test]
    fn read_only() {
        assert!(WearingFeature.encode_set(&DeviceState::default(), "normal").is_err());
        assert!(check(Table::T2, &[0xF2, 0x00], Mode::ReadOnly).is_ok());
        for p in [&[0xF4, 0x00, 0][..], &[0xF0, 0x00], &[0xF2, 0x00, 0]] {
            assert_eq!(check(Table::T2, p, Mode::Normal).unwrap_err(), GuardError::NotAllowed, "{p:02X?}");
        }
    }

    #[test]
    fn query_gate() {
        // the real XM6 list: only T1 0xF1 of the three
        assert!(WearingFeature.available(&info(true, &[0xF1], &[])));
        assert!(WearingFeature.available(&info(true, &[0xF6], &[])));
        assert!(WearingFeature.available(&info(true, &[], &[0xF0])));
        assert!(!WearingFeature.available(&info(true, &[0x25, 0xFC], &[0x30])));
        assert!(!WearingFeature.available(&info(true, &[], &[])));
        // no Table 2 at all
        assert!(!WearingFeature.available(&info(false, &[0xF1], &[0xF0])));
    }

    #[test]
    fn sync_on_the_capture_uses_a_t2_frame() {
        let h = FakeHeadset::from_xm6_capture();
        let writes = h.writes.clone();
        let (_handle, rx) = start(h);
        let Event::Connected(i) = next(&rx) else { panic!() };
        assert!(crate::device::feature::supported(&WearingFeature, &i) && !i.has_t2(0xF0) && !i.has_t1(0xF6) && i.has_t1(0xF1));
        assert!(FEATURES.iter().any(|f| f.name() == "wearing"));
        assert_eq!(synced(&rx).wearing, Some(Wearing::Normal));
        let w = writes.lock().unwrap();
        assert!(w.iter().filter_map(|b| unpack(b).ok()).any(|f| f.data_type == TYPE_T2 && f.payload == [0xF2, 0x00]));
    }

    #[test]
    fn not_advertised_sends_nothing() {
        let h = FakeHeadset::xm6(&[0x20], &[], true);
        let writes = h.writes.clone();
        let (handle, rx) = start(h);
        assert_eq!(synced(&rx).wearing, None);
        let sent = || writes.lock().unwrap().iter().filter_map(|b| unpack(b).ok()).filter(|f| f.data_type != TYPE_ACK).any(|f| f.payload == [0xF2, 0x00]);
        assert!(!sent());
        handle.send(Command::Get { feature: "wearing".into() }).unwrap();
        assert!(matches!(next(&rx), Event::Error(CommandError::NotSupported(_))));
        assert!(!sent());
    }
}

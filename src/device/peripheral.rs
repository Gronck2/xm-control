//! Multipoint (T2 data type 0x0E, function 0x31) and the paired-device list (function 0x33, list type 0x02 = with class of device).
//! Multipoint: GET `36 01` -> `37 01 <u8>`, NTFY `39 01 <value> <result>`, SET `38 01 <0|1>`; raw u8, 1 = enabled (NOT the inverted OnOff).
//! List: GET `36 02` -> `37 02 <count> <entry>.. <trailing u8>`, NTFY `39 02 ..` the same; entry = 17 ASCII MAC, connected u8, class of device 3 bytes, length-prefixed name.
//! The list is read-only: unpair, connect and disconnect share the denied T2 `3C`. The trailing byte is unproven and ignored; the pairing-mode status read is not used.

use super::feature::{Feature, Get, Parsed};
use super::handshake::DeviceInfo;
use super::state::DeviceState;
use crate::mdr::guard::Table;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedDevice {
    /// 17 printable ASCII bytes; not hex-checked (the anonymised fixture has `XX:XX:...`).
    pub mac: String,
    pub connected: bool,
    pub class: u32,
    pub name: String,
}

/// The bytes after `37 02` / `39 02`. `None` if a field runs past the end or a MAC is not 17 printable ASCII bytes.
fn parse_list(a: &[u8]) -> Option<Vec<PairedDevice>> {
    let (&count, mut rest) = a.split_first()?;
    let mut out = Vec::new();
    for _ in 0..count {
        let (mac, r) = rest.split_at_checked(17)?;
        let (head, r) = r.split_at_checked(5)?; // connected, class of device (3), name length
        let (name, r) = r.split_at_checked(head[4] as usize)?;
        if !mac.iter().all(|b| (0x20..0x7F).contains(b)) {
            return None;
        }
        out.push(PairedDevice {
            mac: String::from_utf8_lossy(mac).into_owned(),
            connected: head[0] != 0,
            class: u32::from_be_bytes([0, head[1], head[2], head[3]]),
            name: String::from_utf8_lossy(name).into_owned(),
        });
        rest = r;
    }
    Some(out)
}

fn multipoint_get() -> Get {
    Get { table: Table::T2, payload: vec![0x36, 0x01], reply_id: 0x37 }
}

fn list_get() -> Get {
    Get { table: Table::T2, payload: vec![0x36, 0x02], reply_id: 0x37 }
}

pub struct MultipointFeature;

impl Feature for MultipointFeature {
    fn name(&self) -> &'static str {
        "multipoint"
    }

    fn required(&self) -> &'static [(Table, u8)] {
        &[(Table::T2, 0x31)]
    }

    fn initial_gets(&self, _: &DeviceInfo) -> Vec<Get> {
        vec![multipoint_get()]
    }

    fn on_frame(&self, state: &mut DeviceState, table: Table, payload: &[u8]) -> Parsed {
        let (Table::T2, [0x37 | 0x39, 0x01, a @ ..]) = (table, payload) else { return Parsed::NotMine };
        // loose: extra bytes are ignored, a missing value forgets the field; any non-zero is enabled
        let v = a.first().map(|&b| b != 0);
        if state.multipoint == v {
            return Parsed::Unchanged;
        }
        state.multipoint = v;
        Parsed::Changed
    }

    /// Re-read the switch and, if the list is used with this headset, the list (toggling can change who is connected).
    fn after_set(&self, info: &DeviceInfo) -> Vec<Get> {
        let mut v = vec![multipoint_get()];
        if DevicesFeature.available(info) {
            v.push(list_get());
        }
        v
    }

    /// `on` or `off`.
    fn encode_set(&self, _: &DeviceState, value: &str) -> Result<(Table, Vec<u8>), String> {
        match value.trim() {
            "on" => Ok((Table::T2, vec![0x38, 0x01, 1])),
            "off" => Ok((Table::T2, vec![0x38, 0x01, 0])),
            other => Err(format!("expected on or off, got {other:?}")),
        }
    }
}

pub struct DevicesFeature;

impl Feature for DevicesFeature {
    fn name(&self) -> &'static str {
        "devices"
    }

    fn required(&self) -> &'static [(Table, u8)] {
        &[(Table::T2, 0x33)]
    }

    fn available(&self, info: &DeviceInfo) -> bool {
        // only list type 0x02 is implemented: the 0x32 and 0x30 variants must be absent
        info.has_t2(0x33) && !info.has_t2(0x32) && !info.has_t2(0x30)
    }

    fn initial_gets(&self, _: &DeviceInfo) -> Vec<Get> {
        vec![list_get()]
    }

    fn on_frame(&self, state: &mut DeviceState, table: Table, payload: &[u8]) -> Parsed {
        let (Table::T2, [0x37 | 0x39, 0x02, a @ ..]) = (table, payload) else { return Parsed::NotMine };
        let v = parse_list(a); // a bad frame forgets the list
        if state.devices == v {
            return Parsed::Unchanged;
        }
        state.devices = v;
        Parsed::Changed
    }

    fn encode_set(&self, _: &DeviceState, _: &str) -> Result<(Table, Vec<u8>), String> {
        Err("the device list is read-only".into())
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

    static ONLY: &[&dyn Feature] = &[&MultipointFeature, &DevicesFeature];
    const FAST: Timeouts = Timeouts { ack: Duration::from_millis(15), response: Duration::from_millis(60), tries: 3 };
    const MAC: &[u8; 17] = b"AA:BB:CC:DD:EE:FF";

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

    fn info(t2: &[u8]) -> DeviceInfo {
        DeviceInfo {
            protocol_version: 0,
            table2_enabled: true,
            unique_id: String::new(),
            firmware: String::new(),
            model_name: String::new(),
            series: 0,
            color: 0,
            t1_functions: vec![],
            t2_functions: t2.iter().map(|&f| (f, 0)).collect(),
        }
    }

    fn entry(connected: u8, class: [u8; 3], name: &str) -> Vec<u8> {
        [&MAC[..], &[connected], &class, &[name.len() as u8], name.as_bytes()].concat()
    }

    fn list(entries: &[Vec<u8>], tail: &[u8]) -> Vec<u8> {
        [&[0x37, 0x02, entries.len() as u8][..], &entries.concat(), tail].concat()
    }

    #[test]
    fn fixture_list_decodes() {
        let h = FakeHeadset::from_xm6_capture();
        let reply = &h.replies.iter().find(|r| r.0 == TYPE_T2 && r.1 == [0x36, 0x02]).unwrap().2;
        let mut s = DeviceState::default();
        assert_eq!(DevicesFeature.on_frame(&mut s, Table::T2, reply), Parsed::Changed);
        let d = s.devices.as_ref().unwrap();
        assert_eq!(d.len(), 7);
        assert_eq!(d.iter().filter(|d| d.connected).count(), 1);
        assert_eq!((d[0].name.as_str(), d[0].connected, d[0].class, d[0].mac.as_str()), ("PC-DDD", true, 0x2A410C, "XX:XX:XX:XX:XX:XX"));
        assert_eq!((d[1].name.as_str(), d[1].connected, d[1].class), ("Laptop-AAAAAAAAAAAAAAAAAA", false, 0xFFFFFF));
        assert_eq!(DevicesFeature.on_frame(&mut s, Table::T2, reply), Parsed::Unchanged);
    }

    #[test]
    fn hand_built_lists_and_errors() {
        let mut s = DeviceState::default();
        assert_eq!(DevicesFeature.on_frame(&mut s, Table::T2, &list(&[], &[0])), Parsed::Changed);
        assert_eq!(s.devices, Some(vec![]));
        let one = entry(1, [1, 2, 3], "Phone");
        assert_eq!(DevicesFeature.on_frame(&mut s, Table::T2, &list(std::slice::from_ref(&one), &[1])), Parsed::Changed);
        assert_eq!(s.devices, Some(vec![PairedDevice { mac: "AA:BB:CC:DD:EE:FF".into(), connected: true, class: 0x010203, name: "Phone".into() }]));
        // NTFY with two entries (the trailing byte is optional); a non-UTF-8 name is decoded lossily
        let mut bad_name = entry(0, [0; 3], "x");
        *bad_name.last_mut().unwrap() = 0xFF;
        let mut f = list(&[one.clone(), bad_name], &[]);
        f[0] = 0x39;
        assert_eq!(DevicesFeature.on_frame(&mut s, Table::T2, &f), Parsed::Changed);
        assert_eq!(s.devices.as_ref().unwrap()[1].name, "\u{FFFD}");
        // errors forget the list: a short MAC, a name past the frame, a count past the frame, a non-printable MAC, no count
        let mut short_mac = list(std::slice::from_ref(&one), &[]);
        short_mac.remove(3);
        let mut ctrl = list(std::slice::from_ref(&one), &[]);
        ctrl[5] = 0x01;
        let mut cut = list(std::slice::from_ref(&one), &[]);
        cut.truncate(cut.len() - 2);
        let mut count = list(std::slice::from_ref(&one), &[]);
        count[2] = 2;
        for bad in [short_mac, ctrl, cut, count, vec![0x37, 0x02]] {
            DevicesFeature.on_frame(&mut s, Table::T2, &list(std::slice::from_ref(&one), &[1]));
            assert_eq!(DevicesFeature.on_frame(&mut s, Table::T2, &bad), Parsed::Changed, "{bad:02X?}");
            assert_eq!(s.devices, None, "{bad:02X?}");
        }
    }

    #[test]
    fn multipoint_raw_polarity_and_foreign_frames() {
        let mut s = DeviceState::default();
        assert_eq!(MultipointFeature.on_frame(&mut s, Table::T2, &[0x37, 0x01, 0x01]), Parsed::Changed);
        assert_eq!(s.multipoint, Some(true));
        assert_eq!(MultipointFeature.on_frame(&mut s, Table::T2, &[0x39, 0x01, 0x00, 0x00]), Parsed::Changed);
        assert_eq!(s.multipoint, Some(false));
        assert_eq!(MultipointFeature.on_frame(&mut s, Table::T2, &[0x37, 0x01, 0x02]), Parsed::Changed);
        assert_eq!(s.multipoint, Some(true));
        assert_eq!(MultipointFeature.on_frame(&mut s, Table::T2, &[0x37, 0x01]), Parsed::Changed);
        assert_eq!(s.multipoint, None);
        let before = s.clone();
        for (t, p) in [(Table::T1, &[0x37, 0x01, 1][..]), (Table::T2, &[0x37, 0x02, 0]), (Table::T2, &[0x36, 0x01])] {
            assert_eq!(MultipointFeature.on_frame(&mut s, t, p), Parsed::NotMine, "{p:02X?}");
        }
        assert_eq!(s, before);
        // not the inverted OnOff: on = 1, off = 0
        assert_eq!(MultipointFeature.encode_set(&s, "on"), Ok((Table::T2, vec![0x38, 0x01, 1])));
        assert_eq!(MultipointFeature.encode_set(&s, " off "), Ok((Table::T2, vec![0x38, 0x01, 0])));
        for bad in ["", "1", "ON", "on off"] {
            assert!(MultipointFeature.encode_set(&s, bad).is_err(), "{bad:?}");
        }
        assert!(DevicesFeature.encode_set(&s, "on").is_err());
    }

    #[test]
    fn guard_rows() {
        for v in ["on", "off"] {
            let (t, p) = MultipointFeature.encode_set(&DeviceState::default(), v).unwrap();
            assert!(check(t, &p, Mode::Normal).is_ok());
            assert_eq!(check(t, &p, Mode::ReadOnly).unwrap_err(), GuardError::ReadOnly);
        }
        for p in [&[0x36, 0x01][..], &[0x36, 0x02], &[0x32, 0x02]] {
            assert!(check(Table::T2, p, Mode::ReadOnly).is_ok());
        }
        for p in [&[0x38, 0x01, 2][..], &[0x38, 0x01], &[0x38, 0x01, 1, 0], &[0x38, 0x02, 1], &[0x38, 0x00, 1]] {
            assert_eq!(check(Table::T2, p, Mode::Normal).unwrap_err(), GuardError::NotAllowed, "{p:02X?}");
        }
    }

    #[test]
    fn query_gate_from_the_real_list() {
        // XM6: 0x31 and 0x33 present, 0x30 and 0x32 absent
        let xm6 = info(&[0x31, 0x33, 0x34]);
        assert!(MultipointFeature.available(&xm6) && DevicesFeature.available(&xm6));
        assert!(!DevicesFeature.available(&info(&[0x33, 0x32])));
        assert!(!DevicesFeature.available(&info(&[0x33, 0x30])));
        assert!(!DevicesFeature.available(&info(&[0x32])));
        assert!(!MultipointFeature.available(&info(&[0x33])));
    }

    #[test]
    fn sync_on_the_capture_and_a_set_rereads_both() {
        let mut h = FakeHeadset::from_xm6_capture();
        h.no_reply.push(vec![0x38, 0x01, 0]);
        let writes = h.writes.clone();
        let (handle, rx) = start(h);
        assert!(FEATURES.iter().any(|f| f.name() == "multipoint") && FEATURES.iter().any(|f| f.name() == "devices"));
        let s = synced(&rx);
        assert_eq!(s.multipoint, Some(true));
        assert_eq!(s.devices.as_ref().map(Vec::len), Some(7));
        let sent = || -> Vec<Vec<u8>> {
            writes
                .lock()
                .unwrap()
                .iter()
                .filter_map(|b| unpack(b).ok())
                .filter(|f| f.data_type == TYPE_T2 && f.payload != [0x06, 0x00])
                .map(|f| f.payload)
                .collect()
        };
        assert_eq!(sent(), [vec![0x36, 0x01], vec![0x36, 0x02]]);
        handle.send(Command::Set { feature: "multipoint".into(), value: "off".into() }).unwrap();
        handle.send(Command::Get { feature: "multipoint".into() }).unwrap();
        // the Get is queued behind the Set, so once its re-read is on the wire the Set's two re-reads are too
        while sent().len() < 6 {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sent()[2..], [vec![0x38, 0x01, 0], vec![0x36, 0x01], vec![0x36, 0x02], vec![0x36, 0x01]]);
        handle.send(Command::Set { feature: "devices".into(), value: "x".into() }).unwrap();
        while !matches!(next(&rx), Event::Error(CommandError::BadValue(_))) {}
    }

    #[test]
    fn ntfy_39_02_and_39_01_through_the_worker() {
        let mut h = FakeHeadset::from_xm6_capture();
        // the headset answers the GETs with NTFY frames: a one-entry list and multipoint off
        h.replies.retain(|r| !(r.0 == TYPE_T2 && (r.1 == [0x36, 0x02] || r.1 == [0x36, 0x01])));
        let mut f = list(&[entry(1, [1, 2, 3], "Phone")], &[1]);
        f[0] = 0x39;
        h.replies.push((TYPE_T2, vec![0x36, 0x02], f));
        h.replies.push((TYPE_T2, vec![0x36, 0x01], vec![0x39, 0x01, 0x00, 0x00]));
        let (_handle, rx) = start(h);
        let s = synced(&rx);
        assert_eq!(s.multipoint, Some(false));
        assert_eq!(s.devices.as_ref().map(|d| (d.len(), d[0].name.as_str(), d[0].connected)), Some((1, "Phone", true)));
    }

    #[test]
    fn set_without_the_list_function_rereads_only_the_switch() {
        let h = FakeHeadset::xm6(&[], &[0x31], true);
        let writes = h.writes.clone();
        let (handle, rx) = start(h);
        synced(&rx);
        handle.send(Command::Set { feature: "multipoint".into(), value: "on".into() }).unwrap();
        handle.send(Command::Get { feature: "multipoint".into() }).unwrap();
        let sent = || -> Vec<Vec<u8>> {
            let w = writes.lock().unwrap();
            w.iter().filter_map(|b| unpack(b).ok()).filter(|f| f.data_type == TYPE_T2 && f.payload != [0x06, 0x00]).map(|f| f.payload).collect()
        };
        while sent().len() < 4 {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sent(), [vec![0x36, 0x01], vec![0x38, 0x01, 1], vec![0x36, 0x01], vec![0x36, 0x01]]);
    }

    #[test]
    fn not_advertised_sends_nothing() {
        let h = FakeHeadset::xm6(&[], &[0x10], true);
        let writes = h.writes.clone();
        let (handle, rx) = start(h);
        let s = synced(&rx);
        assert_eq!((s.multipoint, s.devices), (None, None));
        handle.send(Command::Get { feature: "multipoint".into() }).unwrap();
        assert!(matches!(next(&rx), Event::Error(CommandError::NotSupported(_))));
        let w = writes.lock().unwrap();
        assert!(!w.iter().filter_map(|b| unpack(b).ok()).filter(|f| f.data_type != TYPE_ACK).any(|f| f.data_type == TYPE_T2 && f.payload[0] == 0x36));
    }
}

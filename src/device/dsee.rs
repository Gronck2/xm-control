//! DSEE upscaling (T1 function 0xE2, UPSCALING_AUTO_OFF): GET `E6 01` -> RET `E7 01 <00 OFF | 01 AUTO>`, NTFY `E9 01 <value>`, SET `E8 01 <value>`;
//! status GET `E2 01` -> RET `E3 01 <EnableDisable>` (00 = available), NTFY `E5 01 <status>`. While the status says disabled no SET is built.

use super::feature::{Feature, Get, Parsed};
use super::handshake::DeviceInfo;
use super::state::DeviceState;
use crate::mdr::guard::Table;

const ID: u8 = 0x01;

/// The value byte of a RET/NTFY: strict, an unknown value is an error.
fn parse(args: &[u8]) -> Result<bool, String> {
    match args {
        [0, ..] => Ok(false),
        [1, ..] => Ok(true),
        _ => Err(format!("unknown DSEE value: {args:02X?}")),
    }
}

pub struct DseeFeature;

impl Feature for DseeFeature {
    fn name(&self) -> &'static str {
        "dsee"
    }

    fn required(&self) -> &'static [(Table, u8)] {
        &[(Table::T1, 0xE2)]
    }

    fn initial_gets(&self, _: &DeviceInfo) -> Vec<Get> {
        vec![Get { table: Table::T1, payload: vec![0xE2, ID], reply_id: 0xE3 }, self.reread()]
    }

    fn on_frame(&self, state: &mut DeviceState, table: Table, payload: &[u8]) -> Parsed {
        match (table, payload) {
            (Table::T1, [0xE7 | 0xE9, ID, a @ ..]) => {
                // an unusable frame makes the stored value stale: forget it
                let v = parse(a).ok();
                if state.dsee == v {
                    return Parsed::Unchanged;
                }
                state.dsee = v;
                Parsed::Changed
            }
            // status RET / NTFY: EnableDisable, DISABLE = 1
            (Table::T1, [0xE3 | 0xE5, ID, flag, ..]) => {
                let off = *flag == 1;
                if state.dsee_disabled == off {
                    return Parsed::Unchanged;
                }
                state.dsee_disabled = off;
                Parsed::Changed
            }
            _ => Parsed::NotMine,
        }
    }

    fn after_set(&self, _: &DeviceInfo) -> Vec<Get> {
        vec![self.reread()]
    }

    /// `off` or `auto`.
    fn encode_set(&self, state: &DeviceState, value: &str) -> Result<(Table, Vec<u8>), String> {
        if state.dsee_disabled {
            return Err("the headset reports DSEE as unavailable".into());
        }
        let v = match value.trim() {
            "off" => 0,
            "auto" => 1,
            other => return Err(format!("expected off or auto, got {other:?}")),
        };
        Ok((Table::T1, vec![0xE8, ID, v]))
    }
}

impl DseeFeature {
    fn reread(&self) -> Get {
        Get { table: Table::T1, payload: vec![0xE6, ID], reply_id: 0xE7 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::mock::FakeHeadset;
    use crate::device::{Command, CommandError, DeviceHandle, Event, FEATURES};
    use crate::mdr::frame::{TYPE_ACK, unpack};
    use crate::mdr::guard::{GuardError, Mode, check};
    use crate::mdr::session::Timeouts;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    static ONLY: &[&dyn Feature] = &[&DseeFeature];
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

    #[test]
    fn parses_values_strictly() {
        assert_eq!(parse(&[1]), Ok(true)); // `E7 01 01` from the capture
        assert_eq!(parse(&[0]), Ok(false));
        assert!(parse(&[2]).is_err() && parse(&[]).is_err());
    }

    #[test]
    fn set_bytes_and_unavailable_blocks_set() {
        let mut s = DeviceState::default();
        assert_eq!(DseeFeature.encode_set(&s, "off"), Ok((Table::T1, vec![0xE8, 1, 0])));
        assert_eq!(DseeFeature.encode_set(&s, " auto "), Ok((Table::T1, vec![0xE8, 1, 1])));
        for bad in ["", "on", "extreme", "1"] {
            assert!(DseeFeature.encode_set(&s, bad).is_err(), "{bad:?}");
        }
        assert_eq!(DseeFeature.on_frame(&mut s, Table::T1, &[0xE3, 1, 1]), Parsed::Changed);
        assert!(DseeFeature.encode_set(&s, "auto").is_err());
        assert_eq!(DseeFeature.on_frame(&mut s, Table::T1, &[0xE5, 1, 0]), Parsed::Changed);
        assert!(DseeFeature.encode_set(&s, "auto").is_ok());
    }

    #[test]
    fn frames_update_state() {
        let mut s = DeviceState::default();
        let f = DseeFeature;
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0xE7, 1, 1]), Parsed::Changed);
        assert_eq!(s.dsee, Some(true));
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0xE7, 1, 1]), Parsed::Unchanged);
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0xE9, 1, 0]), Parsed::Changed);
        assert_eq!(s.dsee, Some(false));
        // status `E3 01 00` = available: nothing to change from the default
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0xE3, 1, 0]), Parsed::Unchanged);
        assert!(!s.dsee_disabled);
        let before = s.clone();
        for (t, p) in [(Table::T1, &[0xE7, 2, 1][..]), (Table::T1, &[0xE6, 1]), (Table::T1, &[0xE3, 1]), (Table::T2, &[0xE7, 1, 1])] {
            assert_eq!(f.on_frame(&mut s, t, p), Parsed::NotMine, "{p:02X?}");
        }
        assert_eq!(s, before);
        // an unknown value forgets the state
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0xE7, 1, 2]), Parsed::Changed);
        assert_eq!(s.dsee, None);
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0xE7, 1, 2]), Parsed::Unchanged);
    }

    #[test]
    fn guard_allows_exactly_the_two_sets() {
        for v in 0..=1 {
            let (_, p) = DseeFeature.encode_set(&DeviceState::default(), ["off", "auto"][v]).unwrap();
            assert!(check(Table::T1, &p, Mode::Normal).is_ok());
            assert_eq!(check(Table::T1, &p, Mode::ReadOnly).unwrap_err(), GuardError::ReadOnly);
        }
    }

    #[test]
    fn gating_and_sync_on_the_capture() {
        let (_h, rx) = start(FakeHeadset::from_xm6_capture());
        let Event::Connected(info) = next(&rx) else { panic!() };
        assert!(info.has_t1(0xE2) && crate::device::feature::supported(&DseeFeature, &info));
        assert!(FEATURES.iter().any(|f| f.name() == "dsee"));
        let mut none = info.clone();
        none.t1_functions.retain(|&(f, _)| f != 0xE2);
        assert!(!crate::device::feature::supported(&DseeFeature, &none));
        let s = synced(&rx);
        assert_eq!((s.dsee, s.dsee_disabled), (Some(true), false));
    }

    #[test]
    fn not_advertised_sends_nothing() {
        let h = FakeHeadset::xm6(&[0x20], &[], true); // no 0xE2
        let writes = h.writes.clone();
        let (handle, rx) = start(h);
        let s = synced(&rx);
        assert_eq!(s.dsee, None);
        let before = writes.lock().unwrap().len();
        handle.send(Command::Set { feature: "dsee".into(), value: "off".into() }).unwrap();
        assert!(matches!(next(&rx), Event::Error(CommandError::NotSupported(_))));
        assert_eq!(writes.lock().unwrap().len(), before);
    }

    #[test]
    fn set_goes_out_and_the_ntfy_updates_state() {
        let mut h = FakeHeadset::from_xm6_capture();
        h.no_reply.push(vec![0xE8, 1, 0]);
        h.ntfy_on = Some((vec![0xE8, 1, 0], vec![0xE9, 1, 0]));
        let writes = h.writes.clone();
        let (handle, rx) = start(h);
        synced(&rx);
        handle.send(Command::Set { feature: "dsee".into(), value: "off".into() }).unwrap();
        loop {
            if let Event::StateChanged { state, .. } = next(&rx) {
                assert_eq!(state.dsee, Some(false));
                break;
            }
        }
        assert!(writes.lock().unwrap().iter().filter_map(|b| unpack(b).ok()).any(|f| f.data_type != TYPE_ACK && f.payload == [0xE8, 1, 0]));
    }
}

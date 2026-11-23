//! Speak-to-Chat (T1 function 0xFC, SMART_TALKING_MODE_TYPE2).
//! Enable: GET `F6 0C` -> RET `F7 0C <onoff> <preview>`, NTFY `F9 0C ..`, SET `F8 0C <onoff> 01` with INVERTED polarity (00 = ON, 01 = OFF), preview always OFF.
//! Sensitivity/timeout: GET `FA 0C` -> RET `FB 0C <sens> <timeout>`, NTFY `FD 0C ..`, SET `FC 0C <sens> <timeout>` (always both bytes, the other one from state).
//! `F8` is shared with `F8 09` (reset, denied); the guard row for `F8 0C` takes exactly `<00|01> 01` after it.

use super::feature::{Feature, Get, Parsed};
use super::handshake::DeviceInfo;
use super::state::DeviceState;
use crate::mdr::guard::Table;

const SENS: [&str; 3] = ["auto", "high", "low"];
const TIMEOUT: [&str; 4] = ["fast", "mid", "slow", "none"];

/// Each field is `None` until read, and again when the headset sends a value we do not know.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stc {
    pub enabled: Option<bool>,
    /// Index into AUTO, HIGH, LOW (the wire value).
    pub sensitivity: Option<u8>,
    /// Index into FAST, MID, SLOW, NONE (the wire value).
    pub timeout: Option<u8>,
}

fn gets() -> Vec<Get> {
    vec![Get { table: Table::T1, payload: vec![0xF6, 0x0C], reply_id: 0xF7 }, Get { table: Table::T1, payload: vec![0xFA, 0x0C], reply_id: 0xFB }]
}

pub struct StcFeature;

impl Feature for StcFeature {
    fn name(&self) -> &'static str {
        "stc"
    }

    fn required(&self) -> &'static [(Table, u8)] {
        &[(Table::T1, 0xFC)]
    }

    fn initial_gets(&self, _: &DeviceInfo) -> Vec<Get> {
        gets()
    }

    fn on_frame(&self, state: &mut DeviceState, table: Table, payload: &[u8]) -> Parsed {
        let mut new = state.stc;
        match (table, payload) {
            // loose: extra bytes are ignored, an unknown value forgets the field
            (Table::T1, [0xF7 | 0xF9, 0x0C, a @ ..]) => new.enabled = a.first().and_then(|&b| (b <= 1).then_some(b == 0)),
            (Table::T1, [0xFB | 0xFD, 0x0C, a @ ..]) => {
                new.sensitivity = a.first().copied().filter(|&b| b <= 2);
                new.timeout = a.get(1).copied().filter(|&b| b <= 3);
            }
            _ => return Parsed::NotMine,
        }
        if new == state.stc {
            return Parsed::Unchanged;
        }
        state.stc = new;
        Parsed::Changed
    }

    fn after_set(&self) -> Vec<Get> {
        gets()
    }

    /// `on`, `off`, `sens auto|high|low` or `timeout fast|mid|slow|none`.
    fn encode_set(&self, state: &DeviceState, value: &str) -> Result<(Table, Vec<u8>), String> {
        let words: Vec<&str> = value.split_whitespace().collect();
        let idx = |list: &[&str], w: &str| list.iter().position(|&x| x == w).map(|i| i as u8);
        match words[..] {
            ["on"] => Ok((Table::T1, vec![0xF8, 0x0C, 0, 1])),
            ["off"] => Ok((Table::T1, vec![0xF8, 0x0C, 1, 1])),
            [kind @ ("sens" | "timeout"), w] => {
                let (list, field) = if kind == "sens" { (&SENS[..], 0) } else { (&TIMEOUT[..], 1) };
                let v = idx(list, w).ok_or_else(|| format!("expected one of {list:?}, got {w:?}"))?;
                let (Some(s), Some(t)) = (state.stc.sensitivity, state.stc.timeout) else {
                    return Err("the current sensitivity/timeout are not known yet".into());
                };
                let mut args = [s, t];
                args[field] = v;
                Ok((Table::T1, vec![0xFC, 0x0C, args[0], args[1]]))
            }
            _ => Err(format!("expected on, off, sens <auto|high|low> or timeout <fast|mid|slow|none>, got {value:?}")),
        }
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

    static ONLY: &[&dyn Feature] = &[&StcFeature];
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

    fn state(enabled: bool, sens: u8, timeout: u8) -> DeviceState {
        DeviceState { stc: Stc { enabled: Some(enabled), sensitivity: Some(sens), timeout: Some(timeout) }, ..Default::default() }
    }

    #[test]
    fn fixture_frames_decode_with_inverted_polarity() {
        let mut s = DeviceState::default();
        // `F7 0C 01 01` from the capture: OFF, preview off
        assert_eq!(StcFeature.on_frame(&mut s, Table::T1, &[0xF7, 0x0C, 1, 1]), Parsed::Changed);
        assert_eq!(s.stc.enabled, Some(false));
        assert_eq!(StcFeature.on_frame(&mut s, Table::T1, &[0xF9, 0x0C, 0, 1]), Parsed::Changed);
        assert_eq!(s.stc.enabled, Some(true));
        // `FB 0C 00 01`: AUTO, MID
        assert_eq!(StcFeature.on_frame(&mut s, Table::T1, &[0xFB, 0x0C, 0, 1]), Parsed::Changed);
        assert_eq!((s.stc.sensitivity, s.stc.timeout), (Some(0), Some(1)));
        assert_eq!(StcFeature.on_frame(&mut s, Table::T1, &[0xFB, 0x0C, 0, 1]), Parsed::Unchanged);
        assert_eq!(StcFeature.on_frame(&mut s, Table::T1, &[0xFD, 0x0C, 2, 3, 9]), Parsed::Changed);
        assert_eq!((s.stc.sensitivity, s.stc.timeout), (Some(2), Some(3)));
    }

    #[test]
    fn loose_device_frames_and_foreign_frames() {
        let mut s = state(true, 1, 1);
        // unknown values forget only their own field
        assert_eq!(StcFeature.on_frame(&mut s, Table::T1, &[0xFB, 0x0C, 1, 7]), Parsed::Changed);
        assert_eq!((s.stc.sensitivity, s.stc.timeout), (Some(1), None));
        assert_eq!(StcFeature.on_frame(&mut s, Table::T1, &[0xF7, 0x0C, 2, 1]), Parsed::Changed);
        assert_eq!(s.stc.enabled, None);
        assert_eq!(StcFeature.on_frame(&mut s, Table::T1, &[0xF7, 0x0C]), Parsed::Unchanged);
        let before = s.clone();
        for (t, p) in [(Table::T1, &[0xF7, 0x0D, 0][..]), (Table::T1, &[0xF6, 0x0C]), (Table::T2, &[0xF7, 0x0C, 0, 1]), (Table::T1, &[0xF8, 0x0C, 0, 1])] {
            assert_eq!(StcFeature.on_frame(&mut s, t, p), Parsed::NotMine, "{p:02X?}");
        }
        assert_eq!(s, before);
    }

    #[test]
    fn set_bytes_and_read_modify_write() {
        let s = state(false, 2, 3);
        assert_eq!(StcFeature.encode_set(&s, "on"), Ok((Table::T1, vec![0xF8, 0x0C, 0, 1])));
        assert_eq!(StcFeature.encode_set(&s, " off "), Ok((Table::T1, vec![0xF8, 0x0C, 1, 1])));
        // each sens/timeout SET keeps the other field; the enable state is not part of it
        assert_eq!(StcFeature.encode_set(&s, "sens auto"), Ok((Table::T1, vec![0xFC, 0x0C, 0, 3])));
        assert_eq!(StcFeature.encode_set(&s, "sens high"), Ok((Table::T1, vec![0xFC, 0x0C, 1, 3])));
        assert_eq!(StcFeature.encode_set(&s, "timeout mid"), Ok((Table::T1, vec![0xFC, 0x0C, 2, 1])));
        assert_eq!(StcFeature.encode_set(&s, "timeout fast"), Ok((Table::T1, vec![0xFC, 0x0C, 2, 0])));
        for bad in ["", "1", "sens", "sens medium", "sens mid", "timeout auto", "timeout", "on off", "sens auto mid", "ON"] {
            assert!(StcFeature.encode_set(&s, bad).is_err(), "{bad:?}");
        }
        // without a known sensitivity/timeout a partial SET would guess: refuse
        assert!(StcFeature.encode_set(&DeviceState::default(), "sens low").is_err());
        assert!(StcFeature.encode_set(&DeviceState::default(), "on").is_ok());
    }

    #[test]
    fn guard_accepts_the_sets_and_only_those() {
        let s = state(true, 0, 1);
        for v in ["on", "off", "sens auto", "sens high", "sens low", "timeout fast", "timeout mid", "timeout slow", "timeout none"] {
            let (_, p) = StcFeature.encode_set(&s, v).unwrap();
            assert!(check(Table::T1, &p, Mode::Normal).is_ok(), "{v}");
            assert_eq!(check(Table::T1, &p, Mode::ReadOnly).unwrap_err(), GuardError::ReadOnly);
        }
        for p in [&[0xF6, 0x0C][..], &[0xFA, 0x0C]] {
            assert!(check(Table::T1, p, Mode::ReadOnly).is_ok());
        }
        for p in [&[0xFC, 0x0C, 3, 0][..], &[0xFC, 0x0C, 0, 4], &[0xFC, 0x0C, 0], &[0xFC, 0x0C], &[0xFC, 0x0C, 0, 0, 0], &[0xFC, 0x0D, 0, 0]] {
            assert_eq!(check(Table::T1, p, Mode::Normal).unwrap_err(), GuardError::NotAllowed, "{p:02X?}");
        }
    }

    #[test]
    fn f8_accepts_only_f8_0c_onoff_preview_off_and_f8_09_stays_denied() {
        let ok = |p: &[u8]| check(Table::T1, p, Mode::Normal).is_ok();
        let mut allowed = Vec::new();
        for b in 0..=255u8 {
            for c in 0..=255u8 {
                for tail in [&[][..], &[0], &[1], &[2], &[0xFF], &[1, 0], &[1, 1]] {
                    let p = [&[0xF8, b, c][..], tail].concat();
                    if ok(&p) {
                        allowed.push(p);
                    }
                    // 2- and 4-byte forms of every (b, c) as well
                    if tail.is_empty() {
                        for d in 0..=255u8 {
                            let p = [0xF8, b, c, d];
                            if ok(&p) {
                                allowed.push(p.to_vec());
                            }
                        }
                    }
                }
            }
        }
        allowed.sort();
        allowed.dedup();
        // F8 01 <00|01> is the auto-pause SET (src/device/power.rs)
        assert_eq!(allowed, [vec![0xF8, 0x01, 0], vec![0xF8, 0x01, 1], vec![0xF8, 0x0C, 0, 1], vec![0xF8, 0x0C, 1, 1]]);
        for p in [
            &[0xF8][..],
            &[0xF8, 0x01],
            &[0xF8, 0x01, 2],
            &[0xF8, 0x01, 0, 1],
            &[0xF8, 0x0C],
            &[0xF8, 0x0C, 0],
            &[0xF8, 0x0C, 1],
            &[0xF8, 0x0C, 2, 1],
            &[0xF8, 0x0C, 0, 0],
            &[0xF8, 0x0C, 0, 1, 1],
        ] {
            assert!(!ok(p), "{p:02X?}");
        }
        for p in [&[0xF8, 0x09][..], &[0xF8, 0x09, 0], &[0xF8, 0x09, 1], &[0xF8, 0x09, 0, 1], &[0xF8, 0x09, 1, 1], &[0xF8, 0x09, 0x0C, 1]] {
            for m in [Mode::Normal, Mode::ReadOnly] {
                assert_eq!(check(Table::T1, p, m).unwrap_err(), GuardError::Denied, "{p:02X?}");
            }
        }
        for p in [&[0xF8, 0x0F, 0, 1][..], &[0xF8, 0x08, 0, 1]] {
            assert_eq!(check(Table::T1, p, Mode::Normal).unwrap_err(), GuardError::NotAllowed);
        }
        assert_eq!(check(Table::T2, &[0xF8, 0x0C, 0, 1], Mode::Normal).unwrap_err(), GuardError::NotAllowed);
    }

    #[test]
    fn gating_and_sync_on_the_capture() {
        let (_h, rx) = start(FakeHeadset::from_xm6_capture());
        let Event::Connected(info) = next(&rx) else { panic!() };
        assert!(info.has_t1(0xFC) && crate::device::feature::supported(&StcFeature, &info));
        assert!(FEATURES.iter().any(|f| f.name() == "stc"));
        let s = synced(&rx);
        // `F7 0C 01 01` = off, `FB 0C 00 01` = AUTO, MID
        assert_eq!(s.stc, Stc { enabled: Some(false), sensitivity: Some(0), timeout: Some(1) });
    }

    #[test]
    fn not_advertised_sends_nothing() {
        let h = FakeHeadset::xm6(&[0x20], &[], true); // no 0xFC
        let writes = h.writes.clone();
        let (handle, rx) = start(h);
        let s = synced(&rx);
        assert_eq!(s.stc, Stc::default());
        let before = writes.lock().unwrap().len();
        handle.send(Command::Set { feature: "stc".into(), value: "on".into() }).unwrap();
        assert!(matches!(next(&rx), Event::Error(CommandError::NotSupported(_))));
        assert_eq!(writes.lock().unwrap().len(), before);
    }

    #[test]
    fn set_goes_out_and_the_ntfy_updates_state() {
        let mut h = FakeHeadset::from_xm6_capture();
        h.no_reply.push(vec![0xF8, 0x0C, 0, 1]);
        h.ntfy_on = Some((vec![0xF8, 0x0C, 0, 1], vec![0xF9, 0x0C, 0, 1]));
        let writes = h.writes.clone();
        let (handle, rx) = start(h);
        synced(&rx);
        handle.send(Command::Set { feature: "stc".into(), value: "on".into() }).unwrap();
        loop {
            if let Event::StateChanged { state, .. } = next(&rx) {
                assert_eq!(state.stc.enabled, Some(true));
                break;
            }
        }
        assert!(writes.lock().unwrap().iter().filter_map(|b| unpack(b).ok()).any(|f| f.data_type != TYPE_ACK && f.payload == [0xF8, 0x0C, 0, 1]));
    }
}

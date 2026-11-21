//! Auto power-off with wearing detection (T1 function 0x25) and auto pause on removal (T1 function 0xF1).
//! Power-off: capability `20 05` -> `21 05 <count> <candidates>`, GET `26 05` -> `27 05 <current> <lastSelect>`, NTFY `29 05 ..`, SET `28 05 <current> 00`.
//! The minute-timer variant (function 0x24) is not advertised by the XM6 and not implemented; no `24 ..` command is ever built.
//! Auto pause: GET `F6 01` -> `F7 01 <onoff>`, NTFY `F9 01 ..`, SET `F8 01 <onoff>` with INVERTED polarity (00 = ON, 01 = OFF).
//! `F8` is shared with `F8 09` (reset, denied); the guard row for `F8 01` takes exactly one `00|01` after it.

use super::feature::{Feature, Get, Parsed};
use super::handshake::DeviceInfo;
use super::state::DeviceState;
use crate::mdr::guard::Table;

/// CLI word -> wire element (POWER_OFF_WHEN_REMOVED_FROM_EARS, DISABLE).
const ELEMENTS: [(&str, u8); 2] = [("removed", 0x10), ("off", 0x11)];

/// `None` until read, and again when the headset sends a frame without the field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Power {
    /// The elements the capability reply lists.
    pub candidates: Option<Vec<u8>>,
    /// The current element as sent (raw, so an element we have no word for is still visible).
    pub current: Option<u8>,
}

pub struct PowerOffFeature;

impl Feature for PowerOffFeature {
    fn name(&self) -> &'static str {
        "poweroff"
    }

    fn required(&self) -> &'static [(Table, u8)] {
        &[(Table::T1, 0x25)]
    }

    fn initial_gets(&self, _: &DeviceInfo) -> Vec<Get> {
        vec![Get { table: Table::T1, payload: vec![0x20, 0x05], reply_id: 0x21 }, self.reread()]
    }

    fn on_frame(&self, state: &mut DeviceState, table: Table, payload: &[u8]) -> Parsed {
        let mut new = state.power.clone();
        match (table, payload) {
            // loose: extra bytes are ignored, a short frame forgets the field
            (Table::T1, [0x21, 0x05, a @ ..]) => new.candidates = a.split_first().map(|(&n, r)| r.iter().take(n as usize).copied().collect()),
            (Table::T1, [0x27 | 0x29, 0x05, a @ ..]) => new.current = a.first().copied(),
            _ => return Parsed::NotMine,
        }
        if new == state.power {
            return Parsed::Unchanged;
        }
        state.power = new;
        Parsed::Changed
    }

    fn after_set(&self) -> Vec<Get> {
        vec![self.reread()]
    }

    /// `removed` (power off when taken off the ears) or `off` (never); only what the capability lists.
    fn encode_set(&self, state: &DeviceState, value: &str) -> Result<(Table, Vec<u8>), String> {
        let word = value.trim();
        let &(_, e) = ELEMENTS.iter().find(|(w, _)| *w == word).ok_or_else(|| format!("expected removed or off, got {word:?}"))?;
        match &state.power.candidates {
            None => Err("the supported power-off options are not known yet".into()),
            Some(c) if !c.contains(&e) => Err(format!("the headset does not offer {word:?} (candidates {c:02X?})")),
            Some(_) => Ok((Table::T1, vec![0x28, 0x05, e, 0x00])),
        }
    }
}

impl PowerOffFeature {
    fn reread(&self) -> Get {
        Get { table: Table::T1, payload: vec![0x26, 0x05], reply_id: 0x27 }
    }
}

pub struct AutoPauseFeature;

impl Feature for AutoPauseFeature {
    fn name(&self) -> &'static str {
        "autopause"
    }

    fn required(&self) -> &'static [(Table, u8)] {
        &[(Table::T1, 0xF1)]
    }

    fn initial_gets(&self, _: &DeviceInfo) -> Vec<Get> {
        vec![self.reread()]
    }

    fn on_frame(&self, state: &mut DeviceState, table: Table, payload: &[u8]) -> Parsed {
        let (Table::T1, [0xF7 | 0xF9, 0x01, a @ ..]) = (table, payload) else { return Parsed::NotMine };
        // loose: extra bytes are ignored, an unknown value forgets the field
        let v = a.first().and_then(|&b| (b <= 1).then_some(b == 0));
        if state.autopause == v {
            return Parsed::Unchanged;
        }
        state.autopause = v;
        Parsed::Changed
    }

    fn after_set(&self) -> Vec<Get> {
        vec![self.reread()]
    }

    /// `on` or `off`.
    fn encode_set(&self, _: &DeviceState, value: &str) -> Result<(Table, Vec<u8>), String> {
        match value.trim() {
            "on" => Ok((Table::T1, vec![0xF8, 0x01, 0])),
            "off" => Ok((Table::T1, vec![0xF8, 0x01, 1])),
            other => Err(format!("expected on or off, got {other:?}")),
        }
    }
}

impl AutoPauseFeature {
    fn reread(&self) -> Get {
        Get { table: Table::T1, payload: vec![0xF6, 0x01], reply_id: 0xF7 }
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

    static ONLY: &[&dyn Feature] = &[&PowerOffFeature, &AutoPauseFeature];
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

    fn with_candidates(c: &[u8]) -> DeviceState {
        DeviceState { power: Power { candidates: Some(c.to_vec()), current: None }, ..Default::default() }
    }

    #[test]
    fn fixture_frames_decode() {
        let mut s = DeviceState::default();
        // `21 05 02 10 11` and `27 05 10 00` from the capture
        assert_eq!(PowerOffFeature.on_frame(&mut s, Table::T1, &[0x21, 0x05, 2, 0x10, 0x11]), Parsed::Changed);
        assert_eq!(PowerOffFeature.on_frame(&mut s, Table::T1, &[0x27, 0x05, 0x10, 0x00]), Parsed::Changed);
        assert_eq!(s.power, Power { candidates: Some(vec![0x10, 0x11]), current: Some(0x10) });
        assert_eq!(PowerOffFeature.on_frame(&mut s, Table::T1, &[0x27, 0x05, 0x10, 0x00]), Parsed::Unchanged);
        assert_eq!(PowerOffFeature.on_frame(&mut s, Table::T1, &[0x29, 0x05, 0x11, 0x00, 9]), Parsed::Changed);
        assert_eq!(s.power.current, Some(0x11));
        // `F7 01 00` = ON (inverted), NTFY `F9 01 01` = OFF
        assert_eq!(AutoPauseFeature.on_frame(&mut s, Table::T1, &[0xF7, 0x01, 0]), Parsed::Changed);
        assert_eq!(s.autopause, Some(true));
        assert_eq!(AutoPauseFeature.on_frame(&mut s, Table::T1, &[0xF9, 0x01, 1]), Parsed::Changed);
        assert_eq!(s.autopause, Some(false));
    }

    #[test]
    fn loose_device_frames_and_foreign_frames() {
        let mut s = with_candidates(&[0x10, 0x11]);
        s.power.current = Some(0x10);
        s.autopause = Some(true);
        // a count larger than the bytes present keeps what is there; a short frame forgets the field
        assert_eq!(PowerOffFeature.on_frame(&mut s, Table::T1, &[0x21, 0x05, 5, 0x11]), Parsed::Changed);
        assert_eq!(s.power.candidates, Some(vec![0x11]));
        assert_eq!(PowerOffFeature.on_frame(&mut s, Table::T1, &[0x21, 0x05]), Parsed::Changed);
        assert_eq!(PowerOffFeature.on_frame(&mut s, Table::T1, &[0x27, 0x05]), Parsed::Changed);
        assert_eq!(s.power, Power::default());
        assert_eq!(AutoPauseFeature.on_frame(&mut s, Table::T1, &[0xF7, 0x01, 2]), Parsed::Changed);
        assert_eq!(s.autopause, None);
        assert_eq!(AutoPauseFeature.on_frame(&mut s, Table::T1, &[0xF7, 0x01]), Parsed::Unchanged);
        let before = s.clone();
        for (t, p) in [
            (Table::T1, &[0x27, 0x04, 0x10][..]), // the minute timer is not ours
            (Table::T1, &[0x26, 0x05]),
            (Table::T2, &[0x27, 0x05, 0x10, 0]),
            (Table::T1, &[0xF7, 0x0C, 0, 1]),
            (Table::T1, &[0xF8, 0x01, 0]),
        ] {
            assert_eq!(PowerOffFeature.on_frame(&mut s, t, p), Parsed::NotMine, "{p:02X?}");
            assert_eq!(AutoPauseFeature.on_frame(&mut s, t, p), Parsed::NotMine, "{p:02X?}");
        }
        assert_eq!(s, before);
    }

    #[test]
    fn set_bytes_and_candidate_filter() {
        let both = with_candidates(&[0x10, 0x11]);
        assert_eq!(PowerOffFeature.encode_set(&both, "removed"), Ok((Table::T1, vec![0x28, 0x05, 0x10, 0x00])));
        assert_eq!(PowerOffFeature.encode_set(&both, " off "), Ok((Table::T1, vec![0x28, 0x05, 0x11, 0x00])));
        // only what the capability lists
        assert!(PowerOffFeature.encode_set(&with_candidates(&[0x11]), "removed").is_err());
        assert!(PowerOffFeature.encode_set(&with_candidates(&[]), "off").is_err());
        assert!(PowerOffFeature.encode_set(&DeviceState::default(), "off").is_err());
        for bad in ["", "5", "00", "never", "ON", "removed off"] {
            assert!(PowerOffFeature.encode_set(&both, bad).is_err(), "{bad:?}");
        }
        let s = DeviceState::default();
        assert_eq!(AutoPauseFeature.encode_set(&s, "on"), Ok((Table::T1, vec![0xF8, 0x01, 0])));
        assert_eq!(AutoPauseFeature.encode_set(&s, "off"), Ok((Table::T1, vec![0xF8, 0x01, 1])));
        for bad in ["", "1", "ON", "on off", "toggle"] {
            assert!(AutoPauseFeature.encode_set(&s, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn guard_accepts_the_sets_and_only_those() {
        for p in [&[0x28, 0x05, 0x10, 0x00][..], &[0x28, 0x05, 0x11, 0x00], &[0xF8, 0x01, 0], &[0xF8, 0x01, 1]] {
            assert!(check(Table::T1, p, Mode::Normal).is_ok(), "{p:02X?}");
            assert_eq!(check(Table::T1, p, Mode::ReadOnly).unwrap_err(), GuardError::ReadOnly);
        }
        for p in [&[0x20, 0x05][..], &[0x26, 0x05], &[0xF6, 0x01]] {
            assert!(check(Table::T1, p, Mode::ReadOnly).is_ok());
        }
        // every 28 05 <a> <b>: only <10|11> 00
        for a in 0..=255u8 {
            for b in 0..=255u8 {
                let ok = check(Table::T1, &[0x28, 0x05, a, b], Mode::Normal).is_ok();
                assert_eq!(ok, matches!((a, b), (0x10 | 0x11, 0)), "28 05 {a:02X} {b:02X}");
            }
        }
        for p in
            [&[0x28, 0x05][..], &[0x28, 0x05, 0x10], &[0x28, 0x05, 0x10, 0, 0], &[0x28, 0x04, 0x11, 0x00], &[0x28, 0x04, 0x10, 0x00], &[0x28, 0x05, 0x01, 0x00]]
        {
            assert_eq!(check(Table::T1, p, Mode::Normal).unwrap_err(), GuardError::NotAllowed, "{p:02X?}");
        }
        // auto pause: exactly one 00|01 byte
        for p in [&[0xF8, 0x01][..], &[0xF8, 0x01, 2], &[0xF8, 0x01, 0xFF], &[0xF8, 0x01, 0, 0], &[0xF8, 0x01, 1, 1]] {
            assert_eq!(check(Table::T1, p, Mode::Normal).unwrap_err(), GuardError::NotAllowed, "{p:02X?}");
        }
        // head gesture and power-off-now are never allowed
        for p in [&[0xF8, 0x0F, 0][..], &[0xF8, 0x0F, 1], &[0xF6, 0x0F]] {
            assert_eq!(check(Table::T1, p, Mode::Normal).unwrap_err(), GuardError::NotAllowed, "{p:02X?}");
        }
        for b in 0..=255u8 {
            for c in 0..=255u8 {
                assert!(check(Table::T1, &[0x24, b, c], Mode::Normal).is_err(), "24 {b:02X} {c:02X}");
            }
        }
        for p in [&[0x24, 0x03, 0xFF][..], &[0x24, 0x07], &[0x24, 0x07, 0x00]] {
            assert_eq!(check(Table::T1, p, Mode::Normal).unwrap_err(), GuardError::Denied, "{p:02X?}");
        }
        assert_eq!(check(Table::T1, &[0x24, 0x03, 0x01], Mode::Normal).unwrap_err(), GuardError::NotAllowed);
    }

    #[test]
    fn gating_and_sync_on_the_capture() {
        let (_h, rx) = start(FakeHeadset::from_xm6_capture());
        let Event::Connected(info) = next(&rx) else { panic!() };
        assert!(info.has_t1(0x25) && info.has_t1(0xF1) && !info.has_t1(0x24));
        assert!(FEATURES.iter().any(|f| f.name() == "poweroff") && FEATURES.iter().any(|f| f.name() == "autopause"));
        let s = synced(&rx);
        assert_eq!(s.power, Power { candidates: Some(vec![0x10, 0x11]), current: Some(0x10) });
        assert_eq!(s.autopause, Some(true));
    }

    #[test]
    fn not_advertised_sends_nothing() {
        let h = FakeHeadset::xm6(&[0x20], &[], true); // no 0x25, no 0xF1
        let writes = h.writes.clone();
        let (handle, rx) = start(h);
        let s = synced(&rx);
        assert_eq!((s.power, s.autopause), (Power::default(), None));
        let before = writes.lock().unwrap().len();
        for (feature, value) in [("poweroff", "off"), ("autopause", "on")] {
            handle.send(Command::Set { feature: feature.into(), value: value.into() }).unwrap();
            assert!(matches!(next(&rx), Event::Error(CommandError::NotSupported(_))));
        }
        assert_eq!(writes.lock().unwrap().len(), before);
    }

    #[test]
    fn sets_go_out_and_the_ntfy_updates_state() {
        let mut h = FakeHeadset::from_xm6_capture();
        h.no_reply.push(vec![0x28, 0x05, 0x11, 0x00]);
        h.ntfy_on = Some((vec![0x28, 0x05, 0x11, 0x00], vec![0x29, 0x05, 0x11, 0x00]));
        let writes = h.writes.clone();
        let (handle, rx) = start(h);
        synced(&rx);
        handle.send(Command::Set { feature: "poweroff".into(), value: "off".into() }).unwrap();
        loop {
            if let Event::StateChanged { state, .. } = next(&rx) {
                assert_eq!(state.power.current, Some(0x11));
                break;
            }
        }
        // a candidate-less request never leaves the process
        handle.send(Command::Set { feature: "poweroff".into(), value: "5min".into() }).unwrap();
        loop {
            if matches!(next(&rx), Event::Error(CommandError::BadValue(_))) {
                break;
            }
        }
        let sent: Vec<_> = writes.lock().unwrap().iter().filter_map(|b| unpack(b).ok()).filter(|f| f.data_type != TYPE_ACK).map(|f| f.payload).collect();
        assert!(sent.contains(&vec![0x28, 0x05, 0x11, 0x00]));
        assert!(!sent.iter().any(|p| p[0] == 0x24));
    }
}

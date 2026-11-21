//! Equalizer (T1 function 0x57 PRESET_EQ_AND_ERRORCODE): 10 bands of -6..=+6 dB plus presets. The XM6 has no Clear Bass slot (the param has exactly 10 values).
//! Reads: capability `50 04 01` -> `51 04`, band frequencies `5A 00` -> `5B 00`, parameter `56 00` -> `57 00`; NTFY `59 00` carries the parameter, SET is `58 00`.
//! EQ status `52 00` is never sent: the XM6 ACKs it but does not answer.
//! Device frames are parsed loosely (an unusable one makes the stored value `None`, so no SET is built from it); only outgoing SETs are validated strictly.

use super::feature::{Feature, Get, Parsed};
use super::handshake::DeviceInfo;
use super::state::DeviceState;
use crate::mdr::guard::{EQ_PRESETS, Table, eq_set_args};

const BANDS: usize = 10;
/// raw step = dB + OFFSET, with the 13 steps the XM6 reports.
const OFFSET: i16 = 6;
const STEPS: u8 = 13;
const CUSTOM: u8 = 0xA0;

/// The current preset and band values in dB. Loose: out-of-range device values are kept and refused by `encode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eq {
    pub preset: u8,
    pub bands: Vec<i16>,
}

/// The capability reply: band count, step count and the presets the headset lists (id, name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cap {
    pub bands: u8,
    pub steps: u8,
    pub presets: Vec<(u8, String)>,
}

/// `<preset> <count> <raw steps...>`; only the 10-value layout is accepted: a 6-value (Clear Bass) layout would put values on the wrong bands.
fn parse_eq(args: &[u8]) -> Result<Eq, String> {
    match *args {
        [preset, 10, ref rest @ ..] if rest.len() >= BANDS => Ok(Eq { preset, bands: rest[..BANDS].iter().map(|&r| r as i16 - OFFSET).collect() }),
        [_, count, ..] => Err(format!("EQ parameter has {count} values, only the 10-band layout is supported: {args:02X?}")),
        _ => Err(format!("EQ parameter is too short: {args:02X?}")),
    }
}

/// `<band> <step> <count> {<id> <len> <name>}...`
fn parse_cap(args: &[u8]) -> Result<Cap, String> {
    let [bands, steps, count, rest @ ..] = args else {
        return Err(format!("EQ capability is too short: {args:02X?}"));
    };
    let (mut rest, mut presets) = (rest, Vec::new());
    for _ in 0..*count {
        let [id, len, tail @ ..] = rest else { return Err(format!("EQ capability is cut off: {args:02X?}")) };
        let (name, tail) = tail.split_at_checked(*len as usize).ok_or_else(|| format!("EQ capability is cut off: {args:02X?}"))?;
        presets.push((*id, String::from_utf8_lossy(name).into_owned()));
        rest = tail;
    }
    Ok(Cap { bands: *bands, steps: *steps, presets })
}

/// `<count> {<type> <freq u16 BE>}...`; the type byte (HZ on the XM6) is not kept.
fn parse_freqs(args: &[u8]) -> Result<Vec<u16>, String> {
    let [count, rest @ ..] = args else { return Err("EQ band info is empty".into()) };
    if rest.len() < *count as usize * 3 {
        return Err(format!("EQ band info is cut off: {args:02X?}"));
    }
    Ok(rest.as_chunks::<3>().0.iter().take(*count as usize).map(|c| u16::from_be_bytes([c[1], c[2]])).collect())
}

/// The SET payload: `58 00 <preset> 00` (preset only) or `58 00 <preset> 0A <10 raw steps>`. Refuses presets the XM6 does not accept and bands outside -6..=+6.
pub fn encode(preset: u8, bands: Option<&[i16]>) -> Result<Vec<u8>, String> {
    if !EQ_PRESETS.contains(&preset) {
        return Err(format!("preset {preset:#04X} is not accepted by the XM6"));
    }
    let mut p = vec![0x58, 0x00, preset];
    match bands {
        None => p.push(0),
        Some(b) if b.len() == BANDS => {
            p.push(BANDS as u8);
            for &v in b {
                if !(-OFFSET..=OFFSET).contains(&v) {
                    return Err(format!("band value {v} is outside -6..6"));
                }
                p.push((v + OFFSET) as u8);
            }
        }
        Some(b) => return Err(format!("expected 10 band values, got {}", b.len())),
    }
    debug_assert!(eq_set_args(&p[2..]));
    Ok(p)
}

/// Sets `slot` from a parse result; an unusable frame forgets the stored value so no SET is built from it.
fn store<T: PartialEq>(slot: &mut Option<T>, r: Result<T, String>) -> Parsed {
    let v = r.ok();
    if *slot == v {
        return Parsed::Unchanged;
    }
    *slot = v;
    Parsed::Changed
}

/// `<name or hex id>` to a preset id the headset lists.
fn preset_id(cap: &Cap, text: &str) -> Result<u8, String> {
    let id = cap
        .presets
        .iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(text))
        .map(|p| p.0)
        .or_else(|| u8::from_str_radix(text.trim_start_matches("0x"), 16).ok())
        .ok_or_else(|| format!("unknown preset {text:?}"))?;
    if cap.presets.iter().any(|p| p.0 == id) { Ok(id) } else { Err(format!("preset {id:#04X} is not offered by the headset")) }
}

pub struct EqFeature;

impl Feature for EqFeature {
    fn name(&self) -> &'static str {
        "eq"
    }

    fn required(&self) -> &'static [(Table, u8)] {
        &[(Table::T1, 0x57)]
    }

    fn initial_gets(&self, _: &DeviceInfo) -> Vec<Get> {
        vec![
            Get { table: Table::T1, payload: vec![0x50, 0x04, 0x01], reply_id: 0x51 },
            Get { table: Table::T1, payload: vec![0x5A, 0x00], reply_id: 0x5B },
            self.reread(),
        ]
    }

    fn on_frame(&self, state: &mut DeviceState, table: Table, payload: &[u8]) -> Parsed {
        match (table, payload) {
            (Table::T1, [0x51, 0x04, a @ ..]) => store(&mut state.eq_cap, parse_cap(a)),
            (Table::T1, [0x5B, 0x00, a @ ..]) => store(&mut state.eq_freqs, parse_freqs(a)),
            (Table::T1, [0x57 | 0x59, 0x00, a @ ..]) => store(&mut state.eq, parse_eq(a)),
            // status RET / NTFY: EnableDisable, DISABLE = 1
            (Table::T1, [0x53 | 0x55, 0x00, flag, ..]) => {
                let off = *flag == 1;
                if state.eq_disabled == off {
                    return Parsed::Unchanged;
                }
                state.eq_disabled = off;
                Parsed::Changed
            }
            _ => Parsed::NotMine,
        }
    }

    fn after_set(&self) -> Vec<Get> {
        vec![self.reread()]
    }

    /// `preset <name|hex id>` or `bands <10 ints>` (dB, -6..6; keeps a user slot, otherwise uses CUSTOM).
    fn encode_set(&self, state: &DeviceState, value: &str) -> Result<(Table, Vec<u8>), String> {
        if state.eq_disabled {
            return Err("the headset reports the equalizer as disabled".into());
        }
        let cap = state
            .eq_cap
            .as_ref()
            .filter(|c| c.bands as usize == BANDS && c.steps == STEPS)
            .ok_or("EQ capability unknown or not 10 bands x 13 steps: EQ is read-only")?;
        let eq = state.eq.as_ref().ok_or("EQ state is not known yet")?;
        let value = value.trim();
        let (cmd, rest) = value.split_once(char::is_whitespace).unwrap_or((value, ""));
        let p = match cmd {
            "preset" => encode(preset_id(cap, rest.trim())?, None)?,
            "bands" => {
                let bands = rest.split_whitespace().map(|v| v.parse::<i16>().map_err(|_| format!("{v:?} is not a number"))).collect::<Result<Vec<_>, _>>()?;
                let preset = if (CUSTOM..=0xA5).contains(&eq.preset) { eq.preset } else { CUSTOM };
                encode(preset_id(cap, &format!("{preset:X}"))?, Some(&bands))?
            }
            _ => return Err(format!("expected `preset <name>` or `bands <10 values>`, got {value:?}")),
        };
        Ok((Table::T1, p))
    }
}

impl EqFeature {
    fn reread(&self) -> Get {
        Get { table: Table::T1, payload: vec![0x56, 0x00], reply_id: 0x57 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::mock::FakeHeadset;
    use crate::device::{Command, DeviceHandle, Event, FEATURES};
    use crate::mdr::frame::{TYPE_ACK, TYPE_T1, unpack};
    use crate::mdr::guard::{GuardError, Mode as GMode, check};
    use crate::mdr::session::Timeouts;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    static ONLY: &[&dyn Feature] = &[&EqFeature];
    const FAST: Timeouts = Timeouts { ack: Duration::from_millis(15), response: Duration::from_millis(60), tries: 3 };
    /// the args of `57 00 ...` in the capture
    const FIXTURE: [u8; 12] = [0x32, 0x0A, 0x0A, 0x0A, 0x05, 0x05, 0x06, 0x06, 0x06, 0x08, 0x08, 0x09];
    const CURVE: [i16; 10] = [4, 4, -1, -1, 0, 0, 0, 2, 2, 3];

    /// `51 04` args: 10 bands, 13 steps, HEAVY CLEAR HARD SOFT CUSTOM and a listed but unsupported ROCK
    fn cap_args() -> Vec<u8> {
        let mut v = vec![10, 13, 6];
        for (id, n) in [(0x30, "HEAVY"), (0x31, "CLEAR"), (0x32, "HARD"), (0x33, "SOFT"), (0xA0, "CUSTOM"), (0x01, "ROCK")] {
            v.extend([id, n.len() as u8]);
            v.extend(n.bytes());
        }
        v
    }

    fn state() -> DeviceState {
        DeviceState { eq: Some(parse_eq(&FIXTURE).unwrap()), eq_cap: Some(parse_cap(&cap_args()).unwrap()), ..Default::default() }
    }

    fn headset() -> FakeHeadset {
        let mut h = FakeHeadset::from_xm6_capture();
        h.replies.push((TYPE_T1, vec![0x50, 0x04, 0x01], [&[0x51, 0x04][..], &cap_args()].concat()));
        h
    }

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
    fn fixture_param_and_clear_bass_layout() {
        let eq = parse_eq(&FIXTURE).unwrap();
        assert_eq!((eq.preset, eq.bands.as_slice()), (0x32, &CURVE[..]));
        // a 6-value (Clear Bass) param, a short one and a preset-only one are errors, never mapped
        assert!(parse_eq(&[0x32, 6, 10, 10, 5, 5, 6, 6]).unwrap_err().contains("6 values"));
        assert!(parse_eq(&FIXTURE[..9]).is_err());
        assert!(parse_eq(&[0x32, 0]).is_err());
        // out-of-range raw values are kept as they are
        assert_eq!(parse_eq(&[0xA0, 10, 0, 12, 13, 0, 0, 0, 0, 0, 0, 255]).unwrap().bands, [-6, 6, 7, -6, -6, -6, -6, -6, -6, 249]);
    }

    #[test]
    fn encode_steps_and_refusals() {
        assert_eq!(encode(0x32, None).unwrap(), [0x58, 0x00, 0x32, 0x00]);
        assert_eq!(encode(0x32, Some(&CURVE)).unwrap(), [0x58, 0x00, 0x32, 0x0A, 10, 10, 5, 5, 6, 6, 6, 8, 8, 9]);
        assert_eq!(encode(0xA0, Some(&[-6, 0, 6, 0, 0, 0, 0, 0, 0, 0])).unwrap()[4..7], [0, 6, 12]);
        for bad in [-7, 7] {
            assert!(encode(0xA0, Some(&[0, 0, 0, 0, 0, bad, 0, 0, 0, 0])).is_err());
        }
        assert!(encode(0xA0, Some(&[0; 9])).is_err());
        assert!(encode(0xA0, Some(&[0; 6])).is_err());
        for id in [0x00, 0x01, 0x10, 0x2F, 0x34, 0x9F, 0xA6, 0xFF] {
            assert!(encode(id, None).is_err(), "{id:#04X}");
        }
    }

    #[test]
    fn capability_and_band_info() {
        let cap = parse_cap(&cap_args()).unwrap();
        assert_eq!((cap.bands, cap.steps), (10, 13));
        assert_eq!(cap.presets[2], (0x32, "HARD".to_string()));
        assert_eq!(cap.presets.len(), 6);
        assert!(parse_cap(&cap_args()[..20]).is_err());
        assert!(parse_cap(&[10, 13]).is_err());
        assert_eq!(parse_freqs(&[2, 1, 0, 31, 1, 3, 0xE8]).unwrap(), [31, 1000]);
        assert!(parse_freqs(&[2, 1, 0, 31, 1, 3]).is_err());
    }

    #[test]
    fn frames_update_state() {
        let mut s = DeviceState::default();
        let f = EqFeature;
        let ret = [&[0x57, 0x00][..], &FIXTURE].concat();
        assert_eq!(f.on_frame(&mut s, Table::T1, &ret), Parsed::Changed);
        assert_eq!(f.on_frame(&mut s, Table::T1, &ret), Parsed::Unchanged);
        // NTFY 59: a preset change on the headset (SOFT)
        assert_eq!(f.on_frame(&mut s, Table::T1, &[&[0x59, 0x00, 0x33][..], &FIXTURE[1..]].concat()), Parsed::Changed);
        assert_eq!(s.eq.as_ref().map(|e| e.preset), Some(0x33));
        assert_eq!(f.on_frame(&mut s, Table::T1, &[&[0x51, 0x04][..], &cap_args()].concat()), Parsed::Changed);
        assert_eq!(f.on_frame(&mut s, Table::T2, &ret), Parsed::NotMine);
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0x5B, 0x02, 0x00, 0x03]), Parsed::NotMine);
        // EQ status: DISABLE stops SET
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0x55, 0x00, 0x01, 0x00]), Parsed::Changed);
        assert!(f.encode_set(&s, "preset HARD").is_err());
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0x55, 0x00, 0x00, 0x00]), Parsed::Changed);
        assert!(f.encode_set(&s, "preset HARD").is_ok());
        // an unusable frame (6-value layout) forgets the state, so SET refuses
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0x59, 0x00, 0x32, 6, 1, 2, 3, 4, 5, 6]), Parsed::Changed);
        assert_eq!(s.eq, None);
        assert!(f.encode_set(&s, "preset HARD").is_err());
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0x59, 0x00]), Parsed::Unchanged);
    }

    #[test]
    fn set_text() {
        let s = state();
        let enc = |t: &str| EqFeature.encode_set(&s, t).map(|r| r.1);
        assert_eq!(enc("preset soft"), Ok(vec![0x58, 0x00, 0x33, 0x00]));
        assert_eq!(enc("preset 0x31"), Ok(vec![0x58, 0x00, 0x31, 0x00]));
        // bands from a factory preset go out as CUSTOM; a user slot is kept
        assert_eq!(enc("bands 6 6 0 0 0 0 0 0 0 -6").unwrap()[2..4], [0xA0, 0x0A]);
        let mut slot = state();
        slot.eq.as_mut().unwrap().preset = 0xA2;
        slot.eq_cap.as_mut().unwrap().presets.push((0xA2, "USER2".into()));
        assert_eq!(EqFeature.encode_set(&slot, "bands 0 0 0 0 0 0 0 0 0 0").unwrap().1[2], 0xA2);
        // listed by the headset but not accepted by the XM6, unlisted, and malformed values
        for bad in [
            "preset ROCK",
            "preset 0x01",
            "preset 0xA1",
            "preset",
            "preset nope",
            "bands 0 0 0",
            "bands 7 0 0 0 0 0 0 0 0 0",
            "bands a 0 0 0 0 0 0 0 0 0",
            "",
            "loud",
        ] {
            assert!(enc(bad).is_err(), "{bad:?}");
        }
        // capability missing, other step count or other band count: read-only
        for mutate in [
            |s: &mut DeviceState| s.eq_cap = None,
            |s: &mut DeviceState| s.eq_cap.as_mut().unwrap().steps = 21,
            |s: &mut DeviceState| s.eq_cap.as_mut().unwrap().bands = 5,
        ] {
            let mut r = state();
            mutate(&mut r);
            assert!(EqFeature.encode_set(&r, "preset HARD").is_err());
        }
        assert!(EqFeature.encode_set(&DeviceState { eq_cap: s.eq_cap.clone(), ..Default::default() }, "preset HARD").is_err());
    }

    #[test]
    fn guard_accepts_valid_sets_only() {
        let ok = |p: &[u8]| check(Table::T1, p, GMode::Normal).map(|_| ());
        let full = encode(0xA0, Some(&CURVE)).unwrap();
        assert!(ok(&full).is_ok());
        assert!(ok(&encode(0x32, None).unwrap()).is_ok());
        assert_eq!(check(Table::T1, &full, GMode::ReadOnly).unwrap_err(), GuardError::ReadOnly);
        for g in [&[0x50, 0x04, 0x01][..], &[0x5A, 0x00], &[0x56, 0x00]] {
            assert!(check(Table::T1, g, GMode::ReadOnly).is_ok());
        }
        assert_eq!(ok(&[0x50, 0x04, 0x02]), Err(GuardError::NotAllowed));
        let mut p = full.clone();
        p[3] = 9; // wrong count
        assert_eq!(ok(&p), Err(GuardError::NotAllowed));
        p = full.clone();
        p[9] = 13; // raw step above 12
        assert_eq!(ok(&p), Err(GuardError::NotAllowed));
        p = full.clone();
        p[2] = 0x01; // ROCK
        assert_eq!(ok(&p), Err(GuardError::NotAllowed));
        assert_eq!(ok(&full[..13]), Err(GuardError::NotAllowed));
        assert_eq!(ok(&[&full[..], &[6]].concat()), Err(GuardError::NotAllowed));
        for p in [&[0x58, 0x00][..], &[0x58, 0x00, 0x32], &[0x58, 0x00, 0x32, 0x00, 0x00], &[0x58, 0x00, 0x32, 0x06, 1, 2, 3, 4, 5, 6]] {
            assert_eq!(ok(p), Err(GuardError::NotAllowed), "{p:02X?}");
        }
    }

    #[test]
    fn sync_on_the_capture_gating_and_no_status_read() {
        let h = headset();
        let writes = h.writes.clone();
        let (_h, rx) = start(h);
        let Event::Connected(info) = next(&rx) else { panic!() };
        assert!(crate::device::feature::supported(&EqFeature, &info));
        assert!(FEATURES.iter().any(|f| f.name() == "eq"));
        let mut none = info.clone();
        none.t1_functions.retain(|&(f, _)| f != 0x57);
        assert!(!crate::device::feature::supported(&EqFeature, &none));
        let s = synced(&rx);
        // the real replies from the capture, including the escaped `3D 2E 80` frame
        assert_eq!(s.eq_freqs, Some(vec![31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]));
        assert_eq!(s.eq.map(|e| (e.preset, e.bands)), Some((0x32, CURVE.to_vec())));
        assert_eq!(s.eq_cap.map(|c| c.steps), Some(13));
        let sent: Vec<_> = writes.lock().unwrap().iter().filter_map(|b| unpack(b).ok()).filter(|f| f.data_type != TYPE_ACK).map(|f| f.payload).collect();
        assert!(sent.contains(&vec![0x50, 0x04, 0x01]));
        assert!(!sent.iter().any(|p| p.starts_with(&[0x52])));
    }

    #[test]
    fn the_capture_without_a_capability_reply_stays_read_only() {
        let mut h = FakeHeadset::from_xm6_capture();
        h.no_reply.push(vec![0x50, 0x04, 0x01]);
        let (handle, rx) = start(h);
        let s = synced(&rx);
        assert!(s.eq.is_some() && s.eq_cap.is_none());
        handle.send(Command::Set { feature: "eq".into(), value: "preset HARD".into() }).unwrap();
        loop {
            if let Event::Error(_) = next(&rx) {
                break;
            }
        }
    }

    #[test]
    fn set_is_followed_by_a_reread_and_ntfy_updates_state() {
        let mut h = headset();
        let set = vec![0x58, 0x00, 0x33, 0x00];
        h.no_reply.push(set.clone());
        h.ntfy_on = Some((set.clone(), [&[0x59, 0x00, 0x33][..], &FIXTURE[1..]].concat()));
        let writes = h.writes.clone();
        let (handle, rx) = start(h);
        synced(&rx);
        handle.send(Command::Set { feature: "eq".into(), value: "preset SOFT".into() }).unwrap();
        loop {
            if let Event::StateChanged { state, .. } = next(&rx) {
                assert_eq!(state.eq.map(|e| e.preset), Some(0x33));
                break;
            }
        }
        // the re-read (the capture still answers 0x32, which wins as the last word)
        loop {
            if let Event::StateChanged { state, .. } = next(&rx) {
                assert_eq!(state.eq.map(|e| e.preset), Some(0x32));
                break;
            }
        }
        let sent: Vec<_> = writes.lock().unwrap().iter().filter_map(|b| unpack(b).ok()).filter(|f| f.data_type != TYPE_ACK).map(|f| f.payload).collect();
        let at = sent.iter().position(|p| *p == set).expect("SET sent");
        assert!(sent[at + 1..].contains(&vec![0x56, 0x00]));
    }
}

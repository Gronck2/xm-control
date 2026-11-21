//! Noise cancelling / ambient sound (T1 function 0x6D, the only variant the XM6 advertises): GET `66 19`, RET `67 19`, NTFY `69 19`, SET `68 19`, all with
//! `<valueChange> <totalEffect> <ncAsmMode> <ambientMode> <ambLevel> <autoAsm> <noiseAdaptiveSensitivity>` after the two id bytes.
//! A SET always writes the whole tuple, built from the current state, so unrelated fields are never reset.

use super::feature::{Feature, Get, Parsed};
use super::handshake::DeviceInfo;
use super::state::DeviceState;
use crate::mdr::guard::Table;
use std::time::{Duration, Instant};

const RET: u8 = 0x67;
const NTFY: u8 = 0x69;
const SET: u8 = 0x68;
const TYPE: u8 = 0x19;
const UNDER_CHANGING: u8 = 0;
const CHANGED: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Nc,
    Ambient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NcAsm {
    /// totalEffect: false = everything off.
    pub effect: bool,
    /// ncAsmMode: false = noise cancelling, true = ambient (ASM). Kept while the effect is off, so Off round-trips.
    pub asm: bool,
    /// ambientMode VOICE: focus on voice.
    pub voice: bool,
    /// 1..=20; the remembered ambient level, also while in NC mode.
    pub level: u8,
    /// autoAsm (not inverted: 1 = on).
    pub auto: bool,
    /// 0 standard, 1 high, 2 low.
    pub sens: u8,
}

impl NcAsm {
    #[allow(dead_code)] // used by the UI, which does not exist yet
    pub fn mode(&self) -> Mode {
        match (self.effect, self.asm) {
            (false, _) => Mode::Off,
            (true, false) => Mode::Nc,
            (true, true) => Mode::Ambient,
        }
    }

    pub fn set_mode(&mut self, m: Mode) {
        match m {
            Mode::Off => self.effect = false,
            Mode::Nc => (self.effect, self.asm) = (true, false),
            Mode::Ambient => (self.effect, self.asm) = (true, true),
        }
    }

    /// The full SET payload `68 19 <7 bytes>`. `changed` false = UNDER_CHANGING (slider still moving).
    pub fn encode(&self, changed: bool) -> Vec<u8> {
        let b = |v: bool| v as u8;
        vec![SET, TYPE, if changed { CHANGED } else { UNDER_CHANGING }, b(self.effect), b(self.asm), b(self.voice), self.level, b(self.auto), self.sens]
    }
}

/// The 7 bytes after the two id bytes of a RET/NTFY (valueChange is not kept). Anything outside its enum is an error, not clamped.
fn parse(args: &[u8]) -> Result<NcAsm, String> {
    if !crate::mdr::guard::nc_set_args(args) {
        return Err(format!("NC/ASM payload is not 7 in-range value bytes: {args:02X?}"));
    }
    Ok(NcAsm { effect: args[1] == 1, asm: args[2] == 1, voice: args[3] == 1, level: args[4], auto: args[5] == 1, sens: args[6] })
}

fn on_off(v: &str) -> Result<bool, String> {
    match v {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("expected on or off, got {v:?}")),
    }
}

/// `<off|nc|ambient> [--level N] [--voice on|off] [--auto on|off] [--sens standard|high|low]`, applied to `v`.
fn apply(mut v: NcAsm, text: &str) -> Result<NcAsm, String> {
    let mut t = text.split_whitespace();
    v.set_mode(match t.next() {
        Some("off") => Mode::Off,
        Some("nc") => Mode::Nc,
        Some("ambient") => Mode::Ambient,
        other => return Err(format!("expected off, nc or ambient, got {other:?}")),
    });
    while let Some(flag) = t.next() {
        let val = t.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag {
            "--level" => v.level = val.parse().ok().filter(|l| (1..=20).contains(l)).ok_or("level must be 1..20")?,
            "--voice" => v.voice = on_off(val)?,
            "--auto" => v.auto = on_off(val)?,
            "--sens" => v.sens = ["standard", "high", "low"].iter().position(|s| *s == val).ok_or("sens must be standard, high or low")? as u8,
            _ => return Err(format!("unknown option {flag:?}")),
        }
    }
    Ok(v)
}

pub struct NcAsmFeature;

impl Feature for NcAsmFeature {
    fn name(&self) -> &'static str {
        "nc"
    }

    fn required(&self) -> &'static [(Table, u8)] {
        &[(Table::T1, 0x6D)]
    }

    fn initial_gets(&self, _: &DeviceInfo) -> Vec<Get> {
        vec![Get { table: Table::T1, payload: vec![0x66, TYPE], reply_id: RET }]
    }

    fn on_frame(&self, state: &mut DeviceState, table: Table, payload: &[u8]) -> Parsed {
        let (Table::T1, [RET | NTFY, TYPE, args @ ..]) = (table, payload) else { return Parsed::NotMine };
        // a malformed frame is reported as unhandled
        let Ok(v) = parse(args) else { return Parsed::NotMine };
        if state.nc == Some(v) {
            return Parsed::Unchanged;
        }
        state.nc = Some(v);
        Parsed::Changed
    }

    fn encode_set(&self, state: &DeviceState, value: &str) -> Result<(Table, Vec<u8>), String> {
        let base = state.nc.ok_or("noise cancelling state is not known yet")?;
        Ok((Table::T1, apply(base, value)?.encode(true)))
    }
}

/// Minimum gap between UNDER_CHANGING updates while a slider is dragged (about 10 per second).
const DRAG_GAP: Duration = Duration::from_millis(100);

/// Rate limit for a slider drag: the UI calls `update` on every move with the latest value and `release` when the user lets go.
#[allow(dead_code)] // used by the UI, which does not exist yet
#[derive(Default)]
pub struct Drag {
    last: Option<Instant>,
}

#[allow(dead_code)] // used by the UI, which does not exist yet
impl Drag {
    /// The UNDER_CHANGING SET to send now, or `None` if the previous one is too recent (the latest value goes out later or with `release`).
    pub fn update(&mut self, now: Instant, v: &NcAsm) -> Option<Vec<u8>> {
        if self.last.is_some_and(|l| now.duration_since(l) < DRAG_GAP) {
            return None;
        }
        self.last = Some(now);
        Some(v.encode(false))
    }

    /// The final CHANGED SET with the latest value; always sent.
    pub fn release(&mut self, v: &NcAsm) -> Vec<u8> {
        self.last = None;
        v.encode(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::mock::FakeHeadset;
    use crate::device::{Command, DeviceHandle, Event, FEATURES};
    use crate::mdr::frame::{TYPE_ACK, unpack};
    use crate::mdr::guard::{GuardError, Mode as GMode, check};
    use crate::mdr::session::Timeouts;
    use std::sync::mpsc::Receiver;

    static ONLY: &[&dyn Feature] = &[&NcAsmFeature];
    const FAST: Timeouts = Timeouts { ack: Duration::from_millis(15), response: Duration::from_millis(60), tries: 3 };
    const FIXTURE: [u8; 7] = [0x01, 0x01, 0x00, 0x00, 0x14, 0x00, 0x00]; // from `67 19 ...` in the capture
    const NC20: NcAsm = NcAsm { effect: true, asm: false, voice: false, level: 20, auto: false, sens: 0 };

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
    fn parses_the_fixture_and_round_trips() {
        assert_eq!(parse(&FIXTURE), Ok(NC20));
        assert_eq!(NC20.mode(), Mode::Nc);
        assert_eq!(NC20.encode(true), [0x68, 0x19, 1, 1, 0, 0, 20, 0, 0]);
        assert_eq!(NC20.encode(false)[2], 0);
        // every field at a non-default value survives encode then parse
        let v = NcAsm { effect: true, asm: true, voice: true, level: 7, auto: true, sens: 2 };
        assert_eq!(parse(&v.encode(true)[2..]), Ok(v));
        assert_eq!(v.encode(true), [0x68, 0x19, 1, 1, 1, 1, 7, 1, 2]);
    }

    #[test]
    fn parse_rejects_bad_values() {
        assert!(parse(&FIXTURE[..6]).is_err());
        assert!(parse(&[1, 1, 0, 0, 20, 0, 0, 0]).is_err());
        for (i, bad) in [(0, 2), (1, 2), (2, 2), (3, 2), (4, 0), (4, 21), (5, 2), (6, 3)] {
            let mut a = FIXTURE;
            a[i] = bad;
            assert!(parse(&a).is_err(), "byte {i} = {bad}");
        }
    }

    #[test]
    fn mode_mapping_both_ways() {
        let mut v = NC20;
        for (m, effect, asm) in [(Mode::Ambient, true, true), (Mode::Nc, true, false)] {
            v.set_mode(m);
            assert_eq!((v.effect, v.asm, v.mode()), (effect, asm, m));
        }
        v.set_mode(Mode::Off);
        assert_eq!((v.effect, v.mode()), (false, Mode::Off));
        // Off keeps the remembered NC/ambient choice
        v.set_mode(Mode::Ambient);
        v.set_mode(Mode::Off);
        assert!(v.asm);
    }

    #[test]
    fn set_text_is_read_modify_write() {
        let state = DeviceState { nc: Some(NC20), ..Default::default() };
        let enc = |t: &str| NcAsmFeature.encode_set(&state, t);
        assert_eq!(enc("nc"), Ok((Table::T1, vec![0x68, 0x19, 1, 1, 0, 0, 20, 0, 0])));
        assert_eq!(enc("off").unwrap().1[3..5], [0, 0]);
        // only the named fields change; level, voice, auto and sens come from the state
        assert_eq!(enc("ambient --level 5").unwrap().1, [0x68, 0x19, 1, 1, 1, 0, 5, 0, 0]);
        assert_eq!(enc("ambient --voice on --auto on --sens low").unwrap().1, [0x68, 0x19, 1, 1, 1, 1, 20, 1, 2]);
        for bad in ["", "loud", "nc --level 0", "nc --level 21", "nc --level x", "nc --level", "nc --voice maybe", "nc --sens medium", "nc --what 1"] {
            assert!(enc(bad).is_err(), "{bad:?}");
        }
        assert!(NcAsmFeature.encode_set(&DeviceState::default(), "nc").is_err());
    }

    #[test]
    fn frames_update_state() {
        let mut s = DeviceState::default();
        let f = NcAsmFeature;
        let ret = [&[0x67, 0x19][..], &FIXTURE].concat();
        assert_eq!(f.on_frame(&mut s, Table::T1, &ret), Parsed::Changed);
        assert_eq!(s.nc, Some(NC20));
        assert_eq!(f.on_frame(&mut s, Table::T1, &ret), Parsed::Unchanged);
        // a button press arrives as NTFY 69: ambient, voice on, level 3
        assert_eq!(f.on_frame(&mut s, Table::T1, &[0x69, 0x19, 1, 1, 1, 1, 3, 0, 0]), Parsed::Changed);
        assert_eq!(s.nc, Some(NcAsm { effect: true, asm: true, voice: true, level: 3, auto: false, sens: 0 }));
        let before = s.clone();
        for (t, p) in [
            (Table::T1, &[0x67, 0x17, 1, 1, 0, 0, 20, 0, 0][..]),
            (Table::T1, &[0x69, 0x22, 1, 1, 0, 0, 20, 0, 0]),
            (Table::T2, &[0x67, 0x19, 1, 1, 0, 0, 20, 0, 0]),
            (Table::T1, &[0x67, 0x19, 1, 1, 0, 0, 21, 0, 0]),
        ] {
            assert_eq!(f.on_frame(&mut s, t, p), Parsed::NotMine);
        }
        assert_eq!(s, before);
    }

    #[test]
    fn guard_accepts_valid_sets_only() {
        let ok = |p: &[u8]| check(Table::T1, p, GMode::Normal).map(|_| ());
        assert!(ok(&NC20.encode(true)).is_ok());
        assert!(ok(&NC20.encode(false)).is_ok());
        assert_eq!(check(Table::T1, &NC20.encode(true), GMode::ReadOnly).unwrap_err(), GuardError::ReadOnly);
        assert!(check(Table::T1, &[0x66, 0x19], GMode::ReadOnly).is_ok());
        let good = NC20.encode(true);
        assert_eq!(ok(&good[..8]), Err(GuardError::NotAllowed));
        assert_eq!(ok(&[&good[..], &[0]].concat()), Err(GuardError::NotAllowed));
        for (i, bad) in [(2, 2), (3, 2), (4, 2), (5, 2), (6, 0), (6, 21), (7, 2), (8, 3)] {
            let mut p = good.clone();
            p[i] = bad;
            assert_eq!(ok(&p), Err(GuardError::NotAllowed), "byte {i} = {bad}");
        }
        // the dropped variants and the test-mode command stay out
        for p in [&[0x68, 0x17, 1, 1, 0, 0, 20, 0, 0][..], &[0x68, 0x22, 1, 1, 0, 0, 20, 0, 0], &[0x66, 0x17], &[0x66, 0x22]] {
            assert_eq!(ok(p), Err(GuardError::NotAllowed), "{p:02X?}");
        }
        assert_eq!(ok(&[0x64, 0x01]), Err(GuardError::Denied));
    }

    #[test]
    fn gating_and_sync_on_the_real_support_list() {
        let (_h, rx) = start(FakeHeadset::from_xm6_capture());
        let Event::Connected(info) = next(&rx) else { panic!() };
        assert!(info.has_t1(0x6D) && !info.has_t1(0x6B) && !info.has_t1(0x67));
        assert!(crate::device::feature::supported(&NcAsmFeature, &info));
        assert!(FEATURES.iter().any(|f| f.name() == "nc"));
        let mut none = info.clone();
        none.t1_functions.retain(|&(f, _)| f != 0x6D);
        assert!(!crate::device::feature::supported(&NcAsmFeature, &none));
        assert_eq!(synced(&rx).nc, Some(NC20));
    }

    #[test]
    fn set_goes_out_whole_and_the_ntfy_updates_state() {
        let mut h = FakeHeadset::from_xm6_capture();
        let set = NcAsm { effect: true, asm: true, voice: false, level: 20, auto: false, sens: 0 }.encode(true);
        h.no_reply.push(set.clone());
        h.ntfy_on = Some((set.clone(), vec![0x69, 0x19, 1, 1, 1, 0, 20, 0, 0]));
        let writes = h.writes.clone();
        let (handle, rx) = start(h);
        synced(&rx);
        handle.send(Command::Set { feature: "nc".into(), value: "ambient".into() }).unwrap();
        loop {
            if let Event::StateChanged { state, .. } = next(&rx) {
                assert_eq!(state.nc.map(|v| v.mode()), Some(Mode::Ambient));
                break;
            }
        }
        assert!(writes.lock().unwrap().iter().filter_map(|b| unpack(b).ok()).any(|f| f.data_type != TYPE_ACK && f.payload == set));
    }

    #[test]
    fn drag_is_rate_limited_and_ends_with_changed() {
        let t0 = Instant::now();
        let mut d = Drag::default();
        let mut v = NC20;
        assert_eq!(d.update(t0, &v).unwrap()[2], 0);
        v.level = 10;
        assert!(d.update(t0 + Duration::from_millis(40), &v).is_none());
        assert!(d.update(t0 + Duration::from_millis(99), &v).is_none());
        assert_eq!(d.update(t0 + Duration::from_millis(100), &v).unwrap()[6], 10);
        v.level = 11;
        assert!(d.update(t0 + Duration::from_millis(120), &v).is_none());
        // release always sends the latest value as CHANGED, even right after an update
        assert_eq!(d.release(&v), [0x68, 0x19, 1, 1, 0, 0, 11, 0, 0]);
        assert!(d.update(t0 + Duration::from_millis(130), &v).is_some());
    }
}

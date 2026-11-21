//! V2 init handshake (docs/arch/protocol.md section 3, steps 1-7): protocol info, capability, firmware, model, series/color and the support-function lists.
//! Every command here is a read-only GET and goes through the guard in `Mode::ReadOnly`.

use crate::mdr::guard::{Mode, Table, check};
use crate::mdr::session::{Session, SessionError, Transport};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub protocol_version: u32,
    pub table2_enabled: bool,
    pub unique_id: String,
    pub firmware: String,
    pub model_name: String,
    pub series: u8,
    pub color: u8,
    /// (function type, priority) as reported by the headset; unknown ids are kept as raw numbers.
    pub t1_functions: Vec<(u8, u8)>,
    pub t2_functions: Vec<(u8, u8)>,
}

impl DeviceInfo {
    pub fn has_t1(&self, function: u8) -> bool {
        self.t1_functions.iter().any(|&(f, _)| f == function)
    }
    pub fn has_t2(&self, function: u8) -> bool {
        self.t2_functions.iter().any(|&(f, _)| f == function)
    }
}

#[derive(Debug)]
pub enum HandshakeError {
    Session(SessionError),
    /// A reply did not have the expected layout.
    Malformed(&'static str),
    Table1Unsupported,
}

impl From<SessionError> for HandshakeError {
    fn from(e: SessionError) -> Self {
        HandshakeError::Session(e)
    }
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandshakeError::Session(e) => write!(f, "session error: {e}"),
            HandshakeError::Malformed(what) => write!(f, "malformed reply: {what}"),
            HandshakeError::Table1Unsupported => write!(f, "the headset does not support command table 1"),
        }
    }
}

fn bad<T>(what: &'static str) -> Result<T, HandshakeError> {
    Err(HandshakeError::Malformed(what))
}

/// Length-prefixed string at the start of `b`.
fn pstr(b: &[u8]) -> Option<String> {
    let len = *b.first()? as usize;
    Some(String::from_utf8_lossy(b.get(1..1 + len)?).into_owned())
}

/// `01 00 <version u32 BE> <table1> <table2>` (exactly 8 bytes; EnableDisable: 0 = enabled). Returns (version, table1, table2).
fn parse_protocol_info(p: &[u8]) -> Result<(u32, bool, bool), HandshakeError> {
    match p {
        [0x01, 0x00, a, b, c, d, t1, t2] => Ok((u32::from_be_bytes([*a, *b, *c, *d]), *t1 == 0, *t2 == 0)),
        _ => bad("protocol info (expected 8 bytes)"),
    }
}

/// `03 00 <counter> <pstr unique id>`
fn parse_capability(p: &[u8]) -> Result<String, HandshakeError> {
    match p {
        [0x03, 0x00, _, rest @ ..] => pstr(rest).ok_or(HandshakeError::Malformed("capability id string")),
        _ => bad("capability info"),
    }
}

/// `05 <kind> <pstr>`
fn parse_device_string(p: &[u8], kind: u8) -> Result<String, HandshakeError> {
    match p {
        [0x05, k, rest @ ..] if *k == kind => pstr(rest).ok_or(HandshakeError::Malformed("device info string")),
        _ => bad("device info"),
    }
}

/// `05 03 <series> <color>`
fn parse_series_color(p: &[u8]) -> Result<(u8, u8), HandshakeError> {
    match p {
        [0x05, 0x03, s, c, ..] => Ok((*s, *c)),
        _ => bad("series and color"),
    }
}

/// `07 00 <count> {function, priority}*count`
fn parse_functions(p: &[u8]) -> Result<Vec<(u8, u8)>, HandshakeError> {
    match p {
        [0x07, 0x00, n, rest @ ..] if rest.len() >= 2 * *n as usize => Ok(rest.chunks(2).take(*n as usize).map(|c| (c[0], c[1])).collect()),
        _ => bad("support function list"),
    }
}

fn ask<T: Transport>(s: &mut Session<T>, table: Table, payload: &[u8], reply_id: u8) -> Result<Vec<u8>, HandshakeError> {
    let cmd = check(table, payload, Mode::ReadOnly).expect("handshake commands are allowlisted GETs");
    Ok(s.request(&cmd, reply_id)?.payload)
}

pub fn handshake<T: Transport>(s: &mut Session<T>) -> Result<DeviceInfo, HandshakeError> {
    let (protocol_version, table1, table2_enabled) = parse_protocol_info(&ask(s, Table::T1, &[0x00, 0x00], 0x01)?)?;
    if !table1 {
        return Err(HandshakeError::Table1Unsupported);
    }
    let unique_id = parse_capability(&ask(s, Table::T1, &[0x02, 0x00], 0x03)?)?;
    let firmware = parse_device_string(&ask(s, Table::T1, &[0x04, 0x02], 0x05)?, 0x02)?;
    let model_name = parse_device_string(&ask(s, Table::T1, &[0x04, 0x01], 0x05)?, 0x01)?;
    let (series, color) = parse_series_color(&ask(s, Table::T1, &[0x04, 0x03], 0x05)?)?;
    let t1_functions = parse_functions(&ask(s, Table::T1, &[0x06, 0x00], 0x07)?)?;
    let t2_functions = if table2_enabled { parse_functions(&ask(s, Table::T2, &[0x06, 0x00], 0x07)?)? } else { Vec::new() };
    Ok(DeviceInfo { protocol_version, table2_enabled, unique_id, firmware, model_name, series, color, t1_functions, t2_functions })
}

// Names come from docs/arch/protocol.md; ids not listed there stay "unknown" in the dump.
pub fn t1_name(id: u8) -> Option<&'static str> {
    Some(match id {
        0x12 => "CODEC_INDICATOR",
        0x20 => "BATTERY_LEVEL_INDICATOR",
        0x24 => "AUTO_POWER_OFF",
        0x25 => "AUTO_POWER_OFF_WITH_WEARING_DETECTION",
        0x28 => "BATTERY_LEVEL_WITH_THRESHOLD",
        0x67 => "NC_ASM (ambient only)",
        0x69 => "NC_AMB_BUTTON",
        0x6B => "NC_ASM",
        0x6D => "NC_ASM_NOISE_ADAPTATION",
        0xA1..=0xA4 => "PLAYBACK",
        0xD1..=0xD4 => "GENERAL_SETTING",
        0xE2 => "UPSCALING_AUTO_OFF (DSEE)",
        0xE6 => "LISTENING_MODE",
        0xF1 => "PLAYBACK_CONTROL_BY_WEARING",
        0xF3 | 0xFE => "ASSIGNABLE_KEYS",
        0xF4 => "VOICE_ASSISTANT",
        0xF6 => "WEARING_STATUS_DETECTOR",
        0xFC => "SMART_TALKING_MODE_TYPE2 (speak-to-chat)",
        0xFF => "HEAD_GESTURE",
        _ => return None,
    })
}

pub fn t2_name(id: u8) -> Option<&'static str> {
    Some(match id {
        0x30 => "PERIPHERAL_CLASSIC_BT",
        0x31 => "MULTIPOINT (source switch control)",
        0x32 | 0x33 => "PERIPHERAL_WITH_BT_CLASS_OF_DEVICE",
        0x42 => "VOICE_GUIDANCE_VOLUME",
        0x50..=0x53 => "SAFE_LISTENING",
        0xF0 => "WEARING_STATUS_CHECKER",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::mock::FakeHeadset;
    use crate::mdr::frame::{TYPE_ACK, TYPE_T2, unpack};
    use crate::mdr::session::Timeouts;
    use std::time::Duration;

    const FAST: Timeouts = Timeouts { ack: Duration::from_millis(15), response: Duration::from_millis(60), tries: 3 };

    fn run(h: FakeHeadset) -> (Result<DeviceInfo, HandshakeError>, Vec<Vec<u8>>) {
        let writes = h.writes.clone();
        let r = handshake(&mut Session::new(h, FAST));
        let w = writes.borrow().clone();
        (r, w)
    }

    fn sent(w: &[Vec<u8>]) -> Vec<(u8, Vec<u8>)> {
        w.iter().filter_map(|b| unpack(b).ok()).filter(|f| f.data_type != TYPE_ACK).map(|f| (f.data_type, f.payload)).collect()
    }

    #[test]
    fn full_handshake_parses_everything() {
        let (r, w) = run(FakeHeadset::xm6(&[0x20, 0x99], &[0x31], true));
        let i = r.unwrap();
        assert_eq!((i.protocol_version, i.table2_enabled), (0x300, true));
        assert_eq!((i.unique_id.as_str(), i.firmware.as_str(), i.model_name.as_str()), ("UID-1234", "3.0.1", "WH-1000XM6"));
        assert_eq!((i.series, i.color), (0x30, 0x02));
        assert_eq!(i.t1_functions, vec![(0x20, 1), (0x99, 1)]);
        assert!(i.has_t1(0x99) && !i.has_t1(0x28) && i.has_t2(0x31) && !i.has_t2(0x20));
        let payloads: Vec<Vec<u8>> = sent(&w).into_iter().map(|(_, p)| p).collect();
        let want: [&[u8]; 7] = [&[0, 0], &[2, 0], &[4, 2], &[4, 1], &[4, 3], &[6, 0], &[6, 0]];
        assert_eq!(payloads, want);
    }

    #[test]
    fn table2_skipped_when_disabled() {
        let (r, w) = run(FakeHeadset::xm6(&[0x20], &[0x31], false));
        let i = r.unwrap();
        assert!(i.t2_functions.is_empty() && !i.table2_enabled);
        assert!(!sent(&w).iter().any(|(t, _)| *t == TYPE_T2));
    }

    #[test]
    fn table1_unsupported_stops_handshake() {
        let mut h = FakeHeadset::xm6(&[], &[], true);
        h.replies[0].2 = vec![0x01, 0x00, 0, 0, 3, 0, 0x01, 0x00];
        let (r, w) = run(h);
        assert!(matches!(r, Err(HandshakeError::Table1Unsupported)));
        assert_eq!(sent(&w).len(), 1);
    }

    #[test]
    fn wrong_size_protocol_info_is_an_error() {
        let mut h = FakeHeadset::xm6(&[], &[], true);
        h.replies[0].2 = vec![0x01, 0x00, 0, 0, 3, 0, 0, 0, 0];
        assert!(matches!(run(h).0, Err(HandshakeError::Malformed(_))));
    }

    #[test]
    fn malformed_replies_error_instead_of_panicking() {
        assert!(parse_protocol_info(&[]).is_err() && parse_protocol_info(&[1, 0, 0, 0, 0, 3, 0]).is_err());
        assert!(parse_capability(&[3, 0]).is_err() && parse_capability(&[3, 0, 1, 5, b'a']).is_err());
        assert!(parse_device_string(&[5, 2], 2).is_err() && parse_device_string(&[5, 1, 2, b'a', b'b'], 2).is_err());
        assert!(parse_series_color(&[5, 3, 0x30]).is_err());
        assert!(parse_functions(&[]).is_err() && parse_functions(&[7, 0]).is_err() && parse_functions(&[7, 0, 2, 0x20, 1, 0x28]).is_err());
        assert_eq!(parse_functions(&[7, 0, 0]).unwrap(), vec![]);
    }

    #[test]
    fn names_known_and_unknown() {
        assert_eq!(t1_name(0x20), Some("BATTERY_LEVEL_INDICATOR"));
        assert_eq!(t1_name(0x99), None);
        assert_eq!(t2_name(0xF0), Some("WEARING_STATUS_CHECKER"));
    }
}

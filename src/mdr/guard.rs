//! Command guard: the only way to turn a payload into an `AllowedCommand`, which is the only thing `frame::pack_command` accepts.
//! The denylist (docs/arch/protocol.md section 5) is checked first, then the allowlist; anything else is rejected.

use super::frame::{TYPE_T1, TYPE_T2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    T1,
    T2,
}

impl Table {
    pub fn data_type(self) -> u8 {
        match self {
            Table::T1 => TYPE_T1,
            Table::T2 => TYPE_T2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    #[allow(dead_code)] // first used by the feature tasks (SETs)
    Normal,
    /// GET commands only (used by `--probe`).
    ReadOnly,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GuardError {
    Empty,
    /// Matches the denylist: must never be sent.
    Denied,
    /// Not on the allowlist (unknown commands are rejected by default).
    NotAllowed,
    /// Allowed, but it is a SET and the guard is read-only.
    ReadOnly,
}

/// A GET row matches its payload exactly. A SET row matches its prefix, and the validator must accept the argument bytes after it
/// (feature tasks add rows; the validator is where each field is range-checked).
#[derive(Clone, Copy)]
enum Kind {
    Get,
    Set(fn(&[u8]) -> bool),
}

/// A payload the guard has approved. Fields are private so only `check` can build one.
#[derive(Debug)]
pub struct AllowedCommand {
    table: Table,
    payload: Vec<u8>,
}

impl AllowedCommand {
    pub fn table(&self) -> Table {
        self.table
    }
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

const ALLOW: &[(Table, &[u8], Kind)] = &[
    (Table::T1, &[0x00, 0x00], Kind::Get), // CONNECT_GET_PROTOCOL_INFO
    (Table::T1, &[0x02, 0x00], Kind::Get), // CONNECT_GET_CAPABILITY_INFO
    (Table::T1, &[0x04, 0x01], Kind::Get), // GET_DEVICE_INFO MODEL_NAME
    (Table::T1, &[0x04, 0x02], Kind::Get), // GET_DEVICE_INFO FW_VERSION
    (Table::T1, &[0x04, 0x03], Kind::Get), // GET_DEVICE_INFO SERIES_AND_COLOR
    (Table::T1, &[0x06, 0x00], Kind::Get), // CONNECT_GET_SUPPORT_FUNCTION
    (Table::T2, &[0x06, 0x00], Kind::Get), // Table 2 support functions
    // read-only feature GETs issued by --probe (src/probe.rs)
    (Table::T1, &[0x22, 0x00], Kind::Get),              // battery level (feature row, src/device/battery.rs)
    (Table::T1, &[0x22, 0x08], Kind::Get),              // battery level with threshold
    (Table::T1, &[0x66, 0x19], Kind::Get),              // NC/ASM noise adaptation (feature row, src/device/ncasm.rs)
    (Table::T1, &[0x68, 0x19], Kind::Set(nc_set_args)), // NC/ASM SET
    (Table::T1, &[0x50, 0x04, 0x01], Kind::Get),        // EQ capability, English names (feature row, src/device/eq.rs)
    (Table::T1, &[0x58, 0x00], Kind::Set(eq_set_args)), // EQ SET
    (Table::T1, &[0x5A, 0x00], Kind::Get),              // EQ band frequencies
    (Table::T1, &[0x56, 0x00], Kind::Get),              // EQ parameters
    (Table::T1, &[0x52, 0x00], Kind::Get),              // EQ status
    (Table::T1, &[0xE6, 0x01], Kind::Get),              // DSEE parameter
    (Table::T1, &[0xE2, 0x01], Kind::Get),              // DSEE status
    // DSEE SET: the only E8 rows. E8 is also the listening-mode SET (XM6 reboot hazard), so these are whole payloads (no argument bytes), never a prefix.
    (Table::T1, &[0xE8, 0x01, 0x00], Kind::Set(no_args)), // DSEE OFF (src/device/dsee.rs)
    (Table::T1, &[0xE8, 0x01, 0x01], Kind::Set(no_args)), // DSEE AUTO
    (Table::T1, &[0xF6, 0x0C], Kind::Get),                // speak-to-chat enable
    (Table::T1, &[0xFA, 0x0C], Kind::Get),                // speak-to-chat sensitivity
    (Table::T1, &[0x20, 0x04], Kind::Get),                // auto power-off capability
    (Table::T1, &[0x26, 0x04], Kind::Get),                // auto power-off parameter
    (Table::T1, &[0x20, 0x05], Kind::Get),                // auto power-off (wearing) capability
    (Table::T1, &[0x26, 0x05], Kind::Get),                // auto power-off (wearing) parameter
    (Table::T1, &[0xF6, 0x01], Kind::Get),                // auto pause on removal
    (Table::T1, &[0x12, 0x02], Kind::Get),                // codec indicator
    (Table::T1, &[0xD6, 0x01], Kind::Get),                // general setting 1
    (Table::T1, &[0xD6, 0x02], Kind::Get),                // general setting 2
    (Table::T1, &[0xD6, 0x03], Kind::Get),                // general setting 3
    (Table::T1, &[0xD6, 0x04], Kind::Get),                // general setting 4
    (Table::T2, &[0xF2, 0x00], Kind::Get),                // wearing status
    (Table::T2, &[0x36, 0x01], Kind::Get),                // multipoint (source switch control)
    (Table::T2, &[0x36, 0x02], Kind::Get),                // paired device list
    (Table::T2, &[0x32, 0x02], Kind::Get),                // pairing mode status
    (Table::T2, &[0x5A, 0x00], Kind::Get),                // safe listening HBS_1
    (Table::T2, &[0x5A, 0x02], Kind::Get),                // safe listening HBS_2
];

/// For SET rows that are a complete payload: no bytes may follow.
fn no_args(a: &[u8]) -> bool {
    a.is_empty()
}

/// The 7 bytes of an NC/ASM `68 19` SET (also used to check replies): valueChange, totalEffect, mode, ambientMode, level 1..=20, autoAsm, sensitivity.
pub fn nc_set_args(a: &[u8]) -> bool {
    matches!(a, [0..=1, 0..=1, 0..=1, 0..=1, 1..=20, 0..=1, 0..=2])
}

/// Presets an XM6 accepts (FW 3.0): HEAVY, CLEAR, HARD, SOFT, CUSTOM and the user slots. Others (OFF, ROCK, ...) are rejected by the device.
/// shortcut: fixed list from the reference docs for FW 3.0, revisit with captures after firmware updates.
pub const EQ_PRESETS: [u8; 10] = [0x30, 0x31, 0x32, 0x33, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5];

/// The bytes after `58 00`: `<preset> 00` (preset only) or `<preset> 0A <10 raw steps 0..=12>`.
pub fn eq_set_args(a: &[u8]) -> bool {
    match a {
        [p, 0] => EQ_PRESETS.contains(p),
        [p, 10, steps @ ..] => EQ_PRESETS.contains(p) && steps.len() == 10 && steps.iter().all(|&s| s <= 12),
        _ => false,
    }
}

fn denied(table: Table, p: &[u8]) -> bool {
    let at = |i: usize| p.get(i).copied();
    match (table, p[0]) {
        // firmware update / DFU (30-3F), LEA (40-4D)
        (Table::T1, 0x30..=0x4D) => true,
        // test / training modes
        (Table::T1, 0x64 | 0xF4 | 0x0F) => true,
        // RESET_SETTINGS and FACTORY_RESET
        (Table::T1, 0xF8) => at(1) == Some(0x09),
        // FACTORY_POWER_OFF, LINK_CONTROL
        (Table::T1, 0x24) => (at(1) == Some(0x03) && at(2) == Some(0xFF)) || at(1) == Some(0x07),
        // UNPAIR
        (Table::T2, 0x3C) => at(2) == Some(0x02),
        // pairing mode, LEA, PARTY
        (Table::T2, 0x34 | 0x60..=0x69 | 0x70..=0x7C) => true,
        _ => false,
    }
}

type Row = (Table, &'static [u8], Kind);

pub fn check(table: Table, payload: &[u8], mode: Mode) -> Result<AllowedCommand, GuardError> {
    check_in(ALLOW, table, payload, mode)
}

fn check_in(allow: &[Row], table: Table, payload: &[u8], mode: Mode) -> Result<AllowedCommand, GuardError> {
    if payload.is_empty() {
        return Err(GuardError::Empty);
    }
    if denied(table, payload) {
        return Err(GuardError::Denied);
    }
    let matches = |&&(t, p, kind): &&Row| {
        t == table
            && match kind {
                Kind::Get => p == payload,
                Kind::Set(valid) => payload.starts_with(p) && valid(&payload[p.len()..]),
            }
    };
    let Some(&(_, _, kind)) = allow.iter().find(matches) else {
        return Err(GuardError::NotAllowed);
    };
    if mode == Mode::ReadOnly && !matches!(kind, Kind::Get) {
        return Err(GuardError::ReadOnly);
    }
    Ok(AllowedCommand { table, payload: payload.to_vec() })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: &[u8; 17] = b"AA:BB:CC:DD:EE:FF";

    fn with_mac(head: &[u8]) -> Vec<u8> {
        [head, MAC].concat()
    }

    #[test]
    fn denylist_rows_rejected() {
        let mut cases: Vec<(Table, Vec<u8>)> = Vec::new();
        for c in 0x30..=0x3F {
            cases.push((Table::T1, vec![c]));
            cases.push((Table::T1, vec![c, 0x00]));
        }
        for c in 0x40..=0x4D {
            cases.push((Table::T1, vec![c, 0x00]));
        }
        cases.push((Table::T1, vec![0xF8, 0x09, 0x00]));
        cases.push((Table::T1, vec![0xF8, 0x09, 0x01]));
        cases.push((Table::T1, vec![0x24, 0x03, 0xFF]));
        cases.push((Table::T1, vec![0x24, 0x07]));
        cases.push((Table::T1, vec![0x24, 0x07, 0x00]));
        for c in [0x64, 0xF4, 0x0F] {
            cases.push((Table::T1, vec![c, 0x00]));
        }
        cases.push((Table::T2, with_mac(&[0x3C, 0x00, 0x02])));
        cases.push((Table::T2, with_mac(&[0x3C, 0x02, 0x02])));
        cases.push((Table::T2, vec![0x34, 0x02, 0x01, 0x00]));
        cases.push((Table::T2, vec![0x34, 0x02, 0x00, 0x00]));
        for c in (0x60..=0x69).chain(0x70..=0x7C) {
            cases.push((Table::T2, vec![c, 0x00]));
        }
        for (table, payload) in cases {
            for mode in [Mode::Normal, Mode::ReadOnly] {
                assert_eq!(check(table, &payload, mode).unwrap_err(), GuardError::Denied, "{table:?} {payload:02X?}");
            }
        }
    }

    #[test]
    fn unknown_commands_rejected() {
        assert_eq!(check(Table::T1, &[], Mode::Normal).unwrap_err(), GuardError::Empty);
        for (t, p) in [
            (Table::T1, &[0x22, 0x01][..]),   // unknown battery inquiry type
            (Table::T1, &[0x00, 0x00, 0x01]), // handshake GET with extra bytes
            (Table::T1, &[0xE8, 0x03, 0x00]), // listening modes
            (Table::T1, &[0x24, 0x03, 0x01]), // power off now: out of scope
            (Table::T2, &[0x04, 0x01]),       // T1 entry on the wrong table
            (Table::T2, &[0x3C, 0x01, 0x01]), // source switch / connect
        ] {
            assert_eq!(check(t, p, Mode::Normal).unwrap_err(), GuardError::NotAllowed, "{t:?} {p:02X?}");
        }
    }

    #[test]
    fn handshake_gets_accepted() {
        for (table, payload, _) in ALLOW.iter().filter(|r| matches!(r.2, Kind::Get)) {
            let c = check(*table, payload, Mode::Normal).unwrap();
            assert_eq!((c.table(), c.payload()), (*table, *payload));
            assert!(check(*table, payload, Mode::ReadOnly).is_ok());
        }
    }

    #[test]
    fn read_only_rejects_sets() {
        let rows: &[Row] = &[(Table::T1, &[0x58, 0x00], Kind::Set(|a| a == [1])), (Table::T1, &[0x56, 0x00], Kind::Get)];
        assert!(check_in(rows, Table::T1, &[0x58, 0x00, 1], Mode::Normal).is_ok());
        assert_eq!(check_in(rows, Table::T1, &[0x58, 0x00, 2], Mode::Normal).unwrap_err(), GuardError::NotAllowed);
        // wrong length: too short and too long for the validator
        assert_eq!(check_in(rows, Table::T1, &[0x58, 0x00], Mode::Normal).unwrap_err(), GuardError::NotAllowed);
        assert_eq!(check_in(rows, Table::T1, &[0x58, 0x00, 1, 1], Mode::Normal).unwrap_err(), GuardError::NotAllowed);
        assert_eq!(check_in(rows, Table::T1, &[0x58, 0x00, 1], Mode::ReadOnly).unwrap_err(), GuardError::ReadOnly);
        assert!(check_in(rows, Table::T1, &[0x56, 0x00], Mode::ReadOnly).is_ok());
        // denylist still wins over an allowlist row
        let bad: &[Row] = &[(Table::T1, &[0x30, 0x00], Kind::Get), (Table::T1, &[0x64], Kind::Set(|_| true))];
        assert_eq!(check_in(bad, Table::T1, &[0x30, 0x00], Mode::Normal).unwrap_err(), GuardError::Denied);
        assert_eq!(check_in(bad, Table::T1, &[0x64, 0x01], Mode::Normal).unwrap_err(), GuardError::Denied);
    }

    #[test]
    fn only_the_two_dsee_payloads_start_with_e8() {
        for p in [&[0xE8, 0x01, 0x00][..], &[0xE8, 0x01, 0x01]] {
            assert!(check(Table::T1, p, Mode::Normal).is_ok(), "{p:02X?}");
            assert_eq!(check(Table::T1, p, Mode::ReadOnly).unwrap_err(), GuardError::ReadOnly);
            assert_eq!(check(Table::T2, p, Mode::Normal).unwrap_err(), GuardError::NotAllowed);
        }
        // every other (second, third) pair, in both tables and both modes
        for (b, c) in (0..=255u8).flat_map(|b| (0..=255u8).map(move |c| (b, c))).filter(|&p| p != (1, 0) && p != (1, 1)) {
            for t in [Table::T1, Table::T2] {
                for m in [Mode::Normal, Mode::ReadOnly] {
                    assert_eq!(check(t, &[0xE8, b, c], m).unwrap_err(), GuardError::NotAllowed, "{t:?} E8 {b:02X} {c:02X}");
                }
            }
        }
        // listening modes (BGM, upmix, voice contents, sound-leak), other lengths, and the neighbouring reads
        for p in [
            &[0xE8, 0x03, 0x00][..],
            &[0xE8, 0x03, 0x01],
            &[0xE8, 0x09, 0x00],
            &[0xE8, 0x04, 0x00],
            &[0xE8, 0x06, 0x00],
            &[0xE8, 0x07, 0x00],
            &[0xE8, 0x01, 0x02],
            &[0xE8],
            &[0xE8, 0x01],
            &[0xE8, 0x01, 0x00, 0x00],
            &[0xE8, 0x01, 0x01, 0x00],
            &[0xE8, 0x03, 0x01, 0x02],
            &[0xE2, 0x03],
            &[0xE2, 0x04],
            &[0xE6, 0x03],
            &[0xE6, 0x00],
            &[0xE6, 0x02],
            &[0xE6, 0x05],
            &[0xE0, 0x01],
        ] {
            for m in [Mode::Normal, Mode::ReadOnly] {
                assert_eq!(check(Table::T1, p, m).unwrap_err(), GuardError::NotAllowed, "{p:02X?}");
            }
        }
        for p in [&[0xE6, 0x01][..], &[0xE2, 0x01]] {
            assert!(check(Table::T1, p, Mode::ReadOnly).is_ok());
        }
    }

    #[test]
    fn allowlist_never_intersects_denylist_or_listening_modes() {
        for (table, payload, _) in ALLOW {
            assert!(!denied(*table, payload), "{table:?} {payload:02X?}");
        }
        // E8 is also the listening-mode SET: only the two DSEE payloads may start with it
        let e8: Vec<_> = ALLOW.iter().filter(|r| r.1[0] == 0xE8).map(|r| (r.0, r.1)).collect();
        assert_eq!(e8, [(Table::T1, &[0xE8, 0x01, 0x00][..]), (Table::T1, &[0xE8, 0x01, 0x01][..])]);
        // a SET row's argument bytes must not turn it into a denied command (the denylist looks at up to the third byte)
        for (table, prefix, _) in ALLOW.iter().filter(|r| matches!(r.2, Kind::Set(_))) {
            for b in 0..=255u8 {
                assert!(!denied(*table, &[prefix, &[b][..]].concat()), "{table:?} {prefix:02X?} {b:02X}");
            }
        }
    }
}

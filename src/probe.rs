//! `--probe <out>`: handshake, then the read-only GETs of every MVP feature (only those the headset advertises), written as a plain-text dump the owner can send back.
//! The session is created through the guard in `Mode::ReadOnly` (via `Table`/`check`), and every command below is allowlisted as a GET.

use crate::bt::format_mac;
use crate::device::handshake::{DeviceInfo, handshake, t1_name, t2_name};
use crate::mdr::guard::{Mode, Table, check};
use crate::mdr::session::{Session, SessionError, Timeouts, Transport};
use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const T1: Table = Table::T1;
const T2: Table = Table::T2;

struct ProbeCmd {
    name: &'static str,
    table: Table,
    payload: &'static [u8],
    /// Sent only if the headset advertises one of these functions (empty = always).
    gate: &'static [(Table, u8)],
}

const fn cmd(name: &'static str, table: Table, payload: &'static [u8], gate: &'static [(Table, u8)]) -> ProbeCmd {
    ProbeCmd { name, table, payload, gate }
}

// The reply id of every GET is its command id + 1 (protocol.md section 4).
// Function ids for EQ are not in protocol.md, so the EQ reads are sent unconditionally (they are plain GETs).
// Not probed (need a display-language byte whose value is not in protocol.md): `50 04` EQ capability, `D0 <n> <lang>` general-setting capability.
const COMMANDS: &[ProbeCmd] = &[
    cmd("battery level", T1, &[0x22, 0x00], &[(T1, 0x20)]),
    cmd("battery level with threshold", T1, &[0x22, 0x08], &[(T1, 0x28)]),
    cmd("noise cancelling, noise adaptation", T1, &[0x66, 0x19], &[(T1, 0x6D)]),
    cmd("equalizer band frequencies", T1, &[0x5A, 0x00], &[]),
    cmd("equalizer parameters", T1, &[0x56, 0x00], &[]),
    cmd("equalizer status", T1, &[0x52, 0x00], &[]),
    cmd("DSEE parameter", T1, &[0xE6, 0x01], &[(T1, 0xE2)]),
    cmd("DSEE status", T1, &[0xE2, 0x01], &[(T1, 0xE2)]),
    cmd("speak-to-chat enable", T1, &[0xF6, 0x0C], &[(T1, 0xFC)]),
    cmd("speak-to-chat sensitivity", T1, &[0xFA, 0x0C], &[(T1, 0xFC)]),
    cmd("auto power-off capability", T1, &[0x20, 0x04], &[(T1, 0x24)]),
    cmd("auto power-off parameter", T1, &[0x26, 0x04], &[(T1, 0x24)]),
    cmd("auto power-off (wearing) capability", T1, &[0x20, 0x05], &[(T1, 0x25)]),
    cmd("auto power-off (wearing) parameter", T1, &[0x26, 0x05], &[(T1, 0x25)]),
    cmd("auto pause on removal", T1, &[0xF6, 0x01], &[(T1, 0xF1)]),
    cmd("codec indicator", T1, &[0x12, 0x02], &[(T1, 0x12)]),
    cmd("general setting 1", T1, &[0xD6, 0x01], &[(T1, 0xD1)]),
    cmd("general setting 2", T1, &[0xD6, 0x02], &[(T1, 0xD2)]),
    cmd("general setting 3", T1, &[0xD6, 0x03], &[(T1, 0xD3)]),
    cmd("general setting 4", T1, &[0xD6, 0x04], &[(T1, 0xD4)]),
    cmd("wearing status", T2, &[0xF2, 0x00], &[(T2, 0xF0), (T1, 0xF6), (T1, 0xF1)]),
    cmd("multipoint", T2, &[0x36, 0x01], &[(T2, 0x31)]),
    cmd("paired device list", T2, &[0x36, 0x02], &[(T2, 0x32), (T2, 0x33)]),
    cmd("pairing mode status", T2, &[0x32, 0x02], &[(T2, 0x32), (T2, 0x33)]),
    cmd("safe listening HBS_1", T2, &[0x5A, 0x00], &[(T2, 0x50), (T2, 0x51), (T2, 0x52), (T2, 0x53)]),
    cmd("safe listening HBS_2", T2, &[0x5A, 0x02], &[(T2, 0x50), (T2, 0x51), (T2, 0x52), (T2, 0x53)]),
];

fn advertised(info: &DeviceInfo, c: &ProbeCmd) -> bool {
    (c.table != T2 || info.table2_enabled)
        && (c.gate.is_empty() || c.gate.iter().any(|&(t, id)| if t == T1 { info.has_t1(id) } else { info.table2_enabled && info.has_t2(id) }))
}

struct Entry {
    ms: u128,
    tx: bool,
    bytes: Vec<u8>,
}

/// Records every chunk that crosses the link, in both directions.
struct Logged<T> {
    inner: T,
    log: Rc<RefCell<Vec<Entry>>>,
    start: Instant,
}

impl<T> Logged<T> {
    fn record(&self, tx: bool, bytes: &[u8]) {
        self.log.borrow_mut().push(Entry { ms: self.start.elapsed().as_millis(), tx, bytes: bytes.to_vec() });
    }
}

impl<T: Transport> Transport for Logged<T> {
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        let n = self.inner.read(buf, timeout)?;
        if n > 0 {
            self.record(false, &buf[..n]);
        }
        Ok(n)
    }
    fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.record(true, data);
        self.inner.write(data)
    }
    fn close(&mut self) {
        self.inner.close();
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}

fn is_mac_text(w: &[u8]) -> bool {
    w.iter().enumerate().all(|(i, &c)| if i % 3 == 2 { c == b':' } else { c.is_ascii_hexdigit() })
}

/// Replaces every ASCII MAC address (`AA:BB:CC:DD:EE:FF`, e.g. in the paired-device list) with `XX:XX:XX:XX:XX:XX`.
fn scrub(b: &[u8]) -> Vec<u8> {
    let (mut out, mut i) = (Vec::with_capacity(b.len()), 0);
    while i < b.len() {
        if b.len() - i >= 17 && is_mac_text(&b[i..i + 17]) {
            out.extend(b"XX:XX:XX:XX:XX:XX");
            i += 17;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn utc(secs: u64) -> String {
    let (days, rem) = ((secs / 86400) as i64, secs % 86400);
    // civil-from-days (H. Hinnant), valid for dates after 1970
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn function_list(out: &mut String, title: &str, list: &[(u8, u8)], name: fn(u8) -> Option<&'static str>) {
    out.push_str(&format!("\n== {title} ({} entries) ==\n", list.len()));
    for &(id, prio) in list {
        out.push_str(&format!("  0x{id:02X}  priority {prio}  {}\n", name(id).unwrap_or("unknown (raw id)")));
    }
}

/// Runs the whole probe and returns the file text plus the overall outcome. The text is complete even when the run failed part-way.
pub fn run<T: Transport>(transport: T, mac: u64, keep_mac: bool, listen: Duration, timeouts: Timeouts) -> (String, Result<(), String>) {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut s = Session::new(Logged { inner: transport, log: log.clone(), start: Instant::now() }, timeouts);
    let show = |b: &[u8]| if keep_mac { hex(b) } else { hex(&scrub(b)) };
    let mut failure = None;
    let (mut reads, mut unsolicited) = (String::new(), String::new());

    let info = match handshake(&mut s) {
        Ok(i) => Some(i),
        Err(e) => {
            failure = Some(format!("handshake failed: {e}"));
            None
        }
    };
    if let Some(info) = &info {
        let mut silent = 0; // consecutive timeouts
        for c in COMMANDS {
            let line = format!("{} ({:?} {})", c.name, c.table, hex(c.payload));
            if !advertised(info, c) {
                reads.push_str(&format!("  skipped, not advertised: {line}\n"));
                continue;
            }
            let (mut lost, mut timed_out) = (false, false);
            let rx_before = log.borrow().iter().filter(|e| !e.tx).count();
            let outcome = match check(c.table, c.payload, Mode::ReadOnly) {
                Err(e) => Err(format!("refused by the guard ({e})")),
                Ok(a) => s.request(&a, c.payload[0] + 1).map(|f| show(&f.payload)).map_err(|e| {
                    lost = matches!(e, SessionError::Io(_));
                    timed_out = matches!(e, SessionError::Timeout);
                    e.to_string()
                }),
            };
            match outcome {
                Ok(reply) => {
                    silent = 0;
                    reads.push_str(&format!("  {line}\n      reply: {reply}\n"));
                }
                Err(e) => {
                    reads.push_str(&format!("  {line}\n      no reply: {e}\n"));
                    // silence = a timeout with nothing received at all (an ACK without a reply is not silence)
                    let heard = log.borrow().iter().filter(|e| !e.tx).count() > rx_before;
                    silent = if timed_out && !heard { silent + 1 } else { 0 };
                    if lost {
                        failure = Some(format!("connection lost during {}: {e}", c.name));
                        break;
                    }
                    if silent >= 2 {
                        failure = Some("headset stopped answering".to_string());
                        break;
                    }
                }
            }
        }
        if failure.is_none() {
            let end = Instant::now() + listen;
            while Instant::now() < end {
                match s.poll_notifications(end.saturating_duration_since(Instant::now())) {
                    Ok(frames) => frames.iter().for_each(|f| unsolicited.push_str(&format!("  table 0x{:02X}: {}\n", f.data_type, show(&f.payload)))),
                    Err(e) => {
                        failure = Some(format!("connection lost while listening: {e}"));
                        break;
                    }
                }
            }
        }
    }
    s.close();

    let mut out = String::new();
    out.push_str("XM Control probe dump\n=====================\n");
    out.push_str(&format!("Tool:      xm-control {}\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("Date:      {}\n", utc(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()))));
    out.push_str(&format!("Model:     {}\n", info.as_ref().map_or("unknown", |i| i.model_name.as_str())));
    out.push_str(&format!("Firmware:  {}\n", info.as_ref().map_or("unknown", |i| i.firmware.as_str())));
    let mac_text = if keep_mac { format_mac(mac) } else { "XX:XX:XX:XX:XX:XX (hidden; run with --keep-mac to include it)".to_string() };
    out.push_str(&format!("MAC:       {mac_text}\n"));
    out.push_str(&format!("Result:    {}\n", failure.as_deref().unwrap_or("completed")));
    out.push_str("\nThis file lists only read-only requests. Before sending it, look through it for anything private:\n");
    out.push_str("the device unique id, and names of other paired devices (\"paired device list\").\n");
    if let Some(i) = &info {
        out.push_str(&format!("\n== Device ==\n  model:             {}\n  firmware:          {}\n", i.model_name, i.firmware));
        let uid = if keep_mac { i.unique_id.clone() } else { String::from_utf8_lossy(&scrub(i.unique_id.as_bytes())).into_owned() };
        out.push_str(&format!("  protocol version:  0x{:08X}\n  unique id:         {uid}\n", i.protocol_version));
        out.push_str(&format!("  series / color:    0x{:02X} / 0x{:02X}\n  table 2 enabled:   {}\n", i.series, i.color, i.table2_enabled));
        function_list(&mut out, "Table 1 support functions", &i.t1_functions, t1_name);
        function_list(&mut out, "Table 2 support functions", &i.t2_functions, t2_name);
    }
    out.push_str(&format!("\n== Feature reads (command, then the headset's reply payload) ==\n{reads}"));
    out.push_str(&format!("\n== Unsolicited frames received after the reads ==\n{unsolicited}"));
    out.push_str("\n== Frame log (ms since start; TX = sent to the headset, RX = received) ==\n");
    // A MAC can span two reads, so scrub the whole RX stream (scrub keeps the length) and cut it back into the original chunks.
    let log = log.borrow();
    let rx: Vec<u8> = log.iter().filter(|e| !e.tx).flat_map(|e| e.bytes.iter().copied()).collect();
    let rx = if keep_mac { rx } else { scrub(&rx) };
    let mut pos = 0;
    for e in log.iter() {
        let bytes = if e.tx {
            &e.bytes[..]
        } else {
            pos += e.bytes.len();
            &rx[pos - e.bytes.len()..pos]
        };
        out.push_str(&format!("{:>7} ms  {}  {}\n", e.ms, if e.tx { "TX" } else { "RX" }, hex(bytes)));
    }
    (out, failure.map_or(Ok(()), Err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::mock::FakeHeadset;
    use crate::mdr::frame::{TYPE_ACK, unpack};

    const FAST: Timeouts = Timeouts { ack: Duration::from_millis(15), response: Duration::from_millis(60), tries: 3 };
    const ALL_T1: [u8; 18] = [0x12, 0x20, 0x28, 0x24, 0x25, 0x67, 0x6B, 0x6D, 0xE2, 0xF1, 0xFC, 0xD1, 0xD2, 0xD3, 0xD4, 0xF6, 0x69, 0x99];
    const ALL_T2: [u8; 5] = [0x31, 0x32, 0x33, 0x50, 0xF0];

    fn full(keep_mac: bool) -> (String, Result<(), String>) {
        let mut h = FakeHeadset::xm6(&ALL_T1, &ALL_T2, true);
        h.ntfy_on = Some((vec![0x12, 0x02], vec![0x15, 0x02, 0x10]));
        run(h, 0xAABBCCDDEEFF, keep_mac, Duration::from_millis(20), FAST)
    }

    fn tx_frames(report: &str) -> Vec<crate::mdr::frame::Frame> {
        let log = report.split("== Frame log").nth(1).unwrap();
        log.lines()
            .filter_map(|l| l.split_once("  TX  "))
            .map(|(_, h)| unpack(&h.split(' ').map(|x| u8::from_str_radix(x, 16).unwrap()).collect::<Vec<_>>()).unwrap())
            .collect()
    }

    #[test]
    fn full_run_has_expected_sections_and_sends_only_read_only_commands() {
        let (report, result) = full(false);
        result.unwrap();
        for s in [
            "Model:     WH-1000XM6",
            "Firmware:  3.0.1",
            "Result:    completed",
            "== Table 1 support functions (18 entries)",
            "== Table 2 support functions (5 entries)",
        ] {
            assert!(report.contains(s), "{s}");
        }
        assert!(report.contains("0x99  priority 1  unknown (raw id)"));
        assert!(report.contains("0x20  priority 1  BATTERY_LEVEL_INDICATOR"));
        assert!(report.contains("battery level (T1 22 00)\n      reply: 23 00 00"));
        assert!(report.contains("== Unsolicited frames received after the reads ==\n  table 0x0C: 15 02 10"));
        assert!(!report.contains("not advertised"));
        // every TX frame is an ACK or a guard-approved GET, and every probe command went out once
        let tx = tx_frames(&report);
        let cmds: Vec<_> = tx.iter().filter(|f| f.data_type != TYPE_ACK).collect();
        assert_eq!(cmds.len(), 7 + COMMANDS.len());
        for f in cmds {
            let table = if f.data_type == T2.data_type() { T2 } else { T1 };
            assert!(check(table, &f.payload, Mode::ReadOnly).is_ok(), "{:02X?}", f.payload);
        }
    }

    #[test]
    fn unadvertised_features_are_skipped() {
        let h = FakeHeadset::xm6(&[0x20], &[], false);
        let (report, result) = run(h, 1, false, Duration::from_millis(5), FAST);
        result.unwrap();
        assert!(report.contains("skipped, not advertised: battery level with threshold (T1 22 08)"));
        assert!(report.contains("skipped, not advertised: wearing status (T2 F2 00)"));
        assert!(report.contains("battery level (T1 22 00)\n      reply:"));
        assert!(!tx_frames(&report).iter().any(|f| f.data_type == T2.data_type()));
    }

    #[test]
    fn mac_is_redacted_by_default_and_kept_on_request() {
        let (hidden, _) = full(false);
        assert!(hidden.contains("MAC:       XX:XX:XX:XX:XX:XX (hidden"));
        assert!(!hidden.contains("AA:BB:CC:DD:EE:FF"));
        let other_mac = hex(b"11:22:33:44:55:66");
        assert!(!hidden.contains(&other_mac) && !hidden.contains(&other_mac[..20]));
        assert!(hidden.contains(&hex(b"XX:XX:XX:XX:XX:XX")));
        let (shown, _) = full(true);
        assert!(shown.contains("MAC:       AA:BB:CC:DD:EE:FF"));
        assert!(shown.contains(&other_mac));
    }

    /// Hands out at most 5 bytes per read, so frames (and the MAC inside) arrive split.
    struct Chunky<T> {
        inner: T,
        pending: std::collections::VecDeque<u8>,
    }

    impl<T: Transport> Transport for Chunky<T> {
        fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
            if self.pending.is_empty() {
                let n = self.inner.read(buf, timeout)?;
                self.pending.extend(&buf[..n]);
            }
            let n = self.pending.len().min(5);
            for b in buf.iter_mut().take(n) {
                *b = self.pending.pop_front().unwrap();
            }
            Ok(n)
        }
        fn write(&mut self, data: &[u8]) -> io::Result<()> {
            self.inner.write(data)
        }
        fn close(&mut self) {
            self.inner.close();
        }
    }

    #[test]
    fn mac_split_across_reads_is_still_redacted() {
        let h = FakeHeadset::xm6(&ALL_T1, &ALL_T2, true);
        let (report, _) = run(Chunky { inner: h, pending: Default::default() }, 1, false, Duration::from_millis(5), FAST);
        let mac = hex(b"11:22:33:44:55:66");
        assert!(!report.contains(&mac) && !report.contains(&mac[..20]) && !report.contains(&mac[mac.len() - 20..]));
        assert!(report.contains(&hex(b"XX:XX:XX:XX:XX:XX")));
    }

    #[test]
    fn silent_headset_stops_after_two_timeouts_and_still_reports() {
        let mut h = FakeHeadset::xm6(&ALL_T1, &ALL_T2, true);
        h.generic = false;
        let (report, result) = run(h, 1, false, Duration::from_millis(5), FAST);
        assert_eq!(result.unwrap_err(), "headset stopped answering");
        assert!(report.contains("Result:    headset stopped answering"));
        assert_eq!(tx_frames(&report).iter().filter(|f| f.data_type != TYPE_ACK).count(), 7 + 2 * 3); // no ACK: each silent command is tried 3 times
    }

    #[test]
    fn acked_but_unanswered_reads_do_not_abort_the_probe() {
        let mut h = FakeHeadset::xm6(&ALL_T1, &ALL_T2, true);
        h.no_reply = vec![vec![0x5A, 0x00], vec![0x56, 0x00]];
        let (report, result) = run(h, 1, false, Duration::from_millis(5), FAST);
        result.unwrap();
        assert!(report.contains("equalizer status (T1 52 00)\n      reply:"));
        assert!(report.contains("safe listening HBS_2 (T2 5A 02)\n      reply:"));
        assert!(report.contains("Result:    completed"));
    }

    #[test]
    fn handshake_failure_still_writes_a_report() {
        let mut h = FakeHeadset::xm6(&[], &[], true);
        h.replies[0].2 = vec![0x01, 0x00, 0, 0, 3, 0, 0x01, 0x00];
        let (report, result) = run(h, 1, false, Duration::from_millis(5), FAST);
        assert!(result.is_err());
        assert!(report.contains("Result:    handshake failed: the headset does not support command table 1"));
        assert!(report.contains("== Frame log"));
    }

    #[test]
    fn scrub_and_date_helpers() {
        assert_eq!(scrub(b"ab11:22:33:44:55:66cd"), b"abXX:XX:XX:XX:XX:XXcd");
        assert_eq!(scrub(b"11:22:33:44:55"), b"11:22:33:44:55");
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(1_700_000_000), "2023-11-14 22:13:20 UTC");
    }
}

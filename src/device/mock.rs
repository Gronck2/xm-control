//! Test-only scripted headset: ACKs every command and answers from a table (or generically), so handshake and probe tests need no hardware.

use crate::mdr::frame::{TYPE_ACK, TYPE_T1, TYPE_T2, pack_ack, pack_raw_for_tests, unpack};
use crate::mdr::session::Transport;
use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct FakeHeadset {
    /// (data type, command payload, reply payload)
    pub replies: Vec<(u8, Vec<u8>, Vec<u8>)>,
    /// Answer every other command with `[cmd + 1, arg, 0]`.
    pub generic: bool,
    /// Commands (payloads) that get an ACK but never a reply; any other command without a reply gets nothing at all.
    pub no_reply: Vec<Vec<u8>>,
    /// When this T1 command arrives, also push this unsolicited T1 payload.
    pub ntfy_on: Option<(Vec<u8>, Vec<u8>)>,
    pub writes: Arc<Mutex<Vec<Vec<u8>>>>,
    /// A write of this payload fails with a broken pipe (simulates a lost link).
    pub fail_on: Option<Vec<u8>>,
    inbox: VecDeque<Vec<u8>>,
    seq: u8,
}

fn pstr(s: &str) -> Vec<u8> {
    [&[s.len() as u8][..], s.as_bytes()].concat()
}

fn functions(id: u8, list: &[u8]) -> Vec<u8> {
    let mut v = vec![id, 0x00, list.len() as u8];
    v.extend(list.iter().flat_map(|&f| [f, 0x01]));
    v
}

impl FakeHeadset {
    /// An XM6-like headset with the given support-function lists.
    pub fn xm6(t1: &[u8], t2: &[u8], table2: bool) -> Self {
        let t2_flag = if table2 { 0x00 } else { 0x01 }; // V2 EnableDisable: ENABLE = 0
        let r = |dt: u8, c: &[u8], reply: Vec<u8>| (dt, c.to_vec(), reply);
        FakeHeadset {
            replies: vec![
                r(TYPE_T1, &[0x00, 0x00], vec![0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, t2_flag]),
                r(TYPE_T1, &[0x02, 0x00], [&[0x03, 0x00, 0x01][..], &pstr("UID-1234")].concat()),
                r(TYPE_T1, &[0x04, 0x02], [&[0x05, 0x02][..], &pstr("3.0.1")].concat()),
                r(TYPE_T1, &[0x04, 0x01], [&[0x05, 0x01][..], &pstr("WH-1000XM6")].concat()),
                r(TYPE_T1, &[0x04, 0x03], vec![0x05, 0x03, 0x30, 0x02]),
                r(TYPE_T1, &[0x06, 0x00], functions(0x07, t1)),
                r(TYPE_T2, &[0x06, 0x00], functions(0x07, t2)),
                // paired device list reply carries another device's MAC as ASCII (must be scrubbed from the dump)
                r(TYPE_T2, &[0x36, 0x02], [&[0x37, 0x02, 0x01][..], b"11:22:33:44:55:66", &[0x01, 0, 0, 0][..], &pstr("Phone")].concat()),
            ],
            generic: true,
            no_reply: vec![],
            ntfy_on: None,
            writes: Arc::default(),
            fail_on: None,
            inbox: VecDeque::new(),
            seq: 0,
        }
    }
}

impl Transport for FakeHeadset {
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        match self.inbox.pop_front() {
            Some(d) => {
                buf[..d.len()].copy_from_slice(&d);
                Ok(d.len())
            }
            None => {
                std::thread::sleep(timeout);
                Ok(0)
            }
        }
    }

    fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.writes.lock().unwrap().push(data.to_vec());
        let Ok(f) = unpack(data) else { return Ok(()) };
        if f.data_type == TYPE_ACK {
            return Ok(());
        }
        if self.fail_on.as_ref() == Some(&f.payload) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let muted = self.no_reply.contains(&f.payload);
        let reply = match self.replies.iter().find(|(dt, c, _)| *dt == f.data_type && *c == f.payload) {
            Some((_, _, r)) => Some(r.clone()),
            None if self.generic && !muted => Some(vec![f.payload[0] + 1, f.payload[1], 0x00]),
            None => None,
        };
        if reply.is_some() || muted {
            self.inbox.push_back(pack_ack(f.seq ^ 1));
        }
        for (dt, p) in
            reply.map(|r| (f.data_type, r)).into_iter().chain(self.ntfy_on.iter().filter(|(t, _)| *t == f.payload).map(|(_, n)| (TYPE_T1, n.clone())))
        {
            self.inbox.push_back(pack_raw_for_tests(dt, self.seq, &p).unwrap());
            self.seq ^= 1;
        }
        Ok(())
    }

    fn close(&mut self) {}
}

/// The RX/TX frames of the "Frame log" of tests/fixtures/xm6-probe.txt as (is_tx, raw bytes).
fn capture_frames() -> Vec<(bool, Vec<u8>)> {
    let text = include_str!("../../tests/fixtures/xm6-probe.txt");
    let log = text.lines().skip_while(|l| !l.starts_with("== Frame log")).skip(1);
    log.filter_map(|l| {
        let mut t = l.split_whitespace().skip(2); // "<ms> ms"
        let tx = t.next()? == "TX";
        Some((tx, t.map(|b| u8::from_str_radix(b, 16).unwrap()).collect()))
    })
    .collect()
}

impl FakeHeadset {
    /// The real XM6 (firmware 3.1.5) as captured: every command in the capture gets the reply the headset gave (the first non-ACK frame after it,
    /// whatever its type byte), commands it only ACKed stay silent, and everything else is not answered at all.
    pub fn from_xm6_capture() -> Self {
        let mut h = FakeHeadset::xm6(&[], &[], true);
        h.generic = false;
        h.replies.clear();
        let mut cur: Option<(u8, Vec<u8>, Option<Vec<u8>>)> = None;
        let mut done = Vec::new();
        for (tx, raw) in capture_frames() {
            let f = unpack(&raw).unwrap();
            if f.data_type == TYPE_ACK {
                continue;
            }
            if tx {
                done.extend(cur.replace((f.data_type, f.payload, None)));
            } else if let Some((_, _, r @ None)) = cur.as_mut() {
                *r = Some(f.payload);
            }
        }
        done.extend(cur);
        for (dt, cmd, reply) in done {
            match reply {
                Some(r) => h.replies.push((dt, cmd, r)),
                None => h.no_reply.push(cmd),
            }
        }
        h
    }
}

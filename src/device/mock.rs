//! Test-only scripted headset: ACKs every command and answers from a table (or generically), so handshake and probe tests need no hardware.

use crate::mdr::frame::{TYPE_ACK, TYPE_T1, TYPE_T2, pack_ack, pack_raw_for_tests, unpack};
use crate::mdr::session::Transport;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;
use std::time::Duration;

pub struct FakeHeadset {
    /// (data type, command payload, reply payload)
    pub replies: Vec<(u8, Vec<u8>, Vec<u8>)>,
    /// Answer every other command with `[cmd + 1, arg, 0]`.
    pub generic: bool,
    /// When this T1 command arrives, also push this unsolicited T1 payload.
    pub ntfy_on: Option<(Vec<u8>, Vec<u8>)>,
    pub writes: Rc<RefCell<Vec<Vec<u8>>>>,
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
            ntfy_on: None,
            writes: Rc::default(),
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
        self.writes.borrow_mut().push(data.to_vec());
        let Ok(f) = unpack(data) else { return Ok(()) };
        if f.data_type == TYPE_ACK {
            return Ok(());
        }
        self.inbox.push_back(pack_ack(f.seq ^ 1));
        let reply = match self.replies.iter().find(|(dt, c, _)| *dt == f.data_type && *c == f.payload) {
            Some((_, _, r)) => Some(r.clone()),
            None if self.generic => Some(vec![f.payload[0] + 1, f.payload[1], 0x00]),
            None => None,
        };
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

//! MDR session over a `Transport` (docs/arch/protocol.md section 2): stop-and-wait with a 0/1 sequence, ACK + retry,
//! an ACK for every received T1/T2 frame, and unsolicited frames queued for the caller. Blocking; meant to be owned by one worker thread.

use super::frame::{Frame, FrameError, FrameParser, MAX_FRAME, TYPE_ACK, TYPE_T1, TYPE_T2, pack_ack, pack_command};
use super::guard::AllowedCommand;
use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

pub trait Transport {
    /// Reads up to `buf.len()` bytes, waiting at most `timeout`. `Ok(0)` means nothing arrived in time; a closed link is an `Err`.
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize>;
    fn write(&mut self, data: &[u8]) -> io::Result<()>;
    fn close(&mut self);
}

#[derive(Debug)]
pub enum SessionError {
    Io(io::Error),
    Frame(FrameError),
    /// No ACK after all tries.
    Timeout,
    /// ACKed, but no matching reply in time (the XM6 does this for some GETs); the session stays usable.
    NoReply,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Io(e) => write!(f, "I/O error: {e}"),
            SessionError::Frame(e) => write!(f, "bad frame from the headset: {e}"),
            SessionError::Timeout => write!(f, "the headset did not answer"),
            SessionError::NoReply => write!(f, "the headset acknowledged but did not reply"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<io::Error> for SessionError {
    fn from(e: io::Error) -> Self {
        SessionError::Io(e)
    }
}

impl From<FrameError> for SessionError {
    fn from(e: FrameError) -> Self {
        SessionError::Frame(e)
    }
}

#[derive(Clone, Copy)]
pub struct Timeouts {
    pub ack: Duration,
    pub response: Duration,
    pub tries: u32,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts { ack: Duration::from_millis(1000), response: Duration::from_millis(5000), tries: 10 }
    }
}

pub struct Session<T: Transport> {
    transport: T,
    parser: FrameParser,
    /// Sequence number of the next command (0 or 1).
    seq: u8,
    /// Received T1/T2 frames nobody has taken yet.
    queue: VecDeque<Frame>,
    timeouts: Timeouts,
}

impl<T: Transport> Session<T> {
    pub fn new(transport: T, timeouts: Timeouts) -> Self {
        Session { transport, parser: FrameParser::default(), seq: 0, queue: VecDeque::new(), timeouts }
    }

    pub fn close(&mut self) {
        self.transport.close();
    }

    /// Sends one command and waits for its ACK, resending with the same seq up to `tries` times.
    pub fn send(&mut self, cmd: &AllowedCommand) -> Result<(), SessionError> {
        let bytes = pack_command(cmd, self.seq)?;
        for _ in 0..self.timeouts.tries {
            self.transport.write(&bytes)?;
            let deadline = Instant::now() + self.timeouts.ack;
            loop {
                if self.pump(deadline, true)? {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    break;
                }
            }
        }
        Err(SessionError::Timeout)
    }

    /// Sends a command, then waits for the reply frame of the same table whose first payload byte is `reply_id`.
    /// Other frames stay queued for `poll_notifications`.
    pub fn request(&mut self, cmd: &AllowedCommand, reply_id: u8) -> Result<Frame, SessionError> {
        let first_new = self.queue.len(); // frames queued before this send can't be its reply
        self.send(cmd)?;
        let data_type = cmd.table().data_type();
        let deadline = Instant::now() + self.timeouts.response;
        // several GETs share a reply id; the reply echoes the command's second byte
        let sub = cmd.payload().get(1);
        loop {
            let matches = |f: &Frame| f.data_type == data_type && f.payload.first() == Some(&reply_id) && (sub.is_none() || f.payload.get(1) == sub);
            if let Some(i) = self.queue.iter().skip(first_new).position(matches) {
                return Ok(self.queue.remove(first_new + i).expect("index just found"));
            }
            if Instant::now() >= deadline {
                return Err(SessionError::NoReply);
            }
            self.pump(deadline, false)?;
        }
    }

    /// Waits up to `wait` for incoming data, ACKs it, and returns every queued frame.
    pub fn poll_notifications(&mut self, wait: Duration) -> Result<Vec<Frame>, SessionError> {
        self.pump(Instant::now() + wait, false)?;
        Ok(self.queue.drain(..).collect())
    }

    /// One read; ACKs and queues T1/T2 frames. Returns true if a matching ACK arrived (only counted when `expect_ack`).
    fn pump(&mut self, deadline: Instant, expect_ack: bool) -> Result<bool, SessionError> {
        let mut buf = [0u8; MAX_FRAME];
        let n = self.transport.read(&mut buf, deadline.saturating_duration_since(Instant::now()))?;
        let mut acked = false;
        for f in self.parser.push(&buf[..n]) {
            match f.data_type {
                // the device ACKs with 1 - seq; anything else is stale and ignored
                TYPE_ACK if expect_ack && !acked && f.seq == self.seq ^ 1 => {
                    self.seq ^= 1;
                    acked = true;
                }
                TYPE_T1 | TYPE_T2 => {
                    self.transport.write(&pack_ack(f.seq ^ 1))?;
                    self.queue.push_back(f);
                }
                _ => {}
            }
        }
        Ok(acked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_error_display_is_readable() {
        let all = [SessionError::Io(io::Error::other("x")), SessionError::Frame(FrameError::TooShort), SessionError::Timeout, SessionError::NoReply];
        for e in &all {
            assert!(!e.to_string().is_empty() && e.to_string() != format!("{e:?}"));
        }
        assert_ne!(SessionError::Timeout.to_string(), SessionError::NoReply.to_string());
    }
    use crate::mdr::frame::pack_raw_for_tests as pack_raw;
    use crate::mdr::guard::{Mode, Table, check};

    enum Step {
        Data(Vec<u8>),
        Close,
    }

    /// Scripted transport: `replies[i]` becomes readable after the i-th write; `inbox` is readable immediately.
    #[derive(Default)]
    struct Mock {
        inbox: VecDeque<Step>,
        replies: VecDeque<Vec<Step>>,
        writes: Vec<Vec<u8>>,
        closed: bool,
    }

    impl Transport for Mock {
        fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
            if self.closed {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            match self.inbox.pop_front() {
                Some(Step::Data(d)) => {
                    buf[..d.len()].copy_from_slice(&d);
                    Ok(d.len())
                }
                Some(Step::Close) => {
                    self.closed = true;
                    Err(io::ErrorKind::UnexpectedEof.into())
                }
                None => {
                    std::thread::sleep(timeout);
                    Ok(0)
                }
            }
        }
        fn write(&mut self, data: &[u8]) -> io::Result<()> {
            if self.closed {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            self.writes.push(data.to_vec());
            self.inbox.extend(self.replies.pop_front().unwrap_or_default());
            Ok(())
        }
        fn close(&mut self) {
            self.closed = true;
        }
    }

    const FAST: Timeouts = Timeouts { ack: Duration::from_millis(15), response: Duration::from_millis(60), tries: 10 };
    const ACK_FOR_0: [u8; 9] = [0x3E, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x02, 0x3C];
    const ACK_FOR_1: [u8; 9] = [0x3E, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x3C];

    fn cmd(payload: &[u8]) -> AllowedCommand {
        check(Table::T1, payload, Mode::Normal).unwrap()
    }

    fn data(b: Vec<u8>) -> Step {
        Step::Data(b)
    }

    fn session(replies: Vec<Vec<Step>>) -> Session<Mock> {
        Session::new(Mock { replies: replies.into(), ..Default::default() }, FAST)
    }

    #[test]
    fn send_waits_for_ack() {
        let mut s = session(vec![vec![data(pack_ack(1))]]);
        s.send(&cmd(&[0, 0])).unwrap();
        assert_eq!(s.transport.writes, vec![pack_command(&cmd(&[0, 0]), 0).unwrap()]);
        assert_eq!(s.seq, 1);
    }

    #[test]
    fn missing_ack_retries_ten_times_with_same_seq() {
        let mut s = session(vec![]);
        assert!(matches!(s.send(&cmd(&[0, 0])), Err(SessionError::Timeout)));
        assert_eq!(s.transport.writes.len(), 10);
        assert!(s.transport.writes.iter().all(|w| *w == s.transport.writes[0]));
        assert_eq!(s.seq, 0);
    }

    #[test]
    fn retry_succeeds_when_late_ack_arrives() {
        let mut s = session(vec![vec![], vec![], vec![data(pack_ack(1))]]);
        s.send(&cmd(&[0, 0])).unwrap();
        assert_eq!(s.transport.writes.len(), 3);
        assert_eq!(s.seq, 1);
    }

    #[test]
    fn seq_flips_only_on_matching_ack_and_stale_is_ignored() {
        // second command has seq 1, so its ACK is seq 0; a leading stale ACK(1) must not count
        let stale_then_good = vec![data([ACK_FOR_0.to_vec(), ACK_FOR_1.to_vec()].concat())];
        let mut s = session(vec![vec![data(pack_ack(1))], stale_then_good]);
        s.send(&cmd(&[0, 0])).unwrap();
        s.send(&cmd(&[2, 0])).unwrap();
        assert_eq!(s.transport.writes[0][2], 0);
        assert_eq!(s.transport.writes[1][2], 1);
        assert_eq!(s.seq, 0);
        // only a stale ACK: no flip, timeout
        let mut s = session(vec![vec![data(ACK_FOR_1.to_vec())]]);
        assert!(matches!(s.send(&cmd(&[0, 0])), Err(SessionError::Timeout)));
        assert_eq!(s.seq, 0);
    }

    #[test]
    fn received_frames_are_acked_with_flipped_seq() {
        for (seq, ack) in [(0u8, ACK_FOR_0), (1, ACK_FOR_1)] {
            let mut s = session(vec![]);
            s.transport.inbox.push_back(data(pack_raw(TYPE_T1, seq, &[0x01, 0x00]).unwrap()));
            let got = s.poll_notifications(Duration::from_millis(5)).unwrap();
            assert_eq!(got.len(), 1);
            assert_eq!(s.transport.writes, vec![ack.to_vec()]);
        }
    }

    #[test]
    fn stray_ack_outside_send_is_ignored() {
        let mut s = session(vec![]);
        s.transport.inbox.push_back(data(ACK_FOR_1.to_vec()));
        assert!(s.poll_notifications(Duration::from_millis(5)).unwrap().is_empty());
        assert_eq!(s.seq, 0);
    }

    #[test]
    fn unsolicited_frame_while_waiting_for_ack_is_acked_and_queued() {
        let ntfy = pack_raw(TYPE_T1, 0, &[0x67, 0x01]).unwrap();
        let mut s = session(vec![vec![data([ntfy, pack_ack(1)].concat())]]);
        s.send(&cmd(&[0, 0])).unwrap();
        assert_eq!(s.transport.writes[1], ACK_FOR_0.to_vec());
        let got = s.poll_notifications(Duration::from_millis(1)).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].payload, vec![0x67, 0x01]);
    }

    #[test]
    fn request_returns_matching_reply_and_keeps_others() {
        let other = pack_raw(TYPE_T1, 0, &[0x67, 0x01]).unwrap();
        let reply = pack_raw(TYPE_T1, 1, &[0x01, 0x00, 0xAA]).unwrap();
        let mut s = session(vec![vec![data(pack_ack(1)), data(other), data(reply)]]);
        let f = s.request(&cmd(&[0, 0]), 0x01).unwrap();
        assert_eq!(f.payload, vec![0x01, 0x00, 0xAA]);
        assert_eq!(s.poll_notifications(Duration::from_millis(1)).unwrap().len(), 1);
    }

    #[test]
    fn request_requires_matching_second_byte() {
        let wrong = pack_raw(TYPE_T1, 0, &[0x21, 0x05, 0xAA]).unwrap();
        let right = pack_raw(TYPE_T1, 1, &[0x21, 0x04, 0xBB]).unwrap();
        let mut s = session(vec![vec![data(pack_ack(1)), data(wrong), data(right)]]);
        let f = s.request(&cmd(&[0x20, 0x04]), 0x21).unwrap();
        assert_eq!(f.payload, vec![0x21, 0x04, 0xBB]);
        assert_eq!(s.poll_notifications(Duration::from_millis(1)).unwrap().len(), 1); // the 05 frame stayed queued
    }

    #[test]
    fn request_ignores_stale_reply_queued_before_send() {
        let mut s = session(vec![vec![data(pack_ack(1))]]);
        s.transport.inbox.push_back(data(pack_raw(TYPE_T1, 0, &[0x01, 0x00, 0xEE]).unwrap()));
        s.pump(Instant::now() + Duration::from_millis(5), false).unwrap();
        assert_eq!(s.queue.len(), 1);
        assert!(matches!(s.request(&cmd(&[0, 0]), 0x01), Err(SessionError::NoReply)));
        assert_eq!(s.queue.len(), 1); // the stale frame is still there for poll_notifications
    }

    #[test]
    fn request_times_out_without_reply() {
        let mut s = session(vec![vec![data(pack_ack(1))]]);
        assert!(matches!(s.request(&cmd(&[0, 0]), 0x01), Err(SessionError::NoReply)));
    }

    #[test]
    fn fragmented_reads_are_reassembled() {
        let ack = pack_ack(1);
        let mut s = session(vec![vec![data(ack[..4].to_vec()), data(ack[4..].to_vec())]]);
        s.send(&cmd(&[0, 0])).unwrap();
    }

    #[test]
    fn close_mid_command_is_an_error() {
        let mut s = session(vec![vec![Step::Close]]);
        assert!(matches!(s.send(&cmd(&[0, 0])), Err(SessionError::Io(_))));
        assert!(matches!(s.send(&cmd(&[0, 0])), Err(SessionError::Io(_))));
    }
}

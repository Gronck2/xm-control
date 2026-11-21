//! MDR frame codec (docs/arch/protocol.md section 2).
//! `3E | esc(type, seq, len u32 BE, payload, checksum) | 3C`. Only the guard can produce a command to pack.

use super::guard::AllowedCommand;

pub const START: u8 = 0x3E;
pub const END: u8 = 0x3C;
const ESC: u8 = 0x3D;
pub const MAX_FRAME: usize = 2048;

pub const TYPE_ACK: u8 = 0x01;
pub const TYPE_T1: u8 = 0x0C;
pub const TYPE_T2: u8 = 0x0E;

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    Markers,
    BadEscape,
    TooShort,
    TooLarge,
    BadChecksum,
    LengthMismatch,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    pub data_type: u8,
    pub seq: u8,
    pub payload: Vec<u8>,
}

fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b))
}

fn escape(bytes: &[u8], out: &mut Vec<u8>) {
    for &b in bytes {
        match b {
            END => out.extend([ESC, 0x2C]),
            ESC => out.extend([ESC, 0x2D]),
            START => out.extend([ESC, 0x2E]),
            _ => out.push(b),
        }
    }
}

fn unescape(bytes: &[u8]) -> Result<Vec<u8>, FrameError> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut it = bytes.iter();
    while let Some(&b) = it.next() {
        if b != ESC {
            out.push(b);
            continue;
        }
        out.push(match it.next() {
            Some(0x2C) => END,
            Some(0x2D) => ESC,
            Some(0x2E) => START,
            _ => return Err(FrameError::BadEscape),
        });
    }
    Ok(out)
}

fn pack_raw(data_type: u8, seq: u8, payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    let mut body = vec![data_type, seq];
    body.extend((payload.len() as u32).to_be_bytes());
    body.extend(payload);
    body.push(checksum(&body));
    let mut out = vec![START];
    escape(&body, &mut out);
    out.push(END);
    if out.len() > MAX_FRAME {
        return Err(FrameError::TooLarge);
    }
    Ok(out)
}

/// Test-only access to arbitrary frames (session tests); never compiled into the binary.
#[cfg(test)]
pub(crate) fn pack_raw_for_tests(data_type: u8, seq: u8, payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    pack_raw(data_type, seq, payload)
}

/// Packs a guard-approved command. This is the only way to build a command frame.
pub fn pack_command(cmd: &AllowedCommand, seq: u8) -> Result<Vec<u8>, FrameError> {
    pack_raw(cmd.table().data_type(), seq, cmd.payload())
}

/// Packs an ACK (empty payload) carrying `seq`.
pub fn pack_ack(seq: u8) -> Vec<u8> {
    pack_raw(TYPE_ACK, seq, &[]).expect("empty frame fits")
}

/// Unpacks one whole frame, markers included. Incoming frames are never filtered by the guard.
pub fn unpack(bytes: &[u8]) -> Result<Frame, FrameError> {
    if bytes.len() > MAX_FRAME {
        return Err(FrameError::TooLarge);
    }
    match bytes {
        [START, inner @ .., END] => {
            let body = unescape(inner)?;
            if body.len() < 7 {
                return Err(FrameError::TooShort);
            }
            let (data, sum) = body.split_at(body.len() - 1);
            if checksum(data) != sum[0] {
                return Err(FrameError::BadChecksum);
            }
            let len = u32::from_be_bytes([data[2], data[3], data[4], data[5]]) as usize;
            if len != data.len() - 6 {
                return Err(FrameError::LengthMismatch);
            }
            Ok(Frame { data_type: data[0], seq: data[1], payload: data[6..].to_vec() })
        }
        _ => Err(FrameError::Markers),
    }
}

/// Streaming parser: feed bytes as they arrive, get complete valid frames; bad frames are dropped and it resyncs on the next `3E`.
#[derive(Default)]
pub struct FrameParser {
    buf: Vec<u8>,
}

impl FrameParser {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Frame> {
        self.buf.extend(bytes);
        let mut frames = Vec::new();
        loop {
            let Some(start) = self.buf.iter().position(|&b| b == START) else {
                self.buf.clear();
                break;
            };
            self.buf.drain(..start);
            // A raw 3E inside a frame is always an escape error: it is the start of the next frame.
            match self.buf[1..].iter().position(|&b| b == END || b == START) {
                Some(i) if self.buf[1 + i] == END => {
                    if let Ok(f) = unpack(&self.buf[..i + 2]) {
                        frames.push(f);
                    }
                    self.buf.drain(..i + 2);
                }
                Some(i) => {
                    self.buf.drain(..i + 1);
                }
                None if self.buf.len() > MAX_FRAME => {
                    self.buf.drain(..1);
                }
                None => break,
            }
        }
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdr::guard::{Mode, Table, check};

    fn cmd(table: Table, payload: &[u8]) -> AllowedCommand {
        check(table, payload, Mode::Normal).unwrap()
    }

    const PROTO: [u8; 11] = [0x3E, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x0E, 0x3C];
    const CAPS: [u8; 11] = [0x3E, 0x0C, 0x01, 0x00, 0x00, 0x00, 0x02, 0x02, 0x00, 0x11, 0x3C];
    const ACK_FOR_0: [u8; 9] = [0x3E, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x02, 0x3C];
    const ACK_FOR_1: [u8; 9] = [0x3E, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x3C];

    #[test]
    fn worked_frames_pack_and_unpack() {
        assert_eq!(pack_command(&cmd(Table::T1, &[0, 0]), 0).unwrap(), PROTO);
        assert_eq!(pack_command(&cmd(Table::T1, &[2, 0]), 1).unwrap(), CAPS);
        assert_eq!(pack_ack(1), ACK_FOR_0);
        assert_eq!(pack_ack(0), ACK_FOR_1);
        assert_eq!(unpack(&PROTO).unwrap(), Frame { data_type: TYPE_T1, seq: 0, payload: vec![0, 0] });
        assert_eq!(unpack(&CAPS).unwrap().seq, 1);
        assert_eq!(unpack(&ACK_FOR_0).unwrap(), Frame { data_type: TYPE_ACK, seq: 1, payload: vec![] });
    }

    #[test]
    fn escaping_round_trip() {
        for b in 0..=255u8 {
            let packed = pack_raw(TYPE_T1, 0, &[b, 0x3C, 0x3D, 0x3E]).unwrap();
            assert!(!packed[1..packed.len() - 1].iter().any(|&x| x == START || x == END));
            assert_eq!(unpack(&packed).unwrap().payload, vec![b, 0x3C, 0x3D, 0x3E]);
        }
        // payload 0x2F makes the checksum 0x3C, which must be escaped too
        let packed = pack_raw(TYPE_T1, 0, &[0x2F]).unwrap();
        assert_eq!(&packed[packed.len() - 3..], &[ESC, 0x2C, END]);
        assert_eq!(unpack(&packed).unwrap().payload, vec![0x2F]);
    }

    #[test]
    fn invalid_frames_rejected() {
        assert_eq!(unpack(&[START, 0x0C, ESC, 0x00, END]), Err(FrameError::BadEscape));
        assert_eq!(unpack(&[START, 0x0C, ESC, END]), Err(FrameError::BadEscape));
        let mut bad_sum = PROTO;
        bad_sum[9] ^= 1;
        assert_eq!(unpack(&bad_sum), Err(FrameError::BadChecksum));
        // len says 3 but payload holds 2; checksum fixed up so only the length is wrong
        let bad_len = [0x3E, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x0F, 0x3C];
        assert_eq!(unpack(&bad_len), Err(FrameError::LengthMismatch));
        assert_eq!(unpack(&[START, 1, 2, 3, 4, 5, 6, END]), Err(FrameError::TooShort));
        assert_eq!(unpack(&[1, 2, 3]), Err(FrameError::Markers));
        assert_eq!(unpack(&vec![0u8; MAX_FRAME + 1]), Err(FrameError::TooLarge));
    }

    #[test]
    fn oversize_payload_rejected_at_pack() {
        assert_eq!(pack_raw(TYPE_T1, 0, &vec![0u8; MAX_FRAME]), Err(FrameError::TooLarge));
    }

    #[test]
    fn parser_split_across_reads() {
        let mut p = FrameParser::default();
        assert!(p.push(&PROTO[..4]).is_empty());
        assert!(p.push(&PROTO[4..10]).is_empty());
        assert_eq!(p.push(&PROTO[10..]).len(), 1);
    }

    #[test]
    fn parser_two_frames_in_one_read() {
        let mut p = FrameParser::default();
        let both = [PROTO.as_slice(), ACK_FOR_0.as_slice()].concat();
        let frames = p.push(&both);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].data_type, TYPE_ACK);
    }

    #[test]
    fn parser_garbage_before_start() {
        let mut p = FrameParser::default();
        let data = [&[0x00, 0x3C, 0x55][..], &PROTO].concat();
        assert_eq!(p.push(&data).len(), 1);
    }

    #[test]
    fn parser_resyncs_after_bad_frame() {
        let mut p = FrameParser::default();
        let mut bad_sum = PROTO;
        bad_sum[9] ^= 1;
        let data = [bad_sum.as_slice(), &CAPS].concat();
        let frames = p.push(&data);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].seq, 1);
        // truncated frame followed by a good one: the raw 3E restarts parsing
        let data = [&PROTO[..6], &CAPS].concat();
        assert_eq!(p.push(&data).len(), 1);
    }

    #[test]
    fn parser_survives_unterminated_flood() {
        let mut p = FrameParser::default();
        p.push(&[START]);
        p.push(&vec![0x0C; MAX_FRAME + 10]);
        assert_eq!(p.push(&PROTO).len(), 1);
    }
}

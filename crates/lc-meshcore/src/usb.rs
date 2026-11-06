//! USB / serial framing (`oxymesh/docs/spec/companion.md` §1.2): app → device `'<' ‖ len u16 ‖
//! frame`, device → app `'>' ‖ len u16 ‖ frame`. Over BLE, one write or notification is one
//! frame and needs no framing.

use crate::{EncodeError, FRAME_MAX};

const TO_DEVICE: u8 = b'<';
const FROM_DEVICE: u8 = b'>';

/// Wraps `frame` for the device into `out`; returns the written length.
pub fn encode(frame: &[u8], out: &mut [u8]) -> Result<usize, EncodeError> {
    if frame.is_empty() || frame.len() > FRAME_MAX {
        return Err(EncodeError::Invalid);
    }
    let total = frame.len() + 3;
    let dst = out.get_mut(..total).ok_or(EncodeError::Overflow)?;
    let len = (frame.len() as u16).to_le_bytes();
    let (head, body) = dst.split_at_mut(3);
    head.copy_from_slice(&[TO_DEVICE, len[0], len[1]]);
    body.copy_from_slice(frame);
    Ok(total)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Len0,
    Len1(u8),
    Body { len: usize, got: usize },
}

/// Streaming decoder for device → app frames. Bytes before `'>'` are skipped; a frame longer
/// than 176 bytes is read and dropped.
#[derive(Debug)]
pub struct Decoder {
    buf: [u8; FRAME_MAX],
    state: State,
    ready: usize,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub const fn new() -> Self {
        Self {
            buf: [0; FRAME_MAX],
            state: State::Idle,
            ready: 0,
        }
    }

    /// Feeds one byte. Returns `true` when a complete frame is available in [`Decoder::frame`];
    /// it stays there until the next call to `push`.
    pub fn push(&mut self, byte: u8) -> bool {
        self.ready = 0;
        self.state = match self.state {
            State::Idle if byte == FROM_DEVICE => State::Len0,
            State::Idle => State::Idle,
            State::Len0 => State::Len1(byte),
            State::Len1(lo) => {
                let len = usize::from(u16::from_le_bytes([lo, byte]));
                if len == 0 {
                    State::Idle
                } else {
                    State::Body { len, got: 0 }
                }
            }
            State::Body { len, got } => {
                if let Some(slot) = self.buf.get_mut(got) {
                    *slot = byte;
                }
                let got = got + 1;
                if got < len {
                    State::Body { len, got }
                } else {
                    if len <= FRAME_MAX {
                        self.ready = len;
                    }
                    State::Idle
                }
            }
        };
        self.ready > 0
    }

    /// The last complete frame, empty if none.
    pub fn frame(&self) -> &[u8] {
        self.buf.get(..self.ready).unwrap_or(&[])
    }
}

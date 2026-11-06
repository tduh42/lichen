//! The 7-byte lichen frame header (§3.1) and frame-level validation.

use crate::{HEADER_LEN, TAG_LEN};

/// Frame family (header byte 4, bits 7-6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Data = 0,
    Ack = 1,
    Abort = 2,
    Control = 3,
}

impl Kind {
    const fn from_bits(b: u8) -> Self {
        match b >> 6 {
            0 => Kind::Data,
            1 => Kind::Ack,
            2 => Kind::Abort,
            _ => Kind::Control,
        }
    }
}

/// Why a frame was dropped. Never answered on the network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// Shorter than the smallest frame of its kind, or longer than 184 bytes.
    Length,
    /// Field values that the spec forbids.
    Malformed,
    /// `dst` is not our hash.
    NotForUs,
    /// The AEAD tag does not verify (or no session or key matches).
    Auth,
    /// Valid but out of the replay window, or about an unknown datagram.
    Stale,
    /// The output buffer is too small.
    Overflow,
}

/// The decoded header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub dst: u8,
    pub src: u8,
    pub tag: u16,
    pub kind: Kind,
    pub index: u8,
    pub total: u8,
    pub attempt: u8,
}

/// Lengths per kind: ACK and ABORT are fixed.
pub const ACK_FRAME_LEN: usize = HEADER_LEN + 9 + TAG_LEN;
pub const ABORT_FRAME_LEN: usize = HEADER_LEN + 1 + TAG_LEN;
/// Largest on-air payload.
const AIR_PAYLOAD_MAX: usize = 184;

impl Header {
    pub const fn encode(&self) -> [u8; HEADER_LEN] {
        let tag = self.tag.to_le_bytes();
        [
            self.dst,
            self.src,
            tag[0],
            tag[1],
            ((self.kind as u8) << 6) | (self.index & 0x3F),
            self.total,
            self.attempt,
        ]
    }

    /// The 96-bit nonce: `header[2..7] ‖ 0×7` (§5).
    pub const fn nonce(&self) -> [u8; 12] {
        let h = self.encode();
        [h[2], h[3], h[4], h[5], h[6], 0, 0, 0, 0, 0, 0, 0]
    }

    /// Splits a frame into its header and body, applying the length and field rules of §3.1.
    pub fn decode(frame: &[u8]) -> Result<(Header, &[u8]), FrameError> {
        if frame.len() < HEADER_LEN + 1 + TAG_LEN || frame.len() > AIR_PAYLOAD_MAX {
            return Err(FrameError::Length);
        }
        let (h, body) = frame.split_at(HEADER_LEN);
        let [dst, src, t0, t1, ki, total, attempt] = h else {
            return Err(FrameError::Length);
        };
        let header = Header {
            dst: *dst,
            src: *src,
            tag: u16::from_le_bytes([*t0, *t1]),
            kind: Kind::from_bits(*ki),
            index: ki & 0x3F,
            total: *total,
            attempt: *attempt,
        };
        if header.total == 0 || usize::from(header.total) > crate::MAX_FRAGMENTS {
            return Err(FrameError::Malformed);
        }
        let ok = match header.kind {
            Kind::Data => header.index < header.total,
            Kind::Ack => header.index == 0 && frame.len() == ACK_FRAME_LEN,
            Kind::Abort => header.index <= 1 && frame.len() == ABORT_FRAME_LEN,
            Kind::Control => header.total == 1 && header.index <= 2,
        };
        if !ok {
            return Err(FrameError::Malformed);
        }
        Ok((header, body))
    }
}

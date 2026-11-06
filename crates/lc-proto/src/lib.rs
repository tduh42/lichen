//! The lichen protocol (`docs/spec/protocol.md`): 7-byte frame header, ChaCha20-Poly1305 with
//! an 8-byte tag, HKDF handshake with forward secrecy, and the ACK-on-Error fragmentation and
//! reassembly machines.
//!
//! Pure state machines: no IO, no clock, no randomness of their own (time in milliseconds and
//! random bytes are passed in). `no_std`, no allocation, no floats, panic-free on any input.
#![no_std]
#![deny(clippy::float_arithmetic)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod aead;
pub mod discovery;
pub mod group;
pub mod handshake;
pub mod header;
pub mod kdf;
pub mod location;
pub mod receiver;
pub mod sender;
pub mod session;
pub mod window;

pub use header::{FrameError, Header, Kind};
pub use session::{Received, Session, SessionEvent, SessionKeys};

/// A 32-byte symmetric key.
pub type Key = [u8; 32];
/// Time in milliseconds, from any monotonic origin chosen by the runner.
pub type Millis = u64;

/// Header length.
pub const HEADER_LEN: usize = 7;
/// Truncated AEAD tag length.
pub const TAG_LEN: usize = 8;
/// Header plus tag.
pub const OVERHEAD: usize = HEADER_LEN + TAG_LEN;
/// Largest lichen frame we send: the largest `RAW_DATA` payload a companion can push (172).
pub const FRAME_MAX: usize = 172;
/// Largest fragment plaintext (`FRAME_MAX − OVERHEAD`).
pub const CHUNK_MAX: usize = FRAME_MAX - OVERHEAD;
/// Maximum number of fragments in a datagram.
pub const MAX_FRAGMENTS: usize = 64;
/// Largest datagram.
pub const DATAGRAM_MAX: usize = MAX_FRAGMENTS * CHUNK_MAX;

/// Fragment plaintext size for a path of `path_bytes` bytes (§3.2).
pub const fn chunk_for_path(path_bytes: usize) -> usize {
    lc_meshcore::command::raw_payload_max(path_bytes) - OVERHEAD
}

/// Abort reasons (1-byte ABORT plaintext).
pub mod reason {
    pub const TIMEOUT: u8 = 1;
    pub const TOO_LARGE: u8 = 2;
    pub const MALFORMED: u8 = 3;
    pub const CANCELLED: u8 = 4;
}

/// Datagram content types (first plaintext byte, §6).
pub mod content {
    pub const TEXT: u8 = 0x01;
    pub const FILE: u8 = 0x02;
    pub const LOCATION: u8 = 0x03;
    pub const ANNOUNCE: u8 = 0x04;
    pub const VOICE: u8 = 0x05;
    pub const MEDIA_REQUEST: u8 = 0x06;
    pub const SENDER_KEY: u8 = 0x10;
    pub const GROUP_STATE: u8 = 0x11;
}

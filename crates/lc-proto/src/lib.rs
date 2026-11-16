//! The lichen protocol (`docs/spec/protocol.md`): 7-byte frame header, ChaCha20-Poly1305 (RFC 8439,
//! 16-byte tag), HKDF handshake with forward secrecy, and the ACK-on-Error fragmentation and
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
pub mod intro;
pub mod kdf;
pub mod location;
pub mod receiver;
pub mod search;
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
/// AEAD tag length: the full Poly1305 tag (ADR-0028).
pub const TAG_LEN: usize = 16;
/// Header plus tag.
pub const OVERHEAD: usize = HEADER_LEN + TAG_LEN;
/// Largest lichen frame: the largest `RAW_DATA` payload that crosses a Bluetooth link whole
/// (`raw_payload_max(0)`, 169; ADR-0031).
pub const FRAME_MAX: usize = lc_meshcore::command::raw_payload_max(0);
/// Largest fragment plaintext (`FRAME_MAX − OVERHEAD`, 146).
pub const CHUNK_MAX: usize = FRAME_MAX - OVERHEAD;
/// Maximum number of fragments in a datagram.
pub const MAX_FRAGMENTS: usize = 64;
/// Largest datagram (9,344 bytes).
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
    pub const SEARCH: u8 = 0x07;
    pub const REPLY: u8 = 0x08;
    pub const REACTION: u8 = 0x09;
    pub const STICKER: u8 = 0x0A;
    pub const GIF: u8 = 0x0B;
    pub const EDIT: u8 = 0x0C;
    pub const DELETE: u8 = 0x0D;
    pub const TIMER: u8 = 0x0E;
    pub const SENDER_KEY: u8 = 0x10;
    pub const GROUP_STATE: u8 = 0x11;
    pub const GROUP_LEAVE: u8 = 0x12;
    pub const GROUP_JOIN: u8 = 0x13;
    pub const SENDER_KEYS: u8 = 0x14;
    pub const GROUP_MISSING: u8 = 0x15;
    pub const GROUP_RESEND: u8 = 0x16;
    /// §10.7: a member asks the admin for the keys.
    pub const GROUP_SYNC: u8 = 0x17;
}

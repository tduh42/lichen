//! App side of the MeshCore v1.18 companion protocol: the subset lichen needs
//! (`docs/spec/protocol.md` §1), USB framing, on-air frame header, MeshCore-style ECDH and LoRa
//! airtime.
//!
//! MeshCore facts come from oxymesh's verified documentation (ADR-0001). `no_std`, no
//! allocation, no floats; zero-copy decoders that never panic.
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

pub mod air;
pub mod airtime;
mod bytes;
pub mod codes;
pub mod command;
pub mod ecdh;
pub mod path;
pub mod response;
pub mod usb;

pub use air::{Packet, PayloadType, Route};
pub use airtime::LoRaParams;
pub use command::{Command, OtherParams};
pub use path::Path;
pub use response::{Contact, DeviceInfo, Response, SelfInfo};

/// Maximum companion-protocol frame size, both directions (`MAX_FRAME_SIZE`).
pub const FRAME_MAX: usize = 176;

/// Largest frame a Bluetooth link carries whole (ADR-0031): ESP32 companions set the ATT MTU
/// to `MAX_FRAME_SIZE` (176) and a notification holds MTU − 3 bytes; a longer frame reaches
/// the app cut short (arduino-esp32 `BLECharacteristic::notify`). Every frame lichen relies
/// on, both ways, stays within it.
pub const BLE_FRAME_MAX: usize = FRAME_MAX - 3;

/// Decoding error: empty, truncated or inconsistent frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Empty,
    Truncated,
    Invalid,
}

/// Encoding error: the frame does not fit the buffer, or a field is out of range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    Overflow,
    Invalid,
}

//! lichen application core (`docs/spec/protocol.md`): drives a stock MeshCore companion,
//! manages contacts, paths, handshakes, sessions, the send queue and the airtime budget, and
//! reports events to the app. IO-free; the apps (through `lc-ffi`) and `lc-cli` are runners.
//!
//! [`fakenet`] simulates a small MeshCore network (companions and repeaters, half duplex,
//! losses, collisions, duplicate filters) to test the whole stack without radios.

pub mod budget;
pub mod content;
pub mod core;
pub mod fakenet;
pub mod group;
pub mod location;
pub mod store;

pub use crate::core::{Config, Contact, Core, Error, Event, FailReason, MsgId, OwnedPath, PubKey};
pub use content::Content;
pub use group::{Group, GroupId, Member};
pub use lc_proto::Millis;
pub use location::{Fix, Precision, Share};
pub use store::Chat;

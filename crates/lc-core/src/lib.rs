//! lichen application core (`docs/spec/protocol.md`): drives a stock MeshCore companion,
//! manages contacts, paths, handshakes, sessions, the send queue and the airtime budget, and
//! reports events to the app. IO-free; the apps (through `lc-ffi`) and `lc-cli` are runners.
//!
//! [`fakenet`] simulates a small MeshCore network (companions and repeaters, half duplex,
//! losses, collisions, duplicate filters) to test the whole stack without radios.

pub mod backup;
pub mod budget;
pub mod cipher;
pub mod content;
pub mod core;
pub mod fakenet;
pub mod group;
pub mod identity;
pub mod import;
pub mod location;
pub mod mesh;
pub mod plain;
pub mod radio;
pub mod search;
mod secret;
pub mod server;
pub mod solver;
pub mod store;

pub use crate::core::{
    Config, Contact, Core, Error, Event, FailReason, MsgId, Outbound, OwnedPath, PubKey, Stage,
    contact_kind,
};
pub use content::{Announce, Content};
pub use group::{Group, GroupId, Member};
pub use lc_proto::Millis;
pub use location::{Fix, Precision, Share, SosStatus};
pub use mesh::{Hop, Node, Trace, TraceStatus};
pub use plain::{Channel, ChannelId, PlainChat, PlainMessage};
pub use search::{Measurement, SearchInfo, TargetFix};
pub use server::{ConsoleLine, Login, Neighbour, Role, Server};
pub use solver::{Estimate, Heat, PathLoss};
pub use store::Chat;

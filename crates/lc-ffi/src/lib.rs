//! UniFFI bindings of the lichen core (`lc-core`) for the iOS and Android apps.
//!
//! The app is the runner: it passes companion frames and the time in, writes the frames
//! `poll_companion` returns to the companion (BLE or USB), calls `tick` at `next_wakeup`, and
//! reads events. Keys and ids cross the boundary as byte arrays.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use lc_core::content::{MessageRef, REACTION_MAX, message_ref};
use lc_core::location::{self as loc, EXTRAPOLATE_MAX_S};
use lc_core::store::{self, Chat, Status, Store, StoredPosition};
use lc_core::{
    ChannelId, Config, Content, Core, Event, FailReason as CoreFail, Fix, GroupId, Outbound,
    PlainChat, Precision, PubKey, Stage,
};
use lc_proto::location::{ALT_UNKNOWN, HEADING_UNKNOWN, SPEED_UNKNOWN};
use lc_proto::search::BearingClass;

mod bridge;
mod demo;

pub use bridge::{BridgeError, LichenBridge, Offered, OnionStatus, onion_address};
pub use demo::DemoNetwork;

uniffi::setup_scaffolding!();

mod amendments;
mod api_contacts;
mod api_groups;
mod api_history;
mod api_identity;
mod api_location;
mod api_plain;
mod api_replies;
mod api_runtime;
mod api_search;
mod api_send;
mod chat;
mod error;
mod event;
mod handle;
mod inner;
mod message;
mod on_event;
mod outbox;
mod position;
mod records;
mod tools;
#[cfg(test)]
mod unit;

pub use chat::*;
pub use error::*;
pub use event::*;
pub use handle::*;
pub(crate) use inner::*;
pub use message::*;
pub use position::*;
pub use records::*;
pub use tools::*;

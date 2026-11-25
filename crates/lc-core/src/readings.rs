//! A node's readings (protocol §16.7, ADR-0050): its battery, its temperature and what its
//! sensors measure, asked with `SEND_TELEMETRY_REQ` through the queue of server requests
//! (§16.5) and answered in CayenneLPP ([`lc_meshcore::lpp`]).
//!
//! The runner stores what the core reports ([`crate::Event::NodeReadings`]): the decoded items
//! of each answer as sent ([`crate::store::Store::put_readings`]), read back with
//! [`Readings::decode`].

use lc_meshcore::lpp::{self, Item, Kind, SELF_CHANNEL, Value};

use crate::core::{Core, Error, PubKey};
use lc_proto::Millis;

/// Readings are kept this long (§16.7)...
pub const KEEP_S: u64 = 7 * 86_400;
/// ...and this many per radio and node at most, the newest.
pub const KEEP_MAX: usize = 100;

/// Where asking a node for its readings stands (§16.7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReadingsState {
    /// Not asked since the radio connected.
    #[default]
    Idle,
    /// Asked: waiting for the radio or for the node.
    Asking,
    /// It answered with readings.
    Answered,
    /// It answered without a reading lichen reads: nothing kept.
    AnsweredEmpty,
    /// No answer in time (the node does not answer us, or it was out of reach).
    NoAnswer,
    /// The radio refused to ask (the node is not one of its contacts).
    Refused,
}

/// One answer, decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Readings {
    /// When it came, Unix seconds by the phone's clock.
    pub at_s: u64,
    /// The node's battery: the voltage of channel 1, millivolts.
    pub battery_mv: Option<i64>,
    /// Its temperature: channel 1's, else the first one; tenths of a degree Celsius.
    pub temperature_dc: Option<i64>,
    /// Every item, in order (the battery and temperature above included).
    pub items: Vec<Item>,
}

impl Readings {
    /// The readings in `data` (CayenneLPP), received at `at_s`.
    pub fn decode(data: &[u8], at_s: u64) -> Self {
        let items: Vec<Item> = lpp::items(data).collect();
        let scalar = |i: &Item, want: Kind| match i.value {
            Value::Scalar { kind, value } if kind == want => Some(value),
            _ => None,
        };
        let battery_mv = items
            .iter()
            .filter(|i| i.channel == SELF_CHANNEL)
            .find_map(|i| scalar(i, Kind::Voltage));
        let temperature_dc = items
            .iter()
            .filter(|i| i.channel == SELF_CHANNEL)
            .find_map(|i| scalar(i, Kind::Temperature))
            .or_else(|| items.iter().find_map(|i| scalar(i, Kind::Temperature)));
        Self {
            at_s,
            battery_mv,
            temperature_dc,
            items,
        }
    }

    /// The battery's charge by the radio's curve (§16.4: 3.3 V empty, 4.2 V full).
    pub fn battery_percent(&self) -> Option<u8> {
        self.battery_mv
            .map(|mv| crate::radio::battery_percent(u16::try_from(mv.max(0)).unwrap_or(u16::MAX)))
    }
}

impl Core {
    /// Asks `node` for its readings (§16.7): queued with the server requests, one per node at a
    /// time (`TooSoon` while one is out). `UnknownContact` for a node not in the radio's
    /// contacts, `NoIdentity` without a radio, `OverBudget` when the request does not fit the
    /// airtime left. [`crate::Event::NodeReadings`] brings the answer; `NodeTool` each change of
    /// [`Core::readings_state`].
    pub fn request_readings(&mut self, node: &PubKey, now: Millis) -> Result<(), Error> {
        if !self.online {
            return Err(Error::NoIdentity);
        }
        let path = self
            .contacts
            .get(node)
            .filter(|_| self.reported_by_radio(node))
            .ok_or(Error::UnknownContact)?
            .out_path
            .clone();
        let bytes = path.as_ref().map_or(0, |p| p.bytes.len());
        let airtime = self.airtime_ms(2 + bytes + 2 + 2 + 16);
        if !self.budget_allows(now, airtime) {
            return Err(Error::OverBudget {
                wait_ms: self.budget_wait(now, airtime),
            });
        }
        self.queue_readings(node, now)
    }

    /// Where asking `node` for its readings stands.
    pub fn readings_state(&self, node: &PubKey) -> ReadingsState {
        self.servers.readings.get(node).copied().unwrap_or_default()
    }
}

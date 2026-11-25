//! Mesh tools (protocol §16, ADR-0041): the nodes the radio knows or has heard, with their
//! type, when they were last heard and the position they advertise; a contact's path, hop by
//! hop; traces.
//!
//! The radio is the source of truth: everything here is read again at each connection, and
//! nothing is stored but what the radio heard of each node (§16.1, ADR-0049): the runner
//! records the readings and gives the last ones back ([`Core::restore_signal`]).

use std::collections::HashMap;

use lc_meshcore::Command;
use lc_meshcore::advert::Advert;
use lc_meshcore::air::{Packet, PayloadType, Route as AirRoute};
use lc_meshcore::response::Contact as RadioContact;
use rand_core::Rng;

use crate::core::{Core, Error, Event, Expect, OwnedPath, PubKey, contact_kind};
use crate::signal::{Reading, SignalRating};
use lc_proto::Millis;

/// Nodes heard but not in the radio's contacts, kept at most (the newest).
const HEARD_MAX: usize = 512;
/// No `TRACE_DATA` within the companion's estimate plus this: no answer (§16.3).
const TRACE_MARGIN_MS: Millis = 3_000;
/// Without a `SENT`, a trace is given up after this.
const TRACE_SENT_MS: Millis = 10_000;
/// The longest `TRACE_DATA` that reaches us whole over Bluetooth (ADR-0031): its frame is
/// `13 + hashes + hops` bytes.
const TRACE_PUSH_MAX: usize = lc_meshcore::BLE_FRAME_MAX;

/// One hop of a path: its hash, and the repeaters whose key starts with it (§16.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hop {
    pub hash: Vec<u8>,
    /// Names of the matching repeaters the radio knows; empty when none, several when the
    /// hash is ambiguous.
    pub names: Vec<String>,
}

/// Where a trace stands (§16.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceStatus {
    /// Handed to the radio, waiting for it to come back.
    Sending,
    /// It came back: every hop's SNR is known.
    Done,
    /// It did not come back in time (a hop did not hear it, or the radio was disconnected).
    NoAnswer,
    /// The radio refused it.
    Refused,
}

/// A trace along a contact's path and back (§16.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trace {
    /// The repeaters traced, out and back.
    pub hops: Vec<Hop>,
    /// SNR × 4 at which each hop heard the trace (as many as `hops` once done).
    pub snrs: Vec<i8>,
    /// SNR × 4 at which our radio heard the last hop.
    pub final_snr4: Option<i8>,
    pub status: TraceStatus,
    /// Round trip, once done.
    pub rtt_ms: Option<Millis>,
    /// Airtime of our transmission.
    pub airtime_ms: Millis,
    /// When it came back, Unix seconds by the phone's clock (when known).
    pub done_s: Option<u64>,
    tag: u32,
    started: Millis,
    deadline: Millis,
}

/// A MeshCore node, as the radio reported it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub key: PubKey,
    /// The name it advertises.
    pub name: String,
    /// [`contact_kind`]: companion, repeater, room server, sensor.
    pub kind: u8,
    /// The radio's path to it; `None` when it floods.
    pub out_path: Option<OwnedPath>,
    /// When it last advertised, by its own clock (Unix seconds); 0 when unknown.
    pub last_advert: u32,
    /// When the radio last heard from it, by the radio's clock (Unix seconds); 0 when unknown.
    pub last_heard: u32,
    /// The position its adverts carry (degrees × 10⁶): only for a node that advertises one.
    pub position: Option<(i32, i32)>,
    /// In the radio's contacts; false for a node heard but not added (manual add, a hop
    /// limit, or a full table: `NEW_ADVERT`).
    pub on_radio: bool,
    /// The last time our radio heard it directly (§16.1); `None` for a node heard only
    /// through repeaters. Rate it with [`Core::signal_rating`].
    pub signal: Option<Reading>,
}

/// A stop on the route to a node (§16.3): our radio, a repeater of its path, or the node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteStop {
    /// The node, when known: our radio's, the node's, or the one repeater the hash names.
    pub key: Option<PubKey>,
    /// A repeater's hash on the path; empty for our radio and for the node.
    pub hash: Vec<u8>,
    /// Its name: our radio's, the node's, or those of the repeaters the hash names (none when
    /// unknown, several when ambiguous).
    pub names: Vec<String>,
    /// The position its adverts carry; none for our radio (the apps put it at the phone's).
    pub position: Option<(i32, i32)>,
}

/// A leg of the route between two stops: the last SNR measured on it towards us, the nearer
/// stop hearing the farther one (§16.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteLeg {
    /// `None`: nothing measured it.
    pub reading: Option<Reading>,
    /// The reading comes from a trace (no RSSI), not from the frames heard.
    pub traced: bool,
}

/// The route to a node (§16.3): our radio first, the node last, a leg between each two.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub stops: Vec<RouteStop>,
    pub legs: Vec<RouteLeg>,
}

/// What a contact record says beyond [`crate::Contact`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Seen {
    last_advert: u32,
    last_heard: u32,
    position: Option<(i32, i32)>,
}

/// The mesh tools' state.
#[derive(Debug, Default)]
pub(crate) struct Mesh {
    seen: HashMap<PubKey, Seen>,
    heard: HashMap<PubKey, Node>,
    /// The last trace per node.
    traces: HashMap<PubKey, Trace>,
    /// The last direct reading per node, for the connected radio (§16.1).
    signals: HashMap<PubKey, Reading>,
    /// When the radio's battery was last asked for (ADR-0042).
    pub(crate) battery_at: Option<Millis>,
    /// Nodes imported from a link, with the `ADD_UPDATE_CONTACT` frame that adds them when
    /// the radio does not take the advert (§16.6).
    adding: HashMap<PubKey, Vec<u8>>,
}

/// What adding a node from a link did (§16.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshAdd {
    /// Sent to the radio: the contact comes as any other (`ContactsUpdated`), or the radio's
    /// refusal as a refused setting ([`crate::radio::RadioSetting::Contact`], outcome `Refused`).
    Sent,
    /// The radio already reported it: nothing sent.
    AlreadyOnTheRadio,
    /// The radio itself.
    OwnRadio,
}

/// An advertised position: MeshCore leaves both coordinates at 0 when the advert has none.
fn position(lat_e6: i32, lon_e6: i32) -> Option<(i32, i32)> {
    let valid = (-90_000_000..=90_000_000).contains(&lat_e6)
        && (-180_000_000..=180_000_000).contains(&lon_e6);
    (valid && (lat_e6, lon_e6) != (0, 0)).then_some((lat_e6, lon_e6))
}

fn seen(c: &RadioContact<'_>) -> Seen {
    Seen {
        last_advert: c.last_advert,
        last_heard: c.lastmod,
        position: position(c.lat_e6, c.lon_e6),
    }
}

impl Core {
    /// Every node the radio reported since the connection: its contacts, and the nodes it
    /// heard without adding them; the most recently heard first (§16.1). Contacts lichen keeps
    /// that the radio did not report (people known from a group) are not nodes it knows.
    pub fn nodes(&self) -> Vec<Node> {
        let mut v: Vec<Node> = self
            .contacts
            .values()
            .filter_map(|c| {
                let s = self.mesh.seen.get(&c.key).copied()?;
                Some(Node {
                    key: c.key,
                    name: c.name.clone(),
                    kind: c.kind,
                    out_path: c.out_path.clone(),
                    last_advert: s.last_advert,
                    last_heard: s.last_heard,
                    position: s.position,
                    on_radio: true,
                    signal: self.mesh.signals.get(&c.key).copied(),
                })
            })
            .collect();
        v.extend(
            self.mesh
                .heard
                .values()
                .filter(|n| !self.contacts.contains_key(&n.key))
                .map(|n| Node {
                    signal: self.mesh.signals.get(&n.key).copied(),
                    ..n.clone()
                }),
        );
        v.sort_by(|a, b| {
            b.last_heard
                .cmp(&a.last_heard)
                .then_with(|| a.name.cmp(&b.name))
        });
        v
    }

    /// A contact record from the radio (`CONTACT`): when it was heard, where it is.
    pub(crate) fn mesh_contact(&mut self, c: &RadioContact<'_>) {
        if self.contacts.contains_key(c.key) {
            self.mesh.seen.insert(*c.key, seen(c));
            self.mesh.heard.remove(c.key);
        }
    }

    /// A node heard but not added (`NEW_ADVERT`).
    pub(crate) fn mesh_heard(&mut self, c: &RadioContact<'_>) {
        if self.contacts.contains_key(c.key) {
            self.mesh_contact(c);
            self.events.push_back(Event::NodesUpdated);
            return;
        }
        if !self.mesh.heard.contains_key(c.key) && self.mesh.heard.len() >= HEARD_MAX {
            // The node heard longest ago makes room.
            if let Some(oldest) = self
                .mesh
                .heard
                .values()
                .min_by_key(|n| n.last_heard)
                .map(|n| n.key)
            {
                self.mesh.heard.remove(&oldest);
            }
        }
        let s = seen(c);
        self.mesh.heard.insert(
            *c.key,
            Node {
                key: *c.key,
                name: crate::identity::shown_name(&String::from_utf8_lossy(c.name)),
                kind: c.kind,
                out_path: c.out_path.map(OwnedPath::from_path),
                last_advert: s.last_advert,
                last_heard: s.last_heard,
                position: s.position,
                on_radio: false,
                signal: None,
            },
        );
        self.events.push_back(Event::NodesUpdated);
    }

    /// Another radio: what it heard of each node is its own (§16.1).
    pub(crate) fn mesh_radio_changed(&mut self) {
        self.mesh.signals.clear();
    }

    /// A frame the radio heard (`LOG_RX_DATA`): a reading of its sender when it came straight
    /// from it and names it (§16.1, ADR-0049).
    pub(crate) fn hear_signal(&mut self, p: &Packet<'_>, snr4: i8, rssi: i8, now: Millis) {
        // A flood gains a hash at each repeater; a direct packet loses one, so an empty path
        // there may come from the last repeater: only an advert is taken from it.
        if p.path.hops() != 0 {
            return;
        }
        let flood = matches!(p.route, AirRoute::Flood | AirRoute::TransportFlood);
        let from = match p.ptype {
            PayloadType::Advert => self.advert_sender(p.payload),
            PayloadType::Req | PayloadType::Response | PayloadType::TxtMsg | PayloadType::Path
                if flood =>
            {
                p.payload.get(1).and_then(|&h| self.only_node_with(h))
            }
            _ => None,
        };
        let (Some(node), Some(at_s)) = (from, self.unix_s(now)) else {
            return;
        };
        let reading = Reading {
            snr4,
            rssi: (rssi != 0).then_some(rssi),
            at_s,
        };
        self.mesh.signals.insert(node, reading);
        self.events.push_back(Event::NodeSignal { node, reading });
    }

    /// The node an advert comes from: its signature verifies, it is not our radio's, and it is
    /// not older than the last one the radio reported for that node (a copy played again).
    fn advert_sender(&self, payload: &[u8]) -> Option<PubKey> {
        let a = Advert::decode(payload).ok()?;
        if self.node_key() == Some(a.key) || !a.verify() {
            return None;
        }
        let last = self
            .mesh
            .seen
            .get(a.key)
            .map(|s| s.last_advert)
            .or_else(|| self.mesh.heard.get(a.key).map(|n| n.last_advert))
            .unwrap_or(0);
        (a.timestamp >= last).then_some(*a.key)
    }

    /// The one node lichen knows whose key starts with `hash` (our radio, its contacts and
    /// ours, the nodes it heard); none when there are several.
    fn only_node_with(&self, hash: u8) -> Option<PubKey> {
        let mut keys = self
            .node_key()
            .into_iter()
            .chain(self.contacts.keys())
            .chain(self.mesh.heard.keys())
            .filter(|k| k.first() == Some(&hash));
        let first = *keys.next()?;
        (keys.all(|k| *k == first) && self.node_key() != Some(&first)).then_some(first)
    }

    /// The radio is another one, or reconnects: what it reports is read again.
    pub(crate) fn mesh_connected(&mut self) {
        self.mesh.seen.clear();
        self.mesh.heard.clear();
        self.mesh.adding.clear();
    }

    /// Adds the node of a link to the radio (§16.6, ADR-0047): a signed advert with
    /// `IMPORT_CONTACT`, then `ADD_UPDATE_CONTACT` if the radio did not take it; a contact
    /// code with `ADD_UPDATE_CONTACT`. Each is followed by `GET_CONTACT_BY_KEY`. `NoIdentity`
    /// without a radio.
    pub fn add_meshcore_contact(
        &mut self,
        link: &crate::plain::MeshLink,
    ) -> Result<MeshAdd, Error> {
        if !self.online {
            return Err(Error::NoIdentity);
        }
        if self.node == Some(link.key) {
            return Ok(MeshAdd::OwnRadio);
        }
        if self.mesh.seen.contains_key(&link.key) {
            return Ok(MeshAdd::AlreadyOnTheRadio);
        }
        let (lat_e6, lon_e6) = link.position.unwrap_or((0, 0));
        let mut buf = [0u8; lc_meshcore::FRAME_MAX];
        let n = Command::AddUpdateContact {
            key: &link.key,
            kind: link.kind,
            name: link.name.as_bytes(),
            last_advert: link.timestamp,
            lat_e6,
            lon_e6,
        }
        .encode(&mut buf)
        .map_err(|_| Error::Invalid)?;
        let add = buf.get(..n).unwrap_or(&[]).to_vec();
        let get = Command::GetContactByKey { key: &link.key };
        match &link.advert {
            Some(packet) => {
                let mut frame = [0u8; lc_meshcore::FRAME_MAX];
                let m = (Command::ImportContact { packet })
                    .encode(&mut frame)
                    .map_err(|_| Error::Invalid)?;
                let import = frame.get(..m).unwrap_or(&[]).to_vec();
                self.commands.push_back((import, Expect::Ok));
                self.command(&get, Expect::LinkImported(link.key));
                self.mesh.adding.insert(link.key, add);
            }
            None => {
                self.commands.push_back((add, Expect::Ok));
                self.command(&get, Expect::LinkAdded(link.key));
            }
        }
        Ok(MeshAdd::Sent)
    }

    /// The radio did not take an imported advert (manual add, the hop limit, a full table):
    /// it is added from the advert's fields (§16.6).
    pub(crate) fn link_not_imported(&mut self, key: PubKey) {
        if let Some(add) = self.mesh.adding.remove(&key) {
            self.commands.push_back((add, Expect::Ok));
            self.command(
                &Command::GetContactByKey { key: &key },
                Expect::LinkAdded(key),
            );
        }
    }

    /// The radio has the node of a link now.
    pub(crate) fn link_added(&mut self, key: &PubKey) {
        self.mesh.adding.remove(key);
    }

    /// The radio did not add the node of a link: refused, as a setting it refuses (§1.1).
    pub(crate) fn link_not_added(&mut self, key: PubKey) {
        self.mesh.adding.remove(&key);
        self.radio_outcome(
            crate::radio::RadioSetting::Contact,
            crate::radio::SettingOutcome::Refused,
        );
    }

    /// The radio is gone: a trace on its way can no longer come back (pushes are lost).
    pub(crate) fn mesh_disconnected(&mut self) {
        let mut changed = Vec::new();
        for (k, t) in &mut self.mesh.traces {
            if t.status == TraceStatus::Sending {
                t.status = TraceStatus::NoAnswer;
                changed.push(*k);
            }
        }
        for node in changed {
            self.events.push_back(Event::NodeTool { node });
        }
    }

    /// A node the radio reported, or one lichen knows as a contact.
    fn node(&self, key: &PubKey) -> Option<(u8, Option<OwnedPath>)> {
        if let Some(c) = self.contacts.get(key) {
            return Some((c.kind, c.out_path.clone()));
        }
        self.mesh
            .heard
            .get(key)
            .map(|n| (n.kind, n.out_path.clone()))
    }

    /// Names of the repeaters the radio knows whose key starts with `hash` (§16.2).
    pub fn hop_names(&self, hash: &[u8]) -> Vec<String> {
        let mut names: Vec<String> = self
            .contacts
            .values()
            .filter(|c| repeats(c.kind) && c.key.starts_with(hash))
            .map(|c| c.name.clone())
            .chain(
                self.mesh
                    .heard
                    .values()
                    .filter(|n| repeats(n.kind) && n.key.starts_with(hash))
                    .map(|n| n.name.clone()),
            )
            .collect();
        names.sort();
        names.dedup();
        names
    }

    fn hop(&self, hash: &[u8]) -> Hop {
        Hop {
            hash: hash.to_vec(),
            names: self.hop_names(hash),
        }
    }

    /// The radio's path to `node`, hop by hop (§16.2): `None` when it floods to reach it,
    /// empty when it hears it directly.
    pub fn path_hops(&self, node: &PubKey) -> Option<Vec<Hop>> {
        let (_, path) = self.node(node)?;
        let path = path?;
        let size = usize::from(path.len_byte >> 6) + 1;
        Some(path.bytes.chunks(size).map(|h| self.hop(h)).collect())
    }

    /// The last trace to `node`.
    pub fn trace_of(&self, node: &PubKey) -> Option<&Trace> {
        self.mesh.traces.get(node)
    }

    /// Traces the radio's path to `node` and back (§16.3): the repeaters on it, and `node`
    /// itself when it is a repeater. `NoPath` when there is no repeater to trace (a node heard
    /// directly, or one the radio floods to).
    pub fn trace(&mut self, node: &PubKey, now: Millis) -> Result<(), Error> {
        let (kind, path) = self.node(node).ok_or(Error::UnknownContact)?;
        if !self.online {
            return Err(Error::NoIdentity);
        }
        if self
            .mesh
            .traces
            .get(node)
            .is_some_and(|t| t.status == TraceStatus::Sending)
        {
            return Err(Error::TooSoon {
                wait_ms: self
                    .mesh
                    .traces
                    .get(node)
                    .map_or(0, |t| t.deadline.saturating_sub(now)),
            });
        }
        let path = path.ok_or(Error::NoPath)?;
        let size = usize::from(path.len_byte >> 6) + 1;
        let mut out: Vec<&[u8]> = path.bytes.chunks(size).collect();
        if repeats(kind) {
            out.push(node.get(..size).ok_or(Error::NoPath)?);
        }
        // Hashes of 1 or 2 bytes on the air (`1 << path_sz`): a 3-byte path is traced by
        // the first two bytes of each hash.
        let (path_sz, width) = if size == 1 { (0u8, 1) } else { (1u8, 2) };
        let back = out.iter().rev().skip(1).copied().collect::<Vec<_>>();
        let hashes: Vec<&[u8]> = out.into_iter().chain(back).collect();
        if hashes.is_empty() {
            return Err(Error::NoPath);
        }
        let mut bytes = Vec::with_capacity(hashes.len() * width);
        for h in &hashes {
            bytes.extend(h.iter().take(width));
        }
        if 13 + bytes.len() + hashes.len() > TRACE_PUSH_MAX
            || bytes.len() > lc_meshcore::command::TRACE_PATH_MAX
        {
            return Err(Error::TooLarge);
        }
        // Header, path length, tag, auth, flags, hashes: our transmission.
        let airtime = self.airtime_ms(2 + 9 + bytes.len());
        if !self.budget_allows(now, airtime) {
            return Err(Error::OverBudget {
                wait_ms: self.budget_wait(now, airtime),
            });
        }
        let tag = self.rng.next_u32();
        self.command(
            &Command::SendTracePath {
                tag,
                auth: 0,
                flags: path_sz,
                path: &bytes,
            },
            Expect::Trace(tag),
        );
        self.record_airtime(now, airtime, Some(path.hops()));
        let hops = hashes.iter().map(|h| self.hop(h)).collect();
        self.mesh.traces.insert(
            *node,
            Trace {
                hops,
                snrs: Vec::new(),
                final_snr4: None,
                status: TraceStatus::Sending,
                rtt_ms: None,
                airtime_ms: airtime,
                done_s: None,
                tag,
                started: now,
                deadline: now + TRACE_SENT_MS,
            },
        );
        self.events.push_back(Event::NodeTool { node: *node });
        self.tick(now);
        Ok(())
    }

    fn trace_by_tag(&mut self, tag: u32) -> Option<(PubKey, &mut Trace)> {
        self.mesh
            .traces
            .iter_mut()
            .find(|(_, t)| t.tag == tag && t.status == TraceStatus::Sending)
            .map(|(k, t)| (*k, t))
    }

    /// `SENT` for trace `tag`: the companion's estimate of the round trip.
    pub(crate) fn on_trace_sent(&mut self, tag: u32, timeout_ms: u32, now: Millis) {
        if let Some((_, t)) = self.trace_by_tag(tag) {
            t.deadline = now + Millis::from(timeout_ms) + TRACE_MARGIN_MS;
        }
    }

    /// The companion refused trace `tag` (`ERR`).
    pub(crate) fn on_trace_refused(&mut self, tag: u32) {
        if let Some((node, t)) = self.trace_by_tag(tag) {
            t.status = TraceStatus::Refused;
            self.events.push_back(Event::NodeTool { node });
        }
    }

    /// Push `TRACE_DATA`.
    pub(crate) fn on_trace_data(&mut self, tag: u32, snrs: &[u8], final_snr4: i8, now: Millis) {
        let done_s = self.unix_s(now);
        if let Some((node, t)) = self.trace_by_tag(tag) {
            t.done_s = done_s;
            t.snrs = snrs.iter().map(|&b| b as i8).collect();
            t.final_snr4 = Some(final_snr4);
            t.status = TraceStatus::Done;
            t.rtt_ms = Some(now.saturating_sub(t.started));
            self.events.push_back(Event::NodeTool { node });
        }
    }

    /// Traces that did not come back in time.
    pub(crate) fn tick_mesh(&mut self, now: Millis) {
        let mut late = Vec::new();
        for (k, t) in &mut self.mesh.traces {
            if t.status == TraceStatus::Sending && now >= t.deadline {
                t.status = TraceStatus::NoAnswer;
                late.push(*k);
            }
        }
        for node in late {
            self.events.push_back(Event::NodeTool { node });
        }
    }

    /// The rating of a reading under the current preset (§16.1); `None` without one.
    pub fn signal_rating(&self, reading: Option<&Reading>) -> SignalRating {
        reading.map_or(SignalRating::None, |r| {
            crate::signal::rate(r.snr4, r.rssi, self.radio.sf)
        })
    }

    /// The last direct reading of `node` for the connected radio.
    pub fn signal_of(&self, node: &PubKey) -> Option<Reading> {
        self.mesh.signals.get(node).copied()
    }

    /// The last reading of `node` the runner stored for this radio (§16.1): kept unless a newer
    /// one is known.
    pub fn restore_signal(&mut self, node: PubKey, reading: Reading) {
        if self
            .mesh
            .signals
            .get(&node)
            .is_none_or(|r| r.at_s < reading.at_s)
        {
            self.mesh.signals.insert(node, reading);
        }
    }

    /// The route to `node` (§16.3): `None` when the radio floods to reach it or does not know
    /// it.
    pub fn route(&self, node: &PubKey) -> Option<Route> {
        let (kind, path) = self.node(node)?;
        let path = path?;
        let me = *self.node_key()?;
        let size = usize::from(path.len_byte >> 6) + 1;
        let hashes: Vec<&[u8]> = path.bytes.chunks(size).collect();
        let mut stops = vec![RouteStop {
            key: Some(me),
            hash: Vec::new(),
            names: vec![self.node_name().to_owned()],
            position: None,
        }];
        for h in &hashes {
            let mut repeaters = self.nodes_with(h).filter(|(_, k)| repeats(*k));
            let only = match (repeaters.next(), repeaters.next()) {
                (Some((key, _)), None) => Some(key),
                _ => None,
            };
            stops.push(RouteStop {
                key: only,
                hash: h.to_vec(),
                names: self.hop_names(h),
                position: only.and_then(|k| self.advertised(&k)),
            });
        }
        stops.push(RouteStop {
            key: Some(*node),
            hash: Vec::new(),
            names: self.node_named(node).into_iter().collect(),
            position: self.advertised(node),
        });
        // What a trace measured, when it came back along today's path: out, then back.
        let mut out: Vec<&[u8]> = hashes.clone();
        if repeats(kind) {
            out.push(node.get(..size)?);
        }
        let m = out.len();
        let trace = self.mesh.traces.get(node).filter(|t| {
            m > 0
                && t.status == TraceStatus::Done
                && t.hops.len() == 2 * m - 1
                && t.hops.iter().zip(&out).all(|(h, o)| h.hash == *o)
        });
        let traced = |snr4: Option<i8>| {
            let (t, snr4) = (trace?, snr4?);
            Some(Reading {
                snr4,
                rssi: None,
                at_s: t.done_s?,
            })
        };
        let legs = (0..stops.len() - 1)
            .map(|k| {
                if k == 0 {
                    // Our radio hearing the first stop: its own reading, or the trace's last
                    // transmission, whichever is newer.
                    let own = stops
                        .get(1)
                        .and_then(|s| s.key)
                        .and_then(|key| self.mesh.signals.get(&key).copied());
                    let back = traced(trace.and_then(|t| t.final_snr4));
                    return match (own, back) {
                        (Some(o), Some(b)) if b.at_s > o.at_s => RouteLeg {
                            reading: Some(b),
                            traced: true,
                        },
                        (Some(o), _) => RouteLeg {
                            reading: Some(o),
                            traced: false,
                        },
                        (None, b) => RouteLeg {
                            reading: b,
                            traced: b.is_some(),
                        },
                    };
                }
                // Stop k hearing stop k + 1: the trace's return half, up to its far end.
                let reading = (k < m)
                    .then(|| traced(trace.and_then(|t| t.snrs.get(2 * m - 1 - k).copied())))
                    .flatten();
                RouteLeg {
                    reading,
                    traced: reading.is_some(),
                }
            })
            .collect();
        Some(Route { stops, legs })
    }

    /// The nodes lichen knows whose key starts with `hash`, with their type.
    fn nodes_with<'a>(&'a self, hash: &'a [u8]) -> impl Iterator<Item = (PubKey, u8)> + 'a {
        let heard = self
            .mesh
            .heard
            .values()
            .filter(|n| !self.contacts.contains_key(&n.key))
            .map(|n| (n.key, n.kind));
        self.contacts
            .values()
            .map(|c| (c.key, c.kind))
            .chain(heard)
            .filter(move |(k, _)| k.starts_with(hash))
    }

    /// The position a node's adverts carry, as the radio reported it.
    fn advertised(&self, key: &PubKey) -> Option<(i32, i32)> {
        self.mesh
            .seen
            .get(key)
            .map(|s| s.position)
            .or_else(|| self.mesh.heard.get(key).map(|n| n.position))
            .flatten()
    }

    /// A node's name, as the radio or lichen knows it.
    fn node_named(&self, key: &PubKey) -> Option<String> {
        self.contacts
            .get(key)
            .map(|c| c.name.clone())
            .or_else(|| self.mesh.heard.get(key).map(|n| n.name.clone()))
    }

    pub(crate) fn next_mesh_wakeup(&self) -> Option<Millis> {
        self.mesh
            .traces
            .values()
            .filter(|t| t.status == TraceStatus::Sending)
            .map(|t| t.deadline)
            .min()
    }
}

/// Whether a node of kind `kind` repeats (only repeaters forward: §16.2).
pub const fn repeats(kind: u8) -> bool {
    kind == contact_kind::REPEATER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_at_zero_are_none() {
        assert_eq!(position(0, 0), None);
        assert_eq!(position(45_000_000, 0), Some((45_000_000, 0)));
        assert_eq!(position(91_000_000, 5), None);
        assert_eq!(position(-45_000_000, -181_000_000), None);
    }
}

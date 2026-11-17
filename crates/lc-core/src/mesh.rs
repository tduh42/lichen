//! Mesh tools (protocol §16, ADR-0041): the nodes the radio knows or has heard, with their
//! type, when they were last heard and the position they advertise; a contact's path, hop by
//! hop; traces.
//!
//! The radio is the source of truth: everything here is read again at each connection and
//! nothing is stored.

use std::collections::HashMap;

use lc_meshcore::Command;
use lc_meshcore::response::Contact as RadioContact;
use rand_core::Rng;

use crate::core::{Core, Error, Event, Expect, OwnedPath, PubKey, contact_kind};
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
    /// When the radio's battery was last asked for (ADR-0042).
    pub(crate) battery_at: Option<Millis>,
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
                })
            })
            .collect();
        v.extend(
            self.mesh
                .heard
                .values()
                .filter(|n| !self.contacts.contains_key(&n.key))
                .cloned(),
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
            },
        );
        self.events.push_back(Event::NodesUpdated);
    }

    /// The radio is another one, or reconnects: what it reports is read again.
    pub(crate) fn mesh_connected(&mut self) {
        self.mesh.seen.clear();
        self.mesh.heard.clear();
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
        if let Some((node, t)) = self.trace_by_tag(tag) {
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

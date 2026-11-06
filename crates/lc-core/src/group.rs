//! Private groups (`docs/spec/protocol.md` §10, ADR-0009): group state from the admin, a sender
//! key per member, signed messages flooded on channel hash `0x7C`.

use std::collections::{HashMap, HashSet};

use lc_meshcore::air::{Packet, PayloadType, Route, transport_code};
use lc_meshcore::ecdh::{sign, verify};
use lc_meshcore::path::Path;
use lc_proto::group::{self, GroupHeader, SIGNATURE_LEN};
use lc_proto::{Millis, content};
use rand_core::Rng;
use sha2::{Digest, Sha256};

use crate::content::Content;
use crate::core::{Core, Error, Event, MsgId, PubKey};

pub type GroupId = [u8; 16];

/// Members per group, admin included.
pub const MAX_MEMBERS: usize = 32;
/// A partial (first of two fragments) is forgotten after this long.
const PARTIAL_MS: Millis = 300_000;
/// Keys that arrived before their group state are kept this long.
const PENDING_KEY_MS: Millis = 600_000;

/// A member as listed in the group state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub key: PubKey,
    pub name: String,
}

/// One sender key of a member, with its replay window.
#[derive(Clone, Debug)]
struct KeyState {
    epoch: u8,
    key: [u8; 32],
    /// Counters below this were never used under this key.
    floor: u32,
    highest: Option<u32>,
    /// Bit `d` set: counter `highest − d` already delivered.
    seen: u64,
}

impl KeyState {
    fn new(epoch: u8, key: [u8; 32], floor: u32) -> Self {
        Self {
            epoch,
            key,
            floor,
            highest: None,
            seen: 0,
        }
    }

    fn accepts(&self, counter: u32) -> bool {
        if counter < self.floor {
            return false;
        }
        match self.highest {
            None => true,
            Some(h) if counter > h => true,
            Some(h) => {
                let d = h - counter;
                d < 64 && (self.seen >> d) & 1 == 0
            }
        }
    }

    fn mark(&mut self, counter: u32) {
        match self.highest {
            None => {
                self.highest = Some(counter);
                self.seen = 1;
            }
            Some(h) if counter > h => {
                self.seen = self.seen.checked_shl(counter - h).unwrap_or(0) | 1;
                self.highest = Some(counter);
            }
            Some(h) => {
                let d = h - counter;
                if d < 64 {
                    self.seen |= 1 << d;
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
struct MemberKeys {
    current: Option<KeyState>,
    previous: Option<KeyState>,
}

/// One fragment of a two-fragment message, waiting for the other.
#[derive(Clone, Debug)]
struct Partial {
    member: PubKey,
    epoch: u8,
    counter: u32,
    index: u8,
    data: Vec<u8>,
    at: Millis,
}

/// A group this node belongs to.
#[derive(Clone, Debug)]
pub struct Group {
    pub gid: GroupId,
    pub name: String,
    pub admin: PubKey,
    pub epoch: u32,
    pub members: Vec<Member>,
    my_key: [u8; 32],
    my_epoch: u8,
    my_counter: u32,
    keys: HashMap<PubKey, MemberKeys>,
    partials: Vec<Partial>,
}

impl Group {
    pub fn gid_hash(&self) -> u16 {
        gid_hash(&self.gid)
    }

    /// Number of other members whose current sender key we hold.
    pub fn keys_known(&self) -> usize {
        self.members
            .iter()
            .filter(|m| self.keys.get(&m.key).is_some_and(|k| k.current.is_some()))
            .count()
    }

    fn is_member(&self, k: &PubKey) -> bool {
        self.members.iter().any(|m| m.key == *k)
    }
}

pub fn gid_hash(gid: &GroupId) -> u16 {
    let d = Sha256::digest(gid);
    u16::from_le_bytes([d[0], d[1]])
}

/// A key that arrived before its group state.
#[derive(Clone, Debug)]
pub(crate) struct PendingKey {
    from: PubKey,
    gid: GroupId,
    epoch: u8,
    key: [u8; 32],
    next_counter: u32,
    at: Millis,
}

// ----- control datagrams (§10.1, §10.2) ----------------------------------------------------

fn put_name(v: &mut Vec<u8>, name: &str) {
    let mut end = name.len().min(32);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    v.push(end as u8);
    v.extend_from_slice(&name.as_bytes()[..end]);
}

fn take<'a>(b: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    let (head, tail) = b.split_at_checked(n)?;
    *b = tail;
    Some(head)
}

fn take_name(b: &mut &[u8]) -> Option<String> {
    let n = usize::from(*take(b, 1)?.first()?);
    if n > 32 {
        return None;
    }
    Some(String::from_utf8_lossy(take(b, n)?).into_owned())
}

/// The group state as sent by the admin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct State {
    gid: GroupId,
    epoch: u32,
    admin: PubKey,
    name: String,
    members: Vec<Member>,
}

impl State {
    fn encode(&self) -> Vec<u8> {
        let mut v = vec![content::GROUP_STATE];
        v.extend_from_slice(&self.gid);
        v.extend_from_slice(&self.epoch.to_le_bytes());
        v.extend_from_slice(&self.admin);
        put_name(&mut v, &self.name);
        v.push(self.members.len() as u8);
        for m in &self.members {
            v.extend_from_slice(&m.key);
            put_name(&mut v, &m.name);
        }
        v
    }

    pub(crate) fn decode(datagram: &[u8]) -> Option<Self> {
        let (&t, mut b) = datagram.split_first()?;
        if t != content::GROUP_STATE {
            return None;
        }
        let gid: GroupId = take(&mut b, 16)?.try_into().ok()?;
        let epoch = u32::from_le_bytes(take(&mut b, 4)?.try_into().ok()?);
        let admin: PubKey = take(&mut b, 32)?.try_into().ok()?;
        let name = take_name(&mut b)?;
        let n = usize::from(*take(&mut b, 1)?.first()?);
        if !(2..=MAX_MEMBERS).contains(&n) {
            return None;
        }
        let mut members = Vec::with_capacity(n);
        for _ in 0..n {
            let key: PubKey = take(&mut b, 32)?.try_into().ok()?;
            members.push(Member {
                key,
                name: take_name(&mut b)?,
            });
        }
        let distinct: HashSet<&PubKey> = members.iter().map(|m| &m.key).collect();
        if !b.is_empty() || distinct.len() != n || !distinct.contains(&admin) {
            return None;
        }
        Some(Self {
            gid,
            epoch,
            admin,
            name,
            members,
        })
    }
}

fn encode_sender_key(gid: &GroupId, epoch: u8, key: &[u8; 32], next_counter: u32) -> Vec<u8> {
    let mut v = vec![content::SENDER_KEY];
    v.extend_from_slice(gid);
    v.push(epoch);
    v.extend_from_slice(key);
    v.extend_from_slice(&next_counter.to_le_bytes());
    v
}

fn decode_sender_key(datagram: &[u8]) -> Option<(GroupId, u8, [u8; 32], u32)> {
    let (&t, b) = datagram.split_first()?;
    if t != content::SENDER_KEY || b.len() != 16 + 1 + 32 + 4 {
        return None;
    }
    Some((
        b.get(..16)?.try_into().ok()?,
        *b.get(16)?,
        b.get(17..49)?.try_into().ok()?,
        u32::from_le_bytes(b.get(49..53)?.try_into().ok()?),
    ))
}

/// What a group signature covers: `"lichen/1 group" ‖ gid ‖ key_epoch ‖ counter ‖ body`.
fn signed_parts<'a>(
    gid: &'a GroupId,
    epoch: &'a [u8; 1],
    counter: &'a [u8; 4],
    body: &'a [u8],
) -> [&'a [u8]; 5] {
    [b"lichen/1 group", gid, epoch, counter, body]
}

// ----- the core's group operations ---------------------------------------------------------

impl Core {
    /// Groups this node belongs to.
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.values()
    }

    fn fresh_key(&mut self) -> [u8; 32] {
        let mut k = [0u8; 32];
        self.rng.fill_bytes(&mut k);
        k
    }

    /// Creates a group with us as admin and `members` (contacts) as the other members.
    pub fn create_group(
        &mut self,
        name: &str,
        members: &[PubKey],
        now: Millis,
    ) -> Result<GroupId, Error> {
        let me = self.key.ok_or(Error::NoIdentity)?;
        let mut list = vec![Member {
            key: me,
            name: self.name.clone(),
        }];
        for k in members {
            let c = self.contacts.get(k).ok_or(Error::UnknownContact)?;
            if list.iter().all(|m| m.key != *k) {
                list.push(Member {
                    key: *k,
                    name: c.name.clone(),
                });
            }
        }
        if list.len() < 2 || list.len() > MAX_MEMBERS {
            return Err(Error::TooLarge);
        }
        let mut gid = [0u8; 16];
        self.rng.fill_bytes(&mut gid);
        let my_key = self.fresh_key();
        let group = Group {
            gid,
            name: name.to_owned(),
            admin: me,
            epoch: 1,
            members: list,
            my_key,
            my_epoch: 0,
            my_counter: 0,
            keys: HashMap::new(),
            partials: Vec::new(),
        };
        self.groups.insert(gid, group);
        self.broadcast_state(&gid, &[]);
        self.send_my_key(&gid, None);
        self.events.push_back(Event::GroupUpdated { gid });
        self.tick(now);
        Ok(gid)
    }

    /// Removes `member` (admin only): new epoch, and every remaining member rotates its key.
    pub fn remove_member(
        &mut self,
        gid: &GroupId,
        member: &PubKey,
        now: Millis,
    ) -> Result<(), Error> {
        let me = self.key.ok_or(Error::NoIdentity)?;
        let g = self.groups.get_mut(gid).ok_or(Error::UnknownGroup)?;
        if g.admin != me || *member == me || !g.is_member(member) {
            return Err(Error::NotAllowed);
        }
        g.members.retain(|m| m.key != *member);
        g.epoch += 1;
        g.keys.remove(member);
        self.broadcast_state(gid, &[*member]);
        self.rotate_my_key(gid);
        self.events.push_back(Event::GroupUpdated { gid: *gid });
        self.tick(now);
        Ok(())
    }

    /// Sends the group state to every other member, and to `also` (removed members).
    fn broadcast_state(&mut self, gid: &GroupId, also: &[PubKey]) {
        let (Some(me), Some(g)) = (self.key, self.groups.get(gid)) else {
            return;
        };
        let state = State {
            gid: *gid,
            epoch: g.epoch,
            admin: g.admin,
            name: g.name.clone(),
            members: g.members.clone(),
        }
        .encode();
        let targets: Vec<PubKey> = g
            .members
            .iter()
            .map(|m| m.key)
            .chain(also.iter().copied())
            .filter(|k| *k != me)
            .collect();
        for k in targets {
            self.enqueue_internal(&k, state.clone());
        }
    }

    /// Sends our current sender key to `only`, or to every other member.
    fn send_my_key(&mut self, gid: &GroupId, only: Option<&[PubKey]>) {
        let (Some(me), Some(g)) = (self.key, self.groups.get(gid)) else {
            return;
        };
        let datagram = encode_sender_key(gid, g.my_epoch, &g.my_key, g.my_counter);
        let targets: Vec<PubKey> = g
            .members
            .iter()
            .map(|m| m.key)
            .filter(|k| *k != me && only.is_none_or(|o| o.contains(k)))
            .collect();
        for k in targets {
            self.enqueue_internal(&k, datagram.clone());
        }
    }

    fn rotate_my_key(&mut self, gid: &GroupId) {
        let key = self.fresh_key();
        if let Some(g) = self.groups.get_mut(gid) {
            g.my_key = key;
            g.my_epoch = g.my_epoch.wrapping_add(1);
            g.my_counter = 0;
        }
        self.send_my_key(gid, None);
    }

    /// A GROUP_STATE or SENDER_KEY datagram from `from` (§10.1, §10.2).
    pub(crate) fn on_group_control(&mut self, from: &PubKey, datagram: &[u8], now: Millis) {
        match datagram.first() {
            Some(&content::GROUP_STATE) => {
                if let Some(state) = State::decode(datagram) {
                    self.on_group_state(from, state, now);
                }
            }
            Some(&content::SENDER_KEY) => {
                if let Some((gid, epoch, key, next)) = decode_sender_key(datagram) {
                    self.on_sender_key(from, gid, epoch, key, next, now);
                }
            }
            _ => {}
        }
    }

    fn on_group_state(&mut self, from: &PubKey, state: State, now: Millis) {
        let Some(me) = self.key else {
            return;
        };
        if state.admin != *from {
            return;
        }
        let listed = state.members.iter().any(|m| m.key == me);
        let gid = state.gid;
        for m in &state.members {
            if m.key != me && !self.contacts.contains_key(&m.key) {
                self.add_contact(m.key, &m.name, None);
            }
        }
        match self.groups.get_mut(&gid) {
            None => {
                if !listed {
                    return;
                }
                let my_key = self.fresh_key();
                self.groups.insert(
                    gid,
                    Group {
                        gid,
                        name: state.name.clone(),
                        admin: state.admin,
                        epoch: state.epoch,
                        members: state.members,
                        my_key,
                        my_epoch: 0,
                        my_counter: 0,
                        keys: HashMap::new(),
                        partials: Vec::new(),
                    },
                );
                self.send_my_key(&gid, None);
                self.events.push_back(Event::GroupJoined {
                    gid,
                    name: state.name,
                    admin: state.admin,
                });
            }
            Some(g) => {
                if g.admin != *from || state.epoch <= g.epoch {
                    return;
                }
                if !listed {
                    self.groups.remove(&gid);
                    self.events.push_back(Event::GroupLeft { gid });
                    return;
                }
                let old: HashSet<PubKey> = g.members.iter().map(|m| m.key).collect();
                let new: HashSet<PubKey> = state.members.iter().map(|m| m.key).collect();
                let removed: Vec<PubKey> = old.difference(&new).copied().collect();
                let added: Vec<PubKey> = new.difference(&old).copied().collect();
                g.name = state.name;
                g.epoch = state.epoch;
                g.members = state.members;
                for k in &removed {
                    g.keys.remove(k);
                }
                if removed.is_empty() {
                    self.send_my_key(&gid, Some(&added));
                } else {
                    self.rotate_my_key(&gid);
                }
                self.events.push_back(Event::GroupUpdated { gid });
            }
        }
        // Keys that came before this state.
        let pending: Vec<PendingKey> = std::mem::take(&mut self.pending_keys);
        for p in pending {
            if p.gid == gid {
                self.on_sender_key(&p.from, p.gid, p.epoch, p.key, p.next_counter, now);
            } else if p.at + PENDING_KEY_MS > now {
                self.pending_keys.push(p);
            }
        }
    }

    fn on_sender_key(
        &mut self,
        from: &PubKey,
        gid: GroupId,
        epoch: u8,
        key: [u8; 32],
        next: u32,
        now: Millis,
    ) {
        let Some(g) = self.groups.get_mut(&gid) else {
            if self.pending_keys.len() < 32 {
                self.pending_keys.push(PendingKey {
                    from: *from,
                    gid,
                    epoch,
                    key,
                    next_counter: next,
                    at: now,
                });
            }
            return;
        };
        if !g.is_member(from) {
            return;
        }
        let mk = g.keys.entry(*from).or_default();
        match &mk.current {
            Some(c) if c.epoch == epoch => {}
            _ => {
                mk.previous = mk.current.take();
                mk.current = Some(KeyState::new(epoch, key, next));
            }
        }
    }

    /// Sends a text to the group (§10.3). Flooded, no acknowledgement.
    pub fn send_group_text(
        &mut self,
        gid: &GroupId,
        text: &str,
        sent_at: u32,
        now: Millis,
    ) -> Result<MsgId, Error> {
        if text.is_empty() {
            return Err(Error::Empty);
        }
        let body = Content::Text {
            sent_at,
            text: text.to_owned(),
        }
        .encode();
        self.send_group(gid, &body, now)
    }

    fn send_group(&mut self, gid: &GroupId, body: &[u8], now: Millis) -> Result<MsgId, Error> {
        let (me, prv) = (
            self.key.ok_or(Error::NoIdentity)?,
            self.prv.ok_or(Error::NoIdentity)?,
        );
        let g = self.groups.get_mut(gid).ok_or(Error::UnknownGroup)?;
        let counter = g.my_counter;
        let (epoch_b, counter_b) = ([g.my_epoch], counter.to_le_bytes());
        let sig =
            sign(&prv, &signed_parts(gid, &epoch_b, &counter_b, body)).ok_or(Error::NoIdentity)?;
        let mut plain = body.to_vec();
        plain.extend_from_slice(&sig);
        let total = group::fragments_for(plain.len()).ok_or(Error::TooLarge)?;
        g.my_counter = g.my_counter.checked_add(1).ok_or(Error::TooLarge)?;
        let (key, epoch, gh) = (g.my_key, g.my_epoch, g.gid_hash());
        let mut due = now;
        for (index, chunk) in plain.chunks(group::FRAGMENT_PLAIN_MAX).enumerate() {
            let header = GroupHeader {
                gid_hash: gh,
                sender: me[0],
                key_epoch: epoch,
                counter,
                index: index as u8,
                total,
            };
            let mut out = [0u8; group::PAYLOAD_MAX];
            let Ok(n) = group::seal(&key, &header, chunk, &mut out) else {
                return Err(Error::TooLarge);
            };
            let raw = self.flood_frame(out.get(..n).unwrap_or(&[]));
            self.group_tx.push_back((due, raw));
            // The second fragment waits until the repeaters have re-flooded the first.
            due += 10 * self.radio.airtime_us(group::PAYLOAD_MAX + 6) / 1000;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.tick(now);
        Ok(id)
    }

    /// A `GRP_DATA` flood frame around `payload`, scoped like the companion's floods.
    pub(crate) fn flood_frame(&self, payload: &[u8]) -> Vec<u8> {
        let (route, transport) = match &self.scope {
            Some(key) => (
                Route::TransportFlood,
                Some([transport_code(key, PayloadType::GrpData, payload), 0]),
            ),
            None => (Route::Flood, None),
        };
        let packet = Packet {
            route,
            ptype: PayloadType::GrpData,
            transport,
            path: Path::EMPTY,
            payload,
        };
        let mut raw = [0u8; 255];
        packet
            .encode(&mut raw)
            .map(|n| raw.get(..n).unwrap_or(&[]).to_vec())
            .unwrap_or_default()
    }

    /// Hands due group frames to the companion, in the node's shared radio slot.
    pub(crate) fn tick_groups(&mut self, now: Millis) {
        while self.group_tx.front().is_some_and(|(due, _)| *due <= now) && now >= self.next_slot {
            let Some((_, raw)) = self.group_tx.pop_front() else {
                break;
            };
            self.send_raw_packet(&raw, now);
        }
        for g in self.groups.values_mut() {
            g.partials.retain(|p| p.at + PARTIAL_MS > now);
        }
    }

    pub(crate) fn next_group_wakeup(&self) -> Option<Millis> {
        self.group_tx
            .front()
            .map(|(due, _)| (*due).max(self.next_slot))
    }

    /// A group envelope heard on the air (§10.3).
    pub(crate) fn on_group_envelope(&mut self, payload: &[u8], snr4: i8, rssi: i8, now: Millis) {
        let Some(me) = self.key else {
            return;
        };
        let Some((h, _)) = group::parse(payload) else {
            return;
        };
        let gids: Vec<GroupId> = self
            .groups
            .values()
            .filter(|g| g.gid_hash() == h.gid_hash)
            .map(|g| g.gid)
            .collect();
        for gid in gids {
            let Some(g) = self.groups.get_mut(&gid) else {
                continue;
            };
            let candidates: Vec<PubKey> = g
                .members
                .iter()
                .map(|m| m.key)
                .filter(|k| k[0] == h.sender && *k != me)
                .collect();
            for member in candidates {
                let Some(mk) = g.keys.get_mut(&member) else {
                    continue;
                };
                for slot in [&mut mk.current, &mut mk.previous] {
                    let Some(ks) = slot.as_mut().filter(|k| k.epoch == h.key_epoch) else {
                        continue;
                    };
                    if !ks.accepts(h.counter) {
                        // Another copy of the same flood, or a replay.
                        return;
                    }
                    let mut buf = payload.to_vec();
                    let Ok((hdr, plain)) = group::open(&ks.key, &mut buf) else {
                        continue;
                    };
                    let message = if hdr.total == 1 {
                        Some(plain.to_vec())
                    } else {
                        let pos = g.partials.iter().position(|p| {
                            p.member == member
                                && p.epoch == hdr.key_epoch
                                && p.counter == hdr.counter
                        });
                        match pos {
                            None => {
                                g.partials.push(Partial {
                                    member,
                                    epoch: hdr.key_epoch,
                                    counter: hdr.counter,
                                    index: hdr.index,
                                    data: plain.to_vec(),
                                    at: now,
                                });
                                None
                            }
                            Some(i) if g.partials[i].index != hdr.index => {
                                let p = g.partials.swap_remove(i);
                                let (first, second) = if p.index == 0 {
                                    (p.data.as_slice(), plain)
                                } else {
                                    (plain, p.data.as_slice())
                                };
                                Some([first, second].concat())
                            }
                            // Another copy of a fragment we already hold.
                            Some(_) => None,
                        }
                    };
                    let Some(message) = message else {
                        return;
                    };
                    ks.mark(hdr.counter);
                    let Some((body, sig)) = message.split_last_chunk::<SIGNATURE_LEN>() else {
                        return;
                    };
                    let (epoch_b, counter_b) = ([hdr.key_epoch], hdr.counter.to_le_bytes());
                    if !verify(
                        &member,
                        &signed_parts(&gid, &epoch_b, &counter_b, body),
                        sig,
                    ) {
                        return;
                    }
                    if let Some(content) = Content::decode(body) {
                        self.events.push_back(Event::GroupMessage {
                            gid,
                            from: member,
                            content,
                            snr4,
                            rssi,
                        });
                    }
                    return;
                }
            }
        }
    }
}

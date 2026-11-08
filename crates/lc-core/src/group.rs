//! Private groups (`docs/spec/protocol.md` §10, ADR-0009): group state from the admin, a sender
//! key per member, signed messages flooded on channel hash `0x7C`.

use std::collections::{HashMap, HashSet};

use lc_meshcore::air::{Packet, PayloadType, Route, transport_code};
use lc_meshcore::ecdh::{sign, verify};
use lc_meshcore::path::Path;
use lc_proto::group::{self, GroupHeader, SIGNATURE_LEN};
use lc_proto::search::Search;
use lc_proto::{Millis, content};
use rand_core::Rng;
use sha2::{Digest, Sha256};

use crate::content::Content;
use crate::core::{Core, Error, Event, MsgId, PubKey};

pub type GroupId = [u8; 16];

/// Members per group, admin included.
pub const MAX_MEMBERS: usize = 32;
/// Every group message is flooded this many times by default (§10.3): a flood lost at its
/// first hop, to a collision with other traffic, is lost for everyone.
pub const COPIES: u8 = 2;
/// A partial (first of two fragments) is forgotten after this long.
const PARTIAL_MS: Millis = 300_000;
/// Keys that arrived before their group state are kept this long.
const PENDING_KEY_MS: Millis = 600_000;
/// Version byte of [`Group::snapshot`]: 2 since members are identities with a node (ADR-0013).
const SNAPSHOT_VERSION: u8 = 2;
/// Envelopes whose sender key has not arrived yet are kept this long (§10.3).
const HELD_MS: Millis = 600_000;
/// And at most this many.
const HELD_MAX: usize = 16;

/// An envelope from a known member whose key we do not hold yet.
#[derive(Clone, Debug)]
pub(crate) struct Held {
    payload: Vec<u8>,
    snr4: i8,
    rssi: i8,
    at: Millis,
}

/// A member as listed in the group state: an identity (ADR-0013) and the node that reaches it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// The member's identity.
    pub key: PubKey,
    /// Its node (routing).
    pub node: PubKey,
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
    /// Our (new) sender key still has to go to the members: restored before the node's
    /// identity was known.
    announce_key: bool,
}

impl Group {
    pub fn gid_hash(&self) -> u16 {
        gid_hash(&self.gid)
    }

    /// The persistent form of the group: state, members, our key epoch and the other members'
    /// sender keys with their replay windows. Our own key is not kept: after a restart a new
    /// one is drawn under the next epoch (§10.2).
    pub fn snapshot(&self) -> Vec<u8> {
        let mut v = vec![SNAPSHOT_VERSION];
        v.extend_from_slice(&self.gid);
        v.extend_from_slice(&self.epoch.to_le_bytes());
        v.extend_from_slice(&self.admin);
        put_name(&mut v, &self.name);
        v.push(self.members.len() as u8);
        for m in &self.members {
            v.extend_from_slice(&m.key);
            v.extend_from_slice(&m.node);
            put_name(&mut v, &m.name);
        }
        v.push(self.my_epoch);
        let keys: Vec<(&PubKey, &MemberKeys)> = self.keys.iter().collect();
        v.push(keys.len() as u8);
        for (k, mk) in keys {
            v.extend_from_slice(k);
            for slot in [&mk.current, &mk.previous] {
                match slot {
                    None => v.push(0),
                    Some(ks) => {
                        v.push(1);
                        v.push(ks.epoch);
                        v.extend_from_slice(&ks.key);
                        v.extend_from_slice(&ks.floor.to_le_bytes());
                        v.extend_from_slice(&ks.highest.map_or(u64::MAX, u64::from).to_le_bytes());
                        v.extend_from_slice(&ks.seen.to_le_bytes());
                    }
                }
            }
        }
        v
    }

    /// Rebuilds a group from [`Group::snapshot`], with a new sender key of ours under the next
    /// epoch.
    fn from_snapshot(b: &[u8], my_key: [u8; 32]) -> Option<Self> {
        let (&version, mut b) = b.split_first()?;
        if version != SNAPSHOT_VERSION {
            return None;
        }
        let b = &mut b;
        let gid: GroupId = take(b, 16)?.try_into().ok()?;
        let epoch = u32::from_le_bytes(take(b, 4)?.try_into().ok()?);
        let admin: PubKey = take(b, 32)?.try_into().ok()?;
        let name = take_name(b)?;
        let n = usize::from(*take(b, 1)?.first()?);
        let mut members = Vec::with_capacity(n);
        for _ in 0..n {
            let key: PubKey = take(b, 32)?.try_into().ok()?;
            let node: PubKey = take(b, 32)?.try_into().ok()?;
            members.push(Member {
                key,
                node,
                name: take_name(b)?,
            });
        }
        let my_epoch = take(b, 1)?.first()?.wrapping_add(1);
        let nk = usize::from(*take(b, 1)?.first()?);
        let mut keys = HashMap::new();
        for _ in 0..nk {
            let k: PubKey = take(b, 32)?.try_into().ok()?;
            let mut slots = [None, None];
            for slot in &mut slots {
                if *take(b, 1)?.first()? == 1 {
                    let epoch = *take(b, 1)?.first()?;
                    let key: [u8; 32] = take(b, 32)?.try_into().ok()?;
                    let floor = u32::from_le_bytes(take(b, 4)?.try_into().ok()?);
                    let highest = u64::from_le_bytes(take(b, 8)?.try_into().ok()?);
                    let seen = u64::from_le_bytes(take(b, 8)?.try_into().ok()?);
                    *slot = Some(KeyState {
                        epoch,
                        key,
                        floor,
                        highest: u32::try_from(highest).ok(),
                        seen,
                    });
                }
            }
            let [current, previous] = slots;
            keys.insert(k, MemberKeys { current, previous });
        }
        b.is_empty().then_some(Self {
            gid,
            name,
            admin,
            epoch,
            members,
            my_key,
            my_epoch,
            my_counter: 0,
            keys,
            partials: Vec::new(),
            announce_key: true,
        })
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

    /// The member reached through node `node`.
    pub fn member_by_node(&self, node: &PubKey) -> Option<&Member> {
        self.members.iter().find(|m| m.node == *node)
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
            v.extend_from_slice(&m.node);
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
            let node: PubKey = take(&mut b, 32)?.try_into().ok()?;
            members.push(Member {
                key,
                node,
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
    /// Groups that changed since the last call, to persist ([`Core::group_snapshot`]); a
    /// group we left has no snapshot any more.
    pub fn take_dirty_groups(&mut self) -> Vec<GroupId> {
        self.dirty_groups.drain().collect()
    }

    pub fn group_snapshot(&self, gid: &GroupId) -> Option<Vec<u8>> {
        self.groups.get(gid).map(Group::snapshot)
    }

    /// Restores a group saved with [`Core::group_snapshot`] (app start). Our sender key is new,
    /// under the next epoch, and goes to every member once the node is connected.
    pub fn restore_group(&mut self, snapshot: &[u8]) -> bool {
        let key = self.fresh_key();
        let Some(g) = Group::from_snapshot(snapshot, key) else {
            return false;
        };
        let gid = g.gid;
        let members = g.members.clone();
        self.groups.insert(gid, g);
        self.learn_members(&members);
        self.dirty_groups.insert(gid);
        true
    }

    /// Members of a group become contacts (their node), with the identity the group gives
    /// when the contact has none yet.
    fn learn_members(&mut self, members: &[Member]) {
        let me = self.key;
        for m in members {
            if Some(m.key) == me {
                continue;
            }
            if !self.contacts.contains_key(&m.node) {
                self.add_contact(m.node, &m.name, None);
            }
            if self.identity_of(&m.node).is_none() {
                self.learn_identity(&m.node, m.key);
            }
        }
    }

    /// How many times each group message is flooded (1 or 2).
    pub fn set_group_copies(&mut self, copies: u8) {
        self.group_copies = copies.clamp(1, 2);
    }

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
        let (me, node) = (
            self.key.ok_or(Error::NoIdentity)?,
            self.node.ok_or(Error::NoIdentity)?,
        );
        let mut list = vec![Member {
            key: me,
            node,
            name: self.name.clone(),
        }];
        for k in members {
            let c = self.contacts.get(k).ok_or(Error::UnknownContact)?;
            let id = c.identity.ok_or(Error::IdentityUnknown)?;
            if list.iter().all(|m| m.key != id) {
                list.push(Member {
                    key: id,
                    node: *k,
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
            announce_key: false,
        };
        self.groups.insert(gid, group);
        self.dirty_groups.insert(gid);
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
        // `member` is the node of the member (as the app knows its contacts).
        let Some(id) = g.member_by_node(member).map(|m| m.key) else {
            return Err(Error::NotAllowed);
        };
        if g.admin != me || id == me {
            return Err(Error::NotAllowed);
        }
        g.members.retain(|m| m.key != id);
        g.epoch += 1;
        g.keys.remove(&id);
        self.dirty_groups.insert(*gid);
        self.broadcast_state(gid, &[*member]);
        self.rotate_my_key(gid);
        self.events.push_back(Event::GroupUpdated { gid: *gid });
        self.tick(now);
        Ok(())
    }

    /// Sends the group state to every other member, and to `also` (nodes of removed members).
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
            .filter(|m| m.key != me)
            .map(|m| m.node)
            .chain(also.iter().copied())
            .collect();
        for k in targets {
            self.enqueue_internal(&k, state.clone());
        }
    }

    /// Sends our current sender key to `only` (identities), or to every other member.
    fn send_my_key(&mut self, gid: &GroupId, only: Option<&[PubKey]>) {
        let (Some(me), Some(g)) = (self.key, self.groups.get(gid)) else {
            return;
        };
        let datagram = encode_sender_key(gid, g.my_epoch, &g.my_key, g.my_counter);
        let targets: Vec<PubKey> = g
            .members
            .iter()
            .filter(|m| m.key != me && only.is_none_or(|o| o.contains(&m.key)))
            .map(|m| m.node)
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
        self.dirty_groups.insert(*gid);
        self.send_my_key(gid, None);
    }

    /// A GROUP_STATE or SENDER_KEY datagram from node `from` (§10.1, §10.2): whose identity
    /// the session proved.
    pub(crate) fn on_group_control(&mut self, from: &PubKey, datagram: &[u8], now: Millis) {
        let Some(from) = self.identity_of(from) else {
            return;
        };
        let from = &from;
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
        self.dirty_groups.insert(gid);
        if listed {
            self.learn_members(&state.members);
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
                        announce_key: false,
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
        if mk.current.as_ref().is_some_and(|c| c.epoch == epoch) {
            return;
        }
        mk.previous = mk.current.take();
        mk.current = Some(KeyState::new(epoch, key, next));
        self.dirty_groups.insert(gid);
        // Messages that came before this key can now be read.
        let held: Vec<Held> = std::mem::take(&mut self.held).into();
        for h in held {
            self.on_group_envelope(&h.payload, h.snr4, h.rssi, h.at.max(now));
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

    /// Floods a datagram (`content_type ‖ body`) to the group, signed (§10.3): an
    /// announcement, for instance (§14.1).
    pub fn send_group_datagram(
        &mut self,
        gid: &GroupId,
        datagram: &[u8],
        now: Millis,
    ) -> Result<MsgId, Error> {
        self.send_group(gid, datagram, now)
    }

    pub(crate) fn send_group(
        &mut self,
        gid: &GroupId,
        body: &[u8],
        now: Millis,
    ) -> Result<MsgId, Error> {
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
        // Each frame waits until the repeaters have re-flooded the previous one, with jitter.
        let spacing = 10 * self.radio.airtime_us(group::PAYLOAD_MAX + 6) / 1000;
        let mut due = now;
        for copy in 0..self.group_copies {
            for (index, chunk) in plain.chunks(group::FRAGMENT_PLAIN_MAX).enumerate() {
                let header = GroupHeader {
                    gid_hash: gh,
                    sender: me[0],
                    key_epoch: epoch,
                    counter,
                    index: index as u8,
                    total,
                    copy,
                };
                let mut out = [0u8; group::PAYLOAD_MAX];
                let Ok(n) = group::seal(&key, &header, chunk, &mut out) else {
                    return Err(Error::TooLarge);
                };
                let raw = self.flood_frame(out.get(..n).unwrap_or(&[]));
                self.group_tx.push_back((due, raw));
                due += spacing + Millis::from(self.rng.next_u32()) % spacing.max(1);
            }
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
        // Restored groups send our new sender key once the node's identity is known.
        let owed: Vec<GroupId> = self
            .groups
            .values_mut()
            .filter(|g| g.announce_key)
            .map(|g| {
                g.announce_key = false;
                g.gid
            })
            .collect();
        for gid in owed {
            self.send_my_key(&gid, None);
        }
        while self.group_tx.front().is_some_and(|(due, _)| *due <= now) && now >= self.next_slot {
            let Some((_, raw)) = self.group_tx.pop_front() else {
                break;
            };
            self.send_raw_packet(&raw, now);
        }
        for g in self.groups.values_mut() {
            g.partials.retain(|p| p.at + PARTIAL_MS > now);
        }
        self.held.retain(|h| h.at + HELD_MS > now);
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
        let mut delivered = None;
        'gids: for gid in gids {
            let Some(g) = self.groups.get_mut(&gid) else {
                continue;
            };
            // Members by identity, with the node the app knows them by.
            let candidates: Vec<(PubKey, PubKey)> = g
                .members
                .iter()
                .filter(|m| m.key[0] == h.sender && m.key != me)
                .map(|m| (m.key, m.node))
                .collect();
            for (member, member_node) in candidates {
                let has_key = g.keys.get(&member).is_some_and(|mk| {
                    [&mk.current, &mk.previous]
                        .iter()
                        .any(|k| k.as_ref().is_some_and(|k| k.epoch == h.key_epoch))
                });
                if !has_key {
                    // The key may still be on its way (pairwise, several hops): keep the envelope.
                    if !self.held.iter().any(|x| x.payload == payload) {
                        if self.held.len() >= HELD_MAX {
                            self.held.pop_front();
                        }
                        self.held.push_back(Held {
                            payload: payload.to_vec(),
                            snr4,
                            rssi,
                            at: now,
                        });
                    }
                    continue;
                }
                let Some(mk) = g.keys.get_mut(&member) else {
                    continue;
                };
                for slot in [&mut mk.current, &mut mk.previous] {
                    let Some(ks) = slot.as_mut().filter(|k| k.epoch == h.key_epoch) else {
                        continue;
                    };
                    if !ks.accepts(h.counter) {
                        // Another copy of the same flood, or a replay, for this candidate: another
                        // member sharing the sender byte may still own the envelope.
                        continue;
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
                    // The signature first: every member holds this sender key, only the sender
                    // can sign. A forgery must not move the sender's replay window.
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
                    ks.mark(hdr.counter);
                    delivered = Some((gid, member, member_node, body.to_vec()));
                    break 'gids;
                }
            }
        }
        let Some((gid, member, member_node, body)) = delivered else {
            return;
        };
        self.dirty_groups.insert(gid);
        // Its direct copies were receptions of that member (search, §11.2).
        self.search_attribute(&h, member, member_node, now);
        if body.first() == Some(&content::SEARCH) {
            if let Some(s) = body.get(1..).and_then(Search::decode) {
                self.on_search(gid, member, member_node, s, now);
            }
            return;
        }
        match Content::decode(&body) {
            Some(Content::Location(p)) => {
                let at = crate::location::receive(
                    &mut self.loc,
                    &mut self.events,
                    member_node,
                    Some(gid),
                    p,
                    snr4,
                    now,
                );
                if let Some(at) = at {
                    let (lat, lon) = crate::location::to_degrees(p.lat, p.lon);
                    self.search_fix(&member, lat, lon, p.prec, at);
                }
            }
            Some(content) => self.events.push_back(Event::GroupMessage {
                gid,
                from: member_node,
                content,
                snr4,
                rssi,
            }),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Config;
    use lc_meshcore::ecdh::{expand_seed, public_key};

    /// Two identities whose public keys share their first byte (the sender byte of envelopes):
    /// seeds counted up until two collide, about 20 tries.
    fn colliding_pair() -> [([u8; 64], PubKey); 2] {
        let mut seen: HashMap<u8, ([u8; 64], PubKey)> = HashMap::new();
        for i in 0u32.. {
            let mut seed = [0u8; 32];
            seed[..4].copy_from_slice(&i.to_le_bytes());
            let prv = expand_seed(&seed);
            let id = public_key(&prv);
            if let Some(first) = seen.insert(id[0], (prv, id)) {
                return [first, (prv, id)];
            }
        }
        unreachable!()
    }

    /// A core that belongs to group `GID` with `members` (identity, sender key, replay floor),
    /// holding each one's sender key at epoch 0.
    fn core_with_group(members: &[(PubKey, [u8; 32], u32)]) -> Core {
        let mut core = Core::new(Config::new("t", [200; 32], [1; 32]));
        core.node = Some([0xEE; 32]);
        let me = *core.key().unwrap();
        let mut list = vec![Member {
            key: me,
            node: [0xEE; 32],
            name: "me".into(),
        }];
        let mut keys = HashMap::new();
        for (i, &(id, key, floor)) in members.iter().enumerate() {
            list.push(Member {
                key: id,
                node: [i as u8 + 1; 32],
                name: format!("m{i}"),
            });
            keys.insert(
                id,
                MemberKeys {
                    current: Some(KeyState::new(0, key, floor)),
                    previous: None,
                },
            );
        }
        core.groups.insert(
            GID,
            Group {
                gid: GID,
                name: "g".into(),
                admin: me,
                epoch: 1,
                members: list,
                my_key: [0; 32],
                my_epoch: 0,
                my_counter: 0,
                keys,
                partials: Vec::new(),
                announce_key: false,
            },
        );
        core
    }

    const GID: GroupId = [5; 16];

    /// The flooded payload of a text from `sender` (identity, sender key) at `counter`,
    /// signed by `signer`.
    fn envelope(
        sender: &PubKey,
        key: &[u8; 32],
        counter: u32,
        signer: &[u8; 64],
        text: &str,
    ) -> Vec<u8> {
        let body = Content::Text {
            sent_at: 1,
            text: text.into(),
        }
        .encode();
        let (epoch_b, counter_b) = ([0u8], counter.to_le_bytes());
        let sig = sign(signer, &signed_parts(&GID, &epoch_b, &counter_b, &body)).unwrap();
        let mut plain = body;
        plain.extend_from_slice(&sig);
        let header = GroupHeader {
            gid_hash: gid_hash(&GID),
            sender: sender[0],
            key_epoch: 0,
            counter,
            index: 0,
            total: 1,
            copy: 0,
        };
        let mut out = [0u8; group::PAYLOAD_MAX];
        let n = group::seal(key, &header, &plain, &mut out).unwrap();
        out[..n].to_vec()
    }

    fn texts(core: &mut Core) -> Vec<String> {
        let mut v = Vec::new();
        while let Some(e) = core.poll_event() {
            if let Event::GroupMessage {
                content: Content::Text { text, .. },
                ..
            } = e
            {
                v.push(text);
            }
        }
        v
    }

    /// A member who holds Bob's sender key (every member does) forges a message in Bob's name
    /// with a huge counter: the signature fails, and Bob's next message still arrives.
    #[test]
    fn forgery_does_not_move_the_replay_window() {
        let [(bob_prv, bob), (mallory_prv, mallory)] = colliding_pair();
        let mut core = core_with_group(&[(bob, [10; 32], 0), (mallory, [20; 32], 0)]);
        let forged = envelope(&bob, &[10; 32], u32::MAX - 1, &mallory_prv, "forged");
        core.on_group_envelope(&forged, 0, 0, 0);
        assert!(texts(&mut core).is_empty());
        let real = envelope(&bob, &[10; 32], 3, &bob_prv, "real");
        core.on_group_envelope(&real, 0, 0, 0);
        assert_eq!(texts(&mut core), vec!["real"]);
        // And it is not delivered twice.
        core.on_group_envelope(&real, 0, 0, 0);
        assert!(texts(&mut core).is_empty());
    }

    /// Two members whose identities share the sender byte, with different replay histories:
    /// each one's messages arrive, in either order.
    #[test]
    fn colliding_senders_are_both_heard() {
        // A has already received counters up to 100; B starts at 0.
        let [(a_prv, a), (b_prv, b)] = colliding_pair();
        assert_eq!(a[0], b[0]);
        let mut core = core_with_group(&[(a, [30; 32], 101), (b, [40; 32], 0)]);
        let from_b = envelope(&b, &[40; 32], 0, &b_prv, "from b");
        let from_a = envelope(&a, &[30; 32], 101, &a_prv, "from a");
        core.on_group_envelope(&from_b, 0, 0, 0);
        core.on_group_envelope(&from_a, 0, 0, 0);
        assert_eq!(texts(&mut core), vec!["from b", "from a"]);
        let from_b2 = envelope(&b, &[40; 32], 1, &b_prv, "b again");
        core.on_group_envelope(&from_b2, 0, 0, 0);
        assert_eq!(texts(&mut core), vec!["b again"]);
    }
}

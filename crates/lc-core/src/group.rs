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

use crate::content::{Content, Notice, notice};
use crate::core::{Core, Error, Event, MsgId, PubKey};
use crate::identity::Invite;

pub type GroupId = [u8; 16];

/// Members per group, admin included.
pub const MAX_MEMBERS: usize = 32;
/// How long an invitation admits people (§10.5).
pub const INVITE_TTL_MS: Millis = 10 * 60_000;
/// How late by our clock a scanned invitation may be and still be tried: phone clocks differ.
pub const INVITE_SKEW_S: u64 = 120;
/// Every group message is flooded this many times by default (§10.3): a flood lost at its
/// first hop, to a collision with other traffic, is lost for everyone.
pub const COPIES: u8 = 2;
/// A partial (first of two fragments) is forgotten after this long.
const PARTIAL_MS: Millis = 300_000;
/// Keys that arrived before their group state are kept this long.
const PENDING_KEY_MS: Millis = 600_000;
/// The admin forwards a member's new key to the members it admitted this recently (§10.2).
const ADMITTED_MS: Millis = 600_000;
/// Version byte of [`Group::snapshot`]: 2 since members are identities with a node (ADR-0013).
const SNAPSHOT_VERSION: u8 = 2;
/// Envelopes whose sender key has not arrived yet are kept this long (§10.3).
const HELD_MS: Millis = 600_000;
/// And at most this many.
const HELD_MAX: usize = 16;

/// A group frame waiting for its time (§10.3).
#[derive(Clone, Debug)]
pub(crate) struct GroupFrame {
    pub(crate) due: Millis,
    pub(crate) raw: Vec<u8>,
    /// The message it belongs to.
    pub(crate) id: MsgId,
    /// Sent by the app (reported with [`Event::GroupSent`]), not by the core itself
    /// (positions, search).
    pub(crate) user: bool,
    pub(crate) copy: u8,
    /// Its rank among the message's frames: 0 leaves first.
    pub(crate) seq: u8,
    /// The last frame of the first copy: the message has gone out once all of it left.
    pub(crate) ends_first_copy: bool,
}

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
    /// The key's owner.
    from: PubKey,
    /// Relayed by this identity (§10.2), to check against the admin once the state is here;
    /// `None`: from its owner.
    via: Option<PubKey>,
    gid: GroupId,
    epoch: u8,
    key: [u8; 32],
    next_counter: u32,
    at: Millis,
}

/// One key in SENDER_KEYS: owner, key epoch, key, next counter (§10.2).
type KeyEntry = (PubKey, u8, [u8; 32], u32);

/// `e` is a later key epoch than `current` (epochs wrap, §10.2).
fn newer(e: u8, current: u8) -> bool {
    (1..=127).contains(&e.wrapping_sub(current))
}

// ----- control datagrams (§10.1, §10.2) ----------------------------------------------------

/// The admin of a state as the actor of a notice: its node and name.
fn admin_actor(state: &State) -> (PubKey, String) {
    state
        .members
        .iter()
        .find(|m| m.key == state.admin)
        .map_or(([0; 32], String::new()), |m| (m.node, m.name.clone()))
}

/// `name` cut to 32 bytes on a character boundary, as a state carries it.
fn cut_name(name: &str) -> String {
    let mut end = name.len().min(32);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name.get(..end).unwrap_or_default().to_owned()
}

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
    String::from_utf8(take(b, n)?.to_vec()).ok()
}

/// The group state as sent by the admin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct State {
    gid: GroupId,
    epoch: u32,
    admin: PubKey,
    name: String,
    members: Vec<Member>,
    /// Why members are gone (§10.1): [`cause::LEFT`] or [`cause::REMOVED`].
    cause: Option<u8>,
}

/// GROUP_STATE `cause` values (§10.1).
pub mod cause {
    /// They left (GROUP_LEAVE).
    pub const LEFT: u8 = 1;
    /// The admin removed them.
    pub const REMOVED: u8 = 2;
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
        v.extend(self.cause);
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
        // One member: the admin closing the group (§10.1, ADR-0017).
        if !(1..=MAX_MEMBERS).contains(&n) {
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
        let cause = match b {
            [] => None,
            [c] => Some(*c),
            _ => return None,
        };
        if distinct.len() != n || !distinct.contains(&admin) {
            return None;
        }
        Some(Self {
            gid,
            epoch,
            admin,
            name,
            members,
            cause,
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

/// SENDER_KEYS (0x14, §10.2): keys the admin relays.
fn encode_sender_keys(gid: &GroupId, entries: &[KeyEntry]) -> Vec<u8> {
    let mut v = vec![content::SENDER_KEYS];
    v.extend_from_slice(gid);
    v.push(entries.len() as u8);
    for (owner, epoch, key, next) in entries {
        v.extend_from_slice(owner);
        v.push(*epoch);
        v.extend_from_slice(key);
        v.extend_from_slice(&next.to_le_bytes());
    }
    v
}

fn decode_sender_keys(datagram: &[u8]) -> Option<(GroupId, Vec<KeyEntry>)> {
    let (&t, mut b) = datagram.split_first()?;
    if t != content::SENDER_KEYS {
        return None;
    }
    let gid: GroupId = take(&mut b, 16)?.try_into().ok()?;
    let n = usize::from(*take(&mut b, 1)?.first()?);
    if !(1..=MAX_MEMBERS).contains(&n) || b.len() != n * 69 {
        return None;
    }
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        let owner: PubKey = take(&mut b, 32)?.try_into().ok()?;
        let epoch = *take(&mut b, 1)?.first()?;
        let key: [u8; 32] = take(&mut b, 32)?.try_into().ok()?;
        let next = u32::from_le_bytes(take(&mut b, 4)?.try_into().ok()?);
        entries.push((owner, epoch, key, next));
    }
    Some((gid, entries))
}

/// A group control datagram as decoded (§10.1, §10.2): for vectors and the fuzzer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Control {
    State {
        gid: GroupId,
        epoch: u32,
        admin: PubKey,
        name: String,
        members: Vec<Member>,
        cause: Option<u8>,
    },
    SenderKey {
        gid: GroupId,
        epoch: u8,
        key: [u8; 32],
        next_counter: u32,
    },
    /// Keys the admin relays: (owner, key epoch, key, next counter).
    SenderKeys {
        gid: GroupId,
        entries: Vec<(PubKey, u8, [u8; 32], u32)>,
    },
}

impl Control {
    pub fn decode(datagram: &[u8]) -> Option<Self> {
        match *datagram.first()? {
            content::GROUP_STATE => State::decode(datagram).map(|s| Self::State {
                gid: s.gid,
                epoch: s.epoch,
                admin: s.admin,
                name: s.name,
                members: s.members,
                cause: s.cause,
            }),
            content::SENDER_KEY => {
                decode_sender_key(datagram).map(|(gid, epoch, key, next_counter)| Self::SenderKey {
                    gid,
                    epoch,
                    key,
                    next_counter,
                })
            }
            content::SENDER_KEYS => {
                decode_sender_keys(datagram).map(|(gid, entries)| Self::SenderKeys { gid, entries })
            }
            _ => None,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::State {
                gid,
                epoch,
                admin,
                name,
                members,
                cause,
            } => State {
                gid: *gid,
                epoch: *epoch,
                admin: *admin,
                name: name.clone(),
                members: members.clone(),
                cause: *cause,
            }
            .encode(),
            Self::SenderKey {
                gid,
                epoch,
                key,
                next_counter,
            } => encode_sender_key(gid, *epoch, key, *next_counter),
            Self::SenderKeys { gid, entries } => encode_sender_keys(gid, entries),
        }
    }
}

/// `new` merged into `queued`, both waiting for the same peer (§10.2): a newer state of the
/// same group replaces the older; keys of the same group join (the newer entry per owner).
pub(crate) fn merge_control(queued: &[u8], new: &[u8]) -> Option<Vec<u8>> {
    match (queued.first(), new.first()) {
        (Some(&content::GROUP_STATE), Some(&content::GROUP_STATE)) => {
            let (q, mut n) = (State::decode(queued)?, State::decode(new)?);
            if q.gid != n.gid || n.epoch <= q.epoch {
                return None;
            }
            // Why members went is kept for the peer that never got the older state.
            if n.cause.is_none() {
                n.cause = q.cause;
            }
            Some(n.encode())
        }
        (Some(&content::SENDER_KEYS), Some(&content::SENDER_KEYS)) => {
            let (qg, mut entries) = decode_sender_keys(queued)?;
            let (ng, more) = decode_sender_keys(new)?;
            if qg != ng {
                return None;
            }
            for e in more {
                match entries.iter_mut().find(|q| q.0 == e.0) {
                    Some(q) => *q = e,
                    None => entries.push(e),
                }
            }
            (entries.len() <= MAX_MEMBERS).then(|| encode_sender_keys(&qg, &entries))
        }
        _ => None,
    }
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

    /// Creates a group with us as admin and `members` (contacts, possibly none) as the other
    /// members.
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
        // The admin may start alone and invite in person (§10.5).
        if list.len() > MAX_MEMBERS {
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
        self.broadcast_state(&gid, &[], None);
        self.send_my_key(&gid, None);
        self.events.push_back(Event::GroupUpdated { gid });
        let actor = self.me_as_actor();
        self.changed(gid, notice::CREATED, actor, Vec::new(), String::new());
        self.tick(now);
        Ok(gid)
    }

    /// Adds members (admin only, an invitation, ADR-0017): new epoch sent to everyone,
    /// newcomers included; our key goes to the newcomers (the others send theirs on the state).
    /// `nodes` are contacts whose identity is known.
    pub fn add_members(
        &mut self,
        gid: &GroupId,
        nodes: &[PubKey],
        now: Millis,
    ) -> Result<(), Error> {
        let me = self.key.ok_or(Error::NoIdentity)?;
        if self.groups.get(gid).ok_or(Error::UnknownGroup)?.admin != me {
            return Err(Error::NotAllowed);
        }
        let mut added = Vec::new();
        for k in nodes {
            let c = self.contacts.get(k).ok_or(Error::UnknownContact)?;
            let id = c.identity.ok_or(Error::IdentityUnknown)?;
            added.push(Member {
                key: id,
                node: *k,
                name: c.name.clone(),
            });
        }
        let g = self.groups.get_mut(gid).ok_or(Error::UnknownGroup)?;
        added.retain(|a| g.members.iter().all(|m| m.key != a.key));
        if added.is_empty() {
            return Ok(());
        }
        if g.members.len() + added.len() > MAX_MEMBERS {
            return Err(Error::TooLarge);
        }
        let ids: Vec<PubKey> = added.iter().map(|m| m.key).collect();
        let subjects = added.iter().map(|m| (m.node, m.name.clone())).collect();
        g.members.extend(added);
        g.epoch += 1;
        self.dirty_groups.insert(*gid);
        self.broadcast_state(gid, &[], None);
        let actor = self.me_as_actor();
        self.changed(*gid, notice::JOINED, actor, subjects, String::new());
        // The newcomers get every key from us, after the state (§10.2).
        self.send_bundle(gid, &ids);
        self.admitted.retain(|(_, _, at)| at + ADMITTED_MS > now);
        self.admitted.extend(ids.iter().map(|id| (*gid, *id, now)));
        self.events.push_back(Event::GroupUpdated { gid: *gid });
        self.tick(now);
        Ok(())
    }

    /// Leaves a group (ADR-0017): a member asks the admin to remove it and forgets the group at
    /// once; the admin closes it (a last state listing the admin alone, so everyone forgets it).
    pub fn leave_group(&mut self, gid: &GroupId, now: Millis) -> Result<(), Error> {
        let me = self.key.ok_or(Error::NoIdentity)?;
        let g = self.groups.get_mut(gid).ok_or(Error::UnknownGroup)?;
        if g.admin == me {
            let others: Vec<PubKey> = g
                .members
                .iter()
                .filter(|m| m.key != me)
                .map(|m| m.node)
                .collect();
            g.members.retain(|m| m.key == me);
            g.epoch += 1;
            self.broadcast_state(gid, &others, None);
            let actor = self.me_as_actor();
            self.changed(*gid, notice::CLOSED, actor, Vec::new(), String::new());
        } else if let Some(admin) = g.members.iter().find(|m| m.key == g.admin).map(|m| m.node) {
            let mut leave = vec![content::GROUP_LEAVE];
            leave.extend_from_slice(gid);
            leave.extend_from_slice(&g.epoch.to_le_bytes());
            self.enqueue_internal(&admin, leave);
            let actor = self.me_as_actor();
            self.changed(*gid, notice::WE_LEFT, actor, Vec::new(), String::new());
        }
        self.groups.remove(gid);
        self.dirty_groups.insert(*gid);
        self.events.push_back(Event::GroupLeft { gid: *gid });
        self.tick(now);
        Ok(())
    }

    /// A member asks us, the admin, to remove it (§10.4).
    fn on_group_leave(&mut self, from: &PubKey, datagram: &[u8], now: Millis) {
        let Some(me) = self.key else {
            return;
        };
        let Some(gid) = datagram.get(1..17).and_then(|g| GroupId::try_from(g).ok()) else {
            return;
        };
        let Some(g) = self.groups.get_mut(&gid) else {
            return;
        };
        if g.admin != me || *from == me || !g.is_member(from) {
            return;
        }
        let leaver = g.members.iter().find(|m| m.key == *from).cloned();
        g.members.retain(|m| m.key != *from);
        g.epoch += 1;
        g.keys.remove(from);
        self.dirty_groups.insert(gid);
        let also: Vec<PubKey> = leaver.iter().map(|m| m.node).collect();
        self.broadcast_state(&gid, &also, Some(cause::LEFT));
        self.rotate_my_key(&gid);
        self.events.push_back(Event::GroupUpdated { gid });
        if let Some(m) = leaver {
            let who = (m.node, m.name);
            self.changed(gid, notice::LEFT, who.clone(), vec![who], String::new());
        }
        self.tick(now);
    }

    /// An invitation to group `gid` (admin only, §10.5), shown as a QR code: it admits anyone
    /// who scans it for 10 minutes, until [`Core::revoke_invite`] or a restart.
    pub fn create_invite(&mut self, gid: &GroupId, now: Millis) -> Result<Invite, Error> {
        let (me, node) = (
            self.key.ok_or(Error::NoIdentity)?,
            self.node.ok_or(Error::NoIdentity)?,
        );
        let g = self.groups.get(gid).ok_or(Error::UnknownGroup)?;
        if g.admin != me {
            return Err(Error::NotAllowed);
        }
        let group_name = g.name.clone();
        let unix = self.unix_s(now).ok_or(Error::NoClock)?;
        let mut token = [0u8; 16];
        self.rng.fill_bytes(&mut token);
        self.invites.retain(|_, (_, until)| *until > now);
        self.invites.insert(token, (*gid, now + INVITE_TTL_MS));
        Ok(Invite {
            admin: me,
            node,
            token,
            expires: u32::try_from(unix + INVITE_TTL_MS / 1000).unwrap_or(u32::MAX),
            admin_name: self.name.clone(),
            group_name,
        })
    }

    /// Closes an invitation: its token admits no one any more.
    pub fn revoke_invite(&mut self, token: &[u8; 16]) {
        self.invites.remove(token);
    }

    /// Asks to join a group with a scanned invitation (§10.5). The admin becomes a contact,
    /// verified (scanned in person), and gets GROUP_JOIN over a pairwise session, sent again
    /// until it arrives. A zero-hop advert goes first: the admin's node, nearby, learns ours
    /// and can answer the handshake (§4.6). [`Event::GroupJoined`] follows once admitted.
    pub fn join(&mut self, invite: &Invite, now: Millis) -> Result<(), Error> {
        let me = self.key.ok_or(Error::NoIdentity)?;
        if invite.admin == me || self.node == Some(invite.node) {
            return Err(Error::NotAllowed);
        }
        if self
            .unix_s(now)
            .is_some_and(|t| t > u64::from(invite.expires) + INVITE_SKEW_S)
        {
            return Err(Error::Expired);
        }
        let (name, path) = self.contacts.get(&invite.node).map_or_else(
            || (invite.admin_name.clone(), None),
            |c| (c.name.clone(), c.out_path.clone()),
        );
        self.add_contact(invite.node, &name, path);
        self.set_identity(&invite.node, invite.admin, true);
        let mut join = vec![content::GROUP_JOIN];
        join.extend_from_slice(&invite.token);
        self.unknown_to(&invite.node);
        self.enqueue_internal(&invite.node, join);
        self.send_advert(false, now);
        Ok(())
    }

    /// A GROUP_JOIN from node `node`, whose identity the session proved (§10.5): a valid token
    /// admits it as an invitation does. Anything else is dropped without an answer.
    fn on_group_join(&mut self, node: &PubKey, datagram: &[u8], now: Millis) {
        let Some(me) = self.key else {
            return;
        };
        let Some(token) = datagram
            .get(1..17)
            .and_then(|t| <[u8; 16]>::try_from(t).ok())
        else {
            return;
        };
        let Some(&(gid, until)) = self.invites.get(&token) else {
            return;
        };
        if now >= until {
            self.invites.remove(&token);
            return;
        }
        let (Some(id), Some(g)) = (self.identity_of(node), self.groups.get(&gid)) else {
            return;
        };
        if g.admin != me || id == me {
            return;
        }
        if g.is_member(&id) {
            // Asked again: it lost the group (a phone restored from its phrase). The state and
            // our key again, to it alone.
            let state = State {
                gid,
                epoch: g.epoch,
                admin: g.admin,
                name: g.name.clone(),
                members: g.members.clone(),
                cause: None,
            }
            .encode();
            self.enqueue_internal(node, state);
            self.send_my_key(&gid, Some(&[id]));
            self.tick(now);
            return;
        }
        if self.add_members(&gid, &[*node], now).is_ok() {
            self.events
                .push_back(Event::MemberJoined { gid, node: *node });
        }
    }

    /// Renames a group we administer (§10.1): a new epoch with the new name, to every member;
    /// nobody is gone, so no key changes. The name is cut to 32 bytes.
    pub fn rename_group(&mut self, gid: &GroupId, name: &str, now: Millis) -> Result<(), Error> {
        let me = self.key.ok_or(Error::NoIdentity)?;
        let name = cut_name(name.trim());
        if name.is_empty() {
            return Err(Error::Empty);
        }
        let g = self.groups.get_mut(gid).ok_or(Error::UnknownGroup)?;
        if g.admin != me {
            return Err(Error::NotAllowed);
        }
        if g.name == name {
            return Ok(());
        }
        g.name.clone_from(&name);
        g.epoch += 1;
        self.dirty_groups.insert(*gid);
        self.broadcast_state(gid, &[], None);
        self.events.push_back(Event::GroupUpdated { gid: *gid });
        let actor = self.me_as_actor();
        self.changed(*gid, notice::RENAMED, actor, Vec::new(), name);
        self.tick(now);
        Ok(())
    }

    /// Ourselves as the actor of a notice: our node and name.
    fn me_as_actor(&self) -> (PubKey, String) {
        (self.node.unwrap_or([0; 32]), self.name.clone())
    }

    /// A notice for the group's conversation (ADR-0021).
    fn changed(
        &mut self,
        gid: GroupId,
        kind: u8,
        actor: (PubKey, String),
        subjects: Vec<(PubKey, String)>,
        text: String,
    ) {
        self.events.push_back(Event::GroupChanged {
            gid,
            notice: Notice {
                kind,
                actor: actor.0,
                actor_name: actor.1,
                subjects,
                text,
            },
        });
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
        let Some((id, name)) = g.member_by_node(member).map(|m| (m.key, m.name.clone())) else {
            return Err(Error::NotAllowed);
        };
        if g.admin != me || id == me {
            return Err(Error::NotAllowed);
        }
        g.members.retain(|m| m.key != id);
        g.epoch += 1;
        g.keys.remove(&id);
        self.dirty_groups.insert(*gid);
        self.broadcast_state(gid, &[*member], Some(cause::REMOVED));
        self.rotate_my_key(gid);
        self.events.push_back(Event::GroupUpdated { gid: *gid });
        let actor = self.me_as_actor();
        self.changed(
            *gid,
            notice::REMOVED,
            actor,
            vec![(*member, name)],
            String::new(),
        );
        self.tick(now);
        Ok(())
    }

    /// Sends the group state to every other member, and to `also` (nodes of removed members).
    fn broadcast_state(&mut self, gid: &GroupId, also: &[PubKey], cause: Option<u8>) {
        let (Some(me), Some(g)) = (self.key, self.groups.get(gid)) else {
            return;
        };
        let state = State {
            gid: *gid,
            epoch: g.epoch,
            admin: g.admin,
            name: g.name.clone(),
            members: g.members.clone(),
            cause,
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
            self.enqueue_control(&k, state.clone());
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

    /// A group control datagram (GROUP_STATE, SENDER_KEY, GROUP_LEAVE, GROUP_JOIN; §10) from
    /// node `from`, whose identity the session proved.
    pub(crate) fn on_group_control(&mut self, from: &PubKey, datagram: &[u8], now: Millis) {
        let node = *from;
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
            Some(&content::GROUP_LEAVE) if datagram.len() == 21 => {
                let from = *from;
                self.on_group_leave(&from, datagram, now);
            }
            Some(&content::GROUP_JOIN) if datagram.len() == 17 => {
                self.on_group_join(&node, datagram, now);
            }
            Some(&content::SENDER_KEYS) => {
                if let Some((gid, entries)) = decode_sender_keys(datagram) {
                    let from = *from;
                    self.on_sender_keys(&from, gid, entries, now);
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
        let admin = admin_actor(&state);
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
                // Our first key goes to the admin, who forwards it (§10.2).
                self.send_my_key(&gid, Some(&[state.admin]));
                self.events.push_back(Event::GroupJoined {
                    gid,
                    name: state.name,
                    admin: state.admin,
                });
                self.changed(gid, notice::ADDED_US, admin, Vec::new(), String::new());
            }
            Some(g) => {
                if g.admin != *from || state.epoch <= g.epoch {
                    return;
                }
                if !listed {
                    self.groups.remove(&gid);
                    self.events.push_back(Event::GroupLeft { gid });
                    // Closed (the admin alone, no cause), or we were removed.
                    let kind = if state.members.len() == 1 && state.cause.is_none() {
                        notice::CLOSED
                    } else {
                        notice::REMOVED_US
                    };
                    self.changed(gid, kind, admin, Vec::new(), String::new());
                    return;
                }
                let old: HashSet<PubKey> = g.members.iter().map(|m| m.key).collect();
                let new: HashSet<PubKey> = state.members.iter().map(|m| m.key).collect();
                let gone: Vec<(PubKey, String)> = g
                    .members
                    .iter()
                    .filter(|m| !new.contains(&m.key))
                    .map(|m| (m.node, m.name.clone()))
                    .collect();
                let joined: Vec<(PubKey, String)> = state
                    .members
                    .iter()
                    .filter(|m| !old.contains(&m.key))
                    .map(|m| (m.node, m.name.clone()))
                    .collect();
                let removed: Vec<PubKey> = old.difference(&new).copied().collect();
                let renamed = (g.name != state.name).then(|| state.name.clone());
                g.name = state.name;
                g.epoch = state.epoch;
                g.members = state.members;
                for k in &removed {
                    g.keys.remove(k);
                }
                // Only adds: the admin carries our key to the newcomers (§10.2).
                if !removed.is_empty() {
                    self.rotate_my_key(&gid);
                }
                self.events.push_back(Event::GroupUpdated { gid });
                if !gone.is_empty() {
                    let (kind, actor) = match state.cause {
                        Some(cause::LEFT) => (notice::LEFT, gone[0].clone()),
                        Some(cause::REMOVED) => (notice::REMOVED, admin.clone()),
                        _ => (notice::GONE, admin.clone()),
                    };
                    self.changed(gid, kind, actor, gone, String::new());
                }
                if !joined.is_empty() {
                    self.changed(gid, notice::JOINED, admin.clone(), joined, String::new());
                }
                if let Some(name) = renamed {
                    self.changed(gid, notice::RENAMED, admin, Vec::new(), name);
                }
            }
        }
        // Keys that came before this state.
        let pending: Vec<PendingKey> = std::mem::take(&mut self.pending_keys);
        for p in pending {
            if p.gid == gid {
                match p.via {
                    None => self.on_sender_key(&p.from, p.gid, p.epoch, p.key, p.next_counter, now),
                    Some(via) => {
                        let entry = (p.from, p.epoch, p.key, p.next_counter);
                        self.on_sender_keys(&via, gid, vec![entry], now);
                    }
                }
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
                    via: None,
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
        let first = g.keys.get(from).is_none_or(|mk| mk.current.is_none());
        if !self.adopt_key(gid, from, epoch, key, next, false, now) {
            return;
        }
        // The admin forwards it (§10.2): to everyone for a member's first key, else to the
        // members it admitted lately (a key in flight during their admission).
        let me = self.key;
        let Some(g) = self.groups.get(&gid) else {
            return;
        };
        if Some(g.admin) != me {
            return;
        }
        self.admitted.retain(|(_, _, at)| at + ADMITTED_MS > now);
        let targets: Vec<PubKey> = g
            .members
            .iter()
            .filter(|m| m.key != *from && Some(m.key) != me)
            .filter(|m| {
                first
                    || self
                        .admitted
                        .iter()
                        .any(|(a, id, _)| *a == gid && *id == m.key)
            })
            .map(|m| m.node)
            .collect();
        let datagram = encode_sender_keys(&gid, &[(*from, epoch, key, next)]);
        for node in targets {
            self.enqueue_control(&node, datagram.clone());
        }
    }

    /// SENDER_KEYS from `from` (§10.2): keys the admin relays. Only from the group's admin;
    /// a relayed key never replaces a newer one.
    fn on_sender_keys(&mut self, from: &PubKey, gid: GroupId, entries: Vec<KeyEntry>, now: Millis) {
        let me = self.key;
        let admin = self.groups.get(&gid).map(|g| g.admin);
        if admin.is_some_and(|a| a != *from) {
            return;
        }
        for (owner, epoch, key, next) in entries {
            if Some(owner) == me {
                continue;
            }
            let listed = self.groups.get(&gid).is_some_and(|g| g.is_member(&owner));
            if !listed {
                // A group or an owner whose state has not reached us yet.
                if self.pending_keys.len() < 32 {
                    self.pending_keys.push(PendingKey {
                        from: owner,
                        via: Some(*from),
                        gid,
                        epoch,
                        key,
                        next_counter: next,
                        at: now,
                    });
                }
                continue;
            }
            self.adopt_key(gid, &owner, epoch, key, next, true, now);
        }
    }

    /// Stores `owner`'s key; a relayed one only when newer than ours. True when adopted; held
    /// envelopes are tried again.
    #[allow(clippy::too_many_arguments)]
    fn adopt_key(
        &mut self,
        gid: GroupId,
        owner: &PubKey,
        epoch: u8,
        key: [u8; 32],
        next: u32,
        relayed: bool,
        now: Millis,
    ) -> bool {
        let Some(g) = self.groups.get_mut(&gid) else {
            return false;
        };
        let mk = g.keys.entry(*owner).or_default();
        match mk.current.as_ref() {
            Some(c) if c.epoch == epoch => return false,
            Some(c) if relayed && !newer(epoch, c.epoch) => return false,
            _ => {}
        }
        mk.previous = mk.current.take();
        mk.current = Some(KeyState::new(epoch, key, next));
        self.dirty_groups.insert(gid);
        // Messages that came before this key can now be read.
        let held: Vec<Held> = std::mem::take(&mut self.held).into();
        for h in held {
            self.on_group_envelope(&h.payload, h.snr4, h.rssi, h.at.max(now));
        }
        true
    }

    /// One SENDER_KEYS to each newcomer (identities) with every current key we hold, ours
    /// included (§10.2).
    fn send_bundle(&mut self, gid: &GroupId, newcomers: &[PubKey]) {
        let (Some(me), Some(g)) = (self.key, self.groups.get(gid)) else {
            return;
        };
        let mut out = Vec::new();
        for id in newcomers {
            let Some(node) = g.members.iter().find(|m| m.key == *id).map(|m| m.node) else {
                continue;
            };
            let mut entries: Vec<KeyEntry> = vec![(me, g.my_epoch, g.my_key, g.my_counter)];
            for m in &g.members {
                if m.key == me || m.key == *id {
                    continue;
                }
                if let Some(c) = g.keys.get(&m.key).and_then(|mk| mk.current.as_ref()) {
                    entries.push((m.key, c.epoch, c.key, c.floor));
                }
            }
            out.push((node, encode_sender_keys(gid, &entries)));
        }
        for (node, datagram) in out {
            self.enqueue_control(&node, datagram);
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
        self.send_group(gid, &body, true, now)
    }

    /// Floods a datagram (`content_type ‖ body`) to the group, signed (§10.3): an
    /// announcement, for instance (§14.1).
    pub fn send_group_datagram(
        &mut self,
        gid: &GroupId,
        datagram: &[u8],
        now: Millis,
    ) -> Result<MsgId, Error> {
        self.send_group(gid, datagram, true, now)
    }

    /// Fragments and airtime (ms, our own floods: every copy, no relays) of a group message of
    /// `body_len` bytes (§10.3): what the sender spends; relays add their own.
    pub fn estimate_group_send(&self, body_len: usize) -> (u8, Millis) {
        let plain = body_len.max(1) + SIGNATURE_LEN;
        let fragments = plain.div_ceil(group::FRAGMENT_PLAIN_MAX);
        let overhead = group::PAYLOAD_MAX - group::FRAGMENT_PLAIN_MAX;
        let transport = if self.scope.is_some() { 4 } else { 0 };
        let mut airtime = 0;
        for i in 0..fragments {
            let chunk = (plain - i * group::FRAGMENT_PLAIN_MAX).min(group::FRAGMENT_PLAIN_MAX);
            airtime += self.radio.airtime_us(2 + transport + chunk + overhead) / 1000;
        }
        (
            fragments.min(255) as u8,
            airtime * Millis::from(self.group_copies),
        )
    }

    /// `user`: sent by the app, which hears when it has gone out ([`Event::GroupSent`]).
    pub(crate) fn send_group(
        &mut self,
        gid: &GroupId,
        body: &[u8],
        user: bool,
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
        let id = self.next_id;
        self.next_id += 1;
        let mut seq = 0u8;
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
                self.group_tx.push_back(GroupFrame {
                    due,
                    raw,
                    id,
                    user,
                    copy,
                    seq,
                    ends_first_copy: copy == 0 && index + 1 == usize::from(total),
                });
                seq = seq.saturating_add(1);
                due += spacing + Millis::from(self.rng.next_u32()) % spacing.max(1);
            }
        }
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
        while self.group_tx.front().is_some_and(|f| f.due <= now) && now >= self.next_slot {
            let Some(f) = self.group_tx.pop_front() else {
                break;
            };
            self.send_raw_packet(&f.raw, now);
            if f.user && f.ends_first_copy {
                self.events.push_back(Event::GroupSent { id: f.id });
            }
        }
        for g in self.groups.values_mut() {
            g.partials.retain(|p| p.at + PARTIAL_MS > now);
        }
        self.held.retain(|h| h.at + HELD_MS > now);
    }

    pub(crate) fn next_group_wakeup(&self) -> Option<Millis> {
        self.group_tx.front().map(|f| f.due.max(self.next_slot))
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
        // Local notices never come from the air (§6).
        if Content::is_local(&body) {
            return;
        }
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

    /// A full group admits no one, even with a valid invitation; once a place is free, the
    /// same invitation admits (§10.5).
    #[test]
    fn a_full_group_refuses_a_join() {
        let mut core = Core::new(Config::new("t", [200; 32], [1; 32]));
        core.node = Some([0xEE; 32]);
        core.set_clock(1_793_000_000_000, 0);
        let nodes: Vec<PubKey> = (1..=MAX_MEMBERS as u8).map(|i| [i; 32]).collect();
        for (i, n) in nodes.iter().enumerate() {
            core.add_contact(*n, &format!("m{i}"), None);
            core.set_identity(n, [100 + n[0]; 32], false);
        }
        let (members, joiner) = nodes.split_at(MAX_MEMBERS - 1);
        let gid = core.create_group("Full", members, 0).unwrap();
        let invite = core.create_invite(&gid, 0).unwrap();
        let mut join = vec![content::GROUP_JOIN];
        join.extend_from_slice(&invite.token);
        while core.poll_event().is_some() {}

        core.on_group_control(&joiner[0], &join, 1_000);
        assert_eq!(core.groups[&gid].members.len(), MAX_MEMBERS);
        assert!(
            std::iter::from_fn(|| core.poll_event())
                .all(|e| !matches!(e, Event::MemberJoined { .. }))
        );

        core.remove_member(&gid, &members[0], 2_000).unwrap();
        core.on_group_control(&joiner[0], &join, 3_000);
        assert!(core.groups[&gid].is_member(&[100 + joiner[0][0]; 32]));
        assert!(std::iter::from_fn(|| core.poll_event()).any(|e| e
            == Event::MemberJoined {
                gid,
                node: joiner[0]
            }));
    }

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

    /// Keys `node` will receive, from what is queued for it: (owner, key epoch).
    fn relayed_to(core: &Core, node: &PubKey) -> Vec<(PubKey, u8)> {
        core.queued(node)
            .iter()
            .filter_map(|d| decode_sender_keys(d))
            .flat_map(|(_, e)| e.into_iter().map(|(o, ep, _, _)| (o, ep)))
            .collect()
    }

    /// The admin relays (§10.2): a newcomer gets every key in one bundle; a member's new key
    /// soon after reaches the newcomer too; a newcomer's first key goes to everyone.
    #[test]
    fn the_admin_relays_keys_to_newcomers() {
        let (a, b) = ([0xA1; 32], [0xB1; 32]);
        let mut core = core_with_group(&[(a, [1; 32], 0), (b, [2; 32], 0)]);
        let me = *core.key().unwrap();
        let (n, n_id) = ([0x4E; 32], [0x4F; 32]);
        core.add_contact(n, "newcomer", None);
        core.set_identity(&n, n_id, false);
        core.add_members(&GID, &[n], 1_000).unwrap();
        let mut got = relayed_to(&core, &n);
        got.sort_unstable();
        let mut want = vec![(a, 0), (b, 0), (me, 0)];
        want.sort_unstable();
        assert_eq!(got, want, "one bundle, ours included");

        // `a` restarted just before: its new key reaches the newcomer through us.
        let a_node = [1; 32];
        core.on_sender_key(&a, GID, 1, [9; 32], 0, 2_000);
        assert!(relayed_to(&core, &n).contains(&(a, 1)));
        assert!(
            relayed_to(&core, &a_node).is_empty(),
            "not back to its owner"
        );
        // …but not to a member admitted long ago (it sent its key there itself).
        assert!(relayed_to(&core, &[2; 32]).is_empty());

        // The newcomer's first key goes to every other member.
        core.on_sender_key(&n_id, GID, 0, [7; 32], 0, 3_000);
        assert!(relayed_to(&core, &a_node).contains(&(n_id, 0)));
        assert!(relayed_to(&core, &[2; 32]).contains(&(n_id, 0)));
    }

    /// Waiting states merge: the newer replaces the older and keeps why members went
    /// (§10.1); relayed keys of one group join, the newer per owner (§10.2).
    #[test]
    fn waiting_control_merges() {
        let m = |k: u8| Member {
            key: [k; 32],
            node: [k; 32],
            name: format!("m{k}"),
        };
        let state = |epoch, members: Vec<Member>, cause| {
            State {
                gid: GID,
                epoch,
                admin: [1; 32],
                name: "g".into(),
                members,
                cause,
            }
            .encode()
        };
        let older = state(2, vec![m(1), m(2)], Some(cause::LEFT));
        let newer = state(3, vec![m(1), m(2), m(4)], None);
        let merged = State::decode(&merge_control(&older, &newer).unwrap()).unwrap();
        assert_eq!(
            (merged.epoch, merged.cause, merged.members.len()),
            (3, Some(cause::LEFT), 3)
        );
        assert_eq!(merge_control(&newer, &older), None, "never an older state");
        let a = encode_sender_keys(&GID, &[([7; 32], 0, [1; 32], 0)]);
        let b = encode_sender_keys(&GID, &[([7; 32], 1, [2; 32], 0), ([8; 32], 0, [3; 32], 0)]);
        let (_, keys) = decode_sender_keys(&merge_control(&a, &b).unwrap()).unwrap();
        assert_eq!(
            keys.iter().map(|k| (k.0[0], k.1)).collect::<Vec<_>>(),
            vec![(7, 1), (8, 0)]
        );
        assert_eq!(merge_control(&a, &older), None);
    }

    /// A relayed key comes only from the admin and never replaces a newer one (§10.2).
    #[test]
    fn relayed_keys_are_checked() {
        let (admin, owner) = ([0xA1; 32], [0xB1; 32]);
        let mut core = core_with_group(&[(admin, [1; 32], 0), (owner, [2; 32], 0)]);
        if let Some(g) = core.groups.get_mut(&GID) {
            g.admin = admin;
        }
        let epoch_of = |core: &Core| {
            core.groups[&GID].keys[&owner]
                .current
                .as_ref()
                .map(|k| (k.epoch, k.key))
        };
        // From a member who is not the admin: ignored.
        core.on_sender_keys(&owner, GID, vec![(admin, 5, [5; 32], 0)], 0);
        assert_eq!(
            core.groups[&GID].keys[&admin]
                .current
                .as_ref()
                .map(|k| k.epoch),
            Some(0)
        );
        // The same epoch, or an older one (wrapping): ignored.
        core.on_sender_keys(&admin, GID, vec![(owner, 0, [8; 32], 0)], 0);
        core.on_sender_keys(&admin, GID, vec![(owner, 200, [8; 32], 0)], 0);
        assert_eq!(epoch_of(&core), Some((0, [2; 32])));
        // A newer one: adopted.
        core.on_sender_keys(&admin, GID, vec![(owner, 1, [3; 32], 0)], 0);
        assert_eq!(epoch_of(&core), Some((1, [3; 32])));
        // From its owner, any other epoch is adopted.
        core.on_sender_key(&owner, GID, 0, [4; 32], 0, 0);
        assert_eq!(epoch_of(&core), Some((0, [4; 32])));
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

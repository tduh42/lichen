//! Private groups (`docs/spec/protocol.md` §10, ADR-0009): group state from the admin, a sender
//! key per member, signed messages flooded on channel hash `0x7C`.

use std::collections::{HashMap, HashSet, VecDeque};

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
const SNAPSHOT_VERSION: u8 = 3;
/// Counters from here on carry what the core sends by itself (positions, search packets), with
/// a replay window of their own: a gap below is a lost message (§10.3, §10.6).
const AUX: u32 = 1 << 31;
/// Envelopes whose sender key has not arrived yet are kept this long (§10.3).
const HELD_MS: Millis = 600_000;
/// And at most this many.
const HELD_MAX: usize = 16;
/// Group messages each member keeps to resend (§10.6), and for how long.
const KEPT_MAX: usize = 64;
const KEPT_MS: Millis = 3_600_000;
/// Missing counters remembered per group (§10.6).
const GAPS_MAX: usize = 16;
/// Requests for messages we miss too, answered once we recover them (§10.6), per group.
const OWED_MAX: usize = 32;
/// A gap is asked for this long after it is seen, plus up to as much again (§10.6).
const ASK_AFTER_MS: Millis = 20_000;
/// How long an answer is awaited before asking the other one, then giving up.
const ANSWER_MS: Millis = 120_000;
/// Counters per GROUP_MISSING, payloads per GROUP_RESEND (§10.6).
pub const MISSING_MAX: usize = 8;
pub const RESEND_MAX: usize = 16;

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

/// One sender key of a member, with its replay windows: messages, and the core's own traffic
/// from [`AUX`] (§10.3).
#[derive(Clone, Debug)]
struct KeyState {
    epoch: u8,
    key: [u8; 32],
    /// Message counters below this were never used under this key.
    floor: u32,
    highest: Option<u32>,
    /// Bit `d` set: counter `highest − d` already delivered.
    seen: u64,
    aux_highest: Option<u32>,
    aux_seen: u64,
}

/// A counter fits the window (`highest`, `seen`): newer, or within 64 and not seen.
fn window_accepts(highest: Option<u32>, seen: u64, counter: u32) -> bool {
    match highest {
        None => true,
        Some(h) if counter > h => true,
        Some(h) => {
            let d = h - counter;
            d < 64 && (seen >> d) & 1 == 0
        }
    }
}

fn window_mark(highest: &mut Option<u32>, seen: &mut u64, counter: u32) {
    match *highest {
        None => {
            *highest = Some(counter);
            *seen = 1;
        }
        Some(h) if counter > h => {
            *seen = seen.checked_shl(counter - h).unwrap_or(0) | 1;
            *highest = Some(counter);
        }
        Some(h) => {
            let d = h - counter;
            if d < 64 {
                *seen |= 1 << d;
            }
        }
    }
}

impl KeyState {
    fn new(epoch: u8, key: [u8; 32], floor: u32) -> Self {
        Self {
            epoch,
            key,
            floor,
            highest: None,
            seen: 0,
            aux_highest: None,
            aux_seen: 0,
        }
    }

    fn accepts(&self, counter: u32) -> bool {
        if counter >= AUX {
            return window_accepts(self.aux_highest, self.aux_seen, counter);
        }
        counter >= self.floor && window_accepts(self.highest, self.seen, counter)
    }

    /// The counters before `counter` that never came under this key and still could (inside
    /// the window once `counter` is marked, §10.6): after the highest message received, or
    /// from the key's next counter for its first one (sent after the key reached us).
    fn skipped_before(&self, counter: u32) -> Vec<u32> {
        let from = match self.highest {
            _ if counter >= AUX => return Vec::new(),
            Some(h) if counter > h => h.saturating_add(1),
            Some(_) => return Vec::new(),
            None => self.floor,
        };
        (from.max(counter.saturating_sub(63))..counter).collect()
    }

    /// The next message counter expected under this key: one past the highest received, or
    /// the key's own next counter (what the admin relays, §10.2).
    fn expected(&self) -> u32 {
        self.highest
            .map_or(self.floor, |h| self.floor.max(h.saturating_add(1)))
    }

    fn mark(&mut self, counter: u32) {
        if counter >= AUX {
            window_mark(&mut self.aux_highest, &mut self.aux_seen, counter);
        } else {
            window_mark(&mut self.highest, &mut self.seen, counter);
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
    /// The fragment as it was on the air (kept for §10.6).
    payload: Vec<u8>,
    at: Millis,
}

/// A group message as it was on the air, kept for members who missed it (§10.6).
#[derive(Clone, Debug)]
struct Kept {
    author: PubKey,
    epoch: u8,
    counter: u32,
    /// One payload per fragment, in order.
    payloads: Vec<Vec<u8>>,
    at: Millis,
}

/// A counter missing from a member's messages (§10.6).
#[derive(Clone, Debug)]
struct Gap {
    author: PubKey,
    epoch: u8,
    counter: u32,
    /// When to ask, or to stop waiting for the member asked.
    due: Millis,
    /// Requests sent so far (0, 1 or 2), and the identity asked last.
    tries: u8,
    asked: Option<PubKey>,
}

/// A member asked for a message we are missing too: it gets it once we recover it (§10.6).
#[derive(Clone, Debug)]
struct Owed {
    node: PubKey,
    author: PubKey,
    epoch: u8,
    counter: u32,
    until: Millis,
}

/// What a member keeps and misses, to recover lost messages (§10.6, ADR-0024). Memory only.
#[derive(Clone, Debug, Default)]
struct Recovery {
    kept: VecDeque<Kept>,
    gaps: Vec<Gap>,
    owed: Vec<Owed>,
    /// When we saw each member added: nobody gets back what was sent before it joined.
    since: HashMap<PubKey, Millis>,
}

impl Recovery {
    fn keep(&mut self, k: Kept) {
        if self
            .kept
            .iter()
            .any(|x| x.author == k.author && x.epoch == k.epoch && x.counter == k.counter)
        {
            return;
        }
        if self.kept.len() >= KEPT_MAX {
            self.kept.pop_front();
        }
        self.kept.push_back(k);
    }
}

/// What to do about one author's gaps (§10.6): (author, key epoch), whom to ask (None: give
/// up), the counters asked, the counters no longer wanted.
type GapPlan = ((PubKey, u8), Option<PubKey>, Vec<u32>, Vec<u32>);

/// Message types worth recovering (§10.6): not positions or search packets.
fn recoverable(body: &[u8]) -> bool {
    matches!(
        body.first(),
        Some(
            &(content::TEXT
                | content::REPLY
                | content::REACTION
                | content::ANNOUNCE
                | content::STICKER
                | content::GIF
                | content::EDIT
                | content::DELETE)
        )
    )
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
    /// The next counter for what the core sends by itself (from [`AUX`]).
    my_aux: u32,
    keys: HashMap<PubKey, MemberKeys>,
    partials: Vec<Partial>,
    /// Our (new) sender key still has to go to the members: restored before the node's
    /// identity was known.
    announce_key: bool,
    recovery: Recovery,
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
                        v.extend_from_slice(
                            &ks.aux_highest.map_or(u64::MAX, u64::from).to_le_bytes(),
                        );
                        v.extend_from_slice(&ks.aux_seen.to_le_bytes());
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
        // Version 2 had no window for the core's own traffic (§10.3).
        if version != SNAPSHOT_VERSION && version != 2 {
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
                    let (aux_highest, aux_seen) = if version >= 3 {
                        (
                            u64::from_le_bytes(take(b, 8)?.try_into().ok()?),
                            u64::from_le_bytes(take(b, 8)?.try_into().ok()?),
                        )
                    } else {
                        (u64::MAX, 0)
                    };
                    *slot = Some(KeyState {
                        epoch,
                        key,
                        floor,
                        highest: u32::try_from(highest).ok(),
                        seen,
                        aux_highest: u32::try_from(aux_highest).ok(),
                        aux_seen,
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
            my_aux: AUX,
            keys,
            partials: Vec::new(),
            announce_key: true,
            recovery: Recovery::default(),
        })
    }

    /// Number of other members whose current sender key we hold.
    pub fn keys_known(&self) -> usize {
        self.members
            .iter()
            .filter(|m| self.keys.get(&m.key).is_some_and(|k| k.current.is_some()))
            .count()
    }

    /// Counter `counter` of `author` under key `epoch` can still be accepted (§10.6): not
    /// received yet, and inside the replay window.
    fn still_wanted(&self, author: &PubKey, epoch: u8, counter: u32) -> bool {
        self.keys.get(author).is_some_and(|mk| {
            [&mk.current, &mk.previous]
                .into_iter()
                .flatten()
                .any(|k| k.epoch == epoch && k.accepts(counter))
        })
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

/// A group that listed us, waiting for the user (§10.1, SR-13).
#[derive(Clone, Debug)]
pub struct Invitation {
    pub(crate) state: State,
    pub(crate) at: Millis,
}

impl Invitation {
    pub const fn gid(&self) -> &GroupId {
        &self.state.gid
    }
    pub fn name(&self) -> &str {
        &self.state.name
    }
    /// The admin's identity.
    pub const fn admin(&self) -> &PubKey {
        &self.state.admin
    }
    /// The admin's node, as the state lists it.
    pub fn admin_node(&self) -> Option<PubKey> {
        self.state
            .members
            .iter()
            .find(|m| m.key == self.state.admin)
            .map(|m| m.node)
    }
    pub fn members(&self) -> &[Member] {
        &self.state.members
    }
}

/// Groups one phone belongs to at most; a further invitation is declined (SR-13).
pub const MAX_GROUPS: usize = 32;
/// Invitations waiting for the user at most; the oldest goes first.
const MAX_INVITATIONS: usize = 8;
/// How long an invitation waits for the user.
const INVITATION_MS: Millis = 7 * 24 * 3_600_000;
/// How long a scanned invitation waits for the admin's state (§10.5).
const JOINING_MS: Millis = 3_600_000;

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
        // Each node belongs to one member: two identities on one node would let one sign
        // what the app shows as the other's (review of 2026-11-15, SR-11).
        let nodes: HashSet<&PubKey> = members.iter().map(|m| &m.node).collect();
        let cause = match b {
            [] => None,
            [c] => Some(*c),
            _ => return None,
        };
        if distinct.len() != n || nodes.len() != n || !distinct.contains(&admin) {
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
    /// Counters of `author`'s messages under `epoch` that never arrived (§10.6).
    Missing {
        gid: GroupId,
        author: PubKey,
        epoch: u8,
        counters: Vec<u32>,
    },
    /// Envelope payloads as they were on the air, handed on (§10.6).
    Resend {
        gid: GroupId,
        payloads: Vec<Vec<u8>>,
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
            content::GROUP_MISSING => {
                decode_missing(datagram).map(|(gid, author, epoch, counters)| Self::Missing {
                    gid,
                    author,
                    epoch,
                    counters,
                })
            }
            content::GROUP_RESEND => {
                decode_resend(datagram).map(|(gid, payloads)| Self::Resend { gid, payloads })
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
            Self::Missing {
                gid,
                author,
                epoch,
                counters,
            } => encode_missing(gid, author, *epoch, counters),
            Self::Resend { gid, payloads } => encode_resend(gid, payloads),
        }
    }
}

/// GROUP_MISSING (§10.6): `gid ‖ author ‖ key_epoch ‖ n (1..8) ‖ n × counter u32`.
fn encode_missing(gid: &GroupId, author: &PubKey, epoch: u8, counters: &[u32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(51 + 4 * counters.len());
    v.push(content::GROUP_MISSING);
    v.extend_from_slice(gid);
    v.extend_from_slice(author);
    v.push(epoch);
    v.push(counters.len().min(255) as u8);
    for c in counters {
        v.extend_from_slice(&c.to_le_bytes());
    }
    v
}

fn decode_missing(datagram: &[u8]) -> Option<(GroupId, PubKey, u8, Vec<u32>)> {
    let mut b = datagram.strip_prefix(&[content::GROUP_MISSING])?;
    let gid: GroupId = take(&mut b, 16)?.try_into().ok()?;
    let author: PubKey = take(&mut b, 32)?.try_into().ok()?;
    let epoch = *take(&mut b, 1)?.first()?;
    let n = usize::from(*take(&mut b, 1)?.first()?);
    if !(1..=MISSING_MAX).contains(&n) || b.len() != 4 * n {
        return None;
    }
    let counters = b
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    Some((gid, author, epoch, counters))
}

/// GROUP_RESEND (§10.6): `gid ‖ n (1..16) ‖ n × (len (1..159) ‖ payload)`.
fn encode_resend(gid: &GroupId, payloads: &[Vec<u8>]) -> Vec<u8> {
    let mut v = vec![content::GROUP_RESEND];
    v.extend_from_slice(gid);
    v.push(payloads.len().min(255) as u8);
    for p in payloads {
        v.push(p.len().min(255) as u8);
        v.extend_from_slice(p);
    }
    v
}

fn decode_resend(datagram: &[u8]) -> Option<(GroupId, Vec<Vec<u8>>)> {
    let mut b = datagram.strip_prefix(&[content::GROUP_RESEND])?;
    let gid: GroupId = take(&mut b, 16)?.try_into().ok()?;
    let n = usize::from(*take(&mut b, 1)?.first()?);
    if !(1..=RESEND_MAX).contains(&n) {
        return None;
    }
    let mut payloads = Vec::with_capacity(n);
    for _ in 0..n {
        let len = usize::from(*take(&mut b, 1)?.first()?);
        if !(1..=group::PAYLOAD_MAX).contains(&len) {
            return None;
        }
        payloads.push(take(&mut b, len)?.to_vec());
    }
    b.is_empty().then_some((gid, payloads))
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
            // The first identity seen for a node is kept; another one waits for the user
            // (§4.6), and that member's messages are not shown meanwhile (SR-11).
            self.learn_identity(&m.node, m.key);
        }
    }

    /// How many times each group message is flooded (1 or 2).
    /// Whether this member asks for the group messages it missed (§10.6): on by default, off
    /// only to measure what recovery brings.
    pub fn set_group_recovery(&mut self, on: bool) {
        self.group_recovery = on;
    }

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
            my_aux: AUX,
            keys: HashMap::new(),
            partials: Vec::new(),
            announce_key: false,
            recovery: Recovery::default(),
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
        for id in &ids {
            g.recovery.since.insert(*id, now);
        }
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
        self.joining
            .retain(|(a, at)| *a != invite.admin && at + JOINING_MS > now);
        self.joining.push((invite.admin, now));
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
            g.my_aux = AUX;
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
            Some(&content::GROUP_MISSING) => {
                if let Some((gid, author, epoch, counters)) = decode_missing(datagram) {
                    let from = *from;
                    self.on_group_missing(&node, &from, &gid, (&author, epoch, &counters), now);
                }
            }
            Some(&content::GROUP_RESEND) => {
                if let Some((gid, payloads)) = decode_resend(datagram) {
                    let from = *from;
                    self.on_group_resend(&from, &gid, &payloads, now);
                }
            }
            _ => {}
        }
    }

    /// A member asks for messages it missed (§10.6): what we kept of them since it joined;
    /// those we miss too, it gets once we recover them.
    fn on_group_missing(
        &mut self,
        node: &PubKey,
        from: &PubKey,
        gid: &GroupId,
        (author, epoch, counters): (&PubKey, u8, &[u32]),
        now: Millis,
    ) {
        let Some(g) = self.groups.get_mut(gid) else {
            return;
        };
        if !g.is_member(from) {
            return;
        }
        let since = g.recovery.since.get(from).copied().unwrap_or(0);
        let mut payloads: Vec<Vec<u8>> = Vec::new();
        let mut held: Vec<&Kept> = g
            .recovery
            .kept
            .iter()
            .filter(|k| {
                k.author == *author
                    && k.epoch == epoch
                    && counters.contains(&k.counter)
                    && k.at >= since
            })
            .collect();
        // In the order they were written, whatever order they came to us in.
        held.sort_by_key(|k| k.counter);
        for k in held {
            if payloads.len() + k.payloads.len() > RESEND_MAX {
                break;
            }
            payloads.extend(k.payloads.iter().cloned());
        }
        let r = &mut g.recovery;
        for &c in counters {
            let ours = |x: &Gap| x.author == *author && x.epoch == epoch && x.counter == c;
            let owed = |o: &Owed| {
                o.node == *node && o.author == *author && o.epoch == epoch && o.counter == c
            };
            if r.gaps.iter().any(ours) && !r.owed.iter().any(owed) && r.owed.len() < OWED_MAX {
                r.owed.push(Owed {
                    node: *node,
                    author: *author,
                    epoch,
                    counter: c,
                    until: now + ANSWER_MS,
                });
            }
        }
        if !payloads.is_empty() {
            let d = encode_resend(gid, &payloads);
            self.enqueue_internal(node, d);
        }
    }

    /// Payloads a member handed on (§10.6): through the envelope receiver like any other.
    fn on_group_resend(&mut self, from: &PubKey, gid: &GroupId, payloads: &[Vec<u8>], now: Millis) {
        let Some(g) = self.groups.get(gid) else {
            return;
        };
        if !g.is_member(from) {
            return;
        }
        let gh = g.gid_hash();
        for p in payloads {
            if group::parse(p).is_some_and(|(h, _)| h.gid_hash == gh) {
                self.receive_envelope(p, 0, 0, true, now);
            }
        }
        // Still missing from what that member had: the author next, if it was not the author.
        if let Some(g) = self.groups.get_mut(gid) {
            g.recovery.gaps.retain_mut(|x| {
                if x.asked != Some(*from) {
                    return true;
                }
                x.due = now;
                x.author != *from && x.tries < 2
            });
        }
    }

    /// Asks for the gaps that are due, and gives up on those asked twice (§10.6).
    fn ask_missing(&mut self, now: Millis) {
        let Some(me) = self.key else {
            return;
        };
        if !self.group_recovery {
            for g in self.groups.values_mut() {
                g.recovery.gaps.clear();
            }
            return;
        }
        let mut requests: Vec<(PubKey, Vec<u8>)> = Vec::new();
        let gids: Vec<GroupId> = self.groups.keys().copied().collect();
        for gid in gids {
            let Some(g) = self.groups.get(&gid) else {
                continue;
            };
            let mut due: Vec<(PubKey, u8)> = Vec::new();
            for x in g.recovery.gaps.iter().filter(|x| x.due <= now) {
                if !due.contains(&(x.author, x.epoch)) {
                    due.push((x.author, x.epoch));
                }
            }
            if due.is_empty() {
                continue;
            }
            let node_of = |id: &PubKey| g.members.iter().find(|m| m.key == *id).map(|m| m.node);
            // (author, epoch) → whom to ask (None: give up), the counters asked, and those that
            // arrived or left the window meanwhile.
            let mut plan: Vec<GapPlan> = Vec::new();
            for (author, epoch) in due {
                let (wanted, stale): (Vec<&Gap>, Vec<&Gap>) = g
                    .recovery
                    .gaps
                    .iter()
                    .filter(|x| x.author == author && x.epoch == epoch && x.due <= now)
                    .partition(|x| g.still_wanted(&author, epoch, x.counter));
                let stale: Vec<u32> = stale.iter().map(|x| x.counter).collect();
                let open: Vec<u32> = wanted.iter().map(|x| x.counter).take(MISSING_MAX).collect();
                let first = g
                    .recovery
                    .gaps
                    .iter()
                    .find(|x| x.author == author && x.epoch == epoch && x.due <= now);
                let (tries, previous) = first.map_or((2, None), |x| (x.tries, x.asked));
                let target = match tries {
                    _ if open.is_empty() => None,
                    0 if me == g.admin || author == g.admin => Some(author),
                    0 if node_of(&author).is_some_and(|n| self.has_session(&n)) => Some(author),
                    0 => Some(g.admin),
                    1 => {
                        let other = if previous == Some(author) {
                            g.admin
                        } else {
                            author
                        };
                        (Some(other) != previous && other != me).then_some(other)
                    }
                    _ => None,
                };
                plan.push(((author, epoch), target, open, stale));
            }
            let mut asked: Vec<GapPlan> = Vec::new();
            for ((author, epoch), target, open, stale) in plan {
                match target.and_then(|t| node_of(&t).map(|n| (t, n))) {
                    Some((t, n)) => {
                        requests.push((n, encode_missing(&gid, &author, epoch, &open)));
                        asked.push(((author, epoch), Some(t), open, stale));
                    }
                    None => asked.push(((author, epoch), None, Vec::new(), stale)),
                }
            }
            if let Some(g) = self.groups.get_mut(&gid) {
                for ((author, epoch), target, open, stale) in asked {
                    g.recovery.gaps.retain_mut(|x| {
                        if x.author != author || x.epoch != epoch || x.due > now {
                            return true;
                        }
                        match target {
                            _ if stale.contains(&x.counter) => false,
                            Some(t) if open.contains(&x.counter) => {
                                x.tries += 1;
                                x.asked = Some(t);
                                x.due = now + ANSWER_MS;
                                true
                            }
                            // More than one request holds: in the next one.
                            Some(_) => true,
                            None => false,
                        }
                    });
                }
            }
        }
        for (node, d) in requests {
            self.enqueue_internal(&node, d);
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
        if !self.groups.contains_key(&gid) {
            self.invitation(state, listed, now);
            return;
        }
        let admin = admin_actor(&state);
        self.dirty_groups.insert(gid);
        if listed {
            self.learn_members(&state.members);
        }
        match self.groups.get_mut(&gid) {
            None => {}
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
                for m in &g.members {
                    if !old.contains(&m.key) {
                        g.recovery.since.insert(m.key, now);
                    }
                }
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
        self.take_pending_keys(&gid, now);
    }

    /// Keys that came before group `gid`'s state (or while it waited for the user) are used;
    /// other groups' keep waiting, ten minutes, or as long as their invitation waits.
    fn take_pending_keys(&mut self, gid: &GroupId, now: Millis) {
        let pending: Vec<PendingKey> = std::mem::take(&mut self.pending_keys);
        for p in pending {
            if p.gid == *gid {
                match p.via {
                    None => self.on_sender_key(&p.from, p.gid, p.epoch, p.key, p.next_counter, now),
                    Some(via) => {
                        let entry = (p.from, p.epoch, p.key, p.next_counter);
                        self.on_sender_keys(&via, *gid, vec![entry], now);
                    }
                }
            } else if p.at + PENDING_KEY_MS > now
                || self.invitations.iter().any(|i| i.state.gid == p.gid)
            {
                self.pending_keys.push(p);
            }
        }
    }

    /// A state for a group we do not belong to (§10.1, SR-13). One that lists us is an
    /// invitation: joined at once when the user asked for it with a scanned invitation
    /// (§10.5) or its admin is a contact verified in person, held for the user otherwise. A newer state from the same admin replaces a
    /// held one; one that no longer lists us withdraws it.
    fn invitation(&mut self, state: State, listed: bool, now: Millis) {
        let gid = state.gid;
        self.invitations.retain(|i| i.at + INVITATION_MS > now);
        self.joining.retain(|(_, at)| at + JOINING_MS > now);
        if let Some(pos) = self.invitations.iter().position(|i| i.state.gid == gid) {
            let held = &self.invitations[pos].state;
            if held.admin != state.admin || state.epoch <= held.epoch {
                return;
            }
            if !listed {
                self.invitations.remove(pos);
                return;
            }
            self.invitations[pos].state = state;
            return;
        }
        if !listed || self.groups.len() >= MAX_GROUPS {
            return;
        }
        let asked = self.joining.iter().any(|(a, _)| *a == state.admin);
        // Someone the user verified in person may add them, as a contact may in other
        // messengers; anyone else's group waits.
        let trusted = self
            .contacts
            .values()
            .any(|c| c.verified && c.identity == Some(state.admin));
        if asked || trusted || !self.group_consent {
            self.joining.retain(|(a, _)| *a != state.admin);
            self.join_group(state, now);
            return;
        }
        if self.invitations.len() >= MAX_INVITATIONS {
            self.invitations.remove(0);
        }
        self.events.push_back(Event::GroupInvited {
            gid,
            name: state.name.clone(),
            admin: state.admin,
            members: state.members.len(),
        });
        self.invitations.push(Invitation { state, at: now });
    }

    /// Becomes a member of the group of `state` (§10.1): its members become contacts, our
    /// first key goes to the admin (§10.2), the keys that came early are used.
    fn join_group(&mut self, state: State, now: Millis) {
        let gid = state.gid;
        let admin = admin_actor(&state);
        self.dirty_groups.insert(gid);
        self.learn_members(&state.members);
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
                my_aux: AUX,
                keys: HashMap::new(),
                partials: Vec::new(),
                announce_key: false,
                recovery: Recovery::default(),
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
        self.take_pending_keys(&gid, now);
    }

    /// Groups that listed us and wait for the user (§10.1).
    pub fn invitations(&self) -> &[Invitation] {
        &self.invitations
    }

    /// The user joins the group of a held invitation.
    pub fn accept_group(&mut self, gid: &GroupId, now: Millis) -> Result<(), Error> {
        let pos = self
            .invitations
            .iter()
            .position(|i| i.state.gid == *gid)
            .ok_or(Error::UnknownGroup)?;
        if self.groups.len() >= MAX_GROUPS {
            return Err(Error::NotAllowed);
        }
        let inv = self.invitations.remove(pos);
        self.join_group(inv.state, now);
        Ok(())
    }

    /// The user does not join: the invitation is forgotten. The admin is not told; it keeps
    /// listing us until it removes us.
    pub fn decline_group(&mut self, gid: &GroupId) {
        self.invitations.retain(|i| i.state.gid != *gid);
        self.pending_keys.retain(|p| p.gid != *gid);
    }

    /// Joins at once when listed (test nodes and bots), or waits for the user (the apps).
    pub fn set_group_consent(&mut self, ask: bool) {
        self.group_consent = ask;
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
                    entries.push((m.key, c.epoch, c.key, c.expected()));
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
        // Messages from 0, what the core sends by itself from AUX: a gap among messages is
        // then a lost message (§10.3, §10.6).
        let counter = if user { g.my_counter } else { g.my_aux };
        let (epoch_b, counter_b) = ([g.my_epoch], counter.to_le_bytes());
        let sig =
            sign(&prv, &signed_parts(gid, &epoch_b, &counter_b, body)).ok_or(Error::NoIdentity)?;
        let mut plain = body.to_vec();
        plain.extend_from_slice(&sig);
        let total = group::fragments_for(plain.len()).ok_or(Error::TooLarge)?;
        if user {
            g.my_counter = g
                .my_counter
                .checked_add(1)
                .filter(|c| *c < AUX)
                .ok_or(Error::TooLarge)?;
        } else {
            g.my_aux = g.my_aux.checked_add(1).ok_or(Error::TooLarge)?;
        }
        let (key, epoch, gh) = (g.my_key, g.my_epoch, g.gid_hash());
        // Each frame waits until the repeaters have re-flooded the previous one, with jitter.
        let spacing = 10 * self.radio.airtime_us(group::PAYLOAD_MAX + 6) / 1000;
        let mut due = now;
        let id = self.next_id;
        self.next_id += 1;
        let mut seq = 0u8;
        // Our own message as it goes on the air, kept for members who miss it (§10.6).
        let mut own: Vec<Vec<u8>> = Vec::new();
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
                if copy == 0 {
                    own.push(out.get(..n).unwrap_or(&[]).to_vec());
                }
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
        if recoverable(body)
            && let Some(g) = self.groups.get_mut(gid)
        {
            g.recovery.keep(Kept {
                author: me,
                epoch,
                counter,
                payloads: own,
                at: now,
            });
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
            g.recovery.kept.retain(|k| k.at + KEPT_MS > now);
            g.recovery.owed.retain(|o| o.until > now);
        }
        self.held.retain(|h| h.at + HELD_MS > now);
        self.ask_missing(now);
    }

    pub(crate) fn next_group_wakeup(&self) -> Option<Millis> {
        let frames = self.group_tx.front().map(|f| f.due.max(self.next_slot));
        let gaps = self
            .groups
            .values()
            .flat_map(|g| g.recovery.gaps.iter().map(|x| x.due))
            .min();
        match (frames, gaps) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// A group envelope heard on the air (§10.3).
    pub(crate) fn on_group_envelope(&mut self, payload: &[u8], snr4: i8, rssi: i8, now: Millis) {
        self.receive_envelope(payload, snr4, rssi, false, now);
    }

    /// A group envelope, heard on the air or handed on by a member (`late`, §10.6): the same
    /// checks either way.
    fn receive_envelope(&mut self, payload: &[u8], snr4: i8, rssi: i8, late: bool, now: Millis) {
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
                        Some((plain.to_vec(), vec![payload.to_vec()]))
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
                                    payload: payload.to_vec(),
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
                                let payloads = if p.index == 0 {
                                    vec![p.payload.clone(), payload.to_vec()]
                                } else {
                                    vec![payload.to_vec(), p.payload.clone()]
                                };
                                Some(([first, second].concat(), payloads))
                            }
                            // Another copy of a fragment we already hold.
                            Some(_) => None,
                        }
                    };
                    let Some((message, payloads)) = message else {
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
                    let skipped = ks.skipped_before(hdr.counter);
                    ks.mark(hdr.counter);
                    delivered = Some((
                        gid,
                        member,
                        member_node,
                        body.to_vec(),
                        (payloads, hdr.key_epoch, hdr.counter, skipped),
                    ));
                    break 'gids;
                }
            }
        }
        let Some((gid, member, member_node, body, (payloads, epoch, counter, skipped))) = delivered
        else {
            return;
        };
        self.dirty_groups.insert(gid);
        // Gaps it fills or opens, and the message kept for members who miss it (§10.6).
        let mut answers: Vec<PubKey> = Vec::new();
        let jitter = if skipped.is_empty() {
            0
        } else {
            Millis::from(self.rng.next_u32()) % ASK_AFTER_MS
        };
        if let Some(g) = self.groups.get_mut(&gid) {
            let r = &mut g.recovery;
            r.gaps
                .retain(|x| !(x.author == member && x.epoch == epoch && x.counter == counter));
            for c in skipped {
                if r.gaps.len() >= GAPS_MAX {
                    break;
                }
                if !r
                    .gaps
                    .iter()
                    .any(|x| x.author == member && x.epoch == epoch && x.counter == c)
                {
                    r.gaps.push(Gap {
                        author: member,
                        epoch,
                        counter: c,
                        due: now + ASK_AFTER_MS + jitter,
                        tries: 0,
                        asked: None,
                    });
                }
            }
            if recoverable(&body) {
                // Members who asked us for it while we missed it too.
                r.owed.retain(|o| {
                    let due = o.author == member && o.epoch == epoch && o.counter == counter;
                    if due {
                        answers.push(o.node);
                    }
                    !due
                });
                r.keep(Kept {
                    author: member,
                    epoch,
                    counter,
                    payloads: payloads.clone(),
                    at: now,
                });
            }
        }
        for node in answers {
            self.enqueue_internal(&node, encode_resend(&gid, &payloads));
        }
        // Local notices never come from the air (§6).
        if Content::is_local(&body) {
            return;
        }
        // The app shows a member by the node it is listed with, and checks edits, deletes and
        // reactions against it: that node must be known under this member's identity. A state
        // that lists someone with another person's node, or a node whose identity waits for
        // the user, shows nothing of theirs (§10.1, review of 2026-11-15, SR-11). The message
        // is still kept above for members who ask for it.
        if self.identity_of(&member_node) != Some(member) {
            return;
        }
        // Its direct copies were receptions of that member (search, §11.2); a payload handed
        // on by a member was not heard.
        if !late {
            self.search_attribute(&h, member, member_node, now);
        }
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
                author: member,
                content,
                snr4,
                rssi,
                late,
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
            let node = [i as u8 + 1; 32];
            core.add_contact(node, &format!("m{i}"), None);
            core.set_identity(&node, id, false);
            list.push(Member {
                key: id,
                node,
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
                my_aux: AUX,
                keys,
                partials: Vec::new(),
                announce_key: false,
                recovery: Recovery::default(),
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

    /// Group events: (text, late).
    fn texts_late(core: &mut Core) -> Vec<(String, bool)> {
        let mut v = Vec::new();
        while let Some(e) = core.poll_event() {
            if let Event::GroupMessage {
                content: Content::Text { text, .. },
                late,
                ..
            } = e
            {
                v.push((text, late));
            }
        }
        v
    }

    /// A member with a real identity, its node a contact of `core` that the session knows.
    fn member(core: &mut Core, seed: u8, node: PubKey) -> ([u8; 64], PubKey) {
        let prv = expand_seed(&[seed; 32]);
        let id = public_key(&prv);
        core.add_contact(node, &format!("n{seed}"), None);
        core.set_identity(&node, id, false);
        (prv, id)
    }

    /// A gap in a member's counters is asked for, and the envelope handed back fills it, marked
    /// late (§10.6); the admin asks the author itself.
    #[test]
    fn a_lost_message_comes_back() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let mut core = core_with_group(&[(bob, [10; 32], 0)]);
        member(&mut core, 7, [1; 32]);
        let e0 = envelope(&bob, &[10; 32], 0, &bob_prv, "leaving the col");
        let e1 = envelope(&bob, &[10; 32], 1, &bob_prv, "lost at the hub");
        let e2 = envelope(&bob, &[10; 32], 2, &bob_prv, "at the refuge");
        core.on_group_envelope(&e0, 0, 0, 0);
        core.on_group_envelope(&e2, 0, 0, 1_000);
        assert_eq!(texts(&mut core), vec!["leaving the col", "at the refuge"]);
        // Not before 20 s: the second copy may still come.
        core.ask_missing(15_000);
        assert!(core.queued(&[1; 32]).is_empty());
        core.ask_missing(41_000);
        let asked: Vec<_> = core
            .queued(&[1; 32])
            .iter()
            .filter_map(|d| decode_missing(d))
            .collect();
        assert_eq!(asked, vec![(GID, bob, 0, vec![1])]);
        core.on_group_control(
            &[1; 32],
            &encode_resend(&GID, std::slice::from_ref(&e1)),
            50_000,
        );
        assert_eq!(
            texts_late(&mut core),
            vec![("lost at the hub".into(), true)]
        );
        assert!(core.groups[&GID].recovery.gaps.is_empty());
        // Handed again: already there.
        core.on_group_control(&[1; 32], &encode_resend(&GID, &[e1]), 51_000);
        assert!(texts(&mut core).is_empty());
    }

    /// A member's first message lost: the next one shows it missing from the key's next
    /// counter, nothing before it (§10.6).
    #[test]
    fn a_lost_first_message_comes_back() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let mut core = core_with_group(&[(bob, [10; 32], 5)]);
        member(&mut core, 7, [1; 32]);
        core.on_group_envelope(&envelope(&bob, &[10; 32], 7, &bob_prv, "c"), 0, 0, 0);
        assert_eq!(texts(&mut core), vec!["c"]);
        core.ask_missing(41_000);
        let asked: Vec<_> = core
            .queued(&[1; 32])
            .iter()
            .filter_map(|d| decode_missing(d))
            .collect();
        assert_eq!(asked, vec![(GID, bob, 0, vec![5, 6])]);
    }

    /// The admin relays a key with the next counter it expects from its owner, not the one the
    /// key came with: a newcomer then misses nothing sent before it joined (§10.2, §10.6).
    #[test]
    fn relayed_keys_start_after_what_was_heard() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let (_, carol) = member(&mut probe, 8, [2; 32]);
        let mut core = core_with_group(&[(bob, [10; 32], 0), (carol, [20; 32], 0)]);
        member(&mut core, 7, [1; 32]);
        member(&mut core, 8, [2; 32]);
        for c in 0..3 {
            core.on_group_envelope(&envelope(&bob, &[10; 32], c, &bob_prv, "x"), 0, 0, 0);
        }
        core.send_bundle(&GID, &[carol]);
        let relayed: Vec<_> = core
            .queued(&[2; 32])
            .iter()
            .filter_map(|d| decode_sender_keys(d))
            .flat_map(|(_, entries)| entries)
            .filter(|e| e.0 == bob)
            .map(|e| e.3)
            .collect();
        assert_eq!(relayed, vec![3]);
    }

    /// Asked for a message we lost too (a busy repeater drops it for everyone behind it, the
    /// admin included): the asker gets it as soon as we recover it, not 2 minutes later from
    /// the author (§10.6).
    #[test]
    fn a_request_waits_for_our_own_recovery() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let (_, carol) = member(&mut probe, 8, [2; 32]);
        let mut core = core_with_group(&[(bob, [10; 32], 0), (carol, [20; 32], 0)]);
        member(&mut core, 7, [1; 32]);
        member(&mut core, 8, [2; 32]);
        let e1 = envelope(&bob, &[10; 32], 1, &bob_prv, "lost at the hub");
        core.on_group_envelope(&envelope(&bob, &[10; 32], 0, &bob_prv, "a"), 0, 0, 0);
        core.on_group_envelope(&envelope(&bob, &[10; 32], 2, &bob_prv, "c"), 0, 0, 1_000);
        texts(&mut core);
        let resent = |core: &Core| -> Vec<_> {
            core.queued(&[2; 32])
                .iter()
                .filter_map(|d| decode_resend(d))
                .collect()
        };
        core.on_group_control(&[2; 32], &encode_missing(&GID, &bob, 0, &[1]), 30_000);
        assert!(resent(&core).is_empty());
        // Bob hands it to us: Carol gets it too.
        core.on_group_control(
            &[1; 32],
            &encode_resend(&GID, std::slice::from_ref(&e1)),
            45_000,
        );
        assert_eq!(resent(&core), vec![(GID, vec![e1.clone()])]);
        // Once only.
        core.on_group_envelope(&e1, 0, 0, 46_000);
        assert_eq!(resent(&core).len(), 1);
        // A request we cannot serve in time is dropped.
        let e3 = envelope(&bob, &[10; 32], 3, &bob_prv, "d");
        let e5 = envelope(&bob, &[10; 32], 5, &bob_prv, "f");
        core.on_group_envelope(&e3, 0, 0, 50_000);
        core.on_group_envelope(&e5, 0, 0, 51_000);
        core.on_group_control(&[2; 32], &encode_missing(&GID, &bob, 0, &[4]), 80_000);
        core.tick_groups(80_000 + ANSWER_MS);
        core.on_group_envelope(&envelope(&bob, &[10; 32], 4, &bob_prv, "e"), 0, 0, 210_000);
        assert_eq!(resent(&core).len(), 1);
    }

    /// A relayed envelope is checked like one heard (§10.6): a forgery or a non-member's
    /// answer delivers nothing.
    #[test]
    fn relayed_envelopes_are_checked() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let (carol_prv, carol) = member(&mut probe, 8, [2; 32]);
        let mut core = core_with_group(&[(bob, [10; 32], 0), (carol, [20; 32], 0)]);
        member(&mut core, 7, [1; 32]);
        member(&mut core, 8, [2; 32]);
        let (_, stranger) = member(&mut core, 9, [3; 32]);
        assert!(!core.groups[&GID].is_member(&stranger));
        core.on_group_envelope(&envelope(&bob, &[10; 32], 0, &bob_prv, "a"), 0, 0, 0);
        core.on_group_envelope(&envelope(&bob, &[10; 32], 2, &bob_prv, "c"), 0, 0, 0);
        texts(&mut core);
        // Carol holds Bob's key but cannot sign as Bob.
        let forged = envelope(&bob, &[10; 32], 1, &carol_prv, "forged");
        core.on_group_control(&[2; 32], &encode_resend(&GID, &[forged]), 1_000);
        assert!(texts(&mut core).is_empty());
        // The real one, from someone outside the group: ignored.
        let real = envelope(&bob, &[10; 32], 1, &bob_prv, "b");
        core.on_group_control(
            &[3; 32],
            &encode_resend(&GID, std::slice::from_ref(&real)),
            2_000,
        );
        assert!(texts(&mut core).is_empty());
        // From Carol, a member: accepted.
        core.on_group_control(&[2; 32], &encode_resend(&GID, &[real]), 3_000);
        assert_eq!(texts(&mut core), vec!["b"]);
    }

    /// A member answers from what it kept, never with what was sent before the asker joined
    /// (§10.6).
    #[test]
    fn answers_skip_what_came_before_joining() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let (_, carol) = member(&mut probe, 8, [2; 32]);
        let (_, dave) = member(&mut probe, 10, [4; 32]);
        let mut core = core_with_group(&[
            (bob, [10; 32], 0),
            (carol, [20; 32], 0),
            (dave, [30; 32], 0),
        ]);
        member(&mut core, 7, [1; 32]);
        member(&mut core, 8, [2; 32]);
        member(&mut core, 10, [4; 32]);
        let e0 = envelope(&bob, &[10; 32], 0, &bob_prv, "before Dave");
        core.on_group_envelope(&e0, 0, 0, 1_000);
        // Dave was added after it was heard; Carol was there.
        core.groups
            .get_mut(&GID)
            .unwrap()
            .recovery
            .since
            .insert(dave, 5_000);
        // Kept out of order (the later one first): handed on in counter order.
        let e2 = envelope(&bob, &[10; 32], 2, &bob_prv, "after a gap");
        let e1 = envelope(&bob, &[10; 32], 1, &bob_prv, "came late");
        core.on_group_envelope(&e2, 0, 0, 2_000);
        core.on_group_envelope(&e1, 0, 0, 3_000);
        core.on_group_control(&[2; 32], &encode_missing(&GID, &bob, 0, &[2, 1]), 4_000);
        let ordered: Vec<_> = core
            .queued(&[2; 32])
            .iter()
            .filter_map(|d| decode_resend(d))
            .collect();
        assert_eq!(ordered, vec![(GID, vec![e1, e2])]);
        let ask = encode_missing(&GID, &bob, 0, &[0]);
        core.on_group_control(&[4; 32], &ask, 6_000);
        assert!(core.queued(&[4; 32]).is_empty());
        core.on_group_control(&[2; 32], &ask, 6_000);
        let answer: Vec<_> = core
            .queued(&[2; 32])
            .iter()
            .filter_map(|d| decode_resend(d))
            .collect();
        assert_eq!(answer.last(), Some(&(GID, vec![e0])));
    }

    /// Positions are not kept: a gap that was a position gets no answer.
    #[test]
    fn only_messages_are_kept() {
        assert!(recoverable(
            &Content::Text {
                sent_at: 1,
                text: "x".into()
            }
            .encode()
        ));
        assert!(!recoverable(&[content::LOCATION, 0, 0]));
        assert!(!recoverable(&[content::SEARCH, 1]));
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
    fn a_node_belongs_to_one_member() {
        let (camille, eve) = ([0x21u8; 32], [0x31u8; 32]);
        let state = |second_node: PubKey| State {
            gid: GID,
            epoch: 2,
            admin: camille,
            name: "g".into(),
            members: vec![
                Member {
                    key: camille,
                    node: [1; 32],
                    name: "Camille".into(),
                },
                Member {
                    key: eve,
                    node: second_node,
                    name: "Camille".into(),
                },
            ],
            cause: None,
        };
        assert!(State::decode(&state([2; 32]).encode()).is_some());
        assert!(State::decode(&state([1; 32]).encode()).is_none());
    }

    /// Review of 2026-11-15, SR-11: a state lists Eve with Camille's node. Eve's own posts,
    /// validly signed, must not be shown as Camille's, nor her edits pass Camille's author
    /// check: nothing of hers is shown until the node's identity is settled.
    #[test]
    fn a_member_on_someone_elses_node_is_not_shown() {
        let camille_prv = expand_seed(&[0x21; 32]);
        let eve_prv = expand_seed(&[0x31; 32]);
        let (camille, eve) = (public_key(&camille_prv), public_key(&eve_prv));
        let mut core = core_with_group(&[(camille, [30; 32], 0)]);
        let camille_node = [1u8; 32];
        // What a state listing Eve with Camille's node leaves behind (decoding refuses such
        // a state; this is the core's last line).
        let g = core.groups.get_mut(&GID).unwrap();
        g.members.push(Member {
            key: eve,
            node: camille_node,
            name: "Camille".into(),
        });
        g.keys.insert(
            eve,
            MemberKeys {
                current: Some(KeyState::new(0, [50; 32], 0)),
                previous: None,
            },
        );
        core.learn_members(&core.groups[&GID].members.clone());
        assert_eq!(core.identity_of(&camille_node), Some(camille));
        assert_eq!(core.pending_identity(&camille_node), Some(&eve));
        while core.poll_event().is_some() {}

        let from_eve = envelope(&eve, &[50; 32], 0, &eve_prv, "it's Camille");
        core.on_group_envelope(&from_eve, 0, 0, 0);
        assert!(texts(&mut core).is_empty());
        let from_camille = envelope(&camille, &[30; 32], 0, &camille_prv, "the real one");
        core.on_group_envelope(&from_camille, 0, 0, 0);
        assert_eq!(texts(&mut core), vec!["the real one"]);
    }

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

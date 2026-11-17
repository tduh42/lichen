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
use crate::secret::Secret;

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
/// Version byte of [`Group::snapshot`]: 2 since members are identities with a node (ADR-0013),
/// 4 since keys have a step (ADR-0039) and handovers and recovery copies are kept (ADR-0038,
/// ADR-0040).
const SNAPSHOT_VERSION: u8 = 4;
/// Counters from here on carry what the core sends by itself (positions, search packets), with
/// a replay window of their own: a gap below is a lost message (§10.3, §10.6).
const AUX: u32 = 1 << 31;
/// A member holding an envelope this long without its key asks the admin (§10.7)…
const SYNC_AFTER_MS: Millis = 60_000;
/// …at most this often per group; the admin answers each member at most this often.
const SYNC_EVERY_MS: Millis = 600_000;
/// GROUP_SYNC answers an admin gives per group in [`SYNC_EVERY_MS`] (§10.7): a forged early
/// envelope cannot make it answer every member with every key.
const SYNC_ANSWERS_MAX: usize = 8;
/// Envelopes whose sender key has not arrived yet are kept this long (§10.3).
const HELD_MS: Millis = 600_000;
/// And at most this many in all…
const HELD_MAX: usize = 64;
/// …and per group hash and sender byte: garbage under one member's byte, which nothing can
/// check before the key comes, cannot push out the others' (review of 2026-11-15, SR-20).
const HELD_PER_SENDER: usize = 8;
/// Fragments kept per two-fragment message, both indices together: the sender's own, and a
/// few that open under its key but may come from another member (SR-17).
const PARTIAL_CANDIDATES: usize = 6;
/// Group messages each member keeps to resend (§10.6), and for how long.
const KEPT_MAX: usize = 64;
const KEPT_MS: Millis = 3_600_000;
/// Missing counters remembered per group (§10.6).
const GAPS_MAX: usize = 16;
/// Requests for messages we miss too, answered once we recover them (§10.6), per group.
const OWED_MAX: usize = 32;
/// Kept messages remembered as handed on, per group (SR-14): more than the kept ones (64)
/// times a few members asking.
const ANSWERED_MAX: usize = 512;
/// A gap is asked for this long after it is seen, plus up to as much again (§10.6).
const ASK_AFTER_MS: Millis = 20_000;
/// How long an answer is awaited before asking the other one, then giving up.
const ANSWER_MS: Millis = 120_000;
/// Counters per GROUP_MISSING, payloads per GROUP_RESEND (§10.6).
pub const MISSING_MAX: usize = 8;
pub const RESEND_MAX: usize = 16;
/// A receiver tries this many steps past its own: a sender that took a newer state first
/// (§10.9, ADR-0039).
const STEP_AHEAD: u32 = 8;
/// Earlier steps are kept while within this many epochs of the newest (§10.9).
const STEP_BACK: u32 = 8;
/// A key farther than this many steps from our epoch is dropped (§10.9).
const STEP_MAX: u32 = 64;
/// Handover links each member keeps, and a GROUP_HANDOVER carries at most (§10.8, ADR-0038).
pub const HANDOVER_LINKS: usize = 4;
/// Our digest goes this long after our last message to a group left, plus up to half as much
/// again, if no other one followed (§10.6, ADR-0040): members' timers do not line up.
pub const DIGEST_AFTER_MS: Millis = 120_000;

/// The key at state epoch `epoch`, from the key at `epoch − 1` (§10.9): one-way.
fn step_key(gid: &GroupId, key: &[u8; 32], epoch: u32) -> Secret {
    let e = epoch.to_le_bytes();
    let info: [&[u8]; 2] = [b"lichen/1 key step", &e];
    Secret(lc_proto::kdf::hkdf(gid, key, &info))
}

/// `key` at step `from` brought forward to step `to` (§10.9), for the vectors.
#[doc(hidden)]
pub fn step_key_to(gid: &GroupId, key: &[u8; 32], from: u32, to: u32) -> Option<[u8; 32]> {
    advance(gid, key, from, to).map(|k| *k)
}

/// `key`, at step `from`, brought forward to step `to`; `None` backwards or farther than
/// [`STEP_MAX`].
fn advance(gid: &GroupId, key: &[u8; 32], from: u32, to: u32) -> Option<Secret> {
    let n = to.checked_sub(from)?;
    if n > STEP_MAX {
        return None;
    }
    let mut k = Secret(*key);
    for e in from.saturating_add(1)..=to {
        k = step_key(gid, &k, e);
    }
    Some(k)
}

/// A message of ours whose first copy has left (§10.6): our digest names it, and members who
/// miss it get it from what we keep. Nothing of it before then: a message cancelled before it
/// left is neither named nor handed on.
#[derive(Clone, Debug)]
struct Left {
    gid: GroupId,
    epoch: u8,
    counter: u32,
    kept: Option<Kept>,
}

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
    /// On that frame, for a message of ours: what follows once it left (§10.6).
    left: Option<Left>,
}

/// An envelope from a known member whose key we do not hold yet.
#[derive(Clone, Debug)]
pub(crate) struct Held {
    payload: Vec<u8>,
    /// Its group hash and sender byte, from the clear header: each sender gets a share.
    gid_hash: u16,
    sender: u8,
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
    /// The key at step `at`, the state epoch it belongs to (§10.9).
    key: Secret,
    at: u32,
    /// The key at an earlier step, from before our last step: frames sent before it, late or
    /// recovered (§10.9).
    base: Option<(Secret, u32)>,
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
    fn new(epoch: u8, key: [u8; 32], at: u32, floor: u32) -> Self {
        Self {
            epoch,
            key: Secret(key),
            at,
            base: None,
            floor,
            highest: None,
            seen: 0,
            aux_highest: None,
            aux_seen: 0,
        }
    }

    /// Brings the key forward to step `to` (§10.9), keeping the earlier one for late frames;
    /// false when `to` is too far to step to.
    fn step_to(&mut self, gid: &GroupId, to: u32) -> bool {
        if to <= self.at {
            return true;
        }
        let Some(next) = advance(gid, &self.key, self.at, to) else {
            return false;
        };
        let old = std::mem::replace(&mut self.key, next);
        // Several steps in a row keep the oldest while it is close enough.
        if self.base.as_ref().is_none_or(|(_, b)| to - b > STEP_BACK) {
            self.base = Some((old, self.at));
        }
        self.at = to;
        true
    }

    /// `key` at step `at` is this key, compared at the later of the two steps (§10.2).
    fn same_key(&self, gid: &GroupId, key: &[u8; 32], at: u32) -> bool {
        if at >= self.at {
            advance(gid, &self.key, self.at, at).is_some_and(|k| k == *key)
        } else {
            advance(gid, key, at, self.at).is_some_and(|k| k == *self.key)
        }
    }

    /// Opens an envelope under this key at the step it was sent at, whichever of ours, the
    /// next [`STEP_AHEAD`], or the earlier ones we keep (§10.9): its header, plaintext and step.
    fn open(&self, gid: &GroupId, payload: &[u8]) -> Option<(GroupHeader, Vec<u8>, u32)> {
        let try_key = |k: &[u8; 32], step: u32| {
            let mut buf = payload.to_vec();
            group::open(k, &mut buf)
                .ok()
                .map(|(h, plain)| (h, plain.to_vec(), step))
        };
        if let Some(found) = try_key(&self.key, self.at) {
            return Some(found);
        }
        let mut k = self.key.clone();
        for step in self.at.saturating_add(1)..=self.at.saturating_add(STEP_AHEAD) {
            k = step_key(gid, &k, step);
            if let Some(found) = try_key(&k, step) {
                return Some(found);
            }
        }
        let (base, from) = self.base.as_ref()?;
        let mut k = base.clone();
        for step in *from..self.at {
            if step > *from {
                k = step_key(gid, &k, step);
            }
            if let Some(found) = try_key(&k, step) {
                return Some(found);
            }
        }
        None
    }

    /// The message counters up to `counter` included that never came under this key and
    /// still could, after the highest received (or from the key's next counter): what a
    /// digest naming `counter` reveals (§10.6, ADR-0040).
    fn missing_through(&self, counter: u32) -> Vec<u32> {
        if counter >= AUX {
            return Vec::new();
        }
        let from = match self.highest {
            None => self.floor,
            Some(h) if counter > h => h.saturating_add(1),
            Some(_) => counter,
        };
        (from.max(counter.saturating_sub(63))..=counter)
            .filter(|&c| self.accepts(c))
            .collect()
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
    /// The keys before those, with their replay windows, the newest last: a key that comes
    /// back comes back with its window, so a relay cannot replay its old traffic by
    /// reinstalling it (review of 2026-11-15, SR-18). Memory only.
    retired: VecDeque<KeyState>,
}

/// Retired keys remembered per member.
const RETIRED_MAX: usize = 8;

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
    /// The step it was sent at (§10.9): it goes to members added at that state or before.
    step: u32,
    /// One payload per fragment, in order.
    payloads: Vec<Vec<u8>>,
    at: Millis,
    /// When it was kept, Unix seconds, if the clock was known: it is stored with the group and
    /// lives its hour across a restart (ADR-0040).
    at_unix: Option<u64>,
    /// Restored from a snapshot: `at` is set at the next tick, from `at_unix`.
    restored: bool,
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

/// What a member keeps and misses, to recover lost messages (§10.6, ADR-0024), and what the
/// admin remembers of recent admissions. Memory only.
#[derive(Clone, Debug, Default)]
struct Recovery {
    kept: VecDeque<Kept>,
    gaps: Vec<Gap>,
    owed: Vec<Owed>,
    /// Kept messages already handed to a member: (member, author, key epoch, counter). Each
    /// goes once to each member: a request is otherwise a tiny frame that makes us send up to
    /// 16 payloads, again and again (review of 2026-11-15, SR-14).
    answered: VecDeque<(PubKey, PubKey, u8, u32)>,
    /// The state epoch that added each member, as far as we saw: nobody gets back what was sent
    /// at an earlier step (§10.6, §10.9). Stored with the group.
    since: HashMap<PubKey, u32>,
    /// As admin, the state that last added each member: a GROUP_LEAVE naming an earlier one
    /// was sent before it came back (review of 2026-11-15, SR-14). Memory only: after a
    /// restart, any state up to the current one is accepted.
    added_in: HashMap<PubKey, u32>,
    /// As admin, when listed members asked to join again (a phone restored from its phrase):
    /// for [`ADMITTED_MS`] their keys go to everyone, as a first key does (§10.5, SR-20).
    rejoined: HashMap<PubKey, Millis>,
    /// When we last asked the admin for the keys (§10.7).
    synced_at: Option<Millis>,
    /// As admin (or former admin, §10.8), when we last answered each member's GROUP_SYNC.
    syncs_answered: HashMap<PubKey, Millis>,
    /// A newer state from a listed member naming itself admin, and that member: kept until a
    /// handover makes it the admin (§10.8).
    orphan: Option<(PubKey, State)>,
    /// When we last asked such a member for the handovers we missed.
    pulled_at: Option<Millis>,
    /// As admin, when we last asked each member for its key (§10.7, GROUP_KEY_REQUEST).
    keys_asked: HashMap<PubKey, Millis>,
    /// When we last sent our key to the admin that asked for it.
    key_sent_at: Option<Millis>,
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
                | content::DELETE
                | content::TIMER)
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
    /// Our sender key, at step `epoch` (§10.9).
    my_key: Secret,
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
    /// The last handover links we took or made (`signature ‖ state`, §10.8), the oldest first:
    /// handed on to members that missed them.
    handovers: VecDeque<Vec<u8>>,
    /// Our digest, when it is due, and the key epoch and counter of our last message (§10.6).
    digest: Option<(Millis, u8, u32)>,
}

/// One key slot of a snapshot: `0`, or `1 ‖ key_epoch ‖ key ‖ at u32 ‖ base ‖ floor u32 ‖
/// highest u64 ‖ seen u64 ‖ aux_highest u64 ‖ aux_seen u64`, `base` being `0` or
/// `1 ‖ key ‖ at u32`.
fn put_slot(v: &mut Vec<u8>, slot: Option<&KeyState>) {
    let Some(ks) = slot else {
        v.push(0);
        return;
    };
    v.push(1);
    v.push(ks.epoch);
    v.extend_from_slice(&ks.key[..]);
    v.extend_from_slice(&ks.at.to_le_bytes());
    match &ks.base {
        None => v.push(0),
        Some((k, at)) => {
            v.push(1);
            v.extend_from_slice(&k[..]);
            v.extend_from_slice(&at.to_le_bytes());
        }
    }
    v.extend_from_slice(&ks.floor.to_le_bytes());
    v.extend_from_slice(&ks.highest.map_or(u64::MAX, u64::from).to_le_bytes());
    v.extend_from_slice(&ks.seen.to_le_bytes());
    v.extend_from_slice(&ks.aux_highest.map_or(u64::MAX, u64::from).to_le_bytes());
    v.extend_from_slice(&ks.aux_seen.to_le_bytes());
}

fn take_u32(b: &mut &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(take(b, 4)?.try_into().ok()?))
}

fn take_u64(b: &mut &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(take(b, 8)?.try_into().ok()?))
}

fn take_byte(b: &mut &[u8]) -> Option<u8> {
    take(b, 1)?.first().copied()
}

fn take_slot(b: &mut &[u8]) -> Option<Option<KeyState>> {
    match take_byte(b)? {
        0 => return Some(None),
        1 => {}
        _ => return None,
    }
    let epoch = take_byte(b)?;
    let key: [u8; 32] = take(b, 32)?.try_into().ok()?;
    let at = take_u32(b)?;
    let base = match take_byte(b)? {
        0 => None,
        1 => {
            let k: [u8; 32] = take(b, 32)?.try_into().ok()?;
            let base_at = take_u32(b)?;
            if base_at >= at {
                return None;
            }
            Some((Secret(k), base_at))
        }
        _ => return None,
    };
    Some(Some(KeyState {
        epoch,
        key: Secret(key),
        at,
        base,
        floor: take_u32(b)?,
        highest: u32::try_from(take_u64(b)?).ok(),
        seen: take_u64(b)?,
        aux_highest: u32::try_from(take_u64(b)?).ok(),
        aux_seen: take_u64(b)?,
    }))
}

impl Group {
    pub fn gid_hash(&self) -> u16 {
        gid_hash(&self.gid)
    }

    /// The persistent form of the group: state, members, our key epoch, the other members'
    /// sender keys with their steps and replay windows, the handovers we keep, when members
    /// were added, and the messages kept for recovery (ADR-0040). Our own key is not kept:
    /// after a restart a new one is drawn under the next epoch (§10.2).
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
        let keys: Vec<(&PubKey, &MemberKeys)> = self.keys.iter().take(255).collect();
        v.push(keys.len() as u8);
        for (k, mk) in keys {
            v.extend_from_slice(k);
            put_slot(&mut v, mk.current.as_ref());
            put_slot(&mut v, mk.previous.as_ref());
        }
        let links: Vec<&Vec<u8>> = self
            .handovers
            .iter()
            .filter(|l| l.len() <= usize::from(u16::MAX))
            .collect();
        v.push(links.len() as u8);
        for l in links {
            v.extend_from_slice(&(l.len() as u16).to_le_bytes());
            v.extend_from_slice(l);
        }
        let since: Vec<(&PubKey, &u32)> = self
            .recovery
            .since
            .iter()
            .filter(|(k, _)| self.is_member(k))
            .take(255)
            .collect();
        v.push(since.len() as u8);
        for (k, e) in since {
            v.extend_from_slice(k);
            v.extend_from_slice(&e.to_le_bytes());
        }
        let kept: Vec<&Kept> = self
            .recovery
            .kept
            .iter()
            .filter(|k| {
                (1..=usize::from(group::MAX_FRAGMENTS)).contains(&k.payloads.len())
                    && k.payloads
                        .iter()
                        .all(|p| (1..=group::PAYLOAD_MAX).contains(&p.len()))
            })
            .take(KEPT_MAX)
            .collect();
        v.push(kept.len() as u8);
        for k in kept {
            v.extend_from_slice(&k.author);
            v.push(k.epoch);
            v.extend_from_slice(&k.counter.to_le_bytes());
            v.extend_from_slice(&k.step.to_le_bytes());
            v.extend_from_slice(&k.at_unix.unwrap_or(0).to_le_bytes());
            v.push(k.payloads.len() as u8);
            for p in &k.payloads {
                v.push(p.len() as u8);
                v.extend_from_slice(p);
            }
        }
        v
    }

    /// Rebuilds a group from [`Group::snapshot`], with a new sender key of ours under the next
    /// epoch.
    fn from_snapshot(b: &[u8], my_key: Secret) -> Option<Self> {
        let (&version, mut b) = b.split_first()?;
        if version != SNAPSHOT_VERSION {
            return None;
        }
        let b = &mut b;
        let gid: GroupId = take(b, 16)?.try_into().ok()?;
        let epoch = take_u32(b)?;
        let admin: PubKey = take(b, 32)?.try_into().ok()?;
        let name = take_name(b)?;
        let n = usize::from(take_byte(b)?);
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
        let my_epoch = take_byte(b)?.wrapping_add(1);
        let nk = usize::from(take_byte(b)?);
        let mut keys = HashMap::new();
        for _ in 0..nk {
            let k: PubKey = take(b, 32)?.try_into().ok()?;
            let current = take_slot(b)?;
            let previous = take_slot(b)?;
            keys.insert(
                k,
                MemberKeys {
                    current,
                    previous,
                    retired: VecDeque::new(),
                },
            );
        }
        let nh = usize::from(take_byte(b)?);
        let mut handovers = VecDeque::new();
        for _ in 0..nh {
            let len = usize::from(u16::from_le_bytes(take(b, 2)?.try_into().ok()?));
            handovers.push_back(take(b, len)?.to_vec());
        }
        if handovers.len() > HANDOVER_LINKS {
            return None;
        }
        let mut recovery = Recovery::default();
        for _ in 0..take_byte(b)? {
            let k: PubKey = take(b, 32)?.try_into().ok()?;
            recovery.since.insert(k, take_u32(b)?);
        }
        let nkept = usize::from(take_byte(b)?);
        if nkept > KEPT_MAX {
            return None;
        }
        for _ in 0..nkept {
            let author: PubKey = take(b, 32)?.try_into().ok()?;
            let key_epoch = take_byte(b)?;
            let counter = take_u32(b)?;
            let step = take_u32(b)?;
            let at_unix = take_u64(b)?;
            let np = take_byte(b)?;
            if !(1..=group::MAX_FRAGMENTS).contains(&np) {
                return None;
            }
            let mut payloads = Vec::new();
            for _ in 0..np {
                let len = usize::from(take_byte(b)?);
                if !(1..=group::PAYLOAD_MAX).contains(&len) {
                    return None;
                }
                payloads.push(take(b, len)?.to_vec());
            }
            recovery.keep(Kept {
                author,
                epoch: key_epoch,
                counter,
                step,
                payloads,
                at: 0,
                at_unix: (at_unix != 0).then_some(at_unix),
                restored: true,
            });
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
            recovery,
            handovers,
            digest: None,
        })
    }

    /// A group we just created or joined at state `epoch`, our key `my_key` at that step.
    fn new(state: State, my_key: Secret) -> Self {
        Self {
            gid: state.gid,
            name: state.name,
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
            handovers: VecDeque::new(),
            digest: None,
        }
    }

    /// Takes state epoch `e` (§10.9): every key we hold steps forward to it, ours included. A
    /// member key too far behind is dropped; false when ours is (a new one must be drawn).
    fn take_epoch(&mut self, e: u32) -> bool {
        if e <= self.epoch {
            return true;
        }
        let gid = self.gid;
        for mk in self.keys.values_mut() {
            for slot in [&mut mk.current, &mut mk.previous] {
                if slot.as_mut().is_some_and(|k| !k.step_to(&gid, e)) {
                    *slot = None;
                }
            }
        }
        let ours = advance(&gid, &self.my_key, self.epoch, e);
        self.epoch = e;
        match ours {
            Some(k) => {
                self.my_key = k;
                true
            }
            None => false,
        }
    }

    /// The state this group is at, as the admin sends it.
    fn state(&self, cause: Option<u8>) -> State {
        State {
            gid: self.gid,
            epoch: self.epoch,
            admin: self.admin,
            name: self.name.clone(),
            members: self.members.clone(),
            cause,
        }
    }

    /// Handover links we keep that are newer than state `epoch`, the oldest first (§10.8).
    fn links_after(&self, epoch: u32) -> Vec<&[u8]> {
        self.handovers
            .iter()
            .filter(|l| {
                l.get(SIGNATURE_LEN..)
                    .and_then(State::decode)
                    .is_some_and(|s| s.epoch > epoch)
            })
            .map(Vec::as_slice)
            .collect()
    }

    fn keep_link(&mut self, link: Vec<u8>) {
        if self.handovers.len() >= HANDOVER_LINKS {
            self.handovers.pop_front();
        }
        self.handovers.push_back(link);
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
    /// When it came, Unix seconds, when the clock was known: kept across restarts.
    pub(crate) at_unix: Option<u64>,
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
/// People one invitation code admits at most (SR-19): a trailhead group, not a photographed
/// screen passed around.
const INVITE_ADMISSIONS_MAX: u8 = 12;
/// Invitation snapshot version (what the app stores, so that a restart loses nothing): 2
/// since keys have a step (ADR-0039).
const INVITATION_SNAPSHOT: u8 = 2;
/// A sender key in an invitation snapshot: owner, relayed or not, by whom, step, epoch, key,
/// next.
const KEY_ENTRY_LEN: usize = 32 + 1 + 32 + 4 + 1 + 32 + 4;

/// A key that arrived before its group state.
#[derive(Clone, Debug)]
pub(crate) struct PendingKey {
    /// The key's owner.
    from: PubKey,
    /// Relayed by this identity (§10.2), to check against the admin once the state is here;
    /// `None`: from its owner.
    via: Option<PubKey>,
    gid: GroupId,
    /// The state epoch the key is at (§10.9).
    step: u32,
    epoch: u8,
    key: Secret,
    next_counter: u32,
    at: Millis,
}

/// One key in SENDER_KEY or SENDER_KEYS (§10.2): its owner, its step (the state epoch it is
/// at, §10.9), key epoch, key, and the next message counter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEntry {
    pub owner: PubKey,
    pub step: u32,
    pub key_epoch: u8,
    pub key: [u8; 32],
    pub next_counter: u32,
}

/// Bytes of a SENDER_KEYS entry.
const SENDER_KEYS_ENTRY: usize = 32 + 4 + 1 + 32 + 4;

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

/// A [`Group::snapshot`] without the other members' sender keys and replay windows: the
/// group's state, its members and our key epoch, what a backup keeps of it (ADR-0037). Restored,
/// it draws our next key as after a restart, and the others' keys come again (§10.7). None when
/// `snapshot` does not decode.
pub fn snapshot_without_keys(snapshot: &[u8]) -> Option<Vec<u8>> {
    let (&version, rest) = snapshot.split_first()?;
    if version != SNAPSHOT_VERSION && version != 2 {
        return None;
    }
    let mut b = rest;
    let b = &mut b;
    // gid, epoch, admin, name.
    take(b, 16 + 4 + 32)?;
    take_name(b)?;
    let n = usize::from(*take(b, 1)?.first()?);
    for _ in 0..n {
        take(b, 64)?;
        take_name(b)?;
    }
    // Our key epoch, then no member key.
    take(b, 1)?;
    let mut v = snapshot.get(..snapshot.len() - b.len())?.to_vec();
    v.push(0);
    Some(v)
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

/// SENDER_KEY (0x10, §10.2): `gid ‖ step u32 ‖ key_epoch ‖ key ‖ next_counter u32`.
fn encode_sender_key(gid: &GroupId, e: &KeyEntry) -> Vec<u8> {
    let mut v = Vec::with_capacity(58);
    v.push(content::SENDER_KEY);
    v.extend_from_slice(gid);
    v.extend_from_slice(&e.step.to_le_bytes());
    v.push(e.key_epoch);
    v.extend_from_slice(&e.key);
    v.extend_from_slice(&e.next_counter.to_le_bytes());
    v
}

/// The entry of a SENDER_KEY, its owner left blank (the session's identity).
fn decode_sender_key(datagram: &[u8]) -> Option<(GroupId, KeyEntry)> {
    let mut b = datagram.strip_prefix(&[content::SENDER_KEY])?;
    let gid: GroupId = take(&mut b, 16)?.try_into().ok()?;
    let entry = take_entry(&mut b, [0; 32])?;
    b.is_empty().then_some((gid, entry))
}

fn take_entry(b: &mut &[u8], owner: PubKey) -> Option<KeyEntry> {
    Some(KeyEntry {
        owner,
        step: take_u32(b)?,
        key_epoch: take_byte(b)?,
        key: take(b, 32)?.try_into().ok()?,
        next_counter: take_u32(b)?,
    })
}

/// SENDER_KEYS (0x14, §10.2): keys the admin relays.
fn encode_sender_keys(gid: &GroupId, entries: &[KeyEntry]) -> Vec<u8> {
    let mut v = Vec::with_capacity(18 + SENDER_KEYS_ENTRY * entries.len());
    v.push(content::SENDER_KEYS);
    v.extend_from_slice(gid);
    v.push(entries.len().min(255) as u8);
    for e in entries {
        v.extend_from_slice(&e.owner);
        v.extend_from_slice(&e.step.to_le_bytes());
        v.push(e.key_epoch);
        v.extend_from_slice(&e.key);
        v.extend_from_slice(&e.next_counter.to_le_bytes());
    }
    v
}

fn decode_sender_keys(datagram: &[u8]) -> Option<(GroupId, Vec<KeyEntry>)> {
    let mut b = datagram.strip_prefix(&[content::SENDER_KEYS])?;
    let gid: GroupId = take(&mut b, 16)?.try_into().ok()?;
    let n = usize::from(take_byte(&mut b)?);
    if !(1..=MAX_MEMBERS).contains(&n) || b.len() != n * SENDER_KEYS_ENTRY {
        return None;
    }
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        let owner: PubKey = take(&mut b, 32)?.try_into().ok()?;
        entries.push(take_entry(&mut b, owner)?);
    }
    Some((gid, entries))
}

/// What a handover link's signature covers (§10.8): `"lichen/1 handover" ‖ SHA-256(state)`.
fn handover_digest(state: &[u8]) -> [u8; 32] {
    Sha256::digest(state).into()
}

fn handover_parts(digest: &[u8; 32]) -> [&[u8]; 2] {
    [b"lichen/1 handover", digest]
}

/// GROUP_HANDOVER (0x18, §10.8): `n (1..4) ‖ n × (len u16 ‖ signature ‖ state)`, from links
/// `signature ‖ state`.
fn encode_handover(links: &[&[u8]]) -> Vec<u8> {
    let mut v = vec![content::GROUP_HANDOVER, links.len().min(255) as u8];
    for l in links {
        let state_len = l.len().saturating_sub(SIGNATURE_LEN);
        v.extend_from_slice(&(state_len.min(usize::from(u16::MAX)) as u16).to_le_bytes());
        v.extend_from_slice(l);
    }
    v
}

/// A handover link: its signature, the state, and the link's bytes (`signature ‖ state`).
type Link = ([u8; SIGNATURE_LEN], State, Vec<u8>);

/// The links of a GROUP_HANDOVER, every one a state of the same group.
fn decode_handover(datagram: &[u8]) -> Option<Vec<Link>> {
    let mut b = datagram.strip_prefix(&[content::GROUP_HANDOVER])?;
    let n = usize::from(take_byte(&mut b)?);
    if !(1..=HANDOVER_LINKS).contains(&n) {
        return None;
    }
    let mut links: Vec<Link> = Vec::with_capacity(n);
    for _ in 0..n {
        let len = usize::from(u16::from_le_bytes(take(&mut b, 2)?.try_into().ok()?));
        let link = take(&mut b, SIGNATURE_LEN + len)?;
        let (sig, state) = link.split_first_chunk::<SIGNATURE_LEN>()?;
        let st = State::decode(state)?;
        if links
            .first()
            .is_some_and(|(_, first, _)| first.gid != st.gid)
        {
            return None;
        }
        links.push((*sig, st, link.to_vec()));
    }
    b.is_empty().then_some(links)
}

/// A group control datagram as decoded (§10.1, §10.2, §10.6–10.8): for vectors and the fuzzer.
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
    /// A member's own key (§10.2): `entry.owner` is left blank, the session tells it.
    SenderKey { gid: GroupId, entry: KeyEntry },
    /// Keys the admin relays.
    SenderKeys {
        gid: GroupId,
        entries: Vec<KeyEntry>,
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
    /// A member asks the admin for the keys, holding state `epoch` (§10.7).
    Sync { gid: GroupId, epoch: u32 },
    /// Handover links, `signature ‖ state` each, the oldest first (§10.8).
    Handover { links: Vec<Vec<u8>> },
    /// An author's last message, `epoch` its key epoch (§10.6): the body of a group message.
    Digest { epoch: u8, counter: u32 },
    /// The admin, holding state `epoch`, asks a member for its sender key (§10.7).
    KeyRequest { gid: GroupId, epoch: u32 },
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
                decode_sender_key(datagram).map(|(gid, entry)| Self::SenderKey { gid, entry })
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
            content::GROUP_SYNC => {
                decode_sync(datagram).map(|(gid, epoch)| Self::Sync { gid, epoch })
            }
            content::GROUP_HANDOVER => decode_handover(datagram).map(|links| Self::Handover {
                links: links.into_iter().map(|(_, _, l)| l).collect(),
            }),
            content::GROUP_DIGEST => {
                decode_digest(datagram).map(|(epoch, counter)| Self::Digest { epoch, counter })
            }
            content::GROUP_KEY_REQUEST => decode_gid_epoch(datagram, content::GROUP_KEY_REQUEST)
                .map(|(gid, epoch)| Self::KeyRequest { gid, epoch }),
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
            Self::SenderKey { gid, entry } => encode_sender_key(gid, entry),
            Self::SenderKeys { gid, entries } => encode_sender_keys(gid, entries),
            Self::Missing {
                gid,
                author,
                epoch,
                counters,
            } => encode_missing(gid, author, *epoch, counters),
            Self::Resend { gid, payloads } => encode_resend(gid, payloads),
            Self::Sync { gid, epoch } => encode_gid_epoch(content::GROUP_SYNC, gid, *epoch),
            Self::KeyRequest { gid, epoch } => {
                encode_gid_epoch(content::GROUP_KEY_REQUEST, gid, *epoch)
            }
            Self::Handover { links } => {
                encode_handover(&links.iter().map(Vec::as_slice).collect::<Vec<_>>())
            }
            Self::Digest { epoch, counter } => encode_digest(*epoch, *counter).to_vec(),
        }
    }
}

/// GROUP_DIGEST (0x19, §10.6): `key_epoch ‖ counter u32`, exactly 6 bytes with its type.
fn encode_digest(epoch: u8, counter: u32) -> [u8; 6] {
    let c = counter.to_le_bytes();
    [content::GROUP_DIGEST, epoch, c[0], c[1], c[2], c[3]]
}

fn decode_digest(body: &[u8]) -> Option<(u8, u32)> {
    match *body {
        [content::GROUP_DIGEST, epoch, c0, c1, c2, c3] => {
            Some((epoch, u32::from_le_bytes([c0, c1, c2, c3])))
        }
        _ => None,
    }
}

/// The group of a membership datagram (GROUP_STATE, SENDER_KEY, SENDER_KEYS,
/// GROUP_HANDOVER): what the admin still owes a member (SR-12).
pub(crate) fn membership_of(datagram: &[u8]) -> Option<GroupId> {
    match datagram.first()? {
        &content::GROUP_STATE | &content::SENDER_KEY | &content::SENDER_KEYS => {
            datagram.get(1..17)?.try_into().ok()
        }
        // n ‖ len ‖ signature ‖ 0x11 ‖ gid
        &content::GROUP_HANDOVER => datagram.get(69..85)?.try_into().ok(),
        _ => None,
    }
}

/// GROUP_SYNC (§10.7): `gid ‖ epoch u32`, exactly 21 bytes.
fn decode_sync(datagram: &[u8]) -> Option<(GroupId, u32)> {
    decode_gid_epoch(datagram, content::GROUP_SYNC)
}

/// `kind ‖ gid ‖ epoch u32` (GROUP_SYNC, GROUP_KEY_REQUEST, §10.7), exactly 21 bytes.
fn decode_gid_epoch(datagram: &[u8], kind: u8) -> Option<(GroupId, u32)> {
    let b = datagram.strip_prefix(&[kind])?;
    let (gid, epoch) = b.split_first_chunk::<16>()?;
    let epoch: [u8; 4] = epoch.try_into().ok()?;
    Some((*gid, u32::from_le_bytes(epoch)))
}

fn encode_gid_epoch(kind: u8, gid: &GroupId, epoch: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(21);
    v.push(kind);
    v.extend_from_slice(gid);
    v.extend_from_slice(&epoch.to_le_bytes());
    v
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
                match entries.iter_mut().find(|q| q.owner == e.owner) {
                    Some(q) => *q = e,
                    None => entries.push(e),
                }
            }
            (entries.len() <= MAX_MEMBERS).then(|| encode_sender_keys(&qg, &entries))
        }
        _ => None,
    }
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

    /// Whether this member floods a digest after its bursts (§10.6, ADR-0040): on by default,
    /// off only to measure what it brings.
    pub fn set_group_digest(&mut self, on: bool) {
        self.group_digest = on;
    }

    pub fn set_group_copies(&mut self, copies: u8) {
        self.group_copies = copies.clamp(1, 2);
    }

    /// Groups this node belongs to.
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.values()
    }

    fn fresh_key(&mut self) -> Secret {
        self.draw_secret()
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
        let state = State {
            gid,
            epoch: 1,
            admin: me,
            name: name.to_owned(),
            members: list,
            cause: None,
        };
        self.groups.insert(gid, Group::new(state, my_key));
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
        let epoch = g.epoch.saturating_add(1);
        for id in &ids {
            g.recovery.since.insert(*id, epoch);
            g.recovery.added_in.insert(*id, epoch);
        }
        // Every key steps: nothing sent before opens under what the newcomers get (§10.9).
        self.take_epoch(gid, epoch);
        self.dirty_groups.insert(*gid);
        self.broadcast_state(gid, &[], None);
        let actor = self.me_as_actor();
        self.changed(*gid, notice::JOINED, actor, subjects, String::new());
        // The newcomers get every key from us, after the state (§10.2).
        self.send_bundle(gid, &ids, false, false);
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
            g.epoch = g.epoch.saturating_add(1);
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

    /// Hands group `gid` over (admin only, §10.8, ADR-0038): the member reached through node
    /// `successor` becomes the admin, in a new state we sign and send to every other member.
    /// With `leave`, we leave at once (the state no longer lists us, `cause` 1) and forget the
    /// group, as a member that leaves; otherwise we stay a member. Our invitations stop.
    pub fn hand_over(
        &mut self,
        gid: &GroupId,
        successor: &PubKey,
        leave: bool,
        now: Millis,
    ) -> Result<(), Error> {
        let (me, prv) = (
            self.key.ok_or(Error::NoIdentity)?,
            self.prv.ok_or(Error::NoIdentity)?,
        );
        let g = self.groups.get(gid).ok_or(Error::UnknownGroup)?;
        if g.admin != me {
            return Err(Error::NotAllowed);
        }
        let next = g
            .member_by_node(successor)
            .cloned()
            .ok_or(Error::NotAllowed)?;
        if next.key == me {
            return Err(Error::NotAllowed);
        }
        let mut state = g.state(leave.then_some(cause::LEFT));
        state.epoch = g.epoch.saturating_add(1);
        state.admin = next.key;
        if leave {
            state.members.retain(|m| m.key != me);
        }
        let bytes = state.encode();
        let sig = sign(&prv, &handover_parts(&handover_digest(&bytes))).ok_or(Error::NoIdentity)?;
        let mut link = sig.to_vec();
        link.extend_from_slice(&bytes);
        let datagram = encode_handover(&[&link]);
        let others: Vec<PubKey> = g
            .members
            .iter()
            .filter(|m| m.key != me)
            .map(|m| m.node)
            .collect();
        for node in &others {
            self.enqueue_internal(node, datagram.clone());
        }
        self.invites.retain(|_, (g, _)| g != gid);
        let actor = self.me_as_actor();
        let subject = vec![(next.node, next.name.clone())];
        self.changed(
            *gid,
            notice::HANDED_OVER,
            actor.clone(),
            subject,
            String::new(),
        );
        self.dirty_groups.insert(*gid);
        if leave {
            self.changed(*gid, notice::WE_LEFT, actor, Vec::new(), String::new());
            self.groups.remove(gid);
            self.events.push_back(Event::GroupLeft { gid: *gid });
        } else {
            self.apply_state(state, None);
            if let Some(g) = self.groups.get_mut(gid) {
                g.keep_link(link);
            }
        }
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
        // The state the leaver held: not one that does not exist yet, nor one from before we
        // last added it (§10.4).
        let Some(held) = datagram
            .get(17..21)
            .and_then(|e| <[u8; 4]>::try_from(e).ok())
            .map(u32::from_le_bytes)
        else {
            return;
        };
        if held > g.epoch || g.recovery.added_in.get(from).is_some_and(|&e| held < e) {
            return;
        }
        let leaver = g.members.iter().find(|m| m.key == *from).cloned();
        g.members.retain(|m| m.key != *from);
        g.keys.remove(from);
        let epoch = g.epoch.saturating_add(1);
        g.take_epoch(epoch);
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
        self.invite_admissions.remove(token);
    }

    /// Asks to join a group with an invitation (§10.5). The admin becomes a contact and gets
    /// GROUP_JOIN over a pairwise session, sent again until it arrives. A zero-hop advert goes
    /// first: the admin's node, nearby, learns ours and can answer the handshake (§4.6).
    /// [`Event::GroupJoined`] follows once admitted.
    ///
    /// Scanned `in_person`, the invitation verifies the admin like a contact card (§4.7).
    /// Pasted or received some other way it proves nothing: a new admin is added unverified,
    /// and an identity other than the one the admin's node has waits for the user like any
    /// identity change ([`Error::IdentityPending`], nothing sent).
    pub fn join(&mut self, invite: &Invite, in_person: bool, now: Millis) -> Result<(), Error> {
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
        // Scanned in person, like a contact card (§4.7): it verifies the admin, and replaces an
        // identity the admin's node no longer has, through the same steps as an accepted
        // change, so the app is told (review of 2026-11-15, SR-19). Pasted, it must not: a
        // code sent by message could otherwise rebind a verified contact's node to whoever
        // wrote it (second read of 2026-11-17, N1).
        let adopted = self.present_identity(&invite.node, invite.admin)
            || (in_person && self.accept_identity(&invite.node, &invite.admin, true));
        if !adopted {
            return Err(Error::IdentityPending);
        }
        if in_person {
            self.set_verified(&invite.node, &invite.admin, true);
        }
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
            // every key we hold, its own last one included so it moves past that epoch; then we
            // forget its key, and relay its next ones to everyone for a while (§10.5, SR-20).
            let state = g.state(None).encode();
            self.enqueue_internal(node, state);
            self.send_bundle(&gid, &[id], true, false);
            if let Some(g) = self.groups.get_mut(&gid) {
                g.keys.remove(&id);
                g.recovery.rejoined.retain(|_, at| *at + ADMITTED_MS > now);
                g.recovery.rejoined.insert(id, now);
            }
            self.dirty_groups.insert(gid);
            self.tick(now);
            return;
        }
        // A photographed code admits a few people, not a crowd (SR-19).
        if self.invite_admissions.get(&token).copied().unwrap_or(0) >= INVITE_ADMISSIONS_MAX {
            return;
        }
        if self.add_members(&gid, &[*node], now).is_ok() {
            *self.invite_admissions.entry(token).or_insert(0) += 1;
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
        let epoch = g.epoch.saturating_add(1);
        self.take_epoch(gid, epoch);
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
        g.keys.remove(&id);
        let epoch = g.epoch.saturating_add(1);
        g.take_epoch(epoch);
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
        let state = g.state(cause).encode();
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
        let datagram = encode_sender_key(
            gid,
            &KeyEntry {
                owner: me,
                step: g.epoch,
                key_epoch: g.my_epoch,
                key: *g.my_key,
                next_counter: g.my_counter,
            },
        );
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

    /// The group knew us up to key epoch `epoch` (we lost our state since): a new key past it,
    /// sent to the members. An epoch already behind ours changes nothing.
    fn move_past(&mut self, gid: &GroupId, epoch: u8) {
        let Some(g) = self.groups.get_mut(gid) else {
            return;
        };
        if epoch != g.my_epoch && !newer(epoch, g.my_epoch) {
            return;
        }
        g.my_epoch = epoch;
        self.rotate_my_key(gid);
    }

    /// Takes state epoch `e` for group `gid` (§10.9): every key steps forward; ours is drawn
    /// anew when it cannot step that far.
    fn take_epoch(&mut self, gid: &GroupId, e: u32) {
        if self.groups.get_mut(gid).is_some_and(|g| !g.take_epoch(e)) {
            self.rotate_my_key(gid);
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

    /// A group control datagram (GROUP_STATE, SENDER_KEY, GROUP_LEAVE, GROUP_JOIN,
    /// GROUP_HANDOVER, …; §10) from node `from`, whose identity the session proved.
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
                if let Some((gid, mut entry)) = decode_sender_key(datagram) {
                    entry.owner = *from;
                    self.on_sender_key(gid, entry, now);
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
            Some(&content::GROUP_SYNC) => {
                if let Some((gid, epoch)) = decode_sync(datagram) {
                    let from = *from;
                    self.on_group_sync(&from, gid, epoch, now);
                }
            }
            Some(&content::GROUP_HANDOVER) => self.on_group_handover(datagram, now),
            Some(&content::GROUP_KEY_REQUEST) => {
                if let Some((gid, _)) = decode_gid_epoch(datagram, content::GROUP_KEY_REQUEST) {
                    let from = *from;
                    self.on_key_request(&from, gid, now);
                }
            }
            // A digest travels in group envelopes only (§10.6).
            _ => {}
        }
    }

    /// A member asks for the keys (§10.7): the handovers it missed (§10.8), the state when its
    /// own is older, and every key we hold, as to a newcomer; once per member in 10 minutes. A
    /// former admin answers with the handovers alone.
    fn on_group_sync(&mut self, from: &PubKey, gid: GroupId, held: u32, now: Millis) {
        let Some(me) = self.key else {
            return;
        };
        let Some(g) = self.groups.get_mut(&gid) else {
            return;
        };
        if *from == me || !g.is_member(from) {
            return;
        }
        let admin = g.admin == me;
        if !admin && g.links_after(held).is_empty() {
            return;
        }
        if g.recovery
            .syncs_answered
            .get(from)
            .is_some_and(|&t| t + SYNC_EVERY_MS > now)
        {
            return;
        }
        g.recovery
            .syncs_answered
            .retain(|_, t| *t + SYNC_EVERY_MS > now);
        // A budget per group: what a forged early envelope may still set off (§10.7).
        if g.recovery.syncs_answered.len() >= SYNC_ANSWERS_MAX {
            return;
        }
        g.recovery.syncs_answered.insert(*from, now);
        let Some(node) = g.members.iter().find(|m| m.key == *from).map(|m| m.node) else {
            return;
        };
        // What still waits for it is older than what follows.
        if admin {
            self.drop_membership(&node, &gid);
        }
        let Some(g) = self.groups.get(&gid) else {
            return;
        };
        let links = g.links_after(held);
        let handover = (!links.is_empty()).then(|| encode_handover(&links));
        let state = (admin && held < g.epoch).then(|| g.state(None).encode());
        if let Some(d) = handover {
            self.enqueue_internal(&node, d);
        }
        if let Some(d) = state {
            self.enqueue_internal(&node, d);
        }
        if admin {
            self.send_bundle(&gid, &[*from], false, true);
        }
        // It reached us: what waits for it goes now, the answer merged into it included.
        self.hurry_internal(&node, now);
        self.tick(now);
    }

    /// Envelopes held a minute from a member of which we hold no key at all (§10.7): a member
    /// asks the admin for the keys (GROUP_SYNC, once in 10 minutes per group); the admin asks
    /// that member for its key (GROUP_KEY_REQUEST, once in 10 minutes per member). A key of a
    /// newer epoch than one held is not asked for: the clear header of an envelope proves
    /// nothing before the key comes.
    fn ask_sync(&mut self, now: Millis) {
        let Some(me) = self.key else {
            return;
        };
        let waiting: Vec<(u16, u8)> = self
            .held
            .iter()
            .filter(|h| h.at + SYNC_AFTER_MS <= now)
            .map(|h| (h.gid_hash, h.sender))
            .collect();
        if waiting.is_empty() {
            return;
        }
        let mut asks = Vec::new();
        for g in self.groups.values_mut() {
            let gh = g.gid_hash();
            let keyless: Vec<(PubKey, PubKey)> = g
                .members
                .iter()
                .filter(|m| m.key != me && waiting.contains(&(gh, m.key[0])))
                .filter(|m| g.keys.get(&m.key).is_none_or(|mk| mk.current.is_none()))
                .map(|m| (m.key, m.node))
                .collect();
            if keyless.is_empty() {
                continue;
            }
            if g.admin == me {
                for (id, node) in keyless {
                    if g.recovery
                        .keys_asked
                        .get(&id)
                        .is_some_and(|&t| t + SYNC_EVERY_MS > now)
                    {
                        continue;
                    }
                    g.recovery.keys_asked.insert(id, now);
                    asks.push((
                        node,
                        encode_gid_epoch(content::GROUP_KEY_REQUEST, &g.gid, g.epoch),
                    ));
                }
                continue;
            }
            if g.recovery
                .synced_at
                .is_some_and(|t| t + SYNC_EVERY_MS > now)
            {
                continue;
            }
            let Some(admin_node) = g.members.iter().find(|m| m.key == g.admin).map(|m| m.node)
            else {
                continue;
            };
            g.recovery.synced_at = Some(now);
            asks.push((
                admin_node,
                encode_gid_epoch(content::GROUP_SYNC, &g.gid, g.epoch),
            ));
        }
        for (node, datagram) in asks {
            self.enqueue_control(&node, datagram);
        }
    }

    /// As admin, asks every member of group `gid` whose key we do not hold (§10.7): our group
    /// was restored without the members' keys. Once in 10 minutes per member.
    fn ask_missing_keys(&mut self, gid: &GroupId, now: Millis) {
        let Some(me) = self.key else {
            return;
        };
        let Some(g) = self.groups.get_mut(gid) else {
            return;
        };
        if g.admin != me {
            return;
        }
        let keyless: Vec<(PubKey, PubKey)> = g
            .members
            .iter()
            .filter(|m| m.key != me)
            .filter(|m| g.keys.get(&m.key).is_none_or(|mk| mk.current.is_none()))
            .filter(|m| {
                g.recovery
                    .keys_asked
                    .get(&m.key)
                    .is_none_or(|&t| t + SYNC_EVERY_MS <= now)
            })
            .map(|m| (m.key, m.node))
            .collect();
        let ask = encode_gid_epoch(content::GROUP_KEY_REQUEST, gid, g.epoch);
        for (id, _) in &keyless {
            g.recovery.keys_asked.insert(*id, now);
        }
        for (_, node) in keyless {
            self.enqueue_control(&node, ask.clone());
        }
    }

    /// The admin asks us for our key (§10.7): our current one goes to it alone, once in 10
    /// minutes per group.
    fn on_key_request(&mut self, from: &PubKey, gid: GroupId, now: Millis) {
        let Some(me) = self.key else {
            return;
        };
        let Some(g) = self.groups.get_mut(&gid) else {
            return;
        };
        if g.admin != *from || *from == me {
            return;
        }
        if g.recovery
            .key_sent_at
            .is_some_and(|t| t + SYNC_EVERY_MS > now)
        {
            return;
        }
        g.recovery.key_sent_at = Some(now);
        self.send_my_key(&gid, Some(&[*from]));
        self.tick(now);
    }

    /// When a held envelope will make us ask (§10.7), if it ever does.
    fn next_sync(&self) -> Option<Millis> {
        let me = self.key?;
        self.held
            .iter()
            .filter_map(|h| {
                self.groups
                    .values()
                    .filter(|g| g.gid_hash() == h.gid_hash)
                    .filter_map(|g| {
                        let after = h.at + SYNC_AFTER_MS;
                        let m = g.members.iter().find(|m| {
                            m.key != me
                                && m.key[0] == h.sender
                                && g.keys.get(&m.key).is_none_or(|mk| mk.current.is_none())
                        })?;
                        let last = if g.admin == me {
                            g.recovery.keys_asked.get(&m.key).copied()
                        } else {
                            g.recovery.synced_at
                        };
                        Some(last.map_or(after, |t| after.max(t + SYNC_EVERY_MS)))
                    })
                    .filter(|&due| due < h.at + HELD_MS)
                    .min()
            })
            .min()
    }

    /// A member asks for messages it missed (§10.6): what we kept of them sent at a step since
    /// it joined (§10.9); those we miss too, it gets once we recover them.
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
        let answered = &g.recovery.answered;
        let mut held: Vec<&Kept> = g
            .recovery
            .kept
            .iter()
            .filter(|k| {
                k.author == *author
                    && k.epoch == epoch
                    && counters.contains(&k.counter)
                    && k.step >= since
                    && !answered.contains(&(*from, k.author, k.epoch, k.counter))
            })
            .collect();
        // In the order they were written, whatever order they came to us in.
        held.sort_by_key(|k| k.counter);
        let mut sent: Vec<u32> = Vec::new();
        for k in held {
            if payloads.len() + k.payloads.len() > RESEND_MAX {
                break;
            }
            payloads.extend(k.payloads.iter().cloned());
            sent.push(k.counter);
        }
        let r = &mut g.recovery;
        for c in sent {
            if r.answered.len() >= ANSWERED_MAX {
                r.answered.pop_front();
            }
            r.answered.push_back((*from, *author, epoch, c));
        }
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
        let Some(g) = self.groups.get(&gid) else {
            self.invitation(state, listed, now);
            return;
        };
        if state.epoch <= g.epoch {
            return;
        }
        if g.admin != *from {
            // A listed member names itself admin with a newer state: a handover we missed
            // (§10.8). The state waits for it, and that member is asked for it.
            if listed && g.is_member(from) {
                self.pull_handover(gid, *from, state, now);
            }
            return;
        }
        self.apply_state(state, None);
        self.take_pending_keys(&gid, now);
    }

    /// A newer state from `from`, a member that is not our admin and names itself admin: kept
    /// aside, and `from` is asked with GROUP_SYNC for the handovers we lack, at most once in
    /// 10 minutes per group (§10.8).
    fn pull_handover(&mut self, gid: GroupId, from: PubKey, state: State, now: Millis) {
        let Some(g) = self.groups.get_mut(&gid) else {
            return;
        };
        if g.recovery
            .orphan
            .as_ref()
            .is_none_or(|(_, o)| o.epoch < state.epoch)
        {
            g.recovery.orphan = Some((from, state));
        }
        if g.recovery
            .pulled_at
            .is_some_and(|t| t + SYNC_EVERY_MS > now)
        {
            return;
        }
        let Some(node) = g.members.iter().find(|m| m.key == from).map(|m| m.node) else {
            return;
        };
        g.recovery.pulled_at = Some(now);
        let ask = Control::Sync {
            gid,
            epoch: g.epoch,
        }
        .encode();
        self.enqueue_control(&node, ask);
        self.tick(now);
    }

    /// Takes a newer state of a group we belong to (§10.1): from its admin, or from a handover
    /// link the admin signed (`handed`: the former admin, as the actor of the notice, §10.8).
    /// Members, name, key step or rotation, notices.
    fn apply_state(&mut self, state: State, handed: Option<(PubKey, String)>) {
        let Some(me) = self.key else {
            return;
        };
        let gid = state.gid;
        let listed = state.members.iter().any(|m| m.key == me);
        let admin = admin_actor(&state);
        self.dirty_groups.insert(gid);
        if listed {
            self.learn_members(&state.members);
        }
        let Some(g) = self.groups.get_mut(&gid) else {
            return;
        };
        let successor = (state.admin != g.admin).then(|| admin.clone());
        if !listed {
            self.groups.remove(&gid);
            self.events.push_back(Event::GroupLeft { gid });
            // Closed (the admin alone, no cause), or we were removed.
            let kind = if state.members.len() == 1 && state.cause.is_none() {
                notice::CLOSED
            } else {
                notice::REMOVED_US
            };
            let actor = handed.unwrap_or(admin);
            self.changed(gid, kind, actor, Vec::new(), String::new());
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
        g.members = state.members;
        if g.admin != state.admin {
            // The admin to ask for the keys changed: we may ask it at once (§10.8).
            g.recovery.synced_at = None;
            g.admin = state.admin;
        }
        for m in &g.members {
            if !old.contains(&m.key) {
                g.recovery.since.insert(m.key, state.epoch);
            }
        }
        for k in &removed {
            g.keys.remove(k);
        }
        // Every key steps to the new epoch (§10.9); someone gone: ours is drawn anew (§10.2).
        self.take_epoch(&gid, state.epoch);
        if !removed.is_empty() {
            self.rotate_my_key(&gid);
        }
        self.events.push_back(Event::GroupUpdated { gid });
        if let (Some(former), Some(next)) = (handed.clone(), successor) {
            self.changed(gid, notice::HANDED_OVER, former, vec![next], String::new());
        }
        let actor = handed.unwrap_or_else(|| admin.clone());
        if !gone.is_empty() {
            let (kind, by) = match state.cause {
                Some(cause::LEFT) => (notice::LEFT, gone[0].clone()),
                Some(cause::REMOVED) => (notice::REMOVED, actor.clone()),
                _ => (notice::GONE, actor.clone()),
            };
            self.changed(gid, kind, by, gone, String::new());
        }
        if !joined.is_empty() {
            self.changed(gid, notice::JOINED, actor.clone(), joined, String::new());
        }
        if let Some(name) = renamed {
            self.changed(gid, notice::RENAMED, actor, Vec::new(), name);
        }
    }

    /// GROUP_HANDOVER (§10.8): links taken in order, each signed by the admin before it, for a
    /// group we belong to or an invitation we hold. Who delivers it does not matter.
    fn on_group_handover(&mut self, datagram: &[u8], now: Millis) {
        let Some(links) = decode_handover(datagram) else {
            return;
        };
        let Some(gid) = links.first().map(|(_, s, _)| s.gid) else {
            return;
        };
        let held = match (
            self.groups.get(&gid),
            self.invitations.iter().find(|i| i.state.gid == gid),
        ) {
            (Some(g), _) => (g.admin, g.epoch),
            (None, Some(i)) => (i.state.admin, i.state.epoch),
            (None, None) => return,
        };
        let (mut admin, mut epoch) = held;
        let mut taken: Vec<Link> = Vec::new();
        for (sig, state, bytes) in links {
            if state.epoch <= epoch {
                continue;
            }
            let Some(signed) = bytes.get(SIGNATURE_LEN..) else {
                break;
            };
            let digest = handover_digest(signed);
            if state.admin == admin || !verify(&admin, &handover_parts(&digest), &sig) {
                break;
            }
            admin = state.admin;
            epoch = state.epoch;
            taken.push((sig, state, bytes));
        }
        if taken.is_empty() {
            return;
        }
        if !self.groups.contains_key(&gid) {
            // A held invitation follows its admin (§10.1): the newest state, or withdrawn.
            let Some(me) = self.key else {
                return;
            };
            let Some((_, last, _)) = taken.pop() else {
                return;
            };
            let listed = last.members.iter().any(|m| m.key == me);
            if let Some(pos) = self.invitations.iter().position(|i| i.state.gid == gid) {
                if listed {
                    self.invitations[pos].state = last;
                } else {
                    self.invitations.remove(pos);
                }
                self.dirty_invitations.insert(gid);
            }
            return;
        }
        for (_, state, bytes) in taken {
            let Some(g) = self.groups.get(&gid) else {
                break;
            };
            let former = g
                .members
                .iter()
                .find(|m| m.key == g.admin)
                .map_or(([0; 32], String::new()), |m| (m.node, m.name.clone()));
            self.apply_state(state, Some(former));
            if let Some(g) = self.groups.get_mut(&gid) {
                g.keep_link(bytes);
            }
        }
        // A state its new admin sent before the handover reached us.
        let orphan = self.groups.get_mut(&gid).and_then(|g| {
            let keep = g
                .recovery
                .orphan
                .as_ref()
                .is_some_and(|(from, o)| *from == g.admin && o.epoch > g.epoch);
            let o = g.recovery.orphan.take();
            keep.then_some(o).flatten()
        });
        if let Some((_, state)) = orphan {
            self.apply_state(state, None);
        }
        self.take_pending_keys(&gid, now);
        self.tick(now);
    }

    /// Keys that came before group `gid`'s state (or while it waited for the user) are used;
    /// other groups' keep waiting, ten minutes, or as long as their invitation waits.
    fn take_pending_keys(&mut self, gid: &GroupId, now: Millis) {
        let pending: Vec<PendingKey> = std::mem::take(&mut self.pending_keys);
        for p in pending {
            if p.gid == *gid {
                let entry = KeyEntry {
                    owner: p.from,
                    step: p.step,
                    key_epoch: p.epoch,
                    key: *p.key,
                    next_counter: p.next_counter,
                };
                let admin = self.groups.get(gid).map(|g| g.admin);
                match p.via {
                    None => self.on_sender_key(*gid, entry, now),
                    Some(via) if admin.is_none_or(|a| a == via) => {
                        self.on_sender_keys(&via, *gid, vec![entry], now);
                    }
                    // Relayed by a member whose handover has not reached us yet (§10.8).
                    Some(_) if p.at + PENDING_KEY_MS > now => self.pending_keys.push(p),
                    Some(_) => {}
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
        self.expire_invitations(now);
        self.joining.retain(|(_, at)| at + JOINING_MS > now);
        if let Some(pos) = self.invitations.iter().position(|i| i.state.gid == gid) {
            let held = &self.invitations[pos].state;
            if held.admin != state.admin || state.epoch <= held.epoch {
                return;
            }
            if !listed {
                self.invitations.remove(pos);
                self.dirty_invitations.insert(gid);
                return;
            }
            self.invitations[pos].state = state;
            self.dirty_invitations.insert(gid);
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
            let old = self.invitations.remove(0);
            self.dirty_invitations.insert(old.state.gid);
        }
        self.events.push_back(Event::GroupInvited {
            gid,
            name: state.name.clone(),
            admin: state.admin,
            members: state.members.len(),
        });
        let at_unix = self.unix_s(now);
        self.invitations.push(Invitation {
            state,
            at: now,
            at_unix,
        });
        self.dirty_invitations.insert(gid);
    }

    /// Becomes a member of the group of `state` (§10.1): its members become contacts, our
    /// first key goes to the admin (§10.2), the keys that came early are used.
    fn join_group(&mut self, state: State, now: Millis) {
        let gid = state.gid;
        let admin = admin_actor(&state);
        self.dirty_groups.insert(gid);
        self.learn_members(&state.members);
        let my_key = self.fresh_key();
        let (name, admin_id) = (state.name.clone(), state.admin);
        self.groups.insert(gid, Group::new(state, my_key));
        // Our first key goes to the admin, who forwards it (§10.2).
        self.send_my_key(&gid, Some(&[admin_id]));
        self.events.push_back(Event::GroupJoined {
            gid,
            name,
            admin: admin_id,
        });
        self.changed(gid, notice::ADDED_US, admin, Vec::new(), String::new());
        self.take_pending_keys(&gid, now);
    }

    /// Whether `payload`, a copy of an envelope heard on the air, verifies under `member`'s
    /// sender key for its epoch, in a group whose hash it names: only such a copy is a
    /// reception of that member (§11.2). Its clear header alone proves nothing (review of
    /// 2026-11-15, SR-16).
    pub(crate) fn copy_is_from(&self, member: &PubKey, payload: &[u8]) -> bool {
        let Some((h, _)) = group::parse(payload) else {
            return false;
        };
        self.groups
            .values()
            .filter(|g| gid_hash(&g.gid) == h.gid_hash)
            .filter_map(|g| g.keys.get(member).map(|k| (g.gid, k)))
            .flat_map(|(gid, k)| [(gid, k.current.as_ref()), (gid, k.previous.as_ref())])
            .filter_map(|(gid, k)| k.map(|k| (gid, k)))
            .filter(|(_, k)| k.epoch == h.key_epoch)
            .any(|(gid, k)| k.open(&gid, payload).is_some())
    }

    /// Forgets the other members' sender keys of group `gid`, as a group restored from a
    /// backup that leaves them out (tests of §10.7).
    #[doc(hidden)]
    pub fn drop_group_keys(&mut self, gid: &GroupId) {
        if let Some(g) = self.groups.get_mut(gid) {
            g.keys.clear();
            self.dirty_groups.insert(*gid);
        }
    }

    /// Whether any sender key this phone holds for group `gid`, at any step it would try,
    /// opens `payload`, whatever the replay windows say: what a member could read of a
    /// recording (tests of the key step, §10.9).
    #[doc(hidden)]
    pub fn group_opens(&self, gid: &GroupId, payload: &[u8]) -> bool {
        let Some(g) = self.groups.get(gid) else {
            return false;
        };
        let ours = KeyState::new(g.my_epoch, *g.my_key, g.epoch, 0);
        g.keys
            .values()
            .flat_map(|mk| {
                [mk.current.as_ref(), mk.previous.as_ref()]
                    .into_iter()
                    .flatten()
                    .chain(mk.retired.iter())
            })
            .chain(std::iter::once(&ours))
            .any(|k| k.open(gid, payload).is_some())
    }

    /// Invitations older than a week go, by the clock of this run or, across restarts, by
    /// the wall clock.
    fn expire_invitations(&mut self, now: Millis) {
        let wall = self.unix_s(now);
        let before = self.invitations.len();
        let gone: Vec<GroupId> = self
            .invitations
            .iter()
            .filter(|i| {
                i.at + INVITATION_MS <= now
                    || wall
                        .zip(i.at_unix)
                        .is_some_and(|(w, a)| w > a + INVITATION_MS / 1000)
            })
            .map(|i| i.state.gid)
            .collect();
        if gone.is_empty() || before == 0 {
            return;
        }
        self.invitations.retain(|i| !gone.contains(&i.state.gid));
        self.pending_keys.retain(|p| !gone.contains(&p.gid));
        self.dirty_invitations.extend(gone);
    }

    /// Invitations that changed since the app last stored them (added, answered, or with new
    /// keys): store [`Core::invitation_snapshot`], or delete it when there is none.
    pub fn take_dirty_invitations(&mut self) -> Vec<GroupId> {
        self.dirty_invitations.drain().collect()
    }

    /// What the app stores for invitation `gid`, so that it survives a restart until the user
    /// answers: `2 ‖ at_unix u64 (0 unknown) ‖ state_len u16 ‖ GROUP_STATE ‖ n u8 ‖ n × key`
    /// (the sender keys that came with it, each `owner ‖ relayed 0|1 ‖ by ‖ step u32 ‖
    /// key_epoch ‖ key ‖ next_counter u32`). Holds keys: it is stored like a group.
    pub fn invitation_snapshot(&self, gid: &GroupId) -> Option<Vec<u8>> {
        let inv = self.invitations.iter().find(|i| i.state.gid == *gid)?;
        let state = inv.state.encode();
        let keys: Vec<&PendingKey> = self.pending_keys.iter().filter(|p| p.gid == *gid).collect();
        let mut v = vec![INVITATION_SNAPSHOT];
        v.extend_from_slice(&inv.at_unix.unwrap_or(0).to_le_bytes());
        v.extend_from_slice(&u16::try_from(state.len()).ok()?.to_le_bytes());
        v.extend_from_slice(&state);
        v.push(u8::try_from(keys.len()).ok()?);
        for k in keys {
            v.extend_from_slice(&k.from);
            v.push(u8::from(k.via.is_some()));
            v.extend_from_slice(&k.via.unwrap_or([0; 32]));
            v.extend_from_slice(&k.step.to_le_bytes());
            v.push(k.epoch);
            v.extend_from_slice(&k.key[..]);
            v.extend_from_slice(&k.next_counter.to_le_bytes());
        }
        Some(v)
    }

    /// An invitation the app stored, back after a restart. False when it does not decode, or
    /// we joined that group meanwhile.
    pub fn restore_invitation(&mut self, snapshot: &[u8], now: Millis) -> bool {
        let Some((&INVITATION_SNAPSHOT, rest)) = snapshot.split_first() else {
            return false;
        };
        let Some((at, rest)) = rest.split_first_chunk::<8>() else {
            return false;
        };
        let Some((len, rest)) = rest.split_first_chunk::<2>() else {
            return false;
        };
        let len = usize::from(u16::from_le_bytes(*len));
        let (Some(state), Some(rest)) = (rest.get(..len), rest.get(len..)) else {
            return false;
        };
        let Some(state) = State::decode(state) else {
            return false;
        };
        let Some((&n, keys)) = rest.split_first() else {
            return false;
        };
        if keys.len() != usize::from(n) * KEY_ENTRY_LEN
            || self.groups.contains_key(&state.gid)
            || self.invitations.iter().any(|i| i.state.gid == state.gid)
        {
            return false;
        }
        let gid = state.gid;
        for k in keys.as_chunks::<KEY_ENTRY_LEN>().0 {
            let field = |a: usize, b: usize| k.get(a..b).unwrap_or(&[]);
            let (Ok(from), Ok(via), Ok(step), Ok(key), Ok(next)) = (
                <[u8; 32]>::try_from(field(0, 32)),
                <[u8; 32]>::try_from(field(33, 65)),
                <[u8; 4]>::try_from(field(65, 69)),
                <[u8; 32]>::try_from(field(70, 102)),
                <[u8; 4]>::try_from(field(102, 106)),
            ) else {
                return false;
            };
            self.pending_keys.push(PendingKey {
                from,
                via: (k.get(32) == Some(&1)).then_some(via),
                gid,
                step: u32::from_le_bytes(step),
                epoch: k.get(69).copied().unwrap_or(0),
                key: Secret(key),
                next_counter: u32::from_le_bytes(next),
                at: now,
            });
        }
        let at_unix = u64::from_le_bytes(*at);
        self.invitations.push(Invitation {
            state,
            at: now,
            at_unix: (at_unix != 0).then_some(at_unix),
        });
        true
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
        self.dirty_invitations.insert(*gid);
        self.join_group(inv.state, now);
        Ok(())
    }

    /// The user does not join: the invitation is forgotten. The admin is not told; it keeps
    /// listing us until it removes us.
    pub fn decline_group(&mut self, gid: &GroupId) {
        self.invitations.retain(|i| i.state.gid != *gid);
        self.pending_keys.retain(|p| p.gid != *gid);
        self.dirty_invitations.insert(*gid);
    }

    /// Joins at once when listed (test nodes and bots), or waits for the user (the apps).
    pub fn set_group_consent(&mut self, ask: bool) {
        self.group_consent = ask;
    }

    /// A SENDER_KEY from its owner, `e.owner` (the session's identity, §10.2).
    fn on_sender_key(&mut self, gid: GroupId, e: KeyEntry, now: Millis) {
        let from = e.owner;
        let Some(g) = self.groups.get_mut(&gid) else {
            if self.invitations.iter().any(|i| i.state.gid == gid) {
                self.dirty_invitations.insert(gid);
            }
            if self.pending_keys.len() < 32 {
                self.pending_keys.push(PendingKey {
                    from,
                    via: None,
                    gid,
                    step: e.step,
                    epoch: e.key_epoch,
                    key: Secret(e.key),
                    next_counter: e.next_counter,
                    at: now,
                });
            }
            return;
        };
        if !g.is_member(&from) {
            return;
        }
        // A key we asked for (§10.7) is no first key: the others hold it.
        let asked = g
            .recovery
            .keys_asked
            .get(&from)
            .is_some_and(|&at| at + SYNC_EVERY_MS > now);
        let first = (g.keys.get(&from).is_none_or(|mk| mk.current.is_none()) && !asked)
            || g.recovery
                .rejoined
                .get(&from)
                .is_some_and(|&at| at + ADMITTED_MS > now);
        if !self.adopt_key(gid, &e, false, now) {
            return;
        }
        // The admin forwards it (§10.2): to everyone for a member's first key (or a member that
        // joined again lately, §10.5), else to the members it admitted lately (a key in flight
        // during their admission). At the step we hold it at, never an earlier one (§10.9).
        let me = self.key;
        let Some(g) = self.groups.get(&gid) else {
            return;
        };
        if Some(g.admin) != me {
            return;
        }
        let Some(c) = g.keys.get(&from).and_then(|mk| mk.current.as_ref()) else {
            return;
        };
        let entry = KeyEntry {
            owner: from,
            step: c.at,
            key_epoch: c.epoch,
            key: *c.key,
            next_counter: e.next_counter,
        };
        self.admitted.retain(|(_, _, at)| at + ADMITTED_MS > now);
        let targets: Vec<PubKey> = g
            .members
            .iter()
            .filter(|m| m.key != from && Some(m.key) != me)
            .filter(|m| {
                first
                    || self
                        .admitted
                        .iter()
                        .any(|(a, id, _)| *a == gid && *id == m.key)
            })
            .map(|m| m.node)
            .collect();
        let datagram = encode_sender_keys(&gid, &[entry]);
        for node in targets {
            self.enqueue_control(&node, datagram.clone());
        }
    }

    /// SENDER_KEYS from `from` (§10.2): keys the admin relays. Only from the group's admin;
    /// a relayed key never replaces a newer one.
    fn on_sender_keys(&mut self, from: &PubKey, gid: GroupId, entries: Vec<KeyEntry>, now: Millis) {
        let me = self.key;
        let group = self.groups.get(&gid);
        if let Some(g) = group
            && g.admin != *from
        {
            // From the member whose newer state waits for its handover (§10.8): the keys wait
            // with it.
            let waiting = g.recovery.orphan.as_ref().is_some_and(|(o, _)| o == from);
            if waiting {
                for e in entries {
                    if self.pending_keys.len() >= 32 {
                        break;
                    }
                    self.pending_keys.push(PendingKey {
                        from: e.owner,
                        via: Some(*from),
                        gid,
                        step: e.step,
                        epoch: e.key_epoch,
                        key: Secret(e.key),
                        next_counter: e.next_counter,
                        at: now,
                    });
                }
            }
            return;
        }
        // One entry per owner: a bundle cannot walk a key's epoch around the ring (SR-18).
        let mut seen: HashSet<PubKey> = HashSet::new();
        for e in entries {
            if !seen.insert(e.owner) {
                continue;
            }
            if Some(e.owner) == me {
                // Our own last key, from before we lost the group: our next one goes past it,
                // so the others take it as newer (§10.5, SR-20).
                self.move_past(&gid, e.key_epoch);
                continue;
            }
            let listed = self.groups.get(&gid).is_some_and(|g| g.is_member(&e.owner));
            if !listed {
                // A group or an owner whose state has not reached us yet.
                if self.invitations.iter().any(|i| i.state.gid == gid) {
                    self.dirty_invitations.insert(gid);
                }
                if self.pending_keys.len() < 32 {
                    self.pending_keys.push(PendingKey {
                        from: e.owner,
                        via: Some(*from),
                        gid,
                        step: e.step,
                        epoch: e.key_epoch,
                        key: Secret(e.key),
                        next_counter: e.next_counter,
                        at: now,
                    });
                }
                continue;
            }
            self.adopt_key(gid, &e, true, now);
        }
    }

    /// Stores `e.owner`'s key, brought to our step (§10.9); a relayed one only when newer than
    /// ours. True when adopted; held envelopes are tried again.
    fn adopt_key(&mut self, gid: GroupId, e: &KeyEntry, relayed: bool, now: Millis) -> bool {
        let Some(g) = self.groups.get_mut(&gid) else {
            return false;
        };
        // Too far from our epoch to step (§10.9).
        if e.step.abs_diff(g.epoch) > STEP_MAX {
            return false;
        }
        let epoch = g.epoch;
        let mk = g.keys.entry(e.owner).or_default();
        match mk.current.as_ref() {
            // The same key again; or a relayed one at the epoch we hold. From its owner, another
            // key at that epoch replaces ours: the owner lost its state (§10.2, SR-20).
            Some(c) if c.epoch == e.key_epoch && (relayed || c.same_key(&gid, &e.key, e.step)) => {
                return false;
            }
            Some(c) if relayed && !newer(e.key_epoch, c.epoch) => return false,
            _ => {}
        }
        // A key held before comes back with its windows, whatever epoch it comes under.
        let earlier = [mk.current.as_ref(), mk.previous.as_ref()]
            .into_iter()
            .flatten()
            .chain(mk.retired.iter())
            .find(|k| k.same_key(&gid, &e.key, e.step))
            .cloned();
        let mut state = match earlier {
            Some(mut k) => {
                k.step_to(&gid, e.step);
                k.epoch = e.key_epoch;
                k.floor = k.floor.max(e.next_counter);
                k
            }
            None => KeyState::new(e.key_epoch, e.key, e.step, e.next_counter),
        };
        if !state.step_to(&gid, epoch) {
            return false;
        }
        if let Some(old) = mk.previous.take() {
            mk.retired.retain(|k| !k.same_key(&gid, &old.key, old.at));
            mk.retired.push_back(old);
            if mk.retired.len() > RETIRED_MAX {
                mk.retired.pop_front();
            }
        }
        mk.previous = mk.current.take();
        mk.current = Some(state);
        self.dirty_groups.insert(gid);
        // Messages that came before this key can now be read.
        let held: Vec<Held> = std::mem::take(&mut self.held).into();
        for h in held {
            self.on_group_envelope(&h.payload, h.snr4, h.rssi, h.at.max(now));
        }
        true
    }

    /// One SENDER_KEYS to each newcomer (identities) with every current key we hold, ours
    /// included, each at the step we hold it at (§10.2, §10.9); with `own`, the newcomer's own
    /// last key too (a member that lost the group, §10.5). Each key from the next counter we
    /// expect, so that a newcomer does not read what came before it; with `from_start`, from
    /// the key's first counter instead (a member that missed the keys and holds what was sent
    /// under them, §10.7).
    fn send_bundle(&mut self, gid: &GroupId, newcomers: &[PubKey], own: bool, from_start: bool) {
        let (Some(me), Some(g)) = (self.key, self.groups.get(gid)) else {
            return;
        };
        let mut out = Vec::new();
        for id in newcomers {
            let Some(node) = g.members.iter().find(|m| m.key == *id).map(|m| m.node) else {
                continue;
            };
            // Our own key starts at counter 0 (it is new at each rotation and restart).
            let mut entries = vec![KeyEntry {
                owner: me,
                step: g.epoch,
                key_epoch: g.my_epoch,
                key: *g.my_key,
                next_counter: if from_start { 0 } else { g.my_counter },
            }];
            for m in &g.members {
                if m.key == me || (m.key == *id && !own) {
                    continue;
                }
                if let Some(c) = g.keys.get(&m.key).and_then(|mk| mk.current.as_ref()) {
                    entries.push(KeyEntry {
                        owner: m.key,
                        step: c.at,
                        key_epoch: c.epoch,
                        key: *c.key,
                        next_counter: if from_start { c.floor } else { c.expected() },
                    });
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
        let id = self.queue_group(gid, body, user, now)?;
        self.tick(now);
        Ok(id)
    }

    /// Signs, seals and queues the frames of a group message (§10.3), without ticking.
    fn queue_group(
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
        let (key, epoch, gh, step) = (g.my_key.clone(), g.my_epoch, g.gid_hash(), g.epoch);
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
                    left: None,
                });
                seq = seq.saturating_add(1);
                due += spacing + Millis::from(self.rng.next_u32()) % spacing.max(1);
            }
        }
        if user {
            let kept = recoverable(body).then_some(Kept {
                author: me,
                epoch,
                counter,
                step,
                payloads: own,
                at: now,
                at_unix: None,
                restored: false,
            });
            if let Some(f) = self
                .group_tx
                .iter_mut()
                .find(|f| f.id == id && f.ends_first_copy)
            {
                f.left = Some(Left {
                    gid: *gid,
                    epoch,
                    counter,
                    kept,
                });
            }
        }
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
            // An admin restored without the members' keys asks for them (§10.7).
            self.ask_missing_keys(&gid, now);
        }
        self.send_digests(now);
        while self.group_tx.front().is_some_and(|f| f.due <= now) && now >= self.next_slot {
            let Some(f) = self.group_tx.pop_front() else {
                break;
            };
            self.send_raw_packet(&f.raw, now);
            if f.user && f.ends_first_copy {
                self.events.push_back(Event::GroupSent { id: f.id });
            }
            if let Some(left) = f.left {
                let at_unix = self.unix_s(now);
                let jitter = Millis::from(self.rng.next_u32()) % (DIGEST_AFTER_MS / 2);
                if let Some(g) = self.groups.get_mut(&left.gid) {
                    // Our digest names it unless another message follows (§10.6, ADR-0040).
                    g.digest = Some((now + DIGEST_AFTER_MS + jitter, left.epoch, left.counter));
                    if let Some(mut k) = left.kept {
                        (k.at, k.at_unix) = (now, at_unix);
                        g.recovery.keep(k);
                    }
                }
            }
        }
        let unix = self.unix_s(now);
        let restored_at = |at_unix: Option<u64>| at_unix.and_then(|u| self.mono_ms(u));
        let mut resolved: Vec<(GroupId, usize, Millis)> = Vec::new();
        for g in self.groups.values() {
            for (i, k) in g.recovery.kept.iter().enumerate() {
                if k.restored {
                    resolved.push((g.gid, i, restored_at(k.at_unix).unwrap_or(now)));
                }
            }
        }
        for (gid, i, at) in resolved {
            if let Some(k) = self
                .groups
                .get_mut(&gid)
                .and_then(|g| g.recovery.kept.get_mut(i))
            {
                k.at = at;
                k.restored = false;
            }
        }
        for g in self.groups.values_mut() {
            g.partials.retain(|p| p.at + PARTIAL_MS > now);
            // An hour by this run's clock, or by the wall clock across a restart (ADR-0040).
            g.recovery.kept.retain(|k| {
                k.at + KEPT_MS > now
                    && unix
                        .zip(k.at_unix)
                        .is_none_or(|(u, a)| a + KEPT_MS / 1000 > u)
            });
            g.recovery.owed.retain(|o| o.until > now);
        }
        self.held.retain(|h| h.at + HELD_MS > now);
        self.ask_sync(now);
        self.ask_missing(now);
    }

    /// Our digests that are due (§10.6, ADR-0040): one per burst, in our own counter range.
    fn send_digests(&mut self, now: Millis) {
        let on = self.group_recovery && self.group_digest;
        let due: Vec<(GroupId, u8, u32)> = self
            .groups
            .values_mut()
            .filter_map(|g| {
                let (at, epoch, counter) = g.digest?;
                if at > now {
                    return None;
                }
                g.digest = None;
                (on && g.members.len() > 1).then_some((g.gid, epoch, counter))
            })
            .collect();
        for (gid, epoch, counter) in due {
            let _ = self.queue_group(&gid, &encode_digest(epoch, counter), false, now);
        }
    }

    pub(crate) fn next_group_wakeup(&self) -> Option<Millis> {
        let frames = self.group_tx.front().map(|f| f.due.max(self.next_slot));
        let gaps = self
            .groups
            .values()
            .flat_map(|g| g.recovery.gaps.iter().map(|x| x.due))
            .min();
        let digests = self
            .groups
            .values()
            .filter_map(|g| g.digest.map(|(at, _, _)| at))
            .min();
        [frames, gaps, digests, self.next_sync()]
            .into_iter()
            .flatten()
            .min()
    }

    /// `counters` of `author`'s messages under key `epoch` are missing (§10.6): asked for 20 s
    /// from now, plus up to as much again.
    fn add_gaps(
        &mut self,
        gid: &GroupId,
        author: &PubKey,
        epoch: u8,
        counters: &[u32],
        now: Millis,
    ) {
        if counters.is_empty() {
            return;
        }
        let jitter = Millis::from(self.rng.next_u32()) % ASK_AFTER_MS;
        let Some(g) = self.groups.get_mut(gid) else {
            return;
        };
        let r = &mut g.recovery;
        for &c in counters {
            if r.gaps.len() >= GAPS_MAX {
                break;
            }
            if !r
                .gaps
                .iter()
                .any(|x| x.author == *author && x.epoch == epoch && x.counter == c)
            {
                r.gaps.push(Gap {
                    author: *author,
                    epoch,
                    counter: c,
                    due: now + ASK_AFTER_MS + jitter,
                    tries: 0,
                    asked: None,
                });
            }
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
                    // The key may still be on its way (pairwise, several hops): keep the envelope,
                    // if its epoch can still come (none held yet, or newer than the one held).
                    let can_come = g
                        .keys
                        .get(&member)
                        .and_then(|mk| mk.current.as_ref())
                        .is_none_or(|c| newer(h.key_epoch, c.epoch));
                    if can_come && !self.held.iter().any(|x| x.payload == payload) {
                        let same = |x: &Held| x.gid_hash == h.gid_hash && x.sender == h.sender;
                        if self.held.iter().filter(|x| same(x)).count() >= HELD_PER_SENDER {
                            if let Some(i) = self.held.iter().position(same) {
                                self.held.remove(i);
                            }
                        } else if self.held.len() >= HELD_MAX {
                            self.held.pop_front();
                        }
                        self.held.push_back(Held {
                            payload: payload.to_vec(),
                            gid_hash: h.gid_hash,
                            sender: h.sender,
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
                    // At whichever step it was sent (§10.9).
                    let Some((hdr, plain, step)) = ks.open(&gid, payload) else {
                        continue;
                    };
                    let plain = plain.as_slice();
                    let message = if hdr.total == 1 {
                        Some((plain.to_vec(), vec![payload.to_vec()]))
                    } else {
                        // Every member holds this sender key: a fragment that opens may still
                        // not be the sender's. Fragments of each index are kept side by side,
                        // and the pair whose signature verifies is the message, so a fragment
                        // injected first does not shut out the sender's own (review of
                        // 2026-11-15, SR-17).
                        let this = |p: &Partial| {
                            p.member == member
                                && p.epoch == hdr.key_epoch
                                && p.counter == hdr.counter
                        };
                        let (epoch_b, counter_b) = ([hdr.key_epoch], hdr.counter.to_le_bytes());
                        let found = g
                            .partials
                            .iter()
                            .filter(|p| this(p) && p.index != hdr.index)
                            .find_map(|p| {
                                let (whole, payloads) = if p.index == 0 {
                                    (
                                        [p.data.as_slice(), plain].concat(),
                                        vec![p.payload.clone(), payload.to_vec()],
                                    )
                                } else {
                                    (
                                        [plain, p.data.as_slice()].concat(),
                                        vec![payload.to_vec(), p.payload.clone()],
                                    )
                                };
                                let (body, sig) = whole.split_last_chunk::<SIGNATURE_LEN>()?;
                                verify(
                                    &member,
                                    &signed_parts(&gid, &epoch_b, &counter_b, body),
                                    sig,
                                )
                                .then(|| (whole.clone(), payloads))
                            });
                        if found.is_some() {
                            g.partials.retain(|p| !this(p));
                        } else if !g
                            .partials
                            .iter()
                            .any(|p| this(p) && p.index == hdr.index && p.data.as_slice() == plain)
                            && g.partials.iter().filter(|p| this(p)).count() < PARTIAL_CANDIDATES
                        {
                            g.partials.push(Partial {
                                member,
                                epoch: hdr.key_epoch,
                                counter: hdr.counter,
                                index: hdr.index,
                                data: plain.to_vec(),
                                payload: payload.to_vec(),
                                at: now,
                            });
                        }
                        found
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
                        (payloads, hdr.key_epoch, hdr.counter, step, skipped),
                    ));
                    break 'gids;
                }
            }
        }
        let Some((gid, member, member_node, body, (payloads, epoch, counter, step, skipped))) =
            delivered
        else {
            return;
        };
        self.dirty_groups.insert(gid);
        // Gaps it fills or opens, and the message kept for members who miss it (§10.6).
        let mut answers: Vec<PubKey> = Vec::new();
        if let Some(g) = self.groups.get_mut(&gid) {
            g.recovery
                .gaps
                .retain(|x| !(x.author == member && x.epoch == epoch && x.counter == counter));
        }
        self.add_gaps(&gid, &member, epoch, &skipped, now);
        // A digest names the author's last message: what it lacks of them are gaps too
        // (§10.6, ADR-0040).
        if let Some((e, last)) = decode_digest(&body) {
            let lacking = self
                .groups
                .get(&gid)
                .and_then(|g| g.keys.get(&member))
                .and_then(|mk| {
                    [mk.current.as_ref(), mk.previous.as_ref()]
                        .into_iter()
                        .flatten()
                        .find(|k| k.epoch == e)
                })
                .map(|k| k.missing_through(last))
                .unwrap_or_default();
            self.add_gaps(&gid, &member, e, &lacking, now);
            return;
        }
        let at_unix = self.unix_s(now);
        if let Some(g) = self.groups.get_mut(&gid) {
            let r = &mut g.recovery;
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
                    step,
                    payloads: payloads.clone(),
                    at: now,
                    at_unix,
                    restored: false,
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
                plaintext: body,
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

    /// Review of 2026-11-15, SR-14: GROUP_LEAVE's epoch is checked. A leave from before the
    /// member was added again (one resent after a new handshake) does not remove it, nor does
    /// one naming a state that does not exist yet.
    #[test]
    fn a_stale_leave_does_not_remove_a_returning_member() {
        let mut core = Core::new(Config::new("t", [200; 32], [1; 32]));
        core.node = Some([0xEE; 32]);
        core.set_clock(1_793_000_000_000, 0);
        let bob = [1u8; 32];
        let bob_id = [101u8; 32];
        core.add_contact(bob, "bob", None);
        core.set_identity(&bob, bob_id, false);
        let gid = core.create_group("g", &[bob], 0).unwrap();
        let leave = |epoch: u32| {
            let mut v = vec![content::GROUP_LEAVE];
            v.extend_from_slice(&gid);
            v.extend_from_slice(&epoch.to_le_bytes());
            v
        };
        let held = core.groups[&gid].epoch;
        core.on_group_control(&bob, &leave(held + 5), 1_000);
        assert!(
            core.groups[&gid].is_member(&bob_id),
            "a state that does not exist"
        );
        core.on_group_control(&bob, &leave(held), 2_000);
        assert!(!core.groups[&gid].is_member(&bob_id), "left");
        core.add_members(&gid, &[bob], 3_000).unwrap();
        core.on_group_control(&bob, &leave(held), 4_000);
        assert!(
            core.groups[&gid].is_member(&bob_id),
            "from before Bob came back"
        );
        let now_held = core.groups[&gid].epoch;
        core.on_group_control(&bob, &leave(now_held), 5_000);
        assert!(!core.groups[&gid].is_member(&bob_id), "left again");
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
                    current: Some(KeyState::new(0, key, 1, floor)),
                    previous: None,
                    retired: VecDeque::new(),
                },
            );
        }
        let mut g = Group::new(
            State {
                gid: GID,
                epoch: 1,
                admin: me,
                name: "g".into(),
                members: list,
                cause: None,
            },
            Secret([0; 32]),
        );
        g.keys = keys;
        core.groups.insert(GID, g);
        core
    }

    /// A key entry at step 1, the epoch of [`core_with_group`]'s group.
    fn ke(owner: PubKey, key_epoch: u8, key: [u8; 32], next_counter: u32) -> KeyEntry {
        KeyEntry {
            owner,
            step: 1,
            key_epoch,
            key,
            next_counter,
        }
    }

    const GID: GroupId = [5; 16];

    /// ADR-0037: a backup keeps a group's state, members and our key epoch, not the others'
    /// sender keys; restored, our next key is drawn past the epoch we had.
    #[test]
    fn a_snapshot_without_keys() {
        let (a, b) = ([0xA1; 32], [0xB2; 32]);
        let mut core = core_with_group(&[(a, [0x11; 32], 0), (b, [0x22; 32], 0)]);
        core.groups.get_mut(&GID).unwrap().my_epoch = 7;
        let full = core.group_snapshot(&GID).unwrap();
        let bare = snapshot_without_keys(&full).unwrap();
        let has_key = |s: &[u8]| s.windows(32).any(|w| w == [0x11; 32] || w == [0x22; 32]);
        assert!(has_key(&full) && !has_key(&bare));
        let mut restored = Core::new(Config::new("t", [200; 32], [2; 32]));
        assert!(restored.restore_group(&bare));
        let g = restored.groups.get(&GID).unwrap();
        assert_eq!(g.members, core.groups.get(&GID).unwrap().members);
        assert_eq!((g.epoch, g.name.as_str(), g.my_epoch), (1, "g", 8));
        assert!(g.keys.is_empty() && g.announce_key);
        // What does not decode is refused, not cut anywhere.
        assert!(snapshot_without_keys(&full[..40]).is_none());
        assert!(snapshot_without_keys(&[9]).is_none());
        assert_eq!(snapshot_without_keys(&bare), Some(bare));
    }

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
        envelope_body(sender, key, counter, signer, &body)
    }

    /// The flooded payload of `body` (`content_type ‖ body`) from `sender`, sealed under `key`
    /// (at whatever step), key epoch 0, at `counter`, signed by `signer`.
    fn envelope_body(
        sender: &PubKey,
        key: &[u8; 32],
        counter: u32,
        signer: &[u8; 64],
        body: &[u8],
    ) -> Vec<u8> {
        let body = body.to_vec();
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

    /// Review of 2026-11-15, SR-20: envelopes wait for their key in a buffer an outsider could
    /// fill with garbage under one member's byte. A real envelope from another member,
    /// waiting for its key, stays and is read once the key comes.
    #[test]
    fn an_outsider_cannot_flush_held_envelopes() {
        let a_prv = expand_seed(&[0x41; 32]);
        let b_prv = expand_seed(&[0x42; 32]);
        let (a, b) = (public_key(&a_prv), public_key(&b_prv));
        assert_ne!(a[0], b[0]);
        let mut core = core_with_group(&[(a, [1; 32], 0), (b, [2; 32], 0)]);
        core.groups.get_mut(&GID).unwrap().keys.remove(&b);
        let real = envelope(&b, &[2; 32], 0, &b_prv, "the real one");
        core.on_group_envelope(&real, 0, 0, 1_000);
        // Garbage under a's byte, for a's next key epoch: it cannot be checked yet.
        for i in 0..20u32 {
            let header = GroupHeader {
                gid_hash: gid_hash(&GID),
                sender: a[0],
                key_epoch: 1,
                counter: i,
                index: 0,
                total: 1,
                copy: 0,
            };
            let mut out = [0u8; group::PAYLOAD_MAX];
            let n = group::seal(&[0x66; 32], &header, b"garbage", &mut out).unwrap();
            core.on_group_envelope(&out[..n], 0, 0, 2_000 + u64::from(i));
        }
        core.on_sender_key(GID, ke(b, 0, [2; 32], 0), 3_000);
        assert!(texts(&mut core).contains(&"the real one".to_owned()));
    }

    /// §10.7, review of 2026-11-15, SR-12: a member holding an envelope it has no key for,
    /// a minute on, asks the admin for the keys; once, not again within 10 minutes.
    #[test]
    fn a_member_without_a_key_asks_the_admin() {
        let admin = [0xA1; 32];
        let b_prv = expand_seed(&[0x62; 32]);
        let b = public_key(&b_prv);
        let mut core = core_with_group(&[(admin, [1; 32], 0), (b, [2; 32], 0)]);
        core.groups.get_mut(&GID).unwrap().admin = admin;
        core.groups.get_mut(&GID).unwrap().keys.remove(&b);
        let admin_node = [1; 32];
        let syncs = |core: &Core| {
            core.queued(&admin_node)
                .iter()
                .filter(|d| matches!(Control::decode(d), Some(Control::Sync { gid, epoch }) if gid == GID && epoch == 1))
                .count()
        };
        core.on_group_envelope(&envelope(&b, &[2; 32], 0, &b_prv, "hello"), 0, 0, 1_000);
        core.tick_groups(30_000);
        assert_eq!(syncs(&core), 0, "too early: the key may be on its way");
        core.tick_groups(62_000);
        assert_eq!(syncs(&core), 1);
        core.on_group_envelope(&envelope(&b, &[2; 32], 1, &b_prv, "again"), 0, 0, 100_000);
        core.tick_groups(200_000);
        assert_eq!(syncs(&core), 1, "once in 10 minutes");
    }

    /// §10.7: the admin answers a listed member once in 10 minutes, with the state when the
    /// member's is older and every key it holds; anyone else gets nothing.
    #[test]
    fn the_admin_answers_a_sync_once() {
        let (a, b) = ([0xA1; 32], [0xB1; 32]);
        let mut core = core_with_group(&[(a, [1; 32], 0), (b, [2; 32], 0)]);
        let me = *core.key().unwrap();
        let a_node = [1; 32];
        let sync = |epoch: u32| Control::Sync { gid: GID, epoch }.encode();
        core.on_group_control(&a_node, &sync(0), 1_000);
        let got = core.queued(&a_node);
        assert!(
            got.iter()
                .any(|d| matches!(Control::decode(d), Some(Control::State { epoch: 1, .. }))),
            "its state is older"
        );
        let mut keys = relayed_to(&core, &a_node);
        keys.sort_unstable();
        let mut want = vec![(b, 0), (me, 0)];
        want.sort_unstable();
        assert_eq!(keys, want);
        let n = core.queued(&a_node).len();
        core.on_group_control(&a_node, &sync(1), 60_000);
        assert_eq!(core.queued(&a_node).len(), n, "once in 10 minutes");
        // Someone not listed: nothing.
        let stranger = [0x77; 32];
        core.add_contact(stranger, "x", None);
        core.set_identity(&stranger, [0x78; 32], false);
        core.on_group_control(&stranger, &sync(0), 2_000);
        assert!(core.queued(&stranger).is_empty());
    }

    /// Review of 2026-11-15, SR-18: an admin walks a member's key epoch around the ring in
    /// relayed bundles and reinstalls the real key: it comes back with the replay window it
    /// had, so an old envelope is not delivered twice. Within one bundle, only the first entry
    /// per owner counts.
    #[test]
    fn a_relay_cannot_reinstall_a_key_with_a_fresh_window() {
        let x_prv = expand_seed(&[0x51; 32]);
        let x = public_key(&x_prv);
        let admin = [0xAD; 32];
        let mut core = core_with_group(&[(x, [3; 32], 0), (admin, [4; 32], 0)]);
        core.groups.get_mut(&GID).unwrap().admin = admin;
        let admin_node = [2; 32];
        let old = envelope(&x, &[3; 32], 5, &x_prv, "an old message");
        core.on_group_envelope(&old, 0, 0, 1_000);
        assert_eq!(texts(&mut core), vec!["an old message".to_owned()]);
        // One bundle walking the ring: only its first entry for x counts.
        let walk = encode_sender_keys(
            &GID,
            &[
                ke(x, 100, [9; 32], 0),
                ke(x, 200, [9; 32], 0),
                ke(x, 0, [3; 32], 0),
            ],
        );
        core.on_group_control(&admin_node, &walk, 2_000);
        assert_eq!(
            core.groups[&GID].keys[&x].current.as_ref().map(|k| k.epoch),
            Some(100)
        );
        // Three bundles: the real key back at epoch 0, with its window.
        for (i, (epoch, key)) in [(200u8, [9u8; 32]), (0, [3; 32])].into_iter().enumerate() {
            let b = encode_sender_keys(&GID, &[ke(x, epoch, key, 0)]);
            core.on_group_control(&admin_node, &b, 3_000 + i as u64);
        }
        assert_eq!(
            core.groups[&GID].keys[&x].current.as_ref().map(|k| k.epoch),
            Some(0)
        );
        core.on_group_envelope(&old, 0, 0, 4_000);
        assert!(texts(&mut core).is_empty(), "replayed");
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
            .flat_map(|(_, e)| e.into_iter().map(|e| (e.owner, e.key_epoch)))
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
        core.on_sender_key(GID, ke(a, 1, [9; 32], 0), 2_000);
        assert!(relayed_to(&core, &n).contains(&(a, 1)));
        assert!(
            relayed_to(&core, &a_node).is_empty(),
            "not back to its owner"
        );
        // …but not to a member admitted long ago (it sent its key there itself).
        assert!(relayed_to(&core, &[2; 32]).is_empty());

        // The newcomer's first key goes to every other member.
        core.on_sender_key(GID, ke(n_id, 0, [7; 32], 0), 3_000);
        assert!(relayed_to(&core, &a_node).contains(&(n_id, 0)));
        assert!(relayed_to(&core, &[2; 32]).contains(&(n_id, 0)));
    }

    /// §10.2: a SENDER_KEY from its owner is adopted, also at the epoch we hold when the key
    /// is another (its owner lost its state); the same key again changes nothing (review of
    /// 2026-11-15, SR-20).
    #[test]
    fn an_owners_key_at_the_same_epoch_is_adopted() {
        let a = [0xA1; 32];
        let mut core = core_with_group(&[(a, [1; 32], 0)]);
        core.on_sender_key(GID, ke(a, 0, [1; 32], 0), 1_000);
        assert_eq!(
            core.groups[&GID].keys[&a]
                .previous
                .as_ref()
                .map(|k| k.epoch),
            None
        );
        core.on_sender_key(GID, ke(a, 0, [2; 32], 0), 2_000);
        let mk = &core.groups[&GID].keys[&a];
        assert_eq!(mk.current.as_ref().map(|k| *k.key), Some([2; 32]));
    }

    /// §10.5, SR-20: a member restored from its phrase asks to join again. The admin sends it
    /// every key it holds, the member's own last one included, and forgets that one, so the
    /// member's next key goes to everyone as a first key.
    #[test]
    fn a_restored_member_gets_every_key_and_is_heard_again() {
        let (a, b) = ([0xA1; 32], [0xB1; 32]);
        let mut core = core_with_group(&[(a, [1; 32], 0), (b, [2; 32], 0)]);
        core.set_clock(1_793_000_000_000, 0);
        let me = *core.key().unwrap();
        let (a_node, b_node) = ([1; 32], [2; 32]);
        core.on_sender_key(GID, ke(b, 3, [3; 32], 0), 500);
        let invite = core.create_invite(&GID, 0).unwrap();
        let mut join = vec![content::GROUP_JOIN];
        join.extend_from_slice(&invite.token);
        core.on_group_control(&b_node, &join, 1_000);
        let mut got = relayed_to(&core, &b_node);
        got.sort_unstable();
        let mut want = vec![(a, 0), (b, 3), (me, 0)];
        want.sort_unstable();
        assert_eq!(got, want, "every key, its own last one included");
        assert!(
            core.groups[&GID]
                .keys
                .get(&b)
                .is_none_or(|k| k.current.is_none())
        );
        // Its new key, one epoch past the last, reaches the others through us.
        core.on_sender_key(GID, ke(b, 4, [4; 32], 0), 2_000);
        assert!(relayed_to(&core, &a_node).contains(&(b, 4)));
    }

    /// The restored member's side: its own identity in SENDER_KEYS moves its key epoch past
    /// that entry, so the others take its new key as newer (SR-20).
    #[test]
    fn our_own_entry_moves_our_epoch_past_it() {
        let a = [0xA1; 32];
        let mut core = core_with_group(&[(a, [1; 32], 0)]);
        let me = *core.key().unwrap();
        // `a` administers the group; we restored our phone.
        core.groups.get_mut(&GID).unwrap().admin = a;
        let bundle = encode_sender_keys(&GID, &[ke(me, 7, [7; 32], 12), ke(a, 0, [1; 32], 0)]);
        core.on_group_control(&[1; 32], &bundle, 1_000);
        let g = &core.groups[&GID];
        assert_eq!(g.my_epoch, 8);
        assert_ne!(g.my_key, [7; 32]);
        assert!(
            core.queued(&[1; 32])
                .iter()
                .any(|d| { decode_sender_key(d).is_some_and(|(_, e)| e.key_epoch == 8) })
        );
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
        let a = encode_sender_keys(&GID, &[ke([7; 32], 0, [1; 32], 0)]);
        let b = encode_sender_keys(
            &GID,
            &[ke([7; 32], 1, [2; 32], 0), ke([8; 32], 0, [3; 32], 0)],
        );
        let (_, keys) = decode_sender_keys(&merge_control(&a, &b).unwrap()).unwrap();
        assert_eq!(
            keys.iter()
                .map(|k| (k.owner[0], k.key_epoch))
                .collect::<Vec<_>>(),
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
                .map(|k| (k.epoch, *k.key))
        };
        // From a member who is not the admin: ignored.
        core.on_sender_keys(&owner, GID, vec![ke(admin, 5, [5; 32], 0)], 0);
        assert_eq!(
            core.groups[&GID].keys[&admin]
                .current
                .as_ref()
                .map(|k| k.epoch),
            Some(0)
        );
        // The same epoch, or an older one (wrapping): ignored.
        core.on_sender_keys(&admin, GID, vec![ke(owner, 0, [8; 32], 0)], 0);
        core.on_sender_keys(&admin, GID, vec![ke(owner, 200, [8; 32], 0)], 0);
        assert_eq!(epoch_of(&core), Some((0, [2; 32])));
        // A newer one: adopted.
        core.on_sender_keys(&admin, GID, vec![ke(owner, 1, [3; 32], 0)], 0);
        assert_eq!(epoch_of(&core), Some((1, [3; 32])));
        // From its owner, any other epoch is adopted.
        core.on_sender_key(GID, ke(owner, 0, [4; 32], 0), 0);
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

    /// An invitation, and a key that came with it, survive a restart until the user answers
    /// (the user's point of 2026-11-15: an invitation lost at a restart is not friendly).
    #[test]
    fn an_invitation_survives_a_restart() {
        let fresh = || {
            let mut c = Core::new(Config::new("t", [200; 32], [1; 32]));
            c.node = Some([0xEE; 32]);
            c.set_clock(1_793_000_000_000, 0);
            c
        };
        let mut core = fresh();
        let me = *core.key().unwrap();
        let (admin, other) = ([0x21u8; 32], [0x31u8; 32]);
        let gid = [9u8; 16];
        let state = State {
            gid,
            epoch: 1,
            admin,
            name: "Refuge".into(),
            members: vec![
                Member {
                    key: admin,
                    node: [1; 32],
                    name: "Camille".into(),
                },
                Member {
                    key: other,
                    node: [2; 32],
                    name: "Julien".into(),
                },
                Member {
                    key: me,
                    node: [0xEE; 32],
                    name: "me".into(),
                },
            ],
            cause: None,
        };
        core.on_group_state(&admin, state, 1_000);
        assert_eq!(core.invitations().len(), 1);
        // Julien's key, relayed by the admin, comes before the user answers.
        core.on_sender_keys(&admin, gid, vec![ke(other, 0, [7; 32], 0)], 2_000);
        let dirty = core.take_dirty_invitations();
        assert_eq!(dirty, vec![gid]);
        let snap = core.invitation_snapshot(&gid).unwrap();

        // The app restarts.
        let mut core = fresh();
        assert!(core.restore_invitation(&snap, 0));
        assert_eq!(core.invitations().len(), 1);
        assert_eq!(core.invitations()[0].name(), "Refuge");
        core.accept_group(&gid, 5_000).unwrap();
        let g = &core.groups[&gid];
        assert_eq!(g.members.len(), 3);
        assert!(g.keys.get(&other).is_some_and(|k| k.current.is_some()));
        // Answered: the stored invitation goes.
        assert!(core.take_dirty_invitations().contains(&gid));
        assert!(core.invitation_snapshot(&gid).is_none());
    }

    /// The two flooded payloads of a long text from `sender` at `counter`, signed by `signer`.
    fn two_fragments(
        sender: &PubKey,
        key: &[u8; 32],
        counter: u32,
        signer: &[u8; 64],
        text: &str,
    ) -> [Vec<u8>; 2] {
        let body = Content::Text {
            sent_at: 1,
            text: text.into(),
        }
        .encode();
        let (epoch_b, counter_b) = ([0u8], counter.to_le_bytes());
        let sig = sign(signer, &signed_parts(&GID, &epoch_b, &counter_b, &body)).unwrap();
        let mut plain = body;
        plain.extend_from_slice(&sig);
        let (a, b) = plain.split_at(group::FRAGMENT_PLAIN_MAX);
        [(0, a), (1, b)].map(|(index, part)| {
            let header = GroupHeader {
                gid_hash: gid_hash(&GID),
                sender: sender[0],
                key_epoch: 0,
                counter,
                index,
                total: 2,
                copy: 0,
            };
            let mut out = [0u8; group::PAYLOAD_MAX];
            let n = group::seal(key, &header, part, &mut out).unwrap();
            out[..n].to_vec()
        })
    }

    /// Review of 2026-11-15, SR-17: Carol holds Bob's sender key, as every member does. She
    /// floods a first fragment of her own under Bob's next counter before Bob's message: his
    /// two real fragments still make his message.
    #[test]
    fn an_injected_fragment_does_not_shut_out_the_real_one() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let mut core = core_with_group(&[(bob, [10; 32], 0)]);
        let long = "Refuge de la Pra: on part à 7 h par le sentier nord, rendez-vous au col \
                    vers 10 h, prévoir de l'eau.";
        let [first, second] = two_fragments(&bob, &[10; 32], 0, &bob_prv, long);
        // Carol's fragment: Bob's key, Bob's counter, her bytes.
        let forged = {
            let header = GroupHeader {
                gid_hash: gid_hash(&GID),
                sender: bob[0],
                key_epoch: 0,
                counter: 0,
                index: 0,
                total: 2,
                copy: 1,
            };
            let mut out = [0u8; group::PAYLOAD_MAX];
            let n = group::seal(
                &[10; 32],
                &header,
                &[0x55; group::FRAGMENT_PLAIN_MAX],
                &mut out,
            )
            .unwrap();
            out[..n].to_vec()
        };
        core.on_group_envelope(&forged, 0, 0, 0);
        core.on_group_envelope(&first, 0, 0, 1_000);
        core.on_group_envelope(&second, 0, 0, 2_000);
        assert_eq!(texts(&mut core), vec![long.to_owned()]);
    }

    /// Review of 2026-11-15, SR-16: search counts a direct copy as a reception of its
    /// member only if it verifies under the member's key. Its clear header can be re-emitted
    /// by anyone near a searcher, with any signal strength.
    #[test]
    fn a_copy_counts_only_if_it_verifies() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let core = core_with_group(&[(bob, [10; 32], 0)]);
        let genuine = envelope(&bob, &[10; 32], 0, &bob_prv, "here");
        assert!(core.copy_is_from(&bob, &genuine));
        let mut forged = genuine.clone();
        let last = forged.len() - 1;
        forged[last] ^= 1;
        assert!(!core.copy_is_from(&bob, &forged));
        let mut header_only = genuine[..group::HEADER_LEN].to_vec();
        header_only.extend_from_slice(&[0u8; 40]);
        assert!(!core.copy_is_from(&bob, &header_only));
    }

    /// Review of 2026-11-15, SR-14: a member asking again and again for the same message gets
    /// it once; a request is a tiny frame and its answer up to sixteen payloads.
    #[test]
    fn a_kept_message_goes_once_to_each_member() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let carol = [0x77; 32];
        let mut core = core_with_group(&[(bob, [10; 32], 0), (carol, [20; 32], 0)]);
        let e0 = envelope(&bob, &[10; 32], 0, &bob_prv, "at the col");
        core.on_group_envelope(&e0, 0, 0, 0);
        assert_eq!(texts(&mut core), vec!["at the col"]);
        let ask = encode_missing(&GID, &bob, 0, &[0]);
        let carol_node = [2u8; 32];
        let resends = |core: &Core| {
            core.queued(&carol_node)
                .iter()
                .filter(|d| d.first() == Some(&content::GROUP_RESEND))
                .count()
        };
        core.on_group_control(&carol_node, &ask, 10_000);
        assert_eq!(resends(&core), 1);
        for t in 1..20 {
            core.on_group_control(&carol_node, &ask, 10_000 + t * 1_000);
        }
        assert_eq!(resends(&core), 1);
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
        core.send_bundle(&GID, &[carol], false, false);
        let relayed: Vec<_> = core
            .queued(&[2; 32])
            .iter()
            .filter_map(|d| decode_sender_keys(d))
            .flat_map(|(_, entries)| entries)
            .filter(|e| e.owner == bob)
            .map(|e| e.next_counter)
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
        // Dave was added by state 2, after it was sent at step 1; Carol was there.
        core.groups
            .get_mut(&GID)
            .unwrap()
            .recovery
            .since
            .insert(dave, 2);
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
                current: Some(KeyState::new(0, [50; 32], 1, 0)),
                previous: None,
                retired: VecDeque::new(),
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

    /// `key`, at step 1 (the epoch of [`core_with_group`]'s group), at step `to` (§10.9).
    fn stepped(key: &[u8; 32], to: u32) -> [u8; 32] {
        step_key_to(&GID, key, 1, to).unwrap()
    }

    /// §10.9, ADR-0039: the keys a newcomer gets open nothing sent before the state that added
    /// it, though they open what follows. Before the key step, the admin handed over the very
    /// key Bob's earlier message was sealed with.
    #[test]
    fn a_newcomer_cannot_open_what_came_before() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let mut core = core_with_group(&[(bob, [10; 32], 0)]);
        let me = *core.key().unwrap();
        let before = envelope(&bob, &[10; 32], 0, &bob_prv, "before Dave");
        core.on_group_envelope(&before, 0, 0, 1_000);
        assert_eq!(texts(&mut core), vec!["before Dave"]);
        let dave_node = [4; 32];
        member(&mut core, 10, dave_node);
        core.add_members(&GID, &[dave_node], 2_000).unwrap();
        let bundle: Vec<KeyEntry> = core
            .queued(&dave_node)
            .iter()
            .filter_map(|d| decode_sender_keys(d))
            .flat_map(|(_, e)| e)
            .collect();
        let mut owners: Vec<PubKey> = bundle.iter().map(|e| e.owner).collect();
        owners.sort_unstable();
        let mut want = vec![bob, me];
        want.sort_unstable();
        assert_eq!(owners, want);
        for e in &bundle {
            assert_eq!(e.step, 2, "keys at the step of the state that added Dave");
            let k = KeyState::new(e.key_epoch, e.key, e.step, 0);
            assert!(
                k.open(&GID, &before).is_none(),
                "Dave opens what came before"
            );
        }
        // Bob took the same state: what he writes now, we and Dave read.
        let after = envelope(&bob, &stepped(&[10; 32], 2), 1, &bob_prv, "after Dave");
        core.on_group_envelope(&after, 0, 0, 3_000);
        assert_eq!(texts(&mut core), vec!["after Dave"]);
        let b = bundle.iter().find(|e| e.owner == bob).unwrap();
        assert!(
            KeyState::new(0, b.key, b.step, 0)
                .open(&GID, &after)
                .is_some()
        );
        assert!(!core.group_opens(&GID, &envelope(&bob, &[11; 32], 2, &bob_prv, "x")));
    }

    /// §10.9: members take a state at different times. Bob takes state 2 before us; a frame
    /// he sent before it arrives after we took it: we read both, and what follows.
    #[test]
    fn members_read_across_a_step() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let admin = [0xAD; 32];
        let mut core = core_with_group(&[(bob, [10; 32], 0), (admin, [4; 32], 0)]);
        core.groups.get_mut(&GID).unwrap().admin = admin;
        let late = envelope(&bob, &[10; 32], 0, &bob_prv, "sent at step 1");
        let early = envelope(
            &bob,
            &stepped(&[10; 32], 2),
            1,
            &bob_prv,
            "Bob took state 2",
        );
        core.on_group_envelope(&early, 0, 0, 1_000);
        assert_eq!(texts(&mut core), vec!["Bob took state 2"]);
        let mut state = core.groups[&GID].state(None);
        state.epoch = 2;
        state.members.push(Member {
            key: [0x4D; 32],
            node: [4; 32],
            name: "dave".into(),
        });
        core.on_group_state(&admin, state, 2_000);
        assert_eq!(core.groups[&GID].epoch, 2);
        core.on_group_envelope(&late, 0, 0, 3_000);
        let next = envelope(&bob, &stepped(&[10; 32], 2), 2, &bob_prv, "at step 2");
        core.on_group_envelope(&next, 0, 0, 4_000);
        let ahead = envelope(&bob, &stepped(&[10; 32], 5), 3, &bob_prv, "at step 5");
        core.on_group_envelope(&ahead, 0, 0, 5_000);
        assert_eq!(
            texts(&mut core),
            vec!["sent at step 1", "at step 2", "at step 5"]
        );
        // Our own key stepped with the state: Bob, at step 2, reads us.
        let g = &core.groups[&GID];
        assert_eq!(g.my_key, stepped(&[0; 32], 2));
    }

    /// §10.6, ADR-0040: Bob's last post is lost; his digest, two minutes on, names it, and we
    /// ask for it. A digest is not a message; one Bob did not sign names nothing.
    #[test]
    fn a_digest_reveals_a_lost_last_post() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let (mallory_prv, _) = member(&mut probe, 8, [2; 32]);
        let fresh = || {
            let mut core = core_with_group(&[(bob, [10; 32], 0)]);
            member(&mut core, 7, [1; 32]);
            let e0 = envelope(&bob, &[10; 32], 0, &bob_prv, "on the ridge");
            core.on_group_envelope(&e0, 0, 0, 0);
            assert_eq!(texts(&mut core), vec!["on the ridge"]);
            core
        };
        let asked = |core: &Core| -> Vec<_> {
            core.queued(&[1; 32])
                .iter()
                .filter_map(|d| decode_missing(d))
                .collect()
        };
        let mut core = fresh();
        let digest = envelope_body(&bob, &[10; 32], AUX, &bob_prv, &encode_digest(0, 1));
        core.on_group_envelope(&digest, 0, 0, 120_000);
        assert!(texts(&mut core).is_empty(), "a digest is not a message");
        core.ask_missing(120_000 + 41_000);
        assert_eq!(asked(&core), vec![(GID, bob, 0, vec![1])]);

        // Signed by someone else: nothing.
        let mut core = fresh();
        let forged = envelope_body(&bob, &[10; 32], AUX, &mallory_prv, &encode_digest(0, 9));
        core.on_group_envelope(&forged, 0, 0, 120_000);
        core.ask_missing(120_000 + 41_000);
        assert!(asked(&core).is_empty());
        // Naming what we hold: nothing.
        let mut core = fresh();
        let ok = envelope_body(&bob, &[10; 32], AUX, &bob_prv, &encode_digest(0, 0));
        core.on_group_envelope(&ok, 0, 0, 120_000);
        core.ask_missing(120_000 + 41_000);
        assert!(asked(&core).is_empty());
    }

    /// ADR-0040: our digest goes two to three minutes after our last message left, once per burst,
    /// from our own counter range; a message meanwhile pushes it back. A message cancelled
    /// before it left is neither named nor kept (§6.1).
    #[test]
    fn our_digest_goes_once_after_a_burst() {
        let a = [0xA1; 32];
        let mut core = core_with_group(&[(a, [1; 32], 0)]);
        let run = |core: &mut Core, from: u64, to: u64| {
            for t in (from..=to).step_by(1_000) {
                core.tick_groups(t);
            }
            core.groups[&GID].my_aux - AUX
        };
        core.send_group_text(&GID, "one", 1, 0).unwrap();
        assert_eq!(run(&mut core, 1_000, 59_000), 0);
        core.send_group_text(&GID, "two", 1, 60_000).unwrap();
        assert_eq!(
            run(&mut core, 61_000, 179_000),
            0,
            "not two minutes after the last one"
        );
        assert_eq!(run(&mut core, 180_000, 260_000), 1, "one digest");
        assert_eq!(run(&mut core, 201_000, 400_000), 1, "once");
        let kept = |core: &Core| core.groups[&GID].recovery.kept.len();
        assert_eq!(kept(&core), 2);
        // Cancelled before it left (the radio busy meanwhile): no digest names it, nobody
        // gets it from us.
        core.next_slot = 510_000;
        let id = core
            .send_group_text(&GID, "never mind", 1, 500_000)
            .unwrap();
        assert_eq!(core.cancel(id), Ok(()));
        assert_eq!(run(&mut core, 501_000, 800_000), 1);
        assert_eq!(kept(&core), 2);
    }

    /// The admin of this module's tests' group, `core_with_group`'s, signs a handover link
    /// with `signer` naming `admin` at `epoch`.
    fn link(core: &Core, signer: &[u8; 64], epoch: u32, admin: PubKey, name: &str) -> Vec<u8> {
        let mut state = core.groups[&GID].state(None);
        state.epoch = epoch;
        state.admin = admin;
        state.name = name.into();
        let bytes = state.encode();
        let sig = sign(signer, &handover_parts(&handover_digest(&bytes))).unwrap();
        [sig.to_vec(), bytes].concat()
    }

    /// §10.8, ADR-0038: the admin names a successor in a state it signs, sent to every other
    /// member; it stays a member and no longer runs the group. A member that missed the
    /// handover and asks it for the keys gets the handover instead.
    #[test]
    fn the_admin_hands_over_in_a_signed_state() {
        let (a, b) = ([0xA1; 32], [0xB1; 32]);
        let mut core = core_with_group(&[(a, [1; 32], 0), (b, [2; 32], 0)]);
        core.set_clock(1_793_000_000_000, 0);
        let me = *core.key().unwrap();
        assert_eq!(
            core.hand_over(&GID, &[0xEE; 32], false, 0),
            Err(Error::NotAllowed),
            "not to ourselves"
        );
        let invite = core.create_invite(&GID, 0).unwrap();
        core.hand_over(&GID, &[1; 32], false, 1_000).unwrap();
        let g = &core.groups[&GID];
        assert_eq!((g.admin, g.epoch), (a, 2));
        assert!(g.is_member(&me));
        let handovers = |core: &Core, node: &PubKey| -> Vec<Vec<Link>> {
            core.queued(node)
                .iter()
                .filter_map(|d| decode_handover(d))
                .collect()
        };
        for node in [[1; 32], [2; 32]] {
            let got = handovers(&core, &node);
            assert_eq!(got.len(), 1, "one handover each");
            let (sig, state, bytes) = &got[0][0];
            let digest = handover_digest(&bytes[SIGNATURE_LEN..]);
            assert!(verify(&me, &handover_parts(&digest), sig));
            assert_eq!((state.admin, state.epoch), (a, 2));
        }
        assert!(
            std::iter::from_fn(|| core.poll_event()).any(|e| matches!(
                e,
                Event::GroupChanged { notice: Notice { kind: notice::HANDED_OVER, ref subjects, .. }, .. }
                    if subjects.first().map(|s| s.0) == Some([1; 32])
            ))
        );
        // Not the admin any more: no renaming, and the invitation admits no one.
        assert_eq!(core.rename_group(&GID, "x", 2_000), Err(Error::NotAllowed));
        assert!(!core.invites.contains_key(&invite.token));
        // `b` missed it and asks us, its admin as it knows: it gets the handover.
        let sync = Control::Sync { gid: GID, epoch: 1 }.encode();
        core.on_group_control(&[2; 32], &sync, 3_000);
        assert_eq!(handovers(&core, &[2; 32]).len(), 2);
        core.on_group_control(&[2; 32], &sync, 4_000);
        assert_eq!(handovers(&core, &[2; 32]).len(), 2, "once in 10 minutes");
    }

    /// §10.8: a member takes another admin only from a link the admin it holds signed,
    /// whoever delivers it; a state the new admin sent before its handover came is kept aside
    /// and taken after it; a replay changes nothing.
    #[test]
    fn a_member_takes_a_new_admin_only_from_its_admin() {
        let camille_prv = expand_seed(&[0x21; 32]);
        let julien_prv = expand_seed(&[0x31; 32]);
        let (camille, julien) = (public_key(&camille_prv), public_key(&julien_prv));
        let mut core = core_with_group(&[(camille, [1; 32], 0), (julien, [2; 32], 0)]);
        core.groups.get_mut(&GID).unwrap().admin = camille;
        let (camille_node, julien_node) = ([1u8; 32], [2u8; 32]);
        let forged = link(&core, &julien_prv, 2, julien, "g");
        let real = link(&core, &camille_prv, 2, julien, "g");
        core.on_group_control(&julien_node, &encode_handover(&[&forged]), 1_000);
        assert_eq!(core.groups[&GID].admin, camille, "Julien named himself");
        // Julien's own state, before Camille's handover reached us: kept, Julien is asked.
        let mut s3 = core.groups[&GID].state(None);
        (s3.epoch, s3.admin, s3.name) = (3, julien, "Bivouac".into());
        core.on_group_state(&julien, s3, 1_500);
        assert_eq!(core.groups[&GID].admin, camille);
        assert!(
            core.queued(&julien_node)
                .iter()
                .any(|d| matches!(Control::decode(d), Some(Control::Sync { epoch: 1, .. })))
        );
        while core.poll_event().is_some() {}
        core.on_group_control(&julien_node, &encode_handover(&[&real]), 2_000);
        let g = &core.groups[&GID];
        assert_eq!((g.admin, g.epoch, g.name.as_str()), (julien, 3, "Bivouac"));
        let kinds: Vec<(u8, PubKey)> = std::iter::from_fn(|| core.poll_event())
            .filter_map(|e| match e {
                Event::GroupChanged { notice: n, .. } => Some((n.kind, n.actor)),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                (notice::HANDED_OVER, camille_node),
                (notice::RENAMED, julien_node)
            ]
        );
        core.on_group_control(&camille_node, &encode_handover(&[&real]), 3_000);
        assert_eq!(core.groups[&GID].epoch, 3, "a replay changes nothing");
    }

    /// §10.8: a member that missed two handovers takes both from one GROUP_HANDOVER, in order.
    #[test]
    fn a_chain_of_handovers_is_taken_in_order() {
        let camille_prv = expand_seed(&[0x21; 32]);
        let julien_prv = expand_seed(&[0x31; 32]);
        let (camille, julien) = (public_key(&camille_prv), public_key(&julien_prv));
        let mut core = core_with_group(&[(camille, [1; 32], 0), (julien, [2; 32], 0)]);
        core.groups.get_mut(&GID).unwrap().admin = camille;
        let first = link(&core, &camille_prv, 2, julien, "g");
        let second = link(&core, &julien_prv, 4, camille, "Back to Camille");
        core.on_group_control(&[2; 32], &encode_handover(&[&second, &first]), 1_000);
        assert_eq!(core.groups[&GID].admin, camille, "out of order: nothing");
        core.on_group_control(&[2; 32], &encode_handover(&[&first, &second]), 2_000);
        let g = &core.groups[&GID];
        assert_eq!((g.admin, g.epoch), (camille, 4));
        assert_eq!(g.handovers.len(), 2);
    }

    /// ADR-0040: the messages kept for recovery are stored with the group: after a restart
    /// they still go to a member that asks, until their hour is over by the wall clock.
    #[test]
    fn recovery_copies_survive_a_restart() {
        let mut probe = Core::new(Config::new("t", [200; 32], [1; 32]));
        let (bob_prv, bob) = member(&mut probe, 7, [1; 32]);
        let (_, carol) = member(&mut probe, 8, [2; 32]);
        let mut core = core_with_group(&[(bob, [10; 32], 0), (carol, [20; 32], 0)]);
        member(&mut core, 7, [1; 32]);
        member(&mut core, 8, [2; 32]);
        let t0 = 1_793_000_000_000;
        core.set_clock(t0, 0);
        let e0 = envelope(&bob, &[10; 32], 0, &bob_prv, "kept");
        core.on_group_envelope(&e0, 0, 0, 1_000);
        let snap = core.group_snapshot(&GID).unwrap();

        let restart = |at_ms: u64| {
            let mut c = Core::new(Config::new("t", [200; 32], [1; 32]));
            c.node = Some([0xEE; 32]);
            c.set_clock(at_ms, 0);
            assert!(c.restore_group(&snap));
            c.tick_groups(1);
            c
        };
        let resent = |c: &Core| -> Vec<_> {
            c.queued(&[2; 32])
                .iter()
                .filter_map(|d| decode_resend(d))
                .collect()
        };
        let ask = encode_missing(&GID, &bob, 0, &[0]);
        let mut again = restart(t0 + 600_000);
        again.on_group_control(&[2; 32], &ask, 2);
        assert_eq!(resent(&again), vec![(GID, vec![e0.clone()])]);
        assert_eq!(
            again.groups[&GID].keys[&bob].current.as_ref().map(|k| k.at),
            Some(1)
        );
        // Its own snapshot restores too.
        let snap2 = again.group_snapshot(&GID).unwrap();
        assert!(Core::new(Config::new("t", [200; 32], [1; 32])).restore_group(&snap2));
        // An hour later by the wall clock: gone.
        let mut late = restart(t0 + 3_700_000);
        late.on_group_control(&[2; 32], &ask, 2);
        assert!(resent(&late).is_empty());
    }

    /// Review of 2026-11-17: an envelope's clear header (group hash, member byte, key epoch)
    /// is anyone's to forge. One under a member whose key we hold, claiming its next epoch,
    /// makes nobody ask the admin (§10.7).
    #[test]
    fn a_forged_newer_epoch_makes_nobody_ask() {
        let admin = [0xA1; 32];
        let b_prv = expand_seed(&[0x62; 32]);
        let b = public_key(&b_prv);
        let mut core = core_with_group(&[(admin, [1; 32], 0), (b, [2; 32], 0)]);
        core.groups.get_mut(&GID).unwrap().admin = admin;
        let header = GroupHeader {
            gid_hash: gid_hash(&GID),
            sender: b[0],
            key_epoch: 1,
            counter: 0,
            index: 0,
            total: 1,
            copy: 0,
        };
        let mut out = [0u8; group::PAYLOAD_MAX];
        let n = group::seal(&[0x66; 32], &header, b"garbage", &mut out).unwrap();
        core.on_group_envelope(&out[..n], 0, 0, 1_000);
        core.tick_groups(62_000);
        assert!(
            core.queued(&[1; 32])
                .iter()
                .all(|d| !matches!(Control::decode(d), Some(Control::Sync { .. })))
        );
    }

    /// §10.7: the admin answers at most 8 GROUP_SYNC per group in 10 minutes.
    #[test]
    fn the_admin_answers_a_few_syncs_at_most() {
        let members: Vec<(PubKey, [u8; 32], u32)> =
            (0..10u8).map(|i| ([0x10 + i; 32], [i; 32], 0)).collect();
        let mut core = core_with_group(&members);
        for i in 0..10u8 {
            let sync = Control::Sync { gid: GID, epoch: 1 }.encode();
            core.on_group_control(&[i + 1; 32], &sync, 1_000 + u64::from(i));
        }
        let answered = (0..10u8)
            .filter(|i| !relayed_to(&core, &[i + 1; 32]).is_empty())
            .count();
        assert_eq!(answered, SYNC_ANSWERS_MAX);
    }

    /// §10.7, ADR-0038: an admin restored without the members' keys asks each member for its
    /// key, once in 10 minutes; a key it asked for is not relayed to the others, who hold it.
    #[test]
    fn a_restored_admin_asks_each_member_for_its_key() {
        let (a, b) = ([0xA1; 32], [0xB1; 32]);
        let mut core = core_with_group(&[(a, [1; 32], 0), (b, [2; 32], 0)]);
        core.drop_group_keys(&GID);
        let snap = core.group_snapshot(&GID).unwrap();
        let mut admin = Core::new(Config::new("t", [200; 32], [1; 32]));
        admin.node = Some([0xEE; 32]);
        assert!(admin.restore_group(&snap));
        let asks = |c: &Core, node: &PubKey| {
            c.queued(node)
                .iter()
                .filter(|d| {
                    matches!(Control::decode(d), Some(Control::KeyRequest { gid, epoch: 1 }) if gid == GID)
                })
                .count()
        };
        admin.tick_groups(1_000);
        assert_eq!((asks(&admin, &[1; 32]), asks(&admin, &[2; 32])), (1, 1));
        admin.tick_groups(2_000);
        assert_eq!((asks(&admin, &[1; 32]), asks(&admin, &[2; 32])), (1, 1));
        admin.on_sender_key(GID, ke(a, 0, [1; 32], 5), 3_000);
        assert!(admin.groups[&GID].keys[&a].current.is_some());
        assert!(relayed_to(&admin, &[2; 32]).is_empty(), "b holds it");
    }

    /// §10.7: a member answers only its admin's GROUP_KEY_REQUEST, with its key to the admin
    /// alone, once in 10 minutes.
    #[test]
    fn a_member_answers_its_admin_alone() {
        let admin = [0xAD; 32];
        let mut core = core_with_group(&[(admin, [4; 32], 0), ([0xB1; 32], [2; 32], 0)]);
        core.groups.get_mut(&GID).unwrap().admin = admin;
        let ask = Control::KeyRequest { gid: GID, epoch: 1 }.encode();
        let keys = |c: &Core, node: &PubKey| {
            c.queued(node)
                .iter()
                .filter(|d| decode_sender_key(d).is_some())
                .count()
        };
        core.on_group_control(&[2; 32], &ask, 1_000);
        assert_eq!(keys(&core, &[2; 32]), 0, "not the admin");
        core.on_group_control(&[1; 32], &ask, 2_000);
        assert_eq!((keys(&core, &[1; 32]), keys(&core, &[2; 32])), (1, 0));
        core.on_group_control(&[1; 32], &ask, 3_000);
        assert_eq!(keys(&core, &[1; 32]), 1, "once in 10 minutes");
    }
}

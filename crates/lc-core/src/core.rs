//! The application core: drives one MeshCore companion through its protocol, keeps contacts,
//! paths, handshakes and sessions, and turns app requests into paced `SEND_RAW_DATA` frames.
//!
//! IO-free: the runner passes companion frames in ([`Core::on_companion_frame`]), takes command
//! frames out ([`Core::poll_companion`]), calls [`Core::tick`] at [`Core::next_wakeup`], and
//! reads [`Event`]s.

use std::collections::{HashMap, HashSet, VecDeque};

use lc_meshcore::air::{Packet, PayloadType, Route, transport_code};
use lc_meshcore::airtime::LoRaParams;
use lc_meshcore::codes::APP_VER;
use lc_meshcore::command::{Command, raw_payload_max};
use lc_meshcore::ecdh::{expand_seed, public_key, shared_secret};
use lc_meshcore::path::{PATH_UNKNOWN, Path};
use lc_meshcore::response::Response;
use lc_proto::content as datagram;
use lc_proto::discovery::{self, Keying, ProbeId};
use lc_proto::handshake::{self, Ends, Hello, HelloAck, SALT_LEN};
use lc_proto::header::ACK_FRAME_MAX;
use lc_proto::intro::{self, Answer, Intro, IntroAck};
use lc_proto::kdf::x25519_public;
use lc_proto::sender::{SendError, Timing};
use lc_proto::{
    DATAGRAM_MAX, FRAME_MAX, FrameError, Header, Kind, Millis, Received, Session, SessionEvent,
    chunk_for_path,
};
use rand_chacha::ChaCha20Rng;
use rand_core::{Rng, SeedableRng};
use sha2::{Digest, Sha256};

use crate::budget::{Budget, EU868};
use crate::content::Content;
use crate::group::{Group, GroupId, Held, PendingKey};
use crate::location::Locations;
use lc_proto::location::Position;

pub type PubKey = [u8; 32];
pub type MsgId = u64;

/// How long to wait for the companion's answer to a command.
const COMMAND_TIMEOUT_MS: Millis = 5_000;
/// HELLO tries before giving up on a peer.
const HELLO_TRIES: u8 = 8;
/// Path discovery probes before reporting `NoPath` (§4.5).
const PROBE_TRIES: u8 = 3;
/// Wait for a PATH_REPLY: uniform in `[PROBE_WAIT_MS, 2 × PROBE_WAIT_MS)`.
const PROBE_WAIT_MS: Millis = 20_000;
/// How long a received datagram is remembered to drop identical resends (§6).
const DUPLICATE_MEMORY_MS: Millis = 3_600_000;
/// A HELLO we cannot open triggers our own introduction at most this often (§4.6).
const INTRO_POKE_MS: Millis = 600_000;
/// A HELLO more than this far from our clock is not answered (§4.4, SR-03).
const HELLO_FRESH_S: u64 = 600;
/// Answers to frames anyone can send (INTRO_ACK, PATH_REPLY, HELLO_ACK, the INTRO that a HELLO
/// we cannot open prompts): at most this many per node in ten minutes, and this many in all
/// in an hour (§9, review of 2026-11-15, SR-04). Data waits for the budget; these did not,
/// and a stranger could make the phone spend its hour answering.
const UNPROMPTED_PER_NODE: usize = 8;
const UNPROMPTED_PER_NODE_MS: Millis = 600_000;
const UNPROMPTED_TOTAL: usize = 60;
const UNPROMPTED_TOTAL_MS: Millis = 3_600_000;
/// INTRO_FIN resends before giving up (the session stays).
const FIN_TRIES: u8 = 8;
/// An answered INTRO is remembered this long (§4.6).
const INTRO_MEMORY_MS: Millis = 600_000;
/// Rounds without any ACK, for a datagram the peer never acknowledged, after which the session
/// is presumed lost (ADR-0006).
const SILENT_ROUNDS_BEFORE_REHANDSHAKE: u8 = 3;

/// A path owned by the core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedPath {
    pub len_byte: u8,
    pub bytes: Vec<u8>,
}

impl OwnedPath {
    pub fn from_path(p: Path<'_>) -> Self {
        Self {
            len_byte: p.len_byte(),
            bytes: p.bytes().to_vec(),
        }
    }

    pub fn path(&self) -> Option<Path<'_>> {
        Path::new(self.len_byte, &self.bytes).ok()
    }

    pub const fn hops(&self) -> u8 {
        self.len_byte & 0x3F
    }

    /// The same path walked backwards.
    pub fn reversed(&self) -> Self {
        let size = usize::from(self.len_byte >> 6) + 1;
        let mut bytes = Vec::with_capacity(self.bytes.len());
        for hash in self.bytes.chunks(size).rev() {
            bytes.extend_from_slice(hash);
        }
        Self {
            len_byte: self.len_byte,
            bytes,
        }
    }
}

/// A contact: a MeshCore node (where to send) and, once known, the identity behind it (who,
/// ADR-0013).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contact {
    /// The node key (MeshCore routing).
    pub key: PubKey,
    pub name: String,
    pub out_path: Option<OwnedPath>,
    /// The phone identity, learned by the introduction (§4.6), a group state or a contact card.
    pub identity: Option<PubKey>,
    /// Compared in person (QR code or safety number, §4.7).
    pub verified: bool,
    /// MeshCore's node type ([`contact_kind`]): only people's companions can talk lichen.
    pub kind: u8,
}

/// MeshCore node types, as companions report their contacts.
pub mod contact_kind {
    pub const UNKNOWN: u8 = 0;
    pub const CHAT: u8 = 1;
    pub const REPEATER: u8 = 2;
    pub const ROOM: u8 = 3;
    pub const SENSOR: u8 = 4;
}

impl Contact {
    /// A person's companion (or a node of unknown type): someone lichen can talk to.
    /// Repeaters, room servers and sensors cannot.
    pub const fn is_person(&self) -> bool {
        matches!(self.kind, contact_kind::UNKNOWN | contact_kind::CHAT)
    }
}

/// Why a message was not delivered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailReason {
    /// No path could be found.
    NoPath,
    /// The handshake got no answer.
    Unreachable,
    /// The transfer stalled.
    Timeout,
    /// The app stopped before the message was delivered.
    Interrupted,
    /// The peer refused the datagram.
    Refused,
    /// The datagram does not fit in 64 fragments on this path.
    TooLarge,
    /// The contact, verified, now presents another identity: nothing goes out until the user
    /// accepts or rejects it (§4.6).
    IdentityChanged,
    /// Its expiry passed before it was delivered (§6.1).
    Expired,
}

/// Where a message the app sent stands while it is unfinished (§6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The companion is disconnected: nothing goes out until it is back.
    Offline,
    /// Behind earlier messages to the same peer (or earlier group frames).
    Behind,
    /// Looking for a path to the peer (§4.5).
    FindingPath,
    /// Opening a session: handshake or introduction (§4.4, §4.6).
    Connecting,
    /// Held by the airtime budget until then (§9).
    Budget { until: Millis },
    /// Being transmitted, or waiting for its acknowledgement.
    Transmitting,
}

/// An unfinished message: its stage, and whether a frame of it already went out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outbound {
    pub stage: Stage,
    /// A frame went out: it may have arrived, and it can no longer be cancelled.
    pub started: bool,
}

/// What the app should know about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The companion answered `APP_START`.
    Connected {
        key: PubKey,
        name: String,
        freq_khz: u32,
        radio: LoRaParams,
    },
    ContactsUpdated,
    /// We learned the identity behind contact `node` (§4.6); `changed`: it replaced another
    /// one, so the contact is to verify again.
    IdentityLearned {
        node: PubKey,
        identity: PubKey,
        changed: bool,
    },
    /// Contact `node`, verified, presented another identity: it is **not** adopted; the user
    /// accepts it ([`Core::accept_identity`]) or rejects it (§4.6).
    IdentityConflict {
        node: PubKey,
        identity: PubKey,
    },
    SessionEstablished {
        peer: PubKey,
    },
    Received {
        from: PubKey,
        content: Content,
        snr4: i8,
        rssi: i8,
    },
    /// A fragment of a message went out (for progress bars).
    Progress {
        id: MsgId,
        sent: u32,
        total: u8,
    },
    Delivered {
        id: MsgId,
    },
    Failed {
        id: MsgId,
        reason: FailReason,
    },
    /// The first copy of every fragment of group message `id` has left (§10.3, §6.1), or
    /// the companion took channel post `id` (§15.3).
    GroupSent {
        id: MsgId,
    },
    /// A plain MeshCore message from someone with a stock client (§15.1).
    PlainReceived(crate::plain::PlainMessage),
    /// The companion's channel list changed, or was read (§15.3).
    ChannelsUpdated,
    /// Node `node`'s phone speaks a newer protocol version than ours (ADR-0029): some of what
    /// it sends may show as unsupported until lichen is updated. Once per node and run.
    NewerVersion {
        node: PubKey,
        version: u8,
    },
    /// A group that lists us waits for the user to join it or not (§10.1, SR-13):
    /// [`Core::accept_group`], [`Core::decline_group`].
    GroupInvited {
        gid: GroupId,
        name: String,
        admin: PubKey,
        members: usize,
    },
    /// We joined a group: invited and accepted, or admitted with a scanned invitation.
    GroupJoined {
        gid: GroupId,
        name: String,
        admin: PubKey,
    },
    /// A group's name or members changed (or we created it).
    GroupUpdated {
        gid: GroupId,
    },
    /// We were removed from a group.
    GroupLeft {
        gid: GroupId,
    },
    /// Node `node` joined group `gid`, which we administer, with an invitation (§10.5).
    MemberJoined {
        gid: GroupId,
        node: PubKey,
    },
    /// A membership change to show in the group's conversation (ADR-0021): recorded by the
    /// app as a local notice, never sent.
    GroupChanged {
        gid: GroupId,
        notice: crate::content::Notice,
    },
    GroupMessage {
        gid: GroupId,
        /// The author's node, as the app knows members.
        from: PubKey,
        /// The author's identity (ADR-0013): what message references are computed with
        /// (§6.2).
        author: PubKey,
        content: Content,
        snr4: i8,
        rssi: i8,
        /// Recovered from a member after it was lost on the air (§10.6): shown as late.
        late: bool,
    },
    /// A position from `from`, direct or in `group` (§12); `at` is its full Unix time.
    Location {
        from: PubKey,
        group: Option<GroupId>,
        position: Position,
        at: u64,
        snr4: i8,
    },
    /// A share ended on its own (expiry, or its contact or group is gone).
    SharesChanged,
    /// `from` asks for a variant of an image we sent (§13.3).
    MediaRequested {
        from: PubKey,
        media_id: crate::content::MediaId,
        full: bool,
    },
    /// Something changed in our search for `target` (identity): a reception, a BEACON, a fix
    /// (§11); the estimate is to read again.
    SearchUpdated {
        target: PubKey,
    },
    /// Member `by` (node) of `gid` is searching for us and we answer (SOS or consent, §11).
    Searched {
        gid: GroupId,
        by: PubKey,
    },
    /// A BEACON from member `from` (node) of `gid` (§11.1): `sos` while it is in SOS (with or
    /// without a position: an SOS without GPS shows here), clear when it left SOS.
    Beacon {
        gid: GroupId,
        from: PubKey,
        sos: bool,
        battery: Option<u8>,
    },
    /// Our radio's TX power is now `dbm`; `previous` while a POWER request holds it, to
    /// restore after a restart ([`Core::restore_tx_power`]: the radio keeps it), `None` once
    /// restored.
    TxPower {
        dbm: i8,
        previous: Option<i8>,
    },
}

/// Why a request was refused right away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    UnknownContact,
    Empty,
    TooLarge,
    /// The node is not connected yet (its key is unknown).
    NoIdentity,
    /// The contact's identity is not known yet: exchange a message or scan their code first.
    IdentityUnknown,
    UnknownGroup,
    /// Only the group admin may do this.
    NotAllowed,
    /// A medium whose airtime does not fit in the budget left; it would fit in `wait_ms`
    /// (§13.4).
    OverBudget {
        wait_ms: Millis,
    },
    /// No search runs for this target.
    NotSearching,
    /// Rate limited (§11.3): possible again in `wait_ms`.
    TooSoon {
        wait_ms: Millis,
    },
    /// Our position is unknown or too old.
    NoPosition,
    /// A frame of the message already went out: it can no longer be cancelled (§6.1).
    Started,
    /// No unfinished message has this id.
    NotQueued,
    /// The wall clock is not known yet ([`Core::set_clock`]).
    NoClock,
    /// The invitation has expired (§10.5).
    Expired,
}

/// Core configuration. Its `Debug` leaves the seeds out (review of 2026-11-15, SR-34).
#[derive(Clone)]
pub struct Config {
    /// Sent in `APP_START`.
    pub app_name: String,
    /// The phone identity's seed (ADR-0013), kept by the app (Keychain / Keystore).
    pub seed: [u8; 32],
    /// Seed of the core's CSPRNG: 32 bytes from the OS random generator.
    pub rng_seed: [u8; 32],
    /// Regulatory profile (TOML).
    pub regulatory: String,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("app_name", &self.app_name)
            .finish_non_exhaustive()
    }
}

impl Config {
    pub fn new(app_name: &str, seed: [u8; 32], rng_seed: [u8; 32]) -> Self {
        Self {
            app_name: app_name.to_owned(),
            seed,
            rng_seed,
            regulatory: EU868.to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Expect {
    SelfInfo,
    DeviceInfo,
    Contacts,
    Contact,
    Ok,
    Scope,
    /// `SET_RADIO_TX_POWER` to this value (search, §11.1).
    TxPower(i8),
    /// `GET_STATS` radio (the noise floor).
    Stats,
    /// `SYNC_NEXT_MESSAGE` (§15.1).
    Sync,
    /// `GET_CHANNEL` of this slot (§15.3).
    Channel(u8),
    /// `SET_CHANNEL` of this slot.
    ChannelSet(u8),
    /// `SEND_TXT_MSG` of this plain direct message (§15.2).
    PlainSent(MsgId),
    /// `SEND_CHANNEL_TXT_MSG` of this post (§15.3).
    ChannelPost(MsgId),
}

/// Tries of a membership datagram (GROUP_STATE, SENDER_KEY, SENDER_KEYS): five within half an
/// hour, then one an hour for a day (§10.1, SR-12).
const MEMBERSHIP_TRIES: u8 = 5 + 24;

#[derive(Clone, Debug)]
struct Outgoing {
    id: MsgId,
    datagram: Vec<u8>,
    retried: bool,
    /// Sent by the core itself (group state, sender keys): not reported to the app, and
    /// retried with a backoff when it fails.
    internal: bool,
    /// Not before this time (retry backoff).
    not_before: Millis,
    tries: u8,
    /// A frame of it went out: it may have arrived, so it can no longer be cancelled (§6.1).
    started: bool,
}

#[derive(Clone, Debug)]
struct Initiating {
    tag: u16,
    salt: [u8; SALT_LEN],
    eph_secret: [u8; 32],
    attempt: u8,
    next_retry: Millis,
    path: OwnedPath,
}

/// Our introduction to a peer whose identity we do not know (§4.6), until INTRO_ACK.
#[derive(Clone, Debug)]
struct Introducing {
    tag: u16,
    salt: [u8; SALT_LEN],
    eph_secret: [u8; 32],
    attempt: u8,
    next_retry: Millis,
    path: OwnedPath,
}

/// The INTRO of an introduction in progress, with its return path `back`.
fn intro_message<'a>(intro: &Introducing, back: &'a OwnedPath) -> Intro<'a> {
    Intro {
        tag: intro.tag,
        attempt: intro.attempt,
        salt: intro.salt,
        ephemeral: x25519_public(&intro.eph_secret),
        return_path_len: back.len_byte,
        return_path: &back.bytes,
    }
}

/// INTRO_FIN resent until the responder answers under the new session (§4.6).
#[derive(Clone, Debug)]
struct Fin {
    salt_i: [u8; SALT_LEN],
    /// The INTRO's hash, bound into the introduction's keys (ADR-0029).
    intro_hash: [u8; 32],
    eph_secret: [u8; 32],
    ack: IntroAck,
    attempt: u8,
    next_retry: Millis,
    path: OwnedPath,
}

/// An INTRO we answered (§4.6): resends get the same answer.
#[derive(Clone, Debug)]
struct AnsweredIntro {
    tag: u16,
    answer: Answer,
    /// Where the INTRO said to answer: adopted as the peer's path once INTRO_FIN proves who
    /// sent it (SR-02).
    path: OwnedPath,
    at: Millis,
    /// INTRO_FIN received: the session is set up.
    done: bool,
}

/// A path discovery in progress (§4.5).
#[derive(Clone, Debug)]
struct Probing {
    id: ProbeId,
    tries: u8,
    next_try: Millis,
    /// Sent without the peer's identity: the reply is anonymous too.
    anonymous: bool,
}

#[derive(Clone, Debug)]
struct Answered {
    tag: u16,
    salt_i: [u8; SALT_LEN],
    salt_r: [u8; SALT_LEN],
    eph_public: [u8; 32],
    /// To seal the same answer again for a resent HELLO (ADR-0029).
    eph_secret: [u8; 32],
}

/// The datagram being sent to a peer.
#[derive(Clone, Debug)]
struct InFlight {
    msg: Outgoing,
    tag: u16,
    /// DATA frames handed to the companion so far (for progress).
    sent: u32,
    /// Sent on the responder's pending session rather than the confirmed one.
    on_pending: bool,
}

#[derive(Debug, Default)]
struct Peer {
    session: Option<Box<Session>>,
    /// Responder side, not confirmed by the initiator yet (§4.4).
    pending: Option<Box<Session>>,
    path: Option<OwnedPath>,
    initiating: Option<Initiating>,
    answered: Option<Answered>,
    queue: VecDeque<Outgoing>,
    in_flight: Option<InFlight>,
    probing: Option<Probing>,
    introducing: Option<Introducing>,
    fin: Option<Fin>,
    answered_intro: Option<AnsweredIntro>,
    /// The peer answered no HELLO: it may have a new identity, introduce again (§4.6).
    intro_needed: bool,
    /// A datagram went unacknowledged: a new handshake runs while the session stays, and
    /// anything that still arrives under the session ends it (ADR-0006).
    suspect: bool,
    /// The peer may not know our identity (we scanned its invitation): probes go anonymous
    /// from the first (§4.5).
    unknown_to_peer: bool,
    /// When the current session was made, and the datagram bytes sent under it: renewed
    /// after 24 h or 4 MB (§4.4).
    session_since: Millis,
    session_bytes: u64,
    /// No self-introduction before this time (answer to a HELLO we cannot open).
    intro_poke: Millis,
}

impl Peer {
    /// Something to send now (a message in flight, or one whose backoff has passed).
    fn has_work(&self, now: Millis) -> bool {
        self.in_flight.is_some() || self.queue.iter().any(|m| m.not_before <= now)
    }

    /// When the next queued message becomes sendable.
    fn next_ready(&self) -> Option<Millis> {
        self.queue.iter().map(|m| m.not_before).min()
    }

    /// The session to send with: the confirmed one, else the pending one.
    fn active(&mut self) -> Option<&mut Session> {
        match (self.session.as_deref_mut(), self.pending.as_deref_mut()) {
            (Some(s), _) | (None, Some(s)) => Some(s),
            (None, None) => None,
        }
    }

    /// Puts the message in flight back at the head of the queue (its session is gone).
    fn requeue_in_flight(&mut self) {
        if let Some(f) = self.in_flight.take() {
            self.queue.push_front(f.msg);
        }
    }

    /// A new responder session (a HELLO or INTRO_FIN answered again, §4.4) replaces the pending
    /// one: a message in flight on the old pending session would never finish (no frame, no
    /// timeout), so it goes back to the queue.
    fn replace_pending(&mut self, s: Session) {
        if self.in_flight.as_ref().is_some_and(|f| f.on_pending) {
            self.requeue_in_flight();
        }
        // We suspected the session was lost, and the peer starts a new one: it was (a
        // restart). What was in flight goes again under the new session (ADR-0006).
        if std::mem::take(&mut self.suspect) {
            self.requeue_in_flight();
            self.session = None;
            self.initiating = None;
            self.introducing = None;
        }
        self.pending = Some(Box::new(s));
    }

    /// The pending session is confirmed and replaces the old one. A message in flight on the
    /// old session is sent again; one already on the pending session simply continues.
    fn promote_pending(&mut self) {
        if self.in_flight.as_ref().is_some_and(|f| !f.on_pending) && self.session.is_some() {
            self.requeue_in_flight();
        }
        if let Some(f) = self.in_flight.as_mut() {
            f.on_pending = false;
        }
        self.session = self.pending.take();
    }
}

/// The application core.
pub struct Core {
    app_name: String,
    pub(crate) rng: ChaCha20Rng,
    regulatory: String,
    /// The phone identity (ADR-0013): private and public key. Always set.
    pub(crate) prv: Option<[u8; 64]>,
    pub(crate) key: Option<PubKey>,
    /// The node's key, from `SELF_INFO`: frame headers and routing only.
    pub(crate) node: Option<PubKey>,
    /// The companion answered and has not been reported gone since (§6.1): nothing goes on
    /// the air, and no timer advances, while it is false.
    online: bool,
    pub(crate) name: String,
    freq_khz: u32,
    pub(crate) radio: LoRaParams,
    budget: Budget,
    pub(crate) contacts: HashMap<PubKey, Contact>,
    secrets: HashMap<PubKey, [u8; 32]>,
    peers: HashMap<PubKey, Peer>,
    commands: VecDeque<(Vec<u8>, Expect)>,
    awaiting: Option<(Expect, Millis)>,
    /// Plain MeshCore text (§15).
    pub(crate) plain: crate::plain::Plain,
    pub(crate) events: VecDeque<Event>,
    pub(crate) next_id: MsgId,
    deferred_until: Option<Millis>,
    /// Hashes of datagrams received recently, per peer, to drop resends (§6).
    recent: HashMap<PubKey, VecDeque<([u8; 16], Millis)>>,
    /// Key of the companion's default flood scope, for our floods' transport codes.
    pub(crate) scope: Option<[u8; 16]>,
    /// Probes already answered (a flood arrives several times, by several paths).
    seen_probes: VecDeque<ProbeId>,
    /// Answers sent to frames anyone can send, by node, for their limits (SR-04).
    unprompted: VecDeque<(PubKey, Millis)>,
    /// Nodes already reported as speaking a newer version.
    newer: HashSet<PubKey>,
    /// Groups we belong to (§10).
    pub(crate) groups: HashMap<GroupId, Group>,
    /// Sender keys that came before their group state.
    pub(crate) pending_keys: Vec<PendingKey>,
    /// Group envelopes waiting for their sender key.
    pub(crate) held: VecDeque<Held>,
    /// Floods per group message (§10.3).
    pub(crate) group_copies: u8,
    /// Members ask for the group messages they missed (§10.6); off only to measure without.
    pub(crate) group_recovery: bool,
    /// Groups changed since the app last persisted them.
    pub(crate) dirty_groups: HashSet<GroupId>,
    /// Invitations shown (§10.5): token → group and expiry. Never stored: a restart revokes
    /// them.
    pub(crate) invites: HashMap<[u8; 16], (GroupId, Millis)>,
    /// People each invitation admitted (SR-19).
    pub(crate) invite_admissions: HashMap<[u8; 16], u8>,
    /// Groups that listed us and wait for the user's answer (§10.1, SR-13). In memory only.
    pub(crate) invitations: Vec<crate::group::Invitation>,
    /// Invitations to store again or delete (SR-13: they survive restarts).
    pub(crate) dirty_invitations: HashSet<GroupId>,
    /// Admins we asked to join with a scanned invitation (§10.5), with when: their state is
    /// the answer, joined without asking again.
    pub(crate) joining: Vec<(PubKey, Millis)>,
    /// A group that lists us waits for the user (true for the apps); false joins at once,
    /// for test nodes and bots.
    pub(crate) group_consent: bool,
    /// Members we admitted, with when (§10.2): a key of another member that changes soon
    /// after reaches them through us. In memory only.
    pub(crate) admitted: Vec<(GroupId, PubKey, Millis)>,
    /// Group frames waiting for their time (the second fragment is spaced out).
    pub(crate) group_tx: VecDeque<crate::group::GroupFrame>,
    /// The radio is shared by every session: the next data frame may go at this time.
    pub(crate) next_slot: Millis,
    /// Session renewal limits (§4.4): age and bytes sent.
    session_max_age: Millis,
    session_max_bytes: u64,
    /// Peer served last, for round-robin between sessions.
    rr_last: Option<PubKey>,
    /// Location sharing (§12).
    pub(crate) loc: Locations,
    /// New identities presented by verified contacts, waiting for the user (§4.6).
    pending_identities: HashMap<PubKey, PubKey>,
    /// Search mode (§11).
    pub(crate) search: crate::search::Searches,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core")
            .field("key", &self.key.map(|k| k[0]))
            .field("contacts", &self.contacts.len())
            .field("peers", &self.peers.len())
            .field("commands", &self.commands.len())
            .finish_non_exhaustive()
    }
}

/// A session is renewed after this long (§4.4)...
pub const SESSION_MAX_AGE_MS: Millis = 24 * 3_600_000;
/// ...or after this many datagram bytes sent under it.
pub const SESSION_MAX_BYTES: u64 = 4 << 20;

/// Default radio until `SELF_INFO` arrives: 62.5 kHz / SF8 / CR 4/5.
const DEFAULT_RADIO: LoRaParams = LoRaParams {
    sf: 8,
    bw_hz: 62_500,
    cr: 5,
    preamble: 32,
};

impl Core {
    pub fn new(config: Config) -> Self {
        let budget = Budget::for_frequency(&config.regulatory, 869_618_000)
            .or_else(|_| Budget::for_frequency(EU868, 869_618_000))
            .unwrap_or_else(|_| unreachable!("the bundled profile parses"));
        Self {
            app_name: config.app_name,
            rng: ChaCha20Rng::from_seed(config.rng_seed),
            regulatory: config.regulatory,
            key: Some(public_key(&expand_seed(&config.seed))),
            prv: Some(expand_seed(&config.seed)),
            node: None,
            online: false,
            name: String::new(),
            freq_khz: 869_618,
            radio: DEFAULT_RADIO,
            budget,
            contacts: HashMap::new(),
            secrets: HashMap::new(),
            peers: HashMap::new(),
            commands: VecDeque::new(),
            awaiting: None,
            plain: crate::plain::Plain::default(),
            events: VecDeque::new(),
            next_id: 1,
            deferred_until: None,
            recent: HashMap::new(),
            scope: None,
            seen_probes: VecDeque::new(),
            unprompted: VecDeque::new(),
            newer: HashSet::new(),
            groups: HashMap::new(),
            pending_keys: Vec::new(),
            held: VecDeque::new(),
            group_copies: crate::group::COPIES,
            group_recovery: true,
            dirty_groups: HashSet::new(),
            invites: HashMap::new(),
            invite_admissions: HashMap::new(),
            invitations: Vec::new(),
            dirty_invitations: HashSet::new(),
            joining: Vec::new(),
            group_consent: true,
            admitted: Vec::new(),
            group_tx: VecDeque::new(),
            next_slot: 0,
            session_max_age: SESSION_MAX_AGE_MS,
            session_max_bytes: SESSION_MAX_BYTES,
            rr_last: None,
            loc: Locations::default(),
            pending_identities: HashMap::new(),
            search: crate::search::Searches::default(),
        }
    }

    // ----- accessors -------------------------------------------------------------------

    /// Our identity key (ADR-0013).
    pub const fn key(&self) -> Option<&PubKey> {
        self.key.as_ref()
    }

    /// The node's key, once `SELF_INFO` gave it.
    pub const fn node_key(&self) -> Option<&PubKey> {
        self.node.as_ref()
    }

    /// The node's name, from `SELF_INFO` (empty before).
    pub fn node_name(&self) -> &str {
        &self.name
    }

    /// The node answered: its key is known and lichen can talk.
    pub const fn ready(&self) -> bool {
        self.node.is_some()
    }

    /// The companion is connected and answered (§6.1).
    pub const fn online(&self) -> bool {
        self.online
    }

    /// Contact `node`'s identity is now `identity`, verified or not (a contact card scanned in
    /// person, or safety numbers compared).
    pub fn set_identity(&mut self, node: &PubKey, identity: PubKey, verified: bool) {
        if let Some(c) = self.contacts.get_mut(node) {
            if c.identity != Some(identity) {
                self.secrets.remove(node);
                if let Some(p) = self.peers.get_mut(node) {
                    p.session = None;
                    p.pending = None;
                }
            }
            c.identity = Some(identity);
            c.verified = verified;
        }
    }

    /// An identity for contact `node` from outside the radio (a contact card, §4.7), held to
    /// the same rule as one learned on the air (§4.6): adopted when the contact has none, kept
    /// when it is the same, waiting for the user when it differs. True when it is now the
    /// contact's identity.
    pub fn present_identity(&mut self, node: &PubKey, identity: PubKey) -> bool {
        self.learn_identity(node, identity)
    }

    /// The new identity a contact presented, waiting for the user (§4.6).
    pub fn pending_identity(&self, node: &PubKey) -> Option<&PubKey> {
        self.pending_identities.get(node)
    }

    /// The user accepts contact `node`'s new identity, the one they compared (`expected`): it
    /// replaces the old one, verified when `verify` (safety numbers matched). Refused, and
    /// nothing changes, when the pending identity is no longer `expected`: another one
    /// arrived while the user compared, and they must compare again (§4.6).
    pub fn accept_identity(&mut self, node: &PubKey, expected: &PubKey, verify: bool) -> bool {
        if self.pending_identities.get(node) != Some(expected) {
            return false;
        }
        let Some(identity) = self.pending_identities.remove(node) else {
            return false;
        };
        let old = self.identity_of(node);
        self.set_identity(node, identity, verify);
        if let Some(p) = self.peers.get_mut(node) {
            p.intro_needed = false;
        }
        self.events.push_back(Event::IdentityLearned {
            node: *node,
            identity,
            changed: old.is_some(),
        });
        true
    }

    /// The user rejects contact `node`'s new identity `expected`: the verified one stays. A
    /// different identity pending since stays pending: the user has not seen it.
    pub fn reject_identity(&mut self, node: &PubKey, expected: &PubKey) {
        if self.pending_identities.get(node) == Some(expected) {
            self.pending_identities.remove(node);
        }
    }

    /// Marks contact `node` verified or not (safety numbers compared, §4.7). Verifying needs
    /// the identity whose number the user compared: refused (false) if the contact's identity
    /// is no longer `identity`. Withdrawing verification always works.
    pub fn set_verified(&mut self, node: &PubKey, identity: &PubKey, verified: bool) -> bool {
        match self.contacts.get_mut(node) {
            Some(c) if c.identity == Some(*identity) || (!verified && c.identity.is_some()) => {
                c.verified = verified;
                true
            }
            _ => false,
        }
    }

    pub fn contacts(&self) -> impl Iterator<Item = &Contact> {
        self.contacts.values()
    }

    pub const fn radio(&self) -> &LoRaParams {
        &self.radio
    }

    /// Frequency reported by the companion, in kHz.
    pub const fn freq_khz(&self) -> u32 {
        self.freq_khz
    }

    /// Fragments and airtime (ms, our transmissions only, first round) of a datagram of
    /// `datagram_len` bytes to `to`, on the path known now (0 hops if none is known yet).
    pub fn estimate_send(&self, to: &PubKey, datagram_len: usize) -> (u8, Millis) {
        let path_bytes = self
            .peers
            .get(to)
            .and_then(|p| p.path.as_ref())
            .or_else(|| self.contacts.get(to).and_then(|c| c.out_path.as_ref()))
            .map_or(0, |p| p.bytes.len());
        let chunk = chunk_for_path(path_bytes);
        let len = datagram_len.max(1);
        let fragments = len.div_ceil(chunk);
        let mut airtime = 0;
        for i in 0..fragments {
            let plain = if i + 1 == fragments {
                len - i * chunk
            } else {
                chunk
            };
            airtime += self
                .radio
                .airtime_us(2 + path_bytes + plain + lc_proto::OVERHEAD)
                / 1000;
        }
        (fragments.min(255) as u8, airtime)
    }

    /// Airtime used in the sliding hour and the limit, in milliseconds.
    pub fn budget(&mut self, now: Millis) -> (Millis, Millis) {
        (self.budget.used_ms(now), self.budget.limit_ms())
    }

    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    // ----- app requests ----------------------------------------------------------------

    /// Starts the connection sequence (`APP_START`, `DEVICE_QUERY`, key export, contacts).
    pub fn connect(&mut self, now: Millis) {
        let name = self.app_name.clone();
        self.command(
            &Command::AppStart {
                name: name.as_bytes(),
            },
            Expect::SelfInfo,
        );
        self.command(
            &Command::DeviceQuery { app_ver: APP_VER },
            Expect::DeviceInfo,
        );
        self.command(&Command::GetDefaultFloodScope, Expect::Scope);
        self.command(&Command::GetContacts { since: None }, Expect::Contacts);
        self.plain_connected();
        self.tick(now);
    }

    /// The companion is gone (Bluetooth or USB lost). Nothing goes on the air and no timer
    /// advances until [`Core::connect`] gets an answer: queued messages wait, a transfer in
    /// progress resumes where it stopped (§6.1). Commands not answered yet are dropped.
    pub fn disconnect(&mut self, _now: Millis) {
        self.online = false;
        self.commands.clear();
        self.awaiting = None;
        self.plain_disconnected();
    }

    /// Adds or updates a contact by hand (contact card, tests); a known identity is kept.
    pub fn add_contact(&mut self, key: PubKey, name: &str, out_path: Option<OwnedPath>) {
        let (identity, verified, kind) = self
            .contacts
            .get(&key)
            .map_or((None, false, contact_kind::CHAT), |c| {
                (c.identity, c.verified, c.kind)
            });
        self.contacts.insert(
            key,
            Contact {
                key,
                name: name.to_owned(),
                out_path,
                identity,
                verified,
                kind,
            },
        );
    }

    /// A contact restored by the app with its identity (app start).
    pub fn restore_contact(&mut self, contact: Contact) {
        self.contacts.insert(contact.key, contact);
    }

    /// Queues a text message. `sent_at` is the wall-clock time in Unix seconds.
    pub fn send_text(
        &mut self,
        to: &PubKey,
        text: &str,
        sent_at: u32,
        now: Millis,
    ) -> Result<MsgId, Error> {
        if text.is_empty() {
            return Err(Error::Empty);
        }
        let content = Content::Text {
            sent_at,
            text: text.to_owned(),
        };
        self.send_datagram(to, content.encode(), now)
    }

    /// Queues a raw datagram (`content_type ‖ body`).
    pub fn send_datagram(
        &mut self,
        to: &PubKey,
        datagram: Vec<u8>,
        now: Millis,
    ) -> Result<MsgId, Error> {
        if !self.contacts.contains_key(to) {
            return Err(Error::UnknownContact);
        }
        if datagram.is_empty() {
            return Err(Error::Empty);
        }
        if datagram.len() > DATAGRAM_MAX {
            return Err(Error::TooLarge);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.peers
            .entry(*to)
            .or_default()
            .queue
            .push_back(Outgoing {
                id,
                datagram,
                retried: false,
                internal: false,
                not_before: 0,
                tries: 0,
                started: false,
            });
        self.tick(now);
        Ok(id)
    }

    /// Queues a medium (§13): direct only, and refused when its first round does not fit in
    /// the airtime left this hour.
    pub fn send_media(
        &mut self,
        to: &PubKey,
        datagram: Vec<u8>,
        now: Millis,
    ) -> Result<MsgId, Error> {
        if !self.contacts.contains_key(to) {
            return Err(Error::UnknownContact);
        }
        // What is already queued will spend its airtime first.
        let (_, airtime) = self.estimate_send(to, datagram.len());
        let queued = self.queued_airtime();
        let at = self.budget.opportunity_after(now, queued, airtime);
        if at > now {
            return Err(Error::OverBudget { wait_ms: at - now });
        }
        self.send_datagram(to, datagram, now)
    }

    /// Airtime (first round) of every datagram queued or in flight, counted whole.
    fn queued_airtime(&self) -> Millis {
        self.peers
            .iter()
            .flat_map(|(k, p)| {
                p.queue
                    .iter()
                    .map(|m| m.datagram.len())
                    .chain(p.in_flight.as_ref().map(|f| f.msg.datagram.len()))
                    .map(move |len| (k, len))
            })
            .map(|(k, len)| self.estimate_send(k, len).1)
            .sum()
    }

    /// Where unfinished message `id` stands (§6.1): `None` once it is delivered, failed,
    /// sent to its group or cancelled, and for the core's own messages.
    pub fn outgoing(&self, id: MsgId, now: Millis) -> Option<Outbound> {
        if let Some(o) = self.plain_outgoing(id) {
            return Some(o);
        }
        let budget = self.deferred_until.filter(|&u| u > now);
        if let Some(pos) = self
            .group_tx
            .iter()
            .position(|f| f.id == id && f.user && f.copy == 0)
        {
            let started = !self.group_tx.iter().any(|f| f.id == id && f.seq == 0);
            let stage = if !self.online {
                Stage::Offline
            } else if self.group_tx.iter().take(pos).any(|f| f.id != id) {
                Stage::Behind
            } else {
                Stage::Transmitting
            };
            return Some(Outbound { stage, started });
        }
        for (k, p) in &self.peers {
            if p.in_flight
                .as_ref()
                .is_some_and(|f| f.msg.id == id && !f.msg.internal)
            {
                let stage = match (self.online, budget) {
                    (false, _) => Stage::Offline,
                    (true, Some(until)) => Stage::Budget { until },
                    (true, None) => Stage::Transmitting,
                };
                // In flight: its session is sending it, it can only be abandoned.
                return Some(Outbound {
                    stage,
                    started: true,
                });
            }
            let Some(pos) = p.queue.iter().position(|m| m.id == id && !m.internal) else {
                continue;
            };
            let started = p.queue.get(pos).is_some_and(|m| m.started);
            let stage = if !self.online {
                Stage::Offline
            } else if p.in_flight.is_some() || p.queue.iter().take(pos).any(|m| m.not_before <= now)
            {
                Stage::Behind
            } else if self.path_for(k, p).is_none() {
                Stage::FindingPath
            } else if p.session.is_none() && p.pending.is_none() {
                Stage::Connecting
            } else if let Some(until) = budget {
                Stage::Budget { until }
            } else {
                Stage::Transmitting
            };
            return Some(Outbound { stage, started });
        }
        None
    }

    /// Cancels a message the app sent, as long as none of its frames went out (§6.1).
    pub fn cancel(&mut self, id: MsgId) -> Result<(), Error> {
        if let Some(r) = self.plain_cancel(id) {
            return r;
        }
        if self.group_tx.iter().any(|f| f.id == id && f.user) {
            if !self.group_tx.iter().any(|f| f.id == id && f.seq == 0) {
                return Err(Error::Started);
            }
            // The counter it took stays unused: receivers' windows accept the gap (§10.3).
            self.group_tx.retain(|f| f.id != id);
            return Ok(());
        }
        for p in self.peers.values_mut() {
            if p.in_flight.as_ref().is_some_and(|f| f.msg.id == id) {
                return Err(Error::Started);
            }
            if let Some(pos) = p.queue.iter().position(|m| m.id == id && !m.internal) {
                if p.queue.get(pos).is_some_and(|m| m.started) {
                    return Err(Error::Started);
                }
                p.queue.remove(pos);
                return Ok(());
            }
        }
        Err(Error::NotQueued)
    }

    /// Gives up an unfinished message whose expiry passed (§6.1): it leaves its queue, or its
    /// transfer is abandoned (an ABORT goes out), and it is reported failed (`Expired`).
    /// Returns whether it was found.
    pub fn abandon(&mut self, id: MsgId) -> bool {
        if self.plain_abandon(id) {
            return true;
        }
        let expired = Event::Failed {
            id,
            reason: FailReason::Expired,
        };
        if self
            .group_tx
            .iter()
            .any(|f| f.id == id && f.user && f.copy == 0)
        {
            self.group_tx.retain(|f| f.id != id);
            self.events.push_back(expired);
            return true;
        }
        for p in self.peers.values_mut() {
            if p.in_flight
                .as_ref()
                .is_some_and(|f| f.msg.id == id && !f.msg.internal)
            {
                let on_pending = p.in_flight.take().is_some_and(|f| f.on_pending);
                let session = if on_pending {
                    p.pending.as_deref_mut()
                } else {
                    p.session.as_deref_mut()
                };
                if let Some(s) = session {
                    s.cancel();
                }
                self.events.push_back(expired);
                return true;
            }
            if let Some(pos) = p.queue.iter().position(|m| m.id == id && !m.internal) {
                p.queue.remove(pos);
                self.events.push_back(expired);
                return true;
            }
        }
        false
    }

    /// Another node answers after a reconnection (§6.1): sessions and paths belonged to the
    /// old one. Queued messages stay; one in flight goes back to its queue.
    fn forget_routes(&mut self) {
        for p in self.peers.values_mut() {
            p.requeue_in_flight();
            p.session = None;
            p.pending = None;
            p.path = None;
            p.initiating = None;
            p.answered = None;
            p.probing = None;
            p.introducing = None;
            p.fin = None;
            p.answered_intro = None;
        }
        for c in self.contacts.values_mut() {
            c.out_path = None;
        }
    }

    /// Random bytes from the core's generator (media ids).
    pub fn random_id(&mut self) -> crate::content::MediaId {
        let mut id = [0u8; 8];
        self.rng.fill_bytes(&mut id);
        id
    }

    /// Queues a group control datagram for `to`, merged into one still waiting for it when
    /// possible (§10.2): a newer GROUP_STATE of the same group replaces the waiting one, and
    /// SENDER_KEYS of the same group join the waiting one.
    pub(crate) fn enqueue_control(&mut self, to: &PubKey, datagram: Vec<u8>) {
        if let Some(p) = self.peers.get_mut(to) {
            for m in p.queue.iter_mut().filter(|m| m.internal && !m.started) {
                if let Some(merged) = crate::group::merge_control(&m.datagram, &datagram) {
                    m.datagram = merged;
                    return;
                }
            }
        }
        self.enqueue_internal(to, datagram);
    }

    /// The datagrams waiting for peer `k` (tests).
    #[cfg(test)]
    pub(crate) fn queued(&self, k: &PubKey) -> Vec<Vec<u8>> {
        self.peers
            .get(k)
            .map(|p| p.queue.iter().map(|m| m.datagram.clone()).collect())
            .unwrap_or_default()
    }

    /// A session with node `k` is open: a datagram to it needs no handshake.
    pub(crate) fn has_session(&self, k: &PubKey) -> bool {
        self.peers.get(k).is_some_and(|p| p.session.is_some())
    }

    /// Peer `k` may not know our identity yet (§4.5): its probes go anonymous.
    pub(crate) fn unknown_to(&mut self, k: &PubKey) {
        self.peers.entry(*k).or_default().unknown_to_peer = true;
    }

    /// Queues a datagram sent by the core itself (not reported to the app).
    pub(crate) fn enqueue_internal(&mut self, to: &PubKey, datagram: Vec<u8>) {
        let id = self.next_id;
        self.next_id += 1;
        self.peers
            .entry(*to)
            .or_default()
            .queue
            .push_back(Outgoing {
                id,
                datagram,
                retried: false,
                internal: true,
                not_before: 0,
                tries: 0,
                started: false,
            });
    }

    /// Queues a position for `to`, replacing one still waiting in its queue (§12.2).
    pub(crate) fn enqueue_location(&mut self, to: &PubKey, datagram: Vec<u8>) -> MsgId {
        let stale: Vec<MsgId> = self
            .peers
            .get(to)
            .map(|p| {
                p.queue
                    .iter()
                    .filter(|m| self.is_location(m.id))
                    .map(|m| m.id)
                    .collect()
            })
            .unwrap_or_default();
        if let Some(p) = self.peers.get_mut(to) {
            p.queue.retain(|m| !stale.contains(&m.id));
        }
        for id in &stale {
            self.loc.in_flight.remove(id);
        }
        let id = self.next_id;
        self.enqueue_internal(to, datagram);
        id
    }

    /// Changes the session renewal limits (§4.4, configurable): age and datagram bytes sent.
    pub fn set_session_limits(&mut self, max_age_ms: Millis, max_bytes: u64) {
        self.session_max_age = max_age_ms;
        self.session_max_bytes = max_bytes;
    }

    /// Announces our node (`SEND_SELF_ADVERT`): radios that hear it add it as a contact.
    /// `flood` lets repeaters relay it; otherwise only direct neighbours hear it. Its airtime
    /// counts like any transmission.
    pub fn send_advert(&mut self, flood: bool, now: Millis) {
        self.command(&Command::SendSelfAdvert { flood }, Expect::Ok);
        // Header and path length, key, timestamp, signature, flags, name.
        let len = 2 + 32 + 4 + 64 + 1 + self.name.len().min(32);
        let airtime = self.radio.airtime_us(len) / 1000;
        self.budget.record(now, airtime);
        self.occupy(now, airtime, None);
        self.tick(now);
    }

    /// Hands a raw on-air frame to the companion (`SEND_RAW_PACKET`), and counts its airtime.
    pub(crate) fn send_raw_packet(&mut self, raw: &[u8], now: Millis) {
        if raw.is_empty() {
            return;
        }
        self.command(&Command::SendRawPacket { priority: 1, raw }, Expect::Ok);
        let airtime = self.radio.airtime_us(raw.len()) / 1000;
        self.budget.record(now, airtime);
        self.occupy(now, airtime, None);
    }

    // ----- runner interface ------------------------------------------------------------

    /// The next command frame for the companion, if one is due. Commands go one at a time:
    /// the next waits for the answer to the previous one (or a timeout).
    pub fn poll_companion(&mut self, now: Millis) -> Option<Vec<u8>> {
        if let Some((_, deadline)) = self.awaiting {
            if now < deadline {
                return None;
            }
            self.awaiting = None;
        }
        let (frame, expect) = self.commands.pop_front()?;
        self.awaiting = Some((expect, now + COMMAND_TIMEOUT_MS));
        Some(frame)
    }

    /// A frame from the companion.
    pub fn on_companion_frame(&mut self, frame: &[u8], now: Millis) {
        let Ok(r) = Response::decode(frame) else {
            return;
        };
        match r {
            Response::RawData {
                snr4,
                rssi,
                payload,
            } => self.on_raw(payload, snr4, rssi, now),
            Response::PathDiscoveryResponse {
                prefix, out_path, ..
            } => self.on_path_discovered(prefix, OwnedPath::from_path(out_path)),
            Response::Advert { key } | Response::PathUpdated { key } => {
                let key = *key;
                self.command(&Command::GetContactByKey { key: &key }, Expect::Contact);
            }
            Response::LogRxData { snr4, rssi, raw } => self.on_log_rx(raw, snr4, rssi, now),
            Response::MsgWaiting => self.plain_sync(),
            Response::SendConfirmed { ack, .. } => self.on_send_confirmed(ack),
            Response::NewAdvert(_) => {}
            Response::Other { code, .. } if code >= 0x80 => {}
            r => self.on_reply(r, now),
        }
        self.tick(now);
    }

    /// Advances timers and pushes due frames out.
    pub fn tick(&mut self, now: Millis) {
        if self.deferred_until.is_some_and(|t| now >= t) {
            self.deferred_until = None;
        }
        if self.node.is_none() || !self.online {
            return;
        }
        self.tick_groups(now);
        self.tick_locations(now);
        self.tick_search(now);
        self.tick_plain(now);
        // Round robin: start after the peer served last, so that sessions share the radio.
        let mut keys: Vec<PubKey> = self.peers.keys().copied().collect();
        keys.sort_unstable();
        if let Some(pos) = self
            .rr_last
            .and_then(|last| keys.iter().position(|k| *k == last))
        {
            keys.rotate_left(pos + 1);
        }
        for k in keys {
            self.pump_peer(k, now);
        }
    }

    /// When [`Core::tick`] or [`Core::poll_companion`] should be called next.
    pub fn next_wakeup(&self) -> Option<Millis> {
        let mut t: Option<Millis> = None;
        if !self.commands.is_empty() {
            t = Some(self.awaiting.map_or(0, |(_, d)| d));
        }
        // Offline: only the commands of a reconnection (§6.1).
        if !self.online {
            return t;
        }
        let mut consider = |x: Millis| t = Some(t.map_or(x, |y: Millis| y.min(x)));
        if let Some(d) = self.deferred_until {
            consider(d);
        }
        if let Some(d) = self.next_group_wakeup() {
            consider(d);
        }
        if let Some(d) = self.next_location_wakeup() {
            consider(d);
        }
        if let Some(d) = self.next_search_wakeup() {
            consider(d);
        }
        if let Some(d) = self.next_plain_wakeup() {
            consider(d);
        }
        let slot = self.next_slot.max(self.deferred_until.unwrap_or(0));
        for p in self.peers.values() {
            for s in [p.session.as_deref(), p.pending.as_deref()]
                .into_iter()
                .flatten()
            {
                if let Some(w) = s.next_ack_wakeup() {
                    consider(w);
                }
                if let Some(w) = s.next_data_wakeup() {
                    consider(w.max(slot));
                }
            }
            if let Some(i) = &p.initiating {
                consider(i.next_retry);
            }
            if let Some(pr) = &p.probing {
                consider(pr.next_try);
            }
            if p.in_flight.is_none()
                && let Some(r) = p.next_ready()
            {
                consider(r);
            }
        }
        t
    }

    // ----- companion plumbing ----------------------------------------------------------

    pub(crate) fn command(&mut self, cmd: &Command<'_>, expect: Expect) {
        let mut buf = [0u8; lc_meshcore::FRAME_MAX];
        if let Ok(n) = cmd.encode(&mut buf) {
            self.commands.push_back((buf[..n].to_vec(), expect));
        }
    }

    fn on_reply(&mut self, r: Response<'_>, now: Millis) {
        let expect = self.awaiting.map(|(e, _)| e);
        let mut done = true;
        match r {
            Response::SelfInfo(si) => {
                if self.node.is_some_and(|n| n != *si.key) {
                    self.forget_routes();
                }
                self.node = Some(*si.key);
                self.online = true;
                self.name = String::from_utf8_lossy(si.name).into_owned();
                self.search.tx_dbm = Some(si.tx_power_dbm);
                self.search.max_tx_dbm = Some(si.max_tx_power_dbm);
                self.freq_khz = si.freq_khz;
                self.radio = LoRaParams::meshcore(si.sf, si.bw_hz, si.cr).unwrap_or(DEFAULT_RADIO);
                if let Ok(b) =
                    Budget::for_frequency(&self.regulatory, u64::from(si.freq_khz) * 1000)
                {
                    self.budget = b;
                }
                self.events.push_back(Event::Connected {
                    key: *si.key,
                    name: self.name.clone(),
                    freq_khz: si.freq_khz,
                    radio: self.radio,
                });
            }
            Response::ContactsStart { .. } => {
                done = false;
                if let Some((_, d)) = self.awaiting.as_mut() {
                    *d = now + COMMAND_TIMEOUT_MS;
                }
            }
            Response::Contact(c) => {
                let key = *c.key;
                let name = String::from_utf8_lossy(c.name).into_owned();
                let out_path = c.out_path.map(OwnedPath::from_path);
                let (identity, verified) = self
                    .contacts
                    .get(&key)
                    .map_or((None, false), |c| (c.identity, c.verified));
                self.contacts.insert(
                    key,
                    Contact {
                        key,
                        name,
                        out_path,
                        identity,
                        verified,
                        kind: c.kind,
                    },
                );
                if expect == Some(Expect::Contacts) {
                    done = false;
                    if let Some((_, d)) = self.awaiting.as_mut() {
                        *d = now + COMMAND_TIMEOUT_MS;
                    }
                } else {
                    self.events.push_back(Event::ContactsUpdated);
                }
            }
            Response::EndOfContacts { .. } => {
                self.events.push_back(Event::ContactsUpdated);
                self.plain_contacts_updated(now);
            }
            Response::DefaultFloodScope { key, .. } => self.scope = key.copied(),
            Response::Ok => match expect {
                Some(Expect::TxPower(dbm)) => self.search_power_applied(dbm),
                Some(Expect::ChannelPost(id)) => self.events.push_back(Event::GroupSent { id }),
                _ => {}
            },
            Response::Err(_) => match expect {
                Some(Expect::TxPower(dbm)) => self.search_power_refused(dbm),
                Some(Expect::Channel(index)) => self.on_channel_info(index, None, now),
                Some(Expect::ChannelSet(index)) => {
                    self.command(&Command::GetChannel { index }, Expect::Channel(index));
                }
                Some(Expect::PlainSent(id)) => self.on_plain_refused(id),
                Some(Expect::ChannelPost(id)) => self.events.push_back(Event::Failed {
                    id,
                    reason: FailReason::Refused,
                }),
                Some(Expect::Sync) => self.plain.syncing = false,
                _ => {}
            },
            Response::Sent {
                tag, timeout_ms, ..
            } => {
                if let Some(Expect::PlainSent(id)) = expect {
                    self.on_plain_sent(id, tag, timeout_ms, now);
                }
            }
            Response::NoMoreMessages => self.plain.syncing = false,
            Response::ContactMsg {
                snr4,
                prefix,
                path_len,
                kind,
                ts,
                text,
            } => {
                self.on_contact_msg(prefix, kind, ts, text, snr4, path_len, now);
                if expect == Some(Expect::Sync) {
                    self.plain.syncing = false;
                    self.plain_sync();
                }
            }
            Response::ChannelMsg {
                snr4,
                channel,
                path_len,
                kind,
                ts,
                text,
            } => {
                self.on_channel_msg(channel, kind, ts, text, snr4, path_len, now);
                if expect == Some(Expect::Sync) {
                    self.plain.syncing = false;
                    self.plain_sync();
                }
            }
            Response::ChannelInfo {
                index,
                name,
                secret,
            } => self.on_channel_info(index, Some((name, secret)), now),
            Response::DeviceInfo(d) => self.plain_set_max_channels(d.max_channels),
            Response::RadioStats { noise, .. } => self.search_noise(noise),
            _ => {}
        }
        if done {
            self.awaiting = None;
        }
    }

    fn on_path_discovered(&mut self, prefix: &[u8; 6], path: OwnedPath) {
        let Some(key) = self
            .peers
            .iter()
            .find(|(k, p)| p.probing.is_some() && k.starts_with(prefix))
            .map(|(k, _)| *k)
        else {
            return;
        };
        if let Some(c) = self.contacts.get_mut(&key) {
            c.out_path = Some(path.clone());
        }
        if let Some(p) = self.peers.get_mut(&key) {
            p.probing = None;
            p.path = Some(path);
        }
    }

    /// The static secret with contact `peer` (a node key): between identities (§2), once the
    /// peer's identity is known.
    fn secret(&mut self, peer: &PubKey) -> Option<[u8; 32]> {
        if let Some(s) = self.secrets.get(peer) {
            return Some(*s);
        }
        let id = self.contacts.get(peer)?.identity?;
        let s = shared_secret(self.prv.as_ref()?, &id)?;
        self.secrets.insert(*peer, s);
        Some(s)
    }

    pub(crate) fn identity_of(&self, node: &PubKey) -> Option<PubKey> {
        self.contacts.get(node)?.identity
    }

    fn timing_for(&self, path: &OwnedPath) -> Timing {
        let pb = path.bytes.len();
        let t_frame = self.radio.airtime_us(2 + pb + raw_payload_max(pb)) / 1000;
        // The longest ACK (64 fragments, §7.2): the timers must hold for every datagram.
        let t_ack = self.radio.airtime_us(2 + pb + ACK_FRAME_MAX) / 1000;
        Timing::for_path(t_frame, t_ack, path.hops())
    }

    /// Timing with fresh random jitter (§7.3): `Δ + U[0, t_frame)` and `W + U[0, W/2)`. Two
    /// senders that cannot hear each other would otherwise stay in lockstep and collide at the
    /// repeater between them on every round.
    fn jittered(&mut self, mut t: Timing, path_bytes: usize) -> Timing {
        let t_frame = (self
            .radio
            .airtime_us(2 + path_bytes + raw_payload_max(path_bytes))
            / 1000)
            .max(1);
        t.pace_ms += Millis::from(self.rng.next_u32()) % t_frame;
        t.ack_wait_ms += Millis::from(self.rng.next_u32()) % (t.ack_wait_ms / 2).max(1);
        t
    }

    /// Wait before resending a HELLO: uniform in `[W, 2W)`. Fixed timers keep two hidden
    /// nodes that started together colliding at the repeater between them forever.
    fn hello_backoff(&mut self, path: &OwnedPath) -> Millis {
        let w = self.timing_for(path).ack_wait_ms.max(1);
        w + Millis::from(self.rng.next_u32()) % w
    }

    fn path_for(&self, k: &PubKey, peer: &Peer) -> Option<OwnedPath> {
        peer.path
            .clone()
            .or_else(|| self.contacts.get(k).and_then(|c| c.out_path.clone()))
    }

    /// Hands a lichen frame to the companion along `path`, and counts its airtime.
    fn send_raw(&mut self, path: &OwnedPath, frame: &[u8], now: Millis) {
        let Some(p) = path.path() else {
            return;
        };
        self.command(
            &Command::SendRawData {
                path: p,
                payload: frame,
            },
            Expect::Ok,
        );
        let airtime = self.radio.airtime_us(2 + path.bytes.len() + frame.len()) / 1000;
        self.budget.record(now, airtime);
        self.occupy(now, airtime, Some(path.hops()));
    }

    /// Reports every queued message of `peer` as failed; internal ones are retried later.
    fn fail_queue(&mut self, peer: &mut Peer, reason: FailReason, now: Millis) {
        peer.requeue_in_flight();
        let msgs: Vec<Outgoing> = peer.queue.drain(..).collect();
        for m in msgs {
            self.fail_message(peer, m, reason, now);
        }
    }

    /// One message failed: report it, or, for an internal one, try again later (1, 2, 4, 8,
    /// 16 minutes). Membership datagrams then go on every hour for a day: a member who misses
    /// a state keeps a removed member, one who misses a key loses a sender (review of
    /// 2026-11-15, SR-12).
    fn fail_message(&mut self, peer: &mut Peer, mut m: Outgoing, reason: FailReason, now: Millis) {
        let membership = matches!(
            m.datagram.first(),
            Some(&(datagram::GROUP_STATE | datagram::SENDER_KEY | datagram::SENDER_KEYS))
        );
        if !m.internal {
            self.events.push_back(Event::Failed { id: m.id, reason });
        } else if self.is_location(m.id) {
            // A stale position is not worth resending: the share sends a fresh one.
            self.location_done(m.id, true);
        } else if m.tries < 5 || (membership && m.tries < MEMBERSHIP_TRIES) {
            m.not_before = now
                + if m.tries < 5 {
                    60_000 << m.tries
                } else {
                    3_600_000
                };
            m.tries += 1;
            m.retried = false;
            peer.queue.push_back(m);
        }
    }

    /// The radio is busy with a frame of `airtime_ms` until it has cleared the path (`hops`
    /// repeaters, `None` for a flood): the next data frame waits (§7.3).
    /// After a frame the peer answers at once (the last fragment of a round, HELLO, INTRO):
    /// our next data frame waits for the answer to come back along `path`. Otherwise it
    /// collides, at a repeater we share, with that answer (§7.4).
    fn expect_answer(&mut self, path: &OwnedPath) {
        let t_ack = self.radio.airtime_us(2 + path.bytes.len() + ACK_FRAME_MAX) / 1000;
        let k = match path.hops() {
            0 => 1,
            1 => 3,
            _ => 7,
        };
        self.next_slot += t_ack * k + 200;
    }

    /// Airtime of an on-air frame of `bytes` bytes, in milliseconds.
    pub(crate) fn airtime_ms(&self, bytes: usize) -> Millis {
        self.radio.airtime_us(bytes) / 1000
    }

    /// Counts a transmission the companion makes for us in the budget and the radio slot.
    pub(crate) fn record_airtime(&mut self, now: Millis, airtime_ms: Millis, hops: Option<u8>) {
        self.budget.record(now, airtime_ms);
        self.occupy(now, airtime_ms, hops);
    }

    /// Whether `airtime_ms` fits in the budget now.
    pub(crate) fn budget_allows(&mut self, now: Millis, airtime_ms: Millis) -> bool {
        self.budget.can_transmit(now, airtime_ms)
    }

    fn occupy(&mut self, now: Millis, airtime_ms: Millis, hops: Option<u8>) {
        let k = match hops {
            Some(0) => 1,
            Some(1) => 3,
            Some(_) => 7,
            None => 10,
        };
        self.next_slot = self.next_slot.max(now) + airtime_ms * k + 100;
    }

    // ----- per-peer engine -------------------------------------------------------------

    fn pump_peer(&mut self, k: PubKey, now: Millis) {
        let Some(mut peer) = self.peers.remove(&k) else {
            return;
        };
        self.drive_peer(&k, &mut peer, now);
        self.peers.insert(k, peer);
    }

    fn drive_peer(&mut self, k: &PubKey, peer: &mut Peer, now: Millis) {
        // HELLO retries.
        if let Some(init) = peer.initiating.as_mut()
            && now >= init.next_retry
        {
            if init.attempt + 1 >= HELLO_TRIES {
                peer.initiating = None;
                // Silent through every HELLO: the session kept meanwhile is lost too.
                if std::mem::take(&mut peer.suspect) {
                    peer.session = None;
                    peer.pending = None;
                }
                // It may have a new identity: introduce again next time (§4.6). When one
                // already waits for the user, that is why nothing answered.
                peer.intro_needed = true;
                // Nor did the path work: the next attempt finds one again (§4.5, SR-02).
                peer.path = None;
                let reason = if self.pending_identities.contains_key(k) {
                    FailReason::IdentityChanged
                } else {
                    FailReason::Unreachable
                };
                self.fail_queue(peer, reason, now);
            } else {
                init.attempt += 1;
                let init = init.clone();
                let next_retry = now + self.hello_backoff(&init.path);
                if let Some(i) = peer.initiating.as_mut() {
                    i.next_retry = next_retry;
                }
                self.send_hello(k, &init, now);
            }
        }
        // INTRO retries.
        if let Some(intro) = peer.introducing.as_mut()
            && now >= intro.next_retry
        {
            if intro.attempt + 1 >= HELLO_TRIES {
                peer.introducing = None;
                peer.path = None;
                if std::mem::take(&mut peer.suspect) {
                    peer.session = None;
                    peer.pending = None;
                }
                self.fail_queue(peer, FailReason::Unreachable, now);
            } else {
                intro.attempt += 1;
                let intro = intro.clone();
                let next_retry = now + self.hello_backoff(&intro.path);
                if let Some(i) = peer.introducing.as_mut() {
                    i.next_retry = next_retry;
                }
                self.send_intro(k, &intro, now);
            }
        }
        // INTRO_FIN resends, until the responder answers under the new session; only while
        // our data waits for it (§4.6): with nothing to send, the responder has nothing to
        // answer and the resends would only cost airtime.
        if let Some(fin) = peer.fin.as_mut()
            && now >= fin.next_retry
            && peer.in_flight.is_some()
        {
            if fin.attempt + 1 >= FIN_TRIES {
                peer.fin = None;
            } else {
                fin.attempt += 1;
                let fin = fin.clone();
                let next_retry = now + self.hello_backoff(&fin.path);
                if let Some(f) = peer.fin.as_mut() {
                    f.next_retry = next_retry;
                }
                self.send_fin(k, &fin, now);
            }
        }
        if peer
            .answered_intro
            .as_ref()
            .is_some_and(|a| now >= a.at + INTRO_MEMORY_MS)
        {
            peer.answered_intro = None;
        }

        let Some(path) = self.path_for(k, peer) else {
            if !peer.has_work(now) {
                peer.probing = None;
                return;
            }
            match peer.probing.as_ref().map(|p| (p.tries, now >= p.next_try)) {
                None => self.probe(k, peer, 1, now),
                Some((tries, true)) if tries >= PROBE_TRIES => {
                    peer.probing = None;
                    self.fail_queue(peer, FailReason::NoPath, now);
                }
                Some((tries, true)) => self.probe(k, peer, tries + 1, now),
                Some((_, false)) => {}
            }
            return;
        };
        peer.probing = None;

        if peer.session.is_none() && peer.pending.is_none() {
            if peer.has_work(now) && peer.initiating.is_none() && peer.introducing.is_none() {
                self.begin_session(k, peer, path, now);
            }
            return;
        }

        let timing = self.jittered(self.timing_for(&path), path.bytes.len());
        let chunk = chunk_for_path(path.bytes.len());
        let Some(session) = peer.active() else {
            return;
        };
        session.set_timing(timing);

        // Next datagram.
        if peer.in_flight.is_none()
            && let Some(i) = peer.queue.iter().position(|m| m.not_before <= now)
            && let Some(msg) = peer.queue.remove(i)
        {
            // Old or worn session: renew it first, like an exhausted tag (§4.4).
            if now.saturating_sub(peer.session_since) >= self.session_max_age
                || peer.session_bytes >= self.session_max_bytes
            {
                peer.queue.push_front(msg);
                peer.session = None;
                peer.pending = None;
                return;
            }
            let len = msg.datagram.len() as u64;
            let Some(session) = peer.active() else {
                return;
            };
            match session.send(&msg.datagram, chunk, now) {
                Ok(tag) => {
                    peer.session_bytes += len;
                    peer.in_flight = Some(InFlight {
                        msg,
                        tag,
                        sent: 0,
                        on_pending: peer.session.is_none(),
                    });
                }
                Err(SendError::TooLarge) => {
                    if !msg.internal {
                        self.events.push_back(Event::Failed {
                            id: msg.id,
                            reason: FailReason::TooLarge,
                        });
                    }
                }
                Err(SendError::Exhausted) => {
                    peer.queue.push_front(msg);
                    peer.session = None;
                    peer.pending = None;
                    return;
                }
                Err(_) => peer.queue.push_front(msg),
            }
        }

        // Due frames. ACKs go at once; data takes the shared radio slot, within the budget.
        let mut buf = [0u8; FRAME_MAX];
        while let Some(len) = peer.active().and_then(|s| s.poll_ack(now, &mut buf)) {
            let frame = buf.get(..len).unwrap_or(&[]).to_vec();
            self.send_raw(&path, &frame, now);
        }
        let worst = self.radio.airtime_us(2 + path.bytes.len() + FRAME_MAX) / 1000;
        if now >= self.next_slot {
            if !self.budget.can_transmit(now, worst) {
                self.deferred_until = Some(self.budget.next_opportunity(now, worst));
            } else if let Some(len) = peer.active().and_then(|s| s.poll_data(now, &mut buf)) {
                let frame = buf.get(..len).unwrap_or(&[]).to_vec();
                if let Ok((h, _)) = Header::decode(&frame)
                    && h.kind == Kind::Data
                    && let Some(f) = peer.in_flight.as_mut()
                    && f.tag == h.tag
                {
                    f.sent += 1;
                    f.msg.started = true;
                    if !f.msg.internal {
                        self.events.push_back(Event::Progress {
                            id: f.msg.id,
                            sent: f.sent,
                            total: h.total,
                        });
                    }
                }
                self.send_raw(&path, &frame, now);
                if peer.active().is_some_and(|s| s.awaiting_ack()) {
                    self.expect_answer(&path);
                }
                self.rr_last = Some(*k);
            }
        }

        // Session events.
        while let Some(e) = peer.active().and_then(Session::take_event) {
            let finished = match e {
                SessionEvent::Delivered { tag } => Some((tag, None)),
                SessionEvent::Failed { tag } => Some((tag, Some(FailReason::Timeout))),
                SessionEvent::PeerAborted { tag, .. } => Some((tag, Some(FailReason::Refused))),
                SessionEvent::ReceiveFailed { .. } => None,
            };
            if let Some((tag, failure)) = finished
                && peer.in_flight.as_ref().is_some_and(|f| f.tag == tag)
                && let Some(f) = peer.in_flight.take()
            {
                match failure {
                    None if !f.msg.internal => {
                        self.events.push_back(Event::Delivered { id: f.msg.id });
                    }
                    None => self.location_done(f.msg.id, false),
                    // The new handshake will tell: sent again under the new session.
                    Some(FailReason::Timeout) if peer.suspect => peer.queue.push_front(f.msg),
                    Some(reason) => self.fail_message(peer, f.msg, reason, now),
                }
            }
        }

        // The peer may have lost the session (restart): a new handshake (ADR-0006). The session
        // stays meanwhile: on a busy repeater the frames were more likely lost than the session,
        // and an ACK that still arrives under it ends the handshake.
        let silent = peer
            .active()
            .and_then(|s| s.in_flight())
            .filter(|&(_, _, acked)| !acked)
            .map(|(_, rounds, _)| rounds);
        if silent.is_some_and(|r| r >= SILENT_ROUNDS_BEFORE_REHANDSHAKE)
            && peer.initiating.is_none()
            && peer.introducing.is_none()
            && let Some(f) = peer.in_flight.as_mut()
            && !f.msg.retried
        {
            f.msg.retried = true;
            peer.suspect = true;
            self.begin_session(k, peer, path, now);
        }
    }

    /// A HELLO when we know the peer's identity, an introduction otherwise (§4.6). While
    /// another identity waits for the user, the known one is still the one we write to, by
    /// HELLO, which only it can answer: an introduction would be answered by whoever sent the
    /// other identity.
    fn begin_session(&mut self, k: &PubKey, peer: &mut Peer, path: OwnedPath, now: Millis) {
        let waiting = self.pending_identities.contains_key(k);
        if self.identity_of(k).is_some() && (waiting || !peer.intro_needed) {
            self.start_handshake(k, peer, path, now);
        } else {
            self.start_intro(k, peer, path, now);
        }
    }

    fn start_intro(&mut self, k: &PubKey, peer: &mut Peer, path: OwnedPath, now: Millis) {
        let mut salt = [0u8; SALT_LEN];
        self.rng.fill_bytes(&mut salt);
        let mut eph_secret = [0u8; 32];
        self.rng.fill_bytes(&mut eph_secret);
        let intro = Introducing {
            tag: self.rng.next_u32() as u16,
            salt,
            eph_secret,
            attempt: 0,
            next_retry: now + self.hello_backoff(&path),
            path,
        };
        self.send_intro(k, &intro, now);
        peer.initiating = None;
        peer.introducing = Some(intro);
    }

    fn send_intro(&mut self, k: &PubKey, intro: &Introducing, now: Millis) {
        let Some(node) = self.node else {
            return;
        };
        let back = intro.path.reversed();
        let msg = intro_message(intro, &back);
        let mut buf = [0u8; FRAME_MAX];
        if let Ok(n) = intro::seal_intro(node[0], k[0], &msg, &mut buf) {
            let frame = buf.get(..n).unwrap_or(&[]).to_vec();
            self.send_raw(&intro.path, &frame, now);
            self.expect_answer(&intro.path);
        }
    }

    fn send_fin(&mut self, k: &PubKey, fin: &Fin, now: Millis) {
        let (Some(me), Some(node), Some(prv)) = (self.key, self.node, self.prv) else {
            return;
        };
        let mut buf = [0u8; FRAME_MAX];
        if let Ok(n) = intro::seal_intro_fin(
            node[0],
            k[0],
            &prv,
            &me,
            &fin.eph_secret,
            &fin.salt_i,
            &fin.intro_hash,
            &fin.ack,
            fin.attempt,
            &mut buf,
        ) {
            let frame = buf.get(..n).unwrap_or(&[]).to_vec();
            self.send_raw(&fin.path, &frame, now);
        }
    }

    /// The identity behind contact `node` is `identity` (§4.6): the first one seen is kept
    /// (unverified), the same one is confirmed. An identity already known is **never replaced
    /// here**, verified or not: another one waits for the user, and this returns false (no
    /// session with it). Anyone can send an introduction under a node's hash byte.
    pub(crate) fn learn_identity(&mut self, node: &PubKey, identity: PubKey) -> bool {
        let Some(c) = self.contacts.get_mut(node) else {
            return false;
        };
        let known = c.identity;
        if known == Some(identity) {
            return true;
        }
        if known.is_some() {
            if self.pending_identities.insert(*node, identity) != Some(identity) {
                self.events.push_back(Event::IdentityConflict {
                    node: *node,
                    identity,
                });
            }
            return false;
        }
        c.identity = Some(identity);
        c.verified = false;
        self.secrets.remove(node);
        self.events.push_back(Event::IdentityLearned {
            node: *node,
            identity,
            changed: false,
        });
        true
    }

    /// Floods a path discovery PROBE for `k` (§4.5): keyed by our identities when we know the
    /// peer's, anonymous otherwise.
    fn probe(&mut self, k: &PubKey, peer: &mut Peer, tries: u8, now: Millis) {
        let (Some(me), Some(node)) = (self.key, self.node) else {
            return;
        };
        let mut id: ProbeId = [0; 8];
        self.rng.fill_bytes(&mut id);
        // The peer may not know our identity even when we know theirs (a card or an
        // invitation scanned one way, a reinstalled app): only a first probe is keyed (§4.5).
        let keyed = tries == 1 && !peer.unknown_to_peer;
        let secret = self.secret(k).filter(|_| keyed);
        let peer_id = self.identity_of(k).filter(|_| keyed);
        let payload = match (secret, peer_id) {
            (Some(s), Some(pid)) => discovery::build_probe(
                &Keying {
                    s: &s,
                    anonymous: false,
                    prober: &me,
                    target: &pid,
                },
                node[0],
                k[0],
                &id,
            ),
            _ => discovery::build_probe(&Keying::anonymous(&node, k), node[0], k[0], &id),
        };
        let anonymous = secret.is_none() || peer_id.is_none();
        let (route, transport) = match &self.scope {
            Some(key) => (
                Route::TransportFlood,
                Some([transport_code(key, PayloadType::GrpData, &payload), 0]),
            ),
            None => (Route::Flood, None),
        };
        let packet = Packet {
            route,
            ptype: PayloadType::GrpData,
            transport,
            path: Path::EMPTY,
            payload: &payload,
        };
        let mut raw = [0u8; 64];
        if let Ok(n) = packet.encode(&mut raw) {
            let raw = raw.get(..n).unwrap_or(&[]).to_vec();
            self.send_raw_packet(&raw, now);
        }
        let wait = PROBE_WAIT_MS + Millis::from(self.rng.next_u32()) % PROBE_WAIT_MS;
        peer.probing = Some(Probing {
            id,
            tries,
            next_try: now + wait,
            anonymous,
        });
    }

    fn start_handshake(&mut self, k: &PubKey, peer: &mut Peer, path: OwnedPath, now: Millis) {
        // The salt starts with our Unix time: the responder answers only a fresh HELLO (§4.4).
        let mut random = [0u8; 12];
        self.rng.fill_bytes(&mut random);
        let t = self
            .unix_s(now)
            .and_then(|t| u32::try_from(t).ok())
            .unwrap_or(0);
        let salt = handshake::hello_salt(t, &random);
        let mut eph_secret = [0u8; 32];
        self.rng.fill_bytes(&mut eph_secret);
        let tag = self.rng.next_u32() as u16;
        let next_retry = now + self.hello_backoff(&path);
        let init = Initiating {
            tag,
            salt,
            eph_secret,
            attempt: 0,
            next_retry,
            path,
        };
        self.send_hello(k, &init, now);
        peer.initiating = Some(init);
    }

    fn send_hello(&mut self, k: &PubKey, init: &Initiating, now: Millis) {
        let (Some(me), Some(node), Some(s), Some(pid)) =
            (self.key, self.node, self.secret(k), self.identity_of(k))
        else {
            return;
        };
        let ends = Ends {
            me: &me,
            peer: &pid,
            me_node: node[0],
            peer_node: k[0],
        };
        let back = init.path.reversed();
        let hello = Hello {
            tag: init.tag,
            attempt: init.attempt,
            salt: init.salt,
            ephemeral: x25519_public(&init.eph_secret),
            version: handshake::VERSION,
            return_path_len: back.len_byte,
            return_path: &back.bytes,
        };
        let mut buf = [0u8; FRAME_MAX];
        if let Ok(n) = handshake::seal_hello(&s, &ends, &init.eph_secret, &hello, &mut buf) {
            let frame = buf.get(..n).unwrap_or(&[]).to_vec();
            self.send_raw(&init.path, &frame, now);
            self.expect_answer(&init.path);
        }
    }

    // ----- incoming frames -------------------------------------------------------------

    fn on_raw(&mut self, payload: &[u8], snr4: i8, rssi: i8, now: Millis) {
        let Ok((h, _)) = Header::decode(payload) else {
            return;
        };
        let Some(node) = self.node else {
            return;
        };
        if h.dst != node[0] {
            return;
        }
        match (h.kind, h.index) {
            (Kind::Control, 0) => self.on_hello(&h, payload, now),
            (Kind::Control, 1) => self.on_hello_ack(&h, payload, now),
            (Kind::Control, 2) => self.on_path_reply(&h, payload, now),
            (Kind::Control, 3) => self.on_intro(payload, now),
            (Kind::Control, 4) => self.on_intro_ack(&h, payload, now),
            (Kind::Control, _) => self.on_intro_fin(&h, payload, now),
            _ => self.on_session_frame(&h, payload, snr4, rssi, now),
        }
    }

    /// A peer's handshake showed its protocol version (ADR-0029): a newer one is reported once.
    fn peer_version(&mut self, node: &PubKey, version: u8) {
        if version > handshake::VERSION && self.newer.insert(*node) {
            self.events.push_back(Event::NewerVersion {
                node: *node,
                version,
            });
        }
    }

    /// Whether we may answer a frame anyone can send, from `node` (SR-04); counted when so.
    fn unprompted(&mut self, node: &PubKey, now: Millis) -> bool {
        self.unprompted
            .retain(|(_, at)| at + UNPROMPTED_TOTAL_MS > now);
        let mine = self
            .unprompted
            .iter()
            .filter(|(n, at)| n == node && at + UNPROMPTED_PER_NODE_MS > now)
            .count();
        if mine >= UNPROMPTED_PER_NODE || self.unprompted.len() >= UNPROMPTED_TOTAL {
            return false;
        }
        self.unprompted.push_back((*node, now));
        true
    }

    /// The contacts a frame from hash byte `src` may come from: people only (repeaters, rooms
    /// and sensors never run lichen, §4.6), in key order.
    fn candidates(&self, src: u8) -> Vec<PubKey> {
        let mut v: Vec<PubKey> = self
            .contacts
            .iter()
            .filter(|(k, c)| k[0] == src && c.is_person())
            .map(|(k, _)| *k)
            .collect();
        v.sort_unstable();
        v
    }

    /// Every frame the companion hears: we look for path discovery probes addressed to us
    /// (§4.5) and group messages (§10.3).
    fn on_log_rx(&mut self, raw: &[u8], snr4: i8, rssi: i8, now: Millis) {
        let (Some(me), Some(node)) = (self.key, self.node) else {
            return;
        };
        let Ok(p) = Packet::decode(raw) else {
            return;
        };
        if p.ptype != PayloadType::GrpData
            || !matches!(p.route, Route::Flood | Route::TransportFlood)
        {
            return;
        }
        let Some(probe) = discovery::parse_probe(p.payload) else {
            // Heard directly: a reception of the sender, for search (§11.2).
            if p.path.hops() == 0 {
                self.search_direct(p.payload, snr4, rssi, now);
            }
            self.on_group_envelope(p.payload, snr4, rssi, now);
            return;
        };
        if probe.dst != node[0] || self.seen_probes.contains(&probe.id) {
            return;
        }
        for c in self.candidates(probe.src) {
            // Keyed by our identities when we know the prober's, else anonymous (§4.5).
            let secret = self.secret(&c);
            let pid = self.identity_of(&c);
            let keyed = match (&secret, &pid) {
                (Some(s), Some(pid)) => Some(Keying {
                    s,
                    anonymous: false,
                    prober: pid,
                    target: &me,
                }),
                _ => None,
            };
            let anon = Keying::anonymous(&c, &node);
            let Some(keying) = [keyed, Some(anon)]
                .into_iter()
                .flatten()
                .find(|k| discovery::verify_probe(k, c[0], node[0], p.payload))
            else {
                continue;
            };
            if self.seen_probes.len() >= 64 {
                self.seen_probes.pop_front();
            }
            self.seen_probes.push_back(probe.id);
            let forward = OwnedPath::from_path(p.path);
            let back = forward.reversed();
            let mut buf = [0u8; FRAME_MAX];
            // Fresh for every reply: a probe answered twice never reuses a key (§4.5).
            let mut r = [0u8; 8];
            self.rng.fill_bytes(&mut r);
            let anonymous = keying.anonymous;
            if !self.unprompted(&c, now) {
                return;
            }
            if let Ok(n) = discovery::seal_path_reply(
                &keying,
                node[0],
                c[0],
                &probe.id,
                &r,
                forward.len_byte,
                &forward.bytes,
                &mut buf,
            ) {
                let frame = buf.get(..n).unwrap_or(&[]).to_vec();
                self.send_raw(&back, &frame, now);
            }
            // An anonymous probe is keyed by public node keys: anyone can send one. It is
            // answered along its path, but that path does not replace ours (SR-02).
            if !anonymous {
                self.peers.entry(c).or_default().path = Some(back);
            }
            return;
        }
    }

    fn on_path_reply(&mut self, h: &Header, payload: &[u8], now: Millis) {
        let (Some(me), Some(node)) = (self.key, self.node) else {
            return;
        };
        for c in self.candidates(h.src) {
            let Some(probing) = self.peers.get(&c).and_then(|p| p.probing.clone()) else {
                continue;
            };
            if u16::from_le_bytes([probing.id[0], probing.id[1]]) != h.tag {
                continue;
            }
            let secret = self.secret(&c).unwrap_or([0; 32]);
            let pid = self.identity_of(&c);
            let keying = match (probing.anonymous, &pid) {
                (false, Some(pid)) => Keying {
                    s: &secret,
                    anonymous: false,
                    prober: &me,
                    target: pid,
                },
                _ => Keying::anonymous(&node, &c),
            };
            let mut buf = payload.to_vec();
            let Ok((id, len_byte, path)) =
                discovery::open_path_reply(&keying, node[0], c[0], &mut buf)
            else {
                continue;
            };
            if id != probing.id {
                continue;
            }
            let path = OwnedPath {
                len_byte,
                bytes: path.to_vec(),
            };
            if let Some(p) = self.peers.get_mut(&c) {
                p.probing = None;
                p.path = Some(path);
            }
            self.tick(now);
            return;
        }
    }

    /// An INTRO (§4.6): anyone may send one; we answer with our identity, proven.
    fn on_intro(&mut self, payload: &[u8], now: Millis) {
        let (Some(me), Some(node), Some(prv)) = (self.key, self.node, self.prv) else {
            return;
        };
        let Ok((src, msg)) = intro::open_intro(node[0], payload) else {
            return;
        };
        // INTRO names its sender by one byte only: among the people sharing it, the one whose
        // known path is the return path, else the first (§4.6).
        let candidates = self.candidates(src);
        let on_path = candidates.iter().find(|k| {
            self.contacts
                .get(*k)
                .and_then(|c| c.out_path.as_ref())
                .is_some_and(|p| {
                    p.len_byte == msg.return_path_len && p.bytes.as_slice() == msg.return_path
                })
        });
        let Some(c) = on_path.or(candidates.first()).copied() else {
            return;
        };
        let mut peer = self.peers.remove(&c).unwrap_or_default();
        // Crossing introductions: the smaller node key's wins (§4.4). A HELLO of ours cannot
        // succeed: they do not know us.
        if peer.introducing.is_some() && node < c {
            self.peers.insert(c, peer);
            return;
        }
        peer.introducing = None;
        peer.initiating = None;
        let return_path = (msg.return_path_len != PATH_UNKNOWN).then(|| OwnedPath {
            len_byte: msg.return_path_len,
            bytes: msg.return_path.to_vec(),
        });
        let Some(path) = return_path.or_else(|| self.path_for(&c, &peer)) else {
            self.peers.insert(c, peer);
            return;
        };
        let answered = match &peer.answered_intro {
            Some(a) if a.tag == msg.tag && a.answer.salt_i == msg.salt => a.clone(),
            _ => {
                let mut salt_r = [0u8; SALT_LEN];
                self.rng.fill_bytes(&mut salt_r);
                let mut eph_secret = [0u8; 32];
                self.rng.fill_bytes(&mut eph_secret);
                AnsweredIntro {
                    tag: msg.tag,
                    answer: Answer {
                        salt_i: msg.salt,
                        e_i: msg.ephemeral,
                        salt_r,
                        eph_secret,
                        intro_hash: intro::intro_hash(&msg),
                    },
                    path: path.clone(),
                    at: now,
                    done: false,
                }
            }
        };
        if !self.unprompted(&c, now) {
            self.peers.insert(c, peer);
            return;
        }
        let mut buf = [0u8; FRAME_MAX];
        if let Ok(n) = intro::seal_intro_ack(
            node[0],
            c[0],
            &prv,
            &me,
            &answered.answer,
            msg.tag,
            msg.attempt,
            &mut buf,
        ) {
            let frame = buf.get(..n).unwrap_or(&[]).to_vec();
            self.send_raw(&path, &frame, now);
        }
        // Anyone can send an INTRO under a contact's hash byte: its return path is only
        // adopted with INTRO_FIN (SR-02).
        let mut answered = answered;
        answered.path = path;
        peer.answered_intro = Some(answered);
        self.peers.insert(c, peer);
    }

    /// The INTRO_ACK to our introduction: the peer's identity, proven; the session starts.
    fn on_intro_ack(&mut self, h: &Header, payload: &[u8], now: Millis) {
        let (Some(me), Some(node), Some(prv)) = (self.key, self.node, self.prv) else {
            return;
        };
        for c in self.candidates(h.src) {
            let Some(intro) = self
                .peers
                .get(&c)
                .and_then(|p| p.introducing.clone())
                .filter(|i| i.tag == h.tag)
            else {
                continue;
            };
            // The INTRO we sent, as the responder hashed it (ADR-0029).
            let back = intro.path.reversed();
            let hash = intro::intro_hash(&intro_message(&intro, &back));
            let Ok(ack) = intro::open_intro_ack(
                node[0],
                c[0],
                &intro.eph_secret,
                &intro.salt,
                &hash,
                payload,
            ) else {
                continue;
            };
            let Some(keys) =
                intro::initiator_keys(&prv, &me, &intro.eph_secret, &intro.salt, &hash, &ack)
            else {
                return;
            };
            self.peer_version(&c, ack.version);
            if !self.learn_identity(&c, ack.identity) {
                // The contact is known under another identity: no session with this one (§4.6).
                if let Some(mut p) = self.peers.remove(&c) {
                    p.introducing = None;
                    self.fail_queue(&mut p, FailReason::IdentityChanged, now);
                    self.peers.insert(c, p);
                }
                return;
            }
            let session = Session::new(keys, node[0], c[0], self.timing_for(&intro.path));
            let fin = Fin {
                salt_i: intro.salt,
                intro_hash: hash,
                eph_secret: intro.eph_secret,
                ack,
                attempt: 0,
                next_retry: now + self.hello_backoff(&intro.path),
                path: intro.path.clone(),
            };
            self.send_fin(&c, &fin, now);
            if let Some(p) = self.peers.get_mut(&c) {
                p.requeue_in_flight();
                p.session = Some(Box::new(session));
                p.session_since = now;
                p.session_bytes = 0;
                p.pending = None;
                p.path = Some(intro.path);
                p.introducing = None;
                p.intro_needed = false;
                p.unknown_to_peer = false;
                p.suspect = false;
                p.fin = Some(fin);
            }
            self.events.push_back(Event::SessionEstablished { peer: c });
            self.tick(now);
            return;
        }
    }

    /// The INTRO_FIN of an introduction we answered: the initiator's identity, proven; its
    /// session is pending until its first message (§4.4).
    fn on_intro_fin(&mut self, h: &Header, payload: &[u8], now: Millis) {
        let (Some(me), Some(node), Some(prv)) = (self.key, self.node, self.prv) else {
            return;
        };
        for c in self.candidates(h.src) {
            let Some(answered) = self
                .peers
                .get(&c)
                .and_then(|p| p.answered_intro.clone())
                .filter(|a| a.tag == h.tag)
            else {
                continue;
            };
            let Ok(id_i) =
                intro::open_intro_fin(node[0], c[0], &prv, &me, &answered.answer, payload)
            else {
                continue;
            };
            if answered.done {
                return;
            }
            let Some(keys) = intro::responder_keys(&prv, &me, &answered.answer, &id_i) else {
                return;
            };
            if !self.learn_identity(&c, id_i) {
                // The contact is known under another identity: no session with this one (§4.6).
                return;
            }
            let Some(mut peer) = self.peers.remove(&c) else {
                return;
            };
            let path = answered.path.clone();
            peer.path = Some(path.clone());
            peer.replace_pending(Session::new(keys, node[0], c[0], self.timing_for(&path)));
            peer.intro_needed = false;
            if let Some(a) = peer.answered_intro.as_mut() {
                a.done = true;
            }
            self.peers.insert(c, peer);
            self.tick(now);
            return;
        }
    }

    fn on_hello(&mut self, h: &Header, payload: &[u8], now: Millis) {
        let (Some(me), Some(node), Some(prv)) = (self.key, self.node, self.prv) else {
            return;
        };
        for c in self.candidates(h.src) {
            let (Some(s), Some(pid)) = (self.secret(&c), self.identity_of(&c)) else {
                continue;
            };
            let ends = Ends {
                me: &me,
                peer: &pid,
                me_node: node[0],
                peer_node: c[0],
            };
            let mut buf = payload.to_vec();
            let Ok(hello) = handshake::open_hello(&s, &ends, &prv, &mut buf) else {
                continue;
            };
            // Freshness (§4.4, review of 2026-11-15, SR-03): a HELLO carries its sender's
            // time. One more than ten minutes away from ours is a replay, or a clock gone
            // wrong, and gets no answer. Its resends within those minutes get the same one.
            let t = u64::from(handshake::salt_time(&hello.salt));
            if t != 0
                && self
                    .unix_s(now)
                    .is_some_and(|ours| ours.abs_diff(t) > HELLO_FRESH_S)
            {
                return;
            }
            self.peer_version(&c, hello.version);
            let mut peer = self.peers.remove(&c).unwrap_or_default();
            self.answer_hello(&c, &mut peer, &s, &hello, now);
            self.peers.insert(c, peer);
            return;
        }
        // A HELLO we cannot open, from a contact whose identity we do not know (they scanned
        // our card, we did not scan theirs): introduce ourselves (§4.6).
        if let Some(c) = self
            .candidates(h.src)
            .into_iter()
            .find(|c| self.identity_of(c).is_none())
        {
            let mut peer = self.peers.remove(&c).unwrap_or_default();
            if peer.introducing.is_none() && now >= peer.intro_poke && self.unprompted(&c, now) {
                peer.intro_poke = now + INTRO_POKE_MS;
                if let Some(path) = self.path_for(&c, &peer) {
                    self.start_intro(&c, &mut peer, path, now);
                }
            }
            self.peers.insert(c, peer);
        }
    }

    fn answer_hello(
        &mut self,
        c: &PubKey,
        peer: &mut Peer,
        s: &[u8; 32],
        hello: &Hello<'_>,
        now: Millis,
    ) {
        let (Some(me), Some(node), Some(pid), Some(prv)) =
            (self.key, self.node, self.identity_of(c), self.prv)
        else {
            return;
        };
        // Crossing HELLOs: the smaller node key's handshake wins (§4.4).
        if peer.initiating.is_some() {
            if node < *c {
                return;
            }
            peer.initiating = None;
        }
        let return_path = (hello.return_path_len != PATH_UNKNOWN).then(|| OwnedPath {
            len_byte: hello.return_path_len,
            bytes: hello.return_path.to_vec(),
        });
        let Some(path) = return_path.or_else(|| self.path_for(c, peer)) else {
            return;
        };
        // A recorded HELLO can be replayed by anyone: its answers are limited too (SR-04).
        if !self.unprompted(c, now) {
            return;
        }
        let (answered, fresh) = match &peer.answered {
            Some(a) if a.tag == hello.tag && a.salt_i == hello.salt => (a.clone(), false),
            _ => {
                let mut eph_secret = [0u8; 32];
                self.rng.fill_bytes(&mut eph_secret);
                let mut salt_r = [0u8; SALT_LEN];
                self.rng.fill_bytes(&mut salt_r);
                let a = Answered {
                    tag: hello.tag,
                    salt_i: hello.salt,
                    salt_r,
                    eph_public: x25519_public(&eph_secret),
                    eph_secret,
                };
                (a, true)
            }
        };
        let ack = HelloAck {
            tag: hello.tag,
            attempt: hello.attempt,
            salt: answered.salt_r,
            ephemeral: answered.eph_public,
            version: handshake::VERSION,
        };
        let ends = Ends {
            me: &me,
            peer: &pid,
            me_node: node[0],
            peer_node: c[0],
        };
        let mut buf = [0u8; FRAME_MAX];
        let Ok((n, keys)) =
            handshake::seal_hello_ack(s, &ends, &prv, &answered.eph_secret, hello, &ack, &mut buf)
        else {
            return;
        };
        if fresh {
            let session = Session::new(keys, node[0], c[0], self.timing_for(&path));
            peer.replace_pending(session);
            peer.session_since = now;
            peer.session_bytes = 0;
            peer.path = Some(path.clone());
            // We know each other: our own introduction is no longer needed (§4.6).
            peer.introducing = None;
            peer.answered = Some(answered);
        }
        let frame = buf.get(..n).unwrap_or(&[]).to_vec();
        self.send_raw(&path, &frame, now);
    }

    fn on_hello_ack(&mut self, h: &Header, payload: &[u8], now: Millis) {
        let (Some(me), Some(node), Some(prv)) = (self.key, self.node, self.prv) else {
            return;
        };
        for c in self.candidates(h.src) {
            let Some(init) = self
                .peers
                .get(&c)
                .and_then(|p| p.initiating.clone())
                .filter(|i| i.tag == h.tag)
            else {
                continue;
            };
            let (Some(s), Some(pid)) = (self.secret(&c), self.identity_of(&c)) else {
                continue;
            };
            let ends = Ends {
                me: &me,
                peer: &pid,
                me_node: node[0],
                peer_node: c[0],
            };
            let mut buf = payload.to_vec();
            // The HELLO we sent, as the responder's transcript has it (§4.3).
            let back = init.path.reversed();
            let sent = Hello {
                tag: init.tag,
                attempt: init.attempt,
                salt: init.salt,
                ephemeral: x25519_public(&init.eph_secret),
                version: handshake::VERSION,
                return_path_len: back.len_byte,
                return_path: &back.bytes,
            };
            let Ok((ack, keys)) =
                handshake::open_hello_ack(&s, &ends, &prv, &init.eph_secret, &sent, &mut buf)
            else {
                continue;
            };
            self.peer_version(&c, ack.version);
            let session = Session::new(keys, node[0], c[0], self.timing_for(&init.path));
            if let Some(p) = self.peers.get_mut(&c) {
                p.requeue_in_flight();
                p.session = Some(Box::new(session));
                p.session_since = now;
                p.session_bytes = 0;
                p.pending = None;
                p.path = Some(init.path);
                p.initiating = None;
                p.introducing = None;
                // The identity we know answered: no need to introduce ourselves again.
                p.intro_needed = false;
                p.unknown_to_peer = false;
                p.suspect = false;
            }
            self.events.push_back(Event::SessionEstablished { peer: c });
            self.tick(now);
            return;
        }
    }

    fn on_session_frame(&mut self, h: &Header, payload: &[u8], snr4: i8, rssi: i8, now: Millis) {
        for c in self.candidates(h.src) {
            let Some(mut peer) = self.peers.remove(&c) else {
                continue;
            };
            let mut buf = payload.to_vec();
            // (what the session made of the frame, whether it was the pending session)
            let mut outcome: Option<(Handled, bool)> = None;
            if let Some(s) = peer.session.as_deref_mut() {
                match s.handle(&mut buf, now) {
                    Err(FrameError::Auth) => {}
                    r => outcome = Some((r.map(received_data), false)),
                }
            }
            if outcome.is_none()
                && let Some(s) = peer.pending.as_deref_mut()
            {
                match s.handle(&mut buf, now) {
                    Err(FrameError::Auth) => {}
                    r => outcome = Some((r.map(received_data), true)),
                }
            }
            if let Some((_, true)) = outcome {
                // The initiator confirmed the pending session (§4.4).
                peer.promote_pending();
                peer.unknown_to_peer = false;
                self.events.push_back(Event::SessionEstablished { peer: c });
            }
            if outcome.is_some() {
                // Something arrived under the session: our INTRO_FIN got through (§4.6).
                peer.fin = None;
            }
            if peer.suspect && matches!(outcome, Some((Ok(_), false))) {
                // The peer still has the session: no new handshake (ADR-0006).
                peer.suspect = false;
                peer.initiating = None;
                peer.introducing = None;
            }
            self.peers.insert(c, peer);
            if let Some((result, _)) = outcome {
                if let Ok(Some(datagram)) = &result
                    && matches!(
                        datagram.first(),
                        Some(
                            &lc_proto::content::GROUP_STATE
                                | &lc_proto::content::SENDER_KEY
                                | &lc_proto::content::GROUP_LEAVE
                                | &lc_proto::content::GROUP_JOIN
                                | &lc_proto::content::SENDER_KEYS
                                | &lc_proto::content::GROUP_MISSING
                                | &lc_proto::content::GROUP_RESEND
                        )
                    )
                {
                    self.on_group_control(&c, datagram, now);
                } else if let Ok(Some(datagram)) = result
                    // Local notices never come from the air (§6).
                    && !Content::is_local(&datagram)
                    && self.first_time(&c, &datagram, now)
                    && let Some(content) = Content::decode(&datagram)
                {
                    if let Content::Location(p) = content {
                        self.on_position(c, None, p, snr4, now);
                        return;
                    }
                    if let Content::MediaRequest { media_id, full } = content {
                        self.events.push_back(Event::MediaRequested {
                            from: c,
                            media_id,
                            full,
                        });
                        return;
                    }
                    self.events.push_back(Event::Received {
                        from: c,
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

impl Core {
    /// False when the same datagram came from `peer` within the last hour: a resend after a
    /// new handshake whose original was delivered but never acknowledged.
    fn first_time(&mut self, peer: &PubKey, datagram: &[u8], now: Millis) -> bool {
        let digest = Sha256::digest(datagram);
        let mut h = [0u8; 16];
        for (o, d) in h.iter_mut().zip(digest.iter()) {
            *o = *d;
        }
        let seen = self.recent.entry(*peer).or_default();
        while seen
            .front()
            .is_some_and(|&(_, t)| t + DUPLICATE_MEMORY_MS <= now)
        {
            seen.pop_front();
        }
        if seen.iter().any(|&(x, _)| x == h) {
            return false;
        }
        seen.push_back((h, now));
        true
    }
}

/// What a session made of a frame: the datagram it completed, if any.
type Handled = Result<Option<Vec<u8>>, FrameError>;

fn received_data(r: Received<'_>) -> Option<Vec<u8>> {
    match r {
        Received::Datagram { data, .. } => Some(data.to_vec()),
        Received::Nothing => None,
    }
}

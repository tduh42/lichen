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
use lc_meshcore::ecdh::{public_key, shared_secret};
use lc_meshcore::path::{PATH_UNKNOWN, Path};
use lc_meshcore::response::Response;
use lc_proto::discovery::{self, ProbeId};
use lc_proto::handshake::{self, Hello, HelloAck, SALT_LEN};
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

/// A MeshCore contact known to the companion (or added by hand).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contact {
    pub key: PubKey,
    pub name: String,
    pub out_path: Option<OwnedPath>,
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
    /// The node's private key is known (exported, or given in [`Config`]).
    IdentityReady,
    /// The companion refused `EXPORT_PRIVATE_KEY` (ADR-0004).
    KeyExportRefused,
    ContactsUpdated,
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
    /// Someone added us to a group.
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
    GroupMessage {
        gid: GroupId,
        from: PubKey,
        content: Content,
        snr4: i8,
        rssi: i8,
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
}

/// Why a request was refused right away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    UnknownContact,
    Empty,
    TooLarge,
    /// The node's key is not known yet.
    NoIdentity,
    UnknownGroup,
    /// Only the group admin may do this.
    NotAllowed,
    /// A medium whose airtime does not fit in the budget left; it would fit in `wait_ms`
    /// (§13.4).
    OverBudget {
        wait_ms: Millis,
    },
}

/// Core configuration.
#[derive(Clone, Debug)]
pub struct Config {
    /// Sent in `APP_START`.
    pub app_name: String,
    /// The node's private key when the app already has it (Keychain); otherwise it is
    /// exported from the companion.
    pub identity: Option<[u8; 64]>,
    /// Seed of the core's CSPRNG: 32 bytes from the OS random generator.
    pub rng_seed: [u8; 32],
    /// Regulatory profile (TOML).
    pub regulatory: String,
}

impl Config {
    pub fn new(app_name: &str, rng_seed: [u8; 32]) -> Self {
        Self {
            app_name: app_name.to_owned(),
            identity: None,
            rng_seed,
            regulatory: EU868.to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    SelfInfo,
    DeviceInfo,
    PrivateKey,
    Contacts,
    Contact,
    Ok,
    Scope,
}

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

/// A path discovery in progress (§4.5).
#[derive(Clone, Debug)]
struct Probing {
    id: ProbeId,
    tries: u8,
    next_try: Millis,
}

#[derive(Clone, Debug)]
struct Answered {
    tag: u16,
    salt_i: [u8; SALT_LEN],
    salt_r: [u8; SALT_LEN],
    eph_public: [u8; 32],
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
    pub(crate) prv: Option<[u8; 64]>,
    pub(crate) key: Option<PubKey>,
    pub(crate) name: String,
    freq_khz: u32,
    pub(crate) radio: LoRaParams,
    budget: Budget,
    pub(crate) contacts: HashMap<PubKey, Contact>,
    secrets: HashMap<PubKey, [u8; 32]>,
    peers: HashMap<PubKey, Peer>,
    commands: VecDeque<(Vec<u8>, Expect)>,
    awaiting: Option<(Expect, Millis)>,
    pub(crate) events: VecDeque<Event>,
    pub(crate) next_id: MsgId,
    deferred_until: Option<Millis>,
    /// Hashes of datagrams received recently, per peer, to drop resends (§6).
    recent: HashMap<PubKey, VecDeque<([u8; 16], Millis)>>,
    /// Key of the companion's default flood scope, for our floods' transport codes.
    pub(crate) scope: Option<[u8; 16]>,
    /// Probes already answered (a flood arrives several times, by several paths).
    seen_probes: VecDeque<ProbeId>,
    /// Groups we belong to (§10).
    pub(crate) groups: HashMap<GroupId, Group>,
    /// Sender keys that came before their group state.
    pub(crate) pending_keys: Vec<PendingKey>,
    /// Group envelopes waiting for their sender key.
    pub(crate) held: VecDeque<Held>,
    /// Floods per group message (§10.3).
    pub(crate) group_copies: u8,
    /// Groups changed since the app last persisted them.
    pub(crate) dirty_groups: HashSet<GroupId>,
    /// Group frames waiting for their time (the second fragment is spaced out).
    pub(crate) group_tx: VecDeque<(Millis, Vec<u8>)>,
    /// The radio is shared by every session: the next data frame may go at this time.
    pub(crate) next_slot: Millis,
    /// Peer served last, for round-robin between sessions.
    rr_last: Option<PubKey>,
    /// Location sharing (§12).
    pub(crate) loc: Locations,
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
            key: config.identity.map(|prv| public_key(&prv)),
            prv: config.identity,
            name: String::new(),
            freq_khz: 869_618,
            radio: DEFAULT_RADIO,
            budget,
            contacts: HashMap::new(),
            secrets: HashMap::new(),
            peers: HashMap::new(),
            commands: VecDeque::new(),
            awaiting: None,
            events: VecDeque::new(),
            next_id: 1,
            deferred_until: None,
            recent: HashMap::new(),
            scope: None,
            seen_probes: VecDeque::new(),
            groups: HashMap::new(),
            pending_keys: Vec::new(),
            held: VecDeque::new(),
            group_copies: crate::group::COPIES,
            dirty_groups: HashSet::new(),
            group_tx: VecDeque::new(),
            next_slot: 0,
            rr_last: None,
            loc: Locations::default(),
        }
    }

    // ----- accessors -------------------------------------------------------------------

    /// Our public key, once `SELF_INFO` (or the configured identity) gave it.
    pub const fn key(&self) -> Option<&PubKey> {
        self.key.as_ref()
    }

    pub const fn has_identity(&self) -> bool {
        self.prv.is_some()
    }

    /// The node's private key, for the app to store (Keychain / Keystore).
    pub const fn private_key(&self) -> Option<&[u8; 64]> {
        self.prv.as_ref()
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
        if self.prv.is_none() {
            self.command(&Command::ExportPrivateKey, Expect::PrivateKey);
        }
        self.command(&Command::GetContacts { since: None }, Expect::Contacts);
        self.tick(now);
    }

    /// Adds or updates a contact by hand (QR code, tests).
    pub fn add_contact(&mut self, key: PubKey, name: &str, out_path: Option<OwnedPath>) {
        self.contacts.insert(
            key,
            Contact {
                key,
                name: name.to_owned(),
                out_path,
            },
        );
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

    /// Random bytes from the core's generator (media ids).
    pub fn random_id(&mut self) -> crate::content::MediaId {
        let mut id = [0u8; 8];
        self.rng.fill_bytes(&mut id);
        id
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
            Response::NewAdvert(_) | Response::MsgWaiting | Response::SendConfirmed { .. } => {}
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
        if self.key.is_none() || self.prv.is_none() {
            return;
        }
        self.tick_groups(now);
        self.tick_locations(now);
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
        let mut consider = |x: Millis| t = Some(t.map_or(x, |y: Millis| y.min(x)));
        if !self.commands.is_empty() {
            consider(self.awaiting.map_or(0, |(_, d)| d));
        }
        if let Some(d) = self.deferred_until {
            consider(d);
        }
        if let Some(d) = self.next_group_wakeup() {
            consider(d);
        }
        if let Some(d) = self.next_location_wakeup() {
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

    fn command(&mut self, cmd: &Command<'_>, expect: Expect) {
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
                self.key = Some(*si.key);
                self.name = String::from_utf8_lossy(si.name).into_owned();
                self.freq_khz = si.freq_khz;
                self.radio = LoRaParams::meshcore(si.sf, si.bw_hz, si.cr).unwrap_or(DEFAULT_RADIO);
                if let Ok(b) =
                    Budget::for_frequency(&self.regulatory, u64::from(si.freq_khz) * 1000)
                {
                    self.budget = b;
                }
                if self.prv.is_some_and(|p| public_key(&p) != *si.key) {
                    // The configured identity belongs to another node: export again.
                    self.prv = None;
                    self.secrets.clear();
                    self.command(&Command::ExportPrivateKey, Expect::PrivateKey);
                }
                self.events.push_back(Event::Connected {
                    key: *si.key,
                    name: self.name.clone(),
                    freq_khz: si.freq_khz,
                    radio: self.radio,
                });
                if self.prv.is_some() {
                    self.events.push_back(Event::IdentityReady);
                }
            }
            Response::PrivateKey(k) => {
                if self.key.is_none_or(|key| public_key(k) == key) {
                    self.prv = Some(*k);
                    self.key = Some(public_key(k));
                    self.secrets.clear();
                    self.events.push_back(Event::IdentityReady);
                }
            }
            Response::Disabled | Response::Err(_) if expect == Some(Expect::PrivateKey) => {
                self.events.push_back(Event::KeyExportRefused);
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
                self.contacts.insert(
                    key,
                    Contact {
                        key,
                        name,
                        out_path,
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
            Response::EndOfContacts { .. } => self.events.push_back(Event::ContactsUpdated),
            Response::DefaultFloodScope { key, .. } => self.scope = key.copied(),
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

    fn secret(&mut self, peer: &PubKey) -> Option<[u8; 32]> {
        if let Some(s) = self.secrets.get(peer) {
            return Some(*s);
        }
        let s = shared_secret(self.prv.as_ref()?, peer)?;
        self.secrets.insert(*peer, s);
        Some(s)
    }

    fn timing_for(&self, path: &OwnedPath) -> Timing {
        let pb = path.bytes.len();
        let t_frame = self.radio.airtime_us(2 + pb + raw_payload_max(pb)) / 1000;
        let t_ack = self.radio.airtime_us(2 + pb + 24) / 1000;
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
    /// 16 minutes).
    fn fail_message(&mut self, peer: &mut Peer, mut m: Outgoing, reason: FailReason, now: Millis) {
        if !m.internal {
            self.events.push_back(Event::Failed { id: m.id, reason });
        } else if self.is_location(m.id) {
            // A stale position is not worth resending: the share sends a fresh one.
            self.location_done(m.id, true);
        } else if m.tries < 5 {
            m.not_before = now + (60_000 << m.tries);
            m.tries += 1;
            m.retried = false;
            peer.queue.push_back(m);
        }
    }

    /// The radio is busy with a frame of `airtime_ms` until it has cleared the path (`hops`
    /// repeaters, `None` for a flood): the next data frame waits (§7.3).
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
                self.fail_queue(peer, FailReason::Unreachable, now);
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
            if peer.has_work(now) && peer.initiating.is_none() {
                self.start_handshake(k, peer, path, now);
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
            let Some(session) = peer.active() else {
                return;
            };
            match session.send(&msg.datagram, chunk, now) {
                Ok(tag) => {
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
                    if !f.msg.internal {
                        self.events.push_back(Event::Progress {
                            id: f.msg.id,
                            sent: f.sent,
                            total: h.total,
                        });
                    }
                }
                self.send_raw(&path, &frame, now);
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
                    Some(reason) => self.fail_message(peer, f.msg, reason, now),
                }
            }
        }

        // The peer may have lost the session (restart): new handshake, then resend (ADR-0006).
        let silent = peer
            .active()
            .and_then(|s| s.in_flight())
            .filter(|&(_, _, acked)| !acked)
            .map(|(_, rounds, _)| rounds);
        if silent.is_some_and(|r| r >= SILENT_ROUNDS_BEFORE_REHANDSHAKE)
            && peer.in_flight.as_ref().is_some_and(|f| !f.msg.retried)
            && let Some(f) = peer.in_flight.take()
        {
            let mut msg = f.msg;
            msg.retried = true;
            peer.queue.push_front(msg);
            peer.session = None;
            peer.pending = None;
            self.start_handshake(k, peer, path, now);
        }
    }

    /// Floods a path discovery PROBE for `k` (§4.5).
    fn probe(&mut self, k: &PubKey, peer: &mut Peer, tries: u8, now: Millis) {
        let (Some(me), Some(s)) = (self.key, self.secret(k)) else {
            return;
        };
        let mut id: ProbeId = [0; 8];
        self.rng.fill_bytes(&mut id);
        let payload = discovery::build_probe(&s, &me, k, &id);
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
        });
    }

    fn start_handshake(&mut self, k: &PubKey, peer: &mut Peer, path: OwnedPath, now: Millis) {
        let mut salt = [0u8; SALT_LEN];
        self.rng.fill_bytes(&mut salt);
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
        let (Some(me), Some(s)) = (self.key, self.secret(k)) else {
            return;
        };
        let back = init.path.reversed();
        let hello = Hello {
            tag: init.tag,
            attempt: init.attempt,
            salt: init.salt,
            ephemeral: x25519_public(&init.eph_secret),
            return_path_len: back.len_byte,
            return_path: &back.bytes,
        };
        let mut buf = [0u8; FRAME_MAX];
        if let Ok(n) = handshake::seal_hello(&s, &me, k, &hello, &mut buf) {
            let frame = buf.get(..n).unwrap_or(&[]).to_vec();
            self.send_raw(&init.path, &frame, now);
        }
    }

    // ----- incoming frames -------------------------------------------------------------

    fn on_raw(&mut self, payload: &[u8], snr4: i8, rssi: i8, now: Millis) {
        let Ok((h, _)) = Header::decode(payload) else {
            return;
        };
        let Some(me) = self.key else {
            return;
        };
        if h.dst != me[0] || self.prv.is_none() {
            return;
        }
        match (h.kind, h.index) {
            (Kind::Control, 0) => self.on_hello(&h, payload, now),
            (Kind::Control, 1) => self.on_hello_ack(&h, payload, now),
            (Kind::Control, _) => self.on_path_reply(&h, payload, now),
            _ => self.on_session_frame(&h, payload, snr4, rssi, now),
        }
    }

    fn candidates(&self, src: u8) -> Vec<PubKey> {
        let mut v: Vec<PubKey> = self
            .contacts
            .keys()
            .filter(|k| k[0] == src)
            .copied()
            .collect();
        v.sort_unstable();
        v
    }

    /// Every frame the companion hears: we look for path discovery probes addressed to us
    /// (§4.5) and group messages (§10.3).
    fn on_log_rx(&mut self, raw: &[u8], snr4: i8, rssi: i8, now: Millis) {
        let (Some(me), true) = (self.key, self.prv.is_some()) else {
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
            self.on_group_envelope(p.payload, snr4, rssi, now);
            return;
        };
        if probe.dst != me[0] || self.seen_probes.contains(&probe.id) {
            return;
        }
        for c in self.candidates(probe.src) {
            let Some(s) = self.secret(&c) else {
                continue;
            };
            if !discovery::verify_probe(&s, &c, &me, p.payload) {
                continue;
            }
            if self.seen_probes.len() >= 64 {
                self.seen_probes.pop_front();
            }
            self.seen_probes.push_back(probe.id);
            let forward = OwnedPath::from_path(p.path);
            let back = forward.reversed();
            let mut buf = [0u8; FRAME_MAX];
            if let Ok(n) = discovery::seal_path_reply(
                &s,
                &me,
                &c,
                &probe.id,
                forward.len_byte,
                &forward.bytes,
                &mut buf,
            ) {
                let frame = buf.get(..n).unwrap_or(&[]).to_vec();
                self.send_raw(&back, &frame, now);
            }
            self.peers.entry(c).or_default().path = Some(back);
            return;
        }
    }

    fn on_path_reply(&mut self, h: &Header, payload: &[u8], now: Millis) {
        let Some(me) = self.key else {
            return;
        };
        for c in self.candidates(h.src) {
            let Some(probing) = self.peers.get(&c).and_then(|p| p.probing.clone()) else {
                continue;
            };
            if u16::from_le_bytes([probing.id[0], probing.id[1]]) != h.tag {
                continue;
            }
            let Some(s) = self.secret(&c) else {
                continue;
            };
            let mut buf = payload.to_vec();
            let Ok((id, len_byte, path)) = discovery::open_path_reply(&s, &c, &me, &mut buf) else {
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

    fn on_hello(&mut self, h: &Header, payload: &[u8], now: Millis) {
        let Some(me) = self.key else {
            return;
        };
        for c in self.candidates(h.src) {
            let Some(s) = self.secret(&c) else {
                continue;
            };
            let mut buf = payload.to_vec();
            let Ok(hello) = handshake::open_hello(&s, &c, &me, &mut buf) else {
                continue;
            };
            let return_path = (hello.return_path_len != PATH_UNKNOWN).then(|| OwnedPath {
                len_byte: hello.return_path_len,
                bytes: hello.return_path.to_vec(),
            });
            let (tag, attempt, salt_i, eph_i) =
                (hello.tag, hello.attempt, hello.salt, hello.ephemeral);
            let mut peer = self.peers.remove(&c).unwrap_or_default();
            self.answer_hello(
                &c,
                &mut peer,
                &s,
                (tag, attempt, salt_i, eph_i),
                return_path,
                now,
            );
            self.peers.insert(c, peer);
            return;
        }
    }

    fn answer_hello(
        &mut self,
        c: &PubKey,
        peer: &mut Peer,
        s: &[u8; 32],
        (tag, attempt, salt_i, eph_i): (u16, u8, [u8; SALT_LEN], [u8; 32]),
        return_path: Option<OwnedPath>,
        now: Millis,
    ) {
        let Some(me) = self.key else {
            return;
        };
        // Crossing HELLOs: the smaller public key's handshake wins (§4.4).
        if peer.initiating.is_some() {
            if me < *c {
                return;
            }
            peer.initiating = None;
        }
        let Some(path) = return_path.or_else(|| self.path_for(c, peer)) else {
            return;
        };
        let answered = match &peer.answered {
            Some(a) if a.tag == tag && a.salt_i == salt_i => a.clone(),
            _ => {
                let mut eph_secret = [0u8; 32];
                self.rng.fill_bytes(&mut eph_secret);
                let mut salt_r = [0u8; SALT_LEN];
                self.rng.fill_bytes(&mut salt_r);
                let Some(keys) = handshake::session_keys(
                    s,
                    &eph_secret,
                    &eph_i,
                    &salt_i,
                    &salt_r,
                    c,
                    &me,
                    false,
                ) else {
                    return;
                };
                let session = Session::new(keys, me[0], c[0], self.timing_for(&path));
                peer.pending = Some(Box::new(session));
                peer.path = Some(path.clone());
                let a = Answered {
                    tag,
                    salt_i,
                    salt_r,
                    eph_public: x25519_public(&eph_secret),
                };
                peer.answered = Some(a.clone());
                a
            }
        };
        let ack = HelloAck {
            tag,
            attempt,
            salt: answered.salt_r,
            ephemeral: answered.eph_public,
        };
        let mut buf = [0u8; FRAME_MAX];
        if let Ok(n) = handshake::seal_hello_ack(s, &me, c, &salt_i, &ack, &mut buf) {
            let frame = buf.get(..n).unwrap_or(&[]).to_vec();
            self.send_raw(&path, &frame, now);
        }
    }

    fn on_hello_ack(&mut self, h: &Header, payload: &[u8], now: Millis) {
        let Some(me) = self.key else {
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
            let Some(s) = self.secret(&c) else {
                continue;
            };
            let mut buf = payload.to_vec();
            let Ok(ack) = handshake::open_hello_ack(&s, &c, &me, &init.salt, &mut buf) else {
                continue;
            };
            let Some(keys) = handshake::session_keys(
                &s,
                &init.eph_secret,
                &ack.ephemeral,
                &init.salt,
                &ack.salt,
                &me,
                &c,
                true,
            ) else {
                return;
            };
            let session = Session::new(keys, me[0], c[0], self.timing_for(&init.path));
            if let Some(p) = self.peers.get_mut(&c) {
                p.requeue_in_flight();
                p.session = Some(Box::new(session));
                p.pending = None;
                p.path = Some(init.path);
                p.initiating = None;
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
                self.events.push_back(Event::SessionEstablished { peer: c });
            }
            self.peers.insert(c, peer);
            if let Some((result, _)) = outcome {
                if let Ok(Some(datagram)) = &result
                    && matches!(
                        datagram.first(),
                        Some(&lc_proto::content::GROUP_STATE | &lc_proto::content::SENDER_KEY)
                    )
                {
                    self.on_group_control(&c, datagram, now);
                } else if let Ok(Some(datagram)) = result
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

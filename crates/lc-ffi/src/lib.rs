//! UniFFI bindings of the lichen core (`lc-core`) for the iOS and Android apps.
//!
//! The app is the runner: it passes companion frames and the time in, writes the frames
//! `poll_companion` returns to the companion (BLE or USB), calls `tick` at `next_wakeup`, and
//! reads events. Keys and ids cross the boundary as byte arrays.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use lc_core::location::{self as loc, EXTRAPOLATE_MAX_S};
use lc_core::store::{self, Chat, Status, Store, StoredPosition};
use lc_core::{
    Config, Content, Core, Event, FailReason as CoreFail, Fix, GroupId, Precision, PubKey,
};
use lc_proto::location::{ALT_UNKNOWN, HEADING_UNKNOWN, SPEED_UNKNOWN};

mod demo;

pub use demo::DemoNetwork;

uniffi::setup_scaffolding!();

/// Errors returned to the app.
#[derive(Debug, PartialEq, Eq, uniffi::Error)]
pub enum LichenError {
    UnknownContact,
    UnknownGroup,
    Empty,
    TooLarge,
    NoIdentity,
    NotAllowed,
    /// A key or id does not have the expected length.
    InvalidKey,
    /// The medium does not fit in the airtime left this hour; it would in `wait_s` seconds.
    OverBudget {
        wait_s: u64,
    },
    /// Media go to contacts only (groups wait for the Internet bridge).
    MediaInGroup,
    /// The image could not be made into a thumbnail.
    BadImage,
    /// Not an image preview that can be asked for in full.
    NotAPreview,
    /// The contact's identity is not known yet: exchange a message or scan their code first.
    IdentityUnknown,
    /// Not a lichen contact code, or not 24 valid words.
    BadCode,
}

impl std::fmt::Display for LichenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for LichenError {}

impl From<lc_core::Error> for LichenError {
    fn from(e: lc_core::Error) -> Self {
        match e {
            lc_core::Error::UnknownContact => Self::UnknownContact,
            lc_core::Error::UnknownGroup => Self::UnknownGroup,
            lc_core::Error::Empty => Self::Empty,
            lc_core::Error::TooLarge => Self::TooLarge,
            lc_core::Error::NoIdentity => Self::NoIdentity,
            lc_core::Error::IdentityUnknown => Self::IdentityUnknown,
            lc_core::Error::NotAllowed => Self::NotAllowed,
            lc_core::Error::OverBudget { wait_ms } => Self::OverBudget {
                wait_s: wait_ms.div_ceil(1000),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Radio {
    pub freq_khz: u32,
    pub bw_hz: u32,
    pub sf: u8,
    /// Coding rate denominator (5..8).
    pub cr: u8,
}

/// A contact: its node key identifies it in the app; the identity behind it comes once known
/// (ADR-0013).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ContactInfo {
    /// The node key.
    pub key: Vec<u8>,
    pub name: String,
    /// Repeaters on the known path, if any.
    pub hops: Option<u8>,
    /// The phone identity, once known.
    pub identity: Option<Vec<u8>>,
    /// Compared in person (QR code or safety number).
    pub verified: bool,
}

impl ContactInfo {
    fn of(c: &lc_core::Contact) -> Self {
        Self {
            key: c.key.to_vec(),
            name: c.name.clone(),
            hops: c.out_path.as_ref().map(lc_core::OwnedPath::hops),
            identity: c.identity.map(|i| i.to_vec()),
            verified: c.verified,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct GroupInfo {
    pub gid: Vec<u8>,
    pub name: String,
    pub admin: Vec<u8>,
    pub members: Vec<ContactInfo>,
    /// Members whose sender key we hold (out of `members.len() - 1`).
    pub keys_known: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Budget {
    pub used_ms: u64,
    pub limit_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Estimate {
    pub fragments: u8,
    pub airtime_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Message {
    Text {
        sent_at: u32,
        text: String,
    },
    /// Codec2 frames (§13.1); `mode` 0 = 700C, 1 = 1200. Play with [`decode_voice`].
    Voice {
        sent_at: u32,
        mode: u8,
        frames: u16,
        duration_ms: u32,
        data: Vec<u8>,
    },
    /// An AVIF thumbnail (§13.2): the preview, or (`full`) the full variant.
    Image {
        sent_at: u32,
        media_id: Vec<u8>,
        full: bool,
        width: u16,
        height: u16,
        /// Bytes of the full variant (0: none).
        full_len: u16,
        data: Vec<u8>,
    },
    Other {
        content_type: u8,
        body: Vec<u8>,
    },
}

/// How long a received position stays on the map, in seconds.
const POSITION_KEEP_S: u64 = 24 * 3600;

impl From<Content> for Message {
    fn from(c: Content) -> Self {
        match c {
            Content::Text { sent_at, text } => Self::Text { sent_at, text },
            Content::Location(p) => Self::Other {
                content_type: lc_proto::content::LOCATION,
                body: p.encode().to_vec(),
            },
            Content::Voice {
                sent_at,
                mode,
                frames,
                data,
            } => Self::Voice {
                sent_at,
                mode,
                frames,
                duration_ms: lc_media::voice::duration_ms(frames),
                data,
            },
            Content::Image {
                sent_at,
                media_id,
                full,
                width,
                height,
                full_len,
                data,
            } => Self::Image {
                sent_at,
                media_id: media_id.to_vec(),
                full,
                width,
                height,
                full_len,
                data,
            },
            c @ Content::MediaRequest { .. } => {
                let mut v = c.encode();
                let content_type = v.remove(0);
                Self::Other {
                    content_type,
                    body: v,
                }
            }
            Content::Other { content_type, body } => Self::Other { content_type, body },
        }
    }
}

/// Our position, from the phone's location service.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LocationFix {
    pub lat: f64,
    pub lon: f64,
    pub alt_m: Option<f64>,
    /// Horizontal accuracy (radius), metres.
    pub accuracy_m: f64,
    pub heading_deg: Option<f64>,
    pub speed_mps: Option<f64>,
    /// When it was measured, Unix seconds.
    pub unix_s: u64,
}

/// What a share reveals (protocol.md §12.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SharePrecision {
    Exact,
    M100,
    Km1,
    Km10,
}

impl From<SharePrecision> for Precision {
    fn from(p: SharePrecision) -> Self {
        match p {
            SharePrecision::Exact => Self::Exact,
            SharePrecision::M100 => Self::M100,
            SharePrecision::Km1 => Self::Km1,
            SharePrecision::Km10 => Self::Km10,
        }
    }
}

impl From<Precision> for SharePrecision {
    fn from(p: Precision) -> Self {
        match p {
            Precision::Exact => Self::Exact,
            Precision::M100 => Self::M100,
            Precision::Km1 => Self::Km1,
            Precision::Km10 => Self::Km10,
        }
    }
}

/// One of our shares.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ShareInfo {
    pub chat: ChatRef,
    pub precision: SharePrecision,
    /// End, Unix seconds.
    pub until_s: u64,
}

/// Someone's position as the map shows it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PeerPosition {
    pub from: Vec<u8>,
    /// The group it came through; none for a direct share.
    pub group: Option<Vec<u8>>,
    pub lat: f64,
    pub lon: f64,
    pub alt_m: Option<f64>,
    /// Radius of uncertainty (the precision of the share, or of the GPS).
    pub radius_m: f64,
    pub heading_deg: Option<f64>,
    pub speed_mps: Option<f64>,
    /// When the sender vouched for it, Unix seconds.
    pub at_s: u64,
    pub sos: bool,
    /// `lat`/`lon` follow the sender's heading and speed from `at_s` to now (dead reckoning).
    pub extrapolated: bool,
}

impl PeerPosition {
    fn new(p: &StoredPosition, now_s: u64) -> Self {
        let q = &p.position;
        let (lat, lon) = loc::extrapolate(q, p.at, now_s);
        let moving = q.heading != HEADING_UNKNOWN && q.speed != SPEED_UNKNOWN && q.speed > 0;
        Self {
            from: p.from.to_vec(),
            group: p.group.map(|g| g.to_vec()),
            lat,
            lon,
            alt_m: (q.alt != ALT_UNKNOWN).then_some(f64::from(q.alt)),
            radius_m: loc::radius_m(q.prec),
            heading_deg: (q.heading != HEADING_UNKNOWN).then_some(f64::from(q.heading)),
            speed_mps: (q.speed != SPEED_UNKNOWN).then_some(f64::from(q.speed) * 0.5),
            at_s: p.at,
            sos: q.sos(),
            extrapolated: moving && now_s > p.at && now_s < p.at + EXTRAPOLATE_MAX_S,
        }
    }
}

/// One marker per sender: an SOS first, else the most precise of the positions at most
/// 15 minutes older than the newest, else the newest.
fn best_per_sender(all: Vec<StoredPosition>) -> Vec<StoredPosition> {
    let mut by: HashMap<PubKey, Vec<StoredPosition>> = HashMap::new();
    for p in all {
        by.entry(p.from).or_default().push(p);
    }
    let mut out: Vec<StoredPosition> = by
        .into_values()
        .filter_map(|mut v| {
            v.sort_by_key(|p| (std::cmp::Reverse(p.position.sos()), std::cmp::Reverse(p.at)));
            let first = v.first()?.clone();
            if first.position.sos() {
                return Some(first);
            }
            v.into_iter()
                .filter(|p| p.at + 900 >= first.at)
                .min_by_key(|p| p.position.prec)
        })
        .collect();
    out.sort_by_key(|p| p.from);
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FailReason {
    NoPath,
    Unreachable,
    Timeout,
    Refused,
    TooLarge,
    /// The app stopped before delivery.
    Interrupted,
}

impl From<CoreFail> for FailReason {
    fn from(r: CoreFail) -> Self {
        match r {
            CoreFail::NoPath => Self::NoPath,
            CoreFail::Unreachable => Self::Unreachable,
            CoreFail::Timeout => Self::Timeout,
            CoreFail::Refused => Self::Refused,
            CoreFail::TooLarge => Self::TooLarge,
            CoreFail::Interrupted => Self::Interrupted,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LichenEvent {
    Connected {
        key: Vec<u8>,
        name: String,
        radio: Radio,
    },
    /// We learned the identity behind contact `node`; `changed`: it replaced another one (to
    /// verify again).
    IdentityLearned {
        node: Vec<u8>,
        identity: Vec<u8>,
        changed: bool,
    },
    ContactsUpdated,
    SessionEstablished {
        peer: Vec<u8>,
    },
    Received {
        from: Vec<u8>,
        message: Message,
        snr_db: f32,
        rssi: i8,
    },
    Progress {
        id: u64,
        sent: u32,
        total: u8,
    },
    Delivered {
        id: u64,
    },
    Failed {
        id: u64,
        reason: FailReason,
    },
    GroupJoined {
        gid: Vec<u8>,
        name: String,
        admin: Vec<u8>,
    },
    GroupUpdated {
        gid: Vec<u8>,
    },
    GroupLeft {
        gid: Vec<u8>,
    },
    GroupMessage {
        gid: Vec<u8>,
        from: Vec<u8>,
        message: Message,
        snr_db: f32,
        rssi: i8,
    },
    /// A message was added to the history, or its status changed.
    MessageChanged {
        message: StoredMessage,
    },
    /// A position arrived (`end`: the sender stopped sharing; its marker is gone).
    Location {
        position: PeerPosition,
        end: bool,
    },
    /// One of our shares ended on its own.
    SharesChanged,
    /// `from` asked for the full variant of one of our images; it is on its way (when we still
    /// have it and the budget allows).
    MediaRequested {
        from: Vec<u8>,
    },
}

fn snr(snr4: i8) -> f32 {
    f32::from(snr4) / 4.0
}

impl LichenEvent {
    /// `admin_node` maps a group admin's identity to its node (the app knows contacts by node).
    fn from_core(
        e: Event,
        freq_khz: u32,
        admin_node: impl Fn(&GroupId, &PubKey) -> PubKey,
    ) -> Self {
        match e {
            Event::Connected {
                key,
                name,
                freq_khz: f,
                radio,
            } => Self::Connected {
                key: key.to_vec(),
                name,
                radio: Radio {
                    freq_khz: if f == 0 { freq_khz } else { f },
                    bw_hz: radio.bw_hz,
                    sf: radio.sf,
                    cr: radio.cr,
                },
            },
            Event::IdentityLearned {
                node,
                identity,
                changed,
            } => Self::IdentityLearned {
                node: node.to_vec(),
                identity: identity.to_vec(),
                changed,
            },
            Event::ContactsUpdated => Self::ContactsUpdated,
            Event::SessionEstablished { peer } => Self::SessionEstablished {
                peer: peer.to_vec(),
            },
            Event::Received {
                from,
                content,
                snr4,
                rssi,
            } => Self::Received {
                from: from.to_vec(),
                message: content.into(),
                snr_db: snr(snr4),
                rssi,
            },
            Event::Progress { id, sent, total } => Self::Progress { id, sent, total },
            Event::Delivered { id } => Self::Delivered { id },
            Event::Failed { id, reason } => Self::Failed {
                id,
                reason: reason.into(),
            },
            Event::GroupJoined { gid, name, admin } => Self::GroupJoined {
                gid: gid.to_vec(),
                name,
                admin: admin_node(&gid, &admin).to_vec(),
            },
            Event::GroupUpdated { gid } => Self::GroupUpdated { gid: gid.to_vec() },
            Event::GroupLeft { gid } => Self::GroupLeft { gid: gid.to_vec() },
            Event::GroupMessage {
                gid,
                from,
                content,
                snr4,
                rssi,
            } => Self::GroupMessage {
                gid: gid.to_vec(),
                from: from.to_vec(),
                message: content.into(),
                snr_db: snr(snr4),
                rssi,
            },
            Event::Location {
                from,
                group,
                position,
                at,
                snr4,
            } => Self::Location {
                end: position.end(),
                position: PeerPosition::new(
                    &StoredPosition {
                        from,
                        group,
                        position,
                        at,
                        snr4,
                    },
                    at,
                ),
            },
            Event::SharesChanged => Self::SharesChanged,
            Event::MediaRequested { from, .. } => Self::MediaRequested {
                from: from.to_vec(),
            },
        }
    }
}

fn key(v: &[u8]) -> Result<PubKey, LichenError> {
    v.try_into().map_err(|_| LichenError::InvalidKey)
}

fn gid(v: &[u8]) -> Result<GroupId, LichenError> {
    v.try_into().map_err(|_| LichenError::InvalidKey)
}

fn unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// A conversation.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ChatRef {
    Contact { key: Vec<u8> },
    Group { gid: Vec<u8> },
}

impl ChatRef {
    fn to_store(&self) -> Result<Chat, LichenError> {
        Ok(match self {
            Self::Contact { key: k } => Chat::Contact(key(k)?),
            Self::Group { gid: g } => Chat::Group(gid(g)?),
        })
    }

    fn from_store(c: &Chat) -> Self {
        match c {
            Chat::Contact(k) => Self::Contact { key: k.to_vec() },
            Chat::Group(g) => Self::Group { gid: g.to_vec() },
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MessageStatus {
    Received,
    Sending {
        sent: u32,
        total: u8,
    },
    Delivered,
    Failed {
        reason: FailReason,
    },
    /// Flooded to a group, without acknowledgement.
    Flooded,
}

/// A message from the history.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StoredMessage {
    pub id: i64,
    pub chat: ChatRef,
    pub outgoing: bool,
    pub author: Option<Vec<u8>>,
    pub message: Message,
    /// Unix milliseconds.
    pub at_ms: i64,
    pub status: MessageStatus,
    pub fragments: u8,
    pub airtime_ms: u64,
    pub snr_db: Option<f32>,
}

impl From<store::Message> for StoredMessage {
    fn from(m: store::Message) -> Self {
        Self {
            id: m.id,
            chat: ChatRef::from_store(&m.chat),
            outgoing: m.outgoing,
            author: m.author.map(|a| a.to_vec()),
            message: m.content.into(),
            at_ms: m.at_ms,
            status: match m.status {
                Status::Received => MessageStatus::Received,
                Status::Sending { sent, total } => MessageStatus::Sending { sent, total },
                Status::Delivered => MessageStatus::Delivered,
                Status::Failed(r) => MessageStatus::Failed { reason: r.into() },
                Status::Flooded => MessageStatus::Flooded,
            },
            fragments: m.fragments,
            airtime_ms: m.airtime_ms,
            snr_db: m.snr4.map(snr),
        }
    }
}

/// A conversation with its last message.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Conversation {
    pub chat: ChatRef,
    pub last: StoredMessage,
    pub unread: u32,
}

struct Inner {
    core: Core,
    /// The identity seed (ADR-0013), for the recovery phrase.
    seed: [u8; 32],
    store: Option<Store>,
    data_dir: Option<PathBuf>,
    /// Core message id → row id, for the messages sent by this run.
    inflight: HashMap<u64, i64>,
    /// Events to hand out after the current one (`MessageChanged`).
    extra: VecDeque<LichenEvent>,
    /// The latest `now_ms` the app gave (to answer requests from inside event handling).
    last_now: u64,
}

impl Inner {
    /// Opens the database of our identity (or one in memory) and restores contacts, groups
    /// and shares.
    fn open_store(&mut self, identity: &PubKey) {
        let store = match &self.data_dir {
            Some(dir) => {
                let _ = std::fs::create_dir_all(dir);
                let hex: String = identity
                    .iter()
                    .take(8)
                    .map(|b| format!("{b:02x}"))
                    .collect();
                Store::open(&dir.join(format!("id-{hex}.sqlite")))
            }
            None => Store::in_memory(),
        };
        let Ok(store) = store else {
            return;
        };
        let _ = store.fail_unfinished();
        for c in store.contacts().unwrap_or_default() {
            self.core.restore_contact(c);
        }
        for snap in store.groups().unwrap_or_default() {
            self.core.restore_group(&snap);
        }
        for (chat, precision, until) in store.shares().unwrap_or_default() {
            let _ = self.core.restore_share(chat, precision, until);
        }
        let now_s = u64::try_from(unix_ms() / 1000).unwrap_or(0);
        let _ = store.prune_positions(now_s.saturating_sub(POSITION_KEEP_S));
        self.store = Some(store);
    }

    fn save_contacts(&self) {
        if let Some(st) = &self.store {
            for c in self.core.contacts() {
                let _ = st.put_contact(c);
            }
        }
    }

    fn save_shares(&self) {
        if let Some(st) = &self.store {
            let _ = st.put_shares(self.core.shares());
        }
    }

    /// Saves the groups that changed.
    fn persist(&mut self) {
        let dirty = self.core.take_dirty_groups();
        let Some(st) = &self.store else {
            return;
        };
        for g in dirty {
            let _ = match self.core.group_snapshot(&g) {
                Some(snap) => st.put_group(&g, &snap),
                None => st.delete_group(&g),
            };
        }
    }

    /// Stores a message we just queued and tracks its delivery.
    fn stored_outgoing(
        &mut self,
        to: PubKey,
        content: Content,
        id: u64,
        (fragments, airtime_ms): (u8, u64),
    ) -> StoredMessage {
        let row = store::Message {
            id: 0,
            chat: Chat::Contact(to),
            outgoing: true,
            author: None,
            content,
            at_ms: unix_ms(),
            status: Status::Sending {
                sent: 0,
                total: fragments,
            },
            fragments,
            airtime_ms,
            snr4: None,
            read: true,
        };
        let stored = self
            .store
            .as_ref()
            .and_then(|s| s.insert(&row).ok())
            .unwrap_or(row);
        if stored.id != 0 {
            self.inflight.insert(id, stored.id);
        }
        self.persist();
        stored.into()
    }

    fn record(&mut self, m: store::Message) -> Option<StoredMessage> {
        let row = self.store.as_ref()?.insert(&m).ok()?;
        let out = StoredMessage::from(row);
        self.extra.push_back(LichenEvent::MessageChanged {
            message: out.clone(),
        });
        Some(out)
    }

    fn update(&mut self, core_id: u64, status: Status, done: bool) {
        let Some(&row) = self.inflight.get(&core_id) else {
            return;
        };
        if done {
            self.inflight.remove(&core_id);
        }
        if let Some(m) = self
            .store
            .as_ref()
            .and_then(|s| s.set_status(row, status).ok().flatten())
        {
            self.extra
                .push_back(LichenEvent::MessageChanged { message: m.into() });
        }
    }

    /// Records what an event means for the history.
    fn on_event(&mut self, e: &Event) {
        match e {
            Event::Connected { .. } => self.save_contacts(),
            Event::ContactsUpdated
            | Event::GroupJoined { .. }
            | Event::GroupUpdated { .. }
            | Event::IdentityLearned { .. } => {
                self.save_contacts();
            }
            Event::Received {
                from,
                content:
                    content @ Content::Image {
                        full: true,
                        media_id,
                        ..
                    },
                ..
            } if self
                .store
                .as_ref()
                .and_then(|s| s.find_image(&Chat::Contact(*from), media_id).ok().flatten())
                .is_some() =>
            {
                // The full variant replaces its preview.
                let chat = Chat::Contact(*from);
                if let Some(m) = self.store.as_ref().and_then(|s| {
                    let row = s.find_image(&chat, media_id).ok().flatten()?;
                    s.set_content(row, content).ok().flatten()
                }) {
                    self.extra
                        .push_back(LichenEvent::MessageChanged { message: m.into() });
                }
            }
            Event::MediaRequested {
                from,
                media_id,
                full: true,
            } => {
                let chat = Chat::Contact(*from);
                if let Some((to, datagram)) = self
                    .store
                    .as_ref()
                    .and_then(|s| s.media(media_id).ok().flatten())
                    && to == chat
                {
                    let now = self.last_now;
                    let _ = self.core.send_media(from, datagram, now);
                }
            }
            Event::Received {
                from,
                content,
                snr4,
                ..
            } => {
                self.record(store::Message {
                    id: 0,
                    chat: Chat::Contact(*from),
                    outgoing: false,
                    author: None,
                    content: content.clone(),
                    at_ms: unix_ms(),
                    status: Status::Received,
                    fragments: 0,
                    airtime_ms: 0,
                    snr4: Some(*snr4),
                    read: false,
                });
            }
            Event::GroupMessage {
                gid,
                from,
                content,
                snr4,
                ..
            } => {
                self.record(store::Message {
                    id: 0,
                    chat: Chat::Group(*gid),
                    outgoing: false,
                    author: Some(*from),
                    content: content.clone(),
                    at_ms: unix_ms(),
                    status: Status::Received,
                    fragments: 0,
                    airtime_ms: 0,
                    snr4: Some(*snr4),
                    read: false,
                });
            }
            Event::Progress { id, sent, total } => {
                self.update(
                    *id,
                    Status::Sending {
                        sent: *sent,
                        total: *total,
                    },
                    false,
                );
            }
            Event::Location {
                from,
                group,
                position,
                at,
                snr4,
            } => {
                if let Some(st) = &self.store {
                    let _ = st.put_position(&StoredPosition {
                        from: *from,
                        group: *group,
                        position: *position,
                        at: *at,
                        snr4: *snr4,
                    });
                }
            }
            Event::SharesChanged => self.save_shares(),
            Event::Delivered { id } => self.update(*id, Status::Delivered, true),
            Event::Failed { id, reason } => self.update(*id, Status::Failed(*reason), true),
            _ => {}
        }
    }
}

/// The lichen core with its storage, shared between threads.
#[derive(uniffi::Object)]
pub struct LichenCore {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for LichenCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LichenCore")
    }
}

impl LichenCore {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[uniffi::export]
impl LichenCore {
    /// `rng_seed`: 32 bytes from the OS random generator. `identity_seed`: the phone
    /// identity's 32-byte seed (ADR-0013), drawn once by the app and kept in the Keychain /
    /// Keystore. `data_dir`: where the identity's database lives; without it, history is kept
    /// in memory only (demo).
    #[uniffi::constructor]
    pub fn new(
        app_name: String,
        rng_seed: Vec<u8>,
        identity_seed: Vec<u8>,
        data_dir: Option<String>,
    ) -> Result<Arc<Self>, LichenError> {
        let rng: [u8; 32] = rng_seed
            .as_slice()
            .try_into()
            .map_err(|_| LichenError::InvalidKey)?;
        let seed: [u8; 32] = identity_seed
            .as_slice()
            .try_into()
            .map_err(|_| LichenError::InvalidKey)?;
        let mut inner = Inner {
            core: Core::new(Config::new(&app_name, seed, rng)),
            seed,
            store: None,
            data_dir: data_dir.map(PathBuf::from),
            inflight: HashMap::new(),
            extra: VecDeque::new(),
            last_now: 0,
        };
        // The history is available before the node connects.
        if let Some(id) = inner.core.key().copied() {
            inner.open_store(&id);
        }
        Ok(Arc::new(Self {
            inner: Mutex::new(inner),
        }))
    }

    pub fn connect(&self, now_ms: u64) {
        let mut i = self.lock();
        i.core.connect(now_ms);
        i.persist();
    }

    pub fn on_companion_frame(&self, frame: Vec<u8>, now_ms: u64) {
        let mut i = self.lock();
        i.last_now = now_ms;
        i.core.on_companion_frame(&frame, now_ms);
        i.persist();
    }

    /// The next frame to write to the companion, if one is due.
    pub fn poll_companion(&self, now_ms: u64) -> Option<Vec<u8>> {
        self.lock().core.poll_companion(now_ms)
    }

    pub fn tick(&self, now_ms: u64) {
        let mut i = self.lock();
        i.last_now = now_ms;
        i.core.tick(now_ms);
        i.persist();
    }

    /// When to call `tick` (or `poll_companion`) next, in the same clock as `now_ms`.
    pub fn next_wakeup(&self) -> Option<u64> {
        self.lock().core.next_wakeup()
    }

    /// The next event. History changes come as `MessageChanged` right after the event that
    /// caused them.
    pub fn poll_event(&self) -> Option<LichenEvent> {
        let mut i = self.lock();
        if let Some(e) = i.extra.pop_front() {
            return Some(e);
        }
        let e = i.core.poll_event()?;
        i.on_event(&e);
        i.persist();
        let f = i.core.freq_khz();
        let core = &i.core;
        Some(LichenEvent::from_core(e, f, |gid, admin| {
            core.groups()
                .find(|g| g.gid == *gid)
                .and_then(|g| g.members.iter().find(|m| m.key == *admin))
                .map_or(*admin, |m| m.node)
        }))
    }

    /// Our identity key (ADR-0013).
    pub fn key(&self) -> Option<Vec<u8>> {
        self.lock().core.key().map(|k| k.to_vec())
    }

    /// The connected node's key.
    pub fn node_key(&self) -> Option<Vec<u8>> {
        self.lock().core.node_key().map(|k| k.to_vec())
    }

    /// The 24 words that restore our identity (§2).
    pub fn recovery_phrase(&self) -> Vec<String> {
        lc_core::identity::recovery_phrase(&self.lock().seed)
    }

    /// Our fingerprint: 6 groups of 4 hexadecimal digits (§4.7).
    pub fn fingerprint(&self) -> Vec<String> {
        let i = self.lock();
        i.core
            .key()
            .map(|k| lc_core::identity::fingerprint(k).to_vec())
            .unwrap_or_default()
    }

    /// Our contact card for a QR code (`lichen:1:…`); needs the node connected.
    pub fn contact_card(&self, name: String) -> Result<String, LichenError> {
        let i = self.lock();
        let (Some(identity), Some(node)) = (i.core.key(), i.core.node_key()) else {
            return Err(LichenError::NoIdentity);
        };
        Ok(lc_core::identity::Card {
            identity: *identity,
            node: *node,
            name,
        }
        .encode())
    }

    /// Adds (or updates) the contact of a scanned card, verified: it was scanned in person.
    pub fn add_contact_card(&self, card: String) -> Result<ContactInfo, LichenError> {
        let c = lc_core::identity::Card::decode(&card).ok_or(LichenError::BadCode)?;
        let mut i = self.lock();
        if i.core.key() == Some(&c.identity) {
            return Err(LichenError::BadCode);
        }
        let name = i
            .core
            .contacts()
            .find(|k| k.key == c.node)
            .map_or(c.name.clone(), |k| k.name.clone());
        let path = i
            .core
            .contacts()
            .find(|k| k.key == c.node)
            .and_then(|k| k.out_path.clone());
        i.core.add_contact(c.node, &name, path);
        i.core.set_identity(&c.node, c.identity, true);
        i.save_contacts();
        let out = i
            .core
            .contacts()
            .find(|k| k.key == c.node)
            .map(ContactInfo::of)
            .ok_or(LichenError::UnknownContact)?;
        i.extra.push_back(LichenEvent::ContactsUpdated);
        Ok(out)
    }

    /// The safety number with contact `node` (6 groups of 5 digits), once its identity is known.
    pub fn safety_number(&self, node: Vec<u8>) -> Result<Vec<String>, LichenError> {
        let k = key(&node)?;
        let i = self.lock();
        let peer = i
            .core
            .contacts()
            .find(|c| c.key == k)
            .ok_or(LichenError::UnknownContact)?
            .identity
            .ok_or(LichenError::IdentityUnknown)?;
        let me = i.core.key().ok_or(LichenError::NoIdentity)?;
        Ok(lc_core::identity::safety_number(me, &peer).to_vec())
    }

    /// Marks contact `node` verified (safety numbers matched) or not (they differ).
    pub fn set_verified(&self, node: Vec<u8>, verified: bool) -> Result<(), LichenError> {
        let k = key(&node)?;
        let mut i = self.lock();
        i.core.set_verified(&k, verified);
        i.save_contacts();
        Ok(())
    }

    pub fn radio(&self) -> Radio {
        let i = self.lock();
        let r = i.core.radio();
        Radio {
            freq_khz: i.core.freq_khz(),
            bw_hz: r.bw_hz,
            sf: r.sf,
            cr: r.cr,
        }
    }

    pub fn budget(&self, now_ms: u64) -> Budget {
        let (used_ms, limit_ms) = self.lock().core.budget(now_ms);
        Budget { used_ms, limit_ms }
    }

    pub fn contacts(&self) -> Vec<ContactInfo> {
        let mut v: Vec<ContactInfo> = self.lock().core.contacts().map(ContactInfo::of).collect();
        v.sort_by_key(|c| c.name.to_lowercase());
        v
    }

    pub fn groups(&self) -> Vec<GroupInfo> {
        let i = self.lock();
        let mut v: Vec<GroupInfo> = i
            .core
            .groups()
            .map(|g| GroupInfo {
                gid: g.gid.to_vec(),
                name: g.name.clone(),
                admin: g
                    .members
                    .iter()
                    .find(|m| m.key == g.admin)
                    .map_or(g.admin, |m| m.node)
                    .to_vec(),
                members: g
                    .members
                    .iter()
                    .map(|m| {
                        let contact = i.core.contacts().find(|c| c.key == m.node);
                        ContactInfo {
                            key: m.node.to_vec(),
                            name: m.name.clone(),
                            hops: None,
                            identity: Some(m.key.to_vec()),
                            verified: contact
                                .is_some_and(|c| c.verified && c.identity == Some(m.key)),
                        }
                    })
                    .collect(),
                keys_known: g.keys_known() as u32,
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Cost of a text of `text_len` bytes to `to`: fragments and airtime of the first round.
    pub fn estimate_text(&self, to: Vec<u8>, text_len: u32) -> Result<Estimate, LichenError> {
        let k = key(&to)?;
        let (fragments, airtime_ms) = self.lock().core.estimate_send(&k, 5 + text_len as usize);
        Ok(Estimate {
            fragments,
            airtime_ms,
        })
    }

    /// Sends a text and returns it as stored (status `Sending`).
    pub fn send_text(
        &self,
        to: Vec<u8>,
        text: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let k = key(&to)?;
        let content = Content::Text { sent_at, text };
        let mut i = self.lock();
        let datagram = content.encode();
        let estimate = i.core.estimate_send(&k, datagram.len());
        let id = i.core.send_text(
            &k,
            match &content {
                Content::Text { text, .. } => text,
                _ => "",
            },
            sent_at,
            now_ms,
        )?;
        Ok(i.stored_outgoing(k, content, id, estimate))
    }

    /// Fragments and airtime (first round) of a datagram of `len` bytes to `to`.
    pub fn estimate_datagram(&self, to: Vec<u8>, len: u32) -> Result<Estimate, LichenError> {
        let k = key(&to)?;
        let (fragments, airtime_ms) = self.lock().core.estimate_send(&k, len as usize);
        Ok(Estimate {
            fragments,
            airtime_ms,
        })
    }

    /// Sends a voice message prepared by [`prepare_voice`] (refused when over budget).
    pub fn send_voice(
        &self,
        to: Vec<u8>,
        voice: PreparedVoice,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let k = key(&to)?;
        let content = Content::Voice {
            sent_at,
            mode: voice.mode,
            frames: voice.frames,
            data: voice.data,
        };
        let mut i = self.lock();
        i.last_now = now_ms;
        let datagram = content.encode();
        let estimate = i.core.estimate_send(&k, datagram.len());
        let id = i.core.send_media(&k, datagram, now_ms)?;
        Ok(i.stored_outgoing(k, content, id, estimate))
    }

    /// Sends the preview of an image prepared by [`prepare_image`]; its full variant goes out
    /// when the receiver asks (§13.2). Refused when over budget.
    pub fn send_image(
        &self,
        to: Vec<u8>,
        image: PreparedImage,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let k = key(&to)?;
        let mut i = self.lock();
        i.last_now = now_ms;
        let media_id = i.core.random_id();
        let variant = |full: bool, data: Vec<u8>| Content::Image {
            sent_at,
            media_id,
            full,
            width: image.width,
            height: image.height,
            full_len: image.full.len() as u16,
            data,
        };
        let full = variant(true, image.full.clone());
        let preview = variant(false, image.preview.clone());
        let datagram = preview.encode();
        let estimate = i.core.estimate_send(&k, datagram.len());
        let id = i.core.send_media(&k, datagram, now_ms)?;
        if let Some(st) = &i.store {
            let _ = st.put_media(&media_id, &Chat::Contact(k), &full.encode());
        }
        // What we keep in our history is the full image.
        Ok(i.stored_outgoing(k, full, id, estimate))
    }

    /// Asks the sender of image preview `message_id` for its full variant.
    pub fn request_full_image(&self, message_id: i64, now_ms: u64) -> Result<(), LichenError> {
        let mut i = self.lock();
        let m = i
            .store
            .as_ref()
            .and_then(|s| s.get(message_id).ok().flatten())
            .ok_or(LichenError::NotAPreview)?;
        let (
            Chat::Contact(from),
            Content::Image {
                media_id,
                full: false,
                full_len,
                ..
            },
        ) = (m.chat, &m.content)
        else {
            return Err(LichenError::NotAPreview);
        };
        if m.outgoing || *full_len == 0 {
            return Err(LichenError::NotAPreview);
        }
        let request = Content::MediaRequest {
            media_id: *media_id,
            full: true,
        };
        i.core.send_datagram(&from, request.encode(), now_ms)?;
        Ok(())
    }

    pub fn create_group(
        &self,
        name: String,
        members: Vec<Vec<u8>>,
        now_ms: u64,
    ) -> Result<Vec<u8>, LichenError> {
        let keys = members
            .iter()
            .map(|m| key(m))
            .collect::<Result<Vec<_>, _>>()?;
        let mut i = self.lock();
        let g = i.core.create_group(&name, &keys, now_ms)?;
        i.persist();
        Ok(g.to_vec())
    }

    /// Floods a text to the group and returns it as stored (status `Flooded`).
    pub fn send_group_text(
        &self,
        gid: Vec<u8>,
        text: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let g = self::gid(&gid)?;
        let mut i = self.lock();
        i.core.send_group_text(&g, &text, sent_at, now_ms)?;
        let row = store::Message {
            id: 0,
            chat: Chat::Group(g),
            outgoing: true,
            author: None,
            content: Content::Text { sent_at, text },
            at_ms: unix_ms(),
            status: Status::Flooded,
            fragments: 1,
            airtime_ms: 0,
            snr4: None,
            read: true,
        };
        let stored = i
            .store
            .as_ref()
            .and_then(|s| s.insert(&row).ok())
            .unwrap_or(row);
        i.persist();
        Ok(stored.into())
    }

    pub fn remove_member(
        &self,
        gid: Vec<u8>,
        member: Vec<u8>,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let g = self::gid(&gid)?;
        let m = key(&member)?;
        let mut i = self.lock();
        i.core.remove_member(&g, &m, now_ms)?;
        i.persist();
        Ok(())
    }

    /// Up to `limit` messages of `chat` before message `before_id`, oldest first.
    pub fn history(
        &self,
        chat: ChatRef,
        before_id: Option<i64>,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        Ok(i.store
            .as_ref()
            .and_then(|s| s.history(&c, before_id, limit).ok())
            .unwrap_or_default()
            .into_iter()
            .map(StoredMessage::from)
            .collect())
    }

    /// Conversations that have messages, most recent first.
    pub fn conversations(&self) -> Vec<Conversation> {
        let i = self.lock();
        i.store
            .as_ref()
            .and_then(|s| s.conversations().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|(m, unread)| Conversation {
                chat: ChatRef::from_store(&m.chat),
                last: m.into(),
                unread,
            })
            .collect()
    }

    // ----- location (protocol.md §12) ---------------------------------------------------

    /// The wall clock: `unix_ms` is the current Unix time in milliseconds at `now_ms`. Call it
    /// at start and whenever the clock may have jumped; positions wait for it.
    pub fn set_clock(&self, unix_ms: u64, now_ms: u64) {
        self.lock().core.set_clock(unix_ms, now_ms);
    }

    /// Our position, from the location service.
    pub fn update_location(&self, fix: LocationFix, now_ms: u64) {
        let mut i = self.lock();
        i.core.update_location(
            Fix {
                lat: fix.lat,
                lon: fix.lon,
                alt_m: fix.alt_m,
                accuracy_m: fix.accuracy_m,
                heading_deg: fix.heading_deg,
                speed_mps: fix.speed_mps,
                unix_s: fix.unix_s,
            },
            now_ms,
        );
        i.persist();
    }

    /// Shares our position with `chat` at `precision` until `until_s` (Unix seconds); replaces
    /// a share with the same conversation.
    pub fn start_share(
        &self,
        chat: ChatRef,
        precision: SharePrecision,
        until_s: u64,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let mut i = self.lock();
        i.core.start_share(c, precision.into(), until_s, now_ms)?;
        i.save_shares();
        i.persist();
        Ok(())
    }

    /// Stops sharing with `chat` (the receiver removes our marker).
    pub fn stop_share(&self, chat: ChatRef, now_ms: u64) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let mut i = self.lock();
        i.core.stop_share(&c, now_ms);
        i.save_shares();
        i.persist();
        Ok(())
    }

    pub fn shares(&self) -> Vec<ShareInfo> {
        let mut v: Vec<ShareInfo> = self
            .lock()
            .core
            .shares()
            .iter()
            .map(|s| ShareInfo {
                chat: ChatRef::from_store(&s.chat),
                precision: s.precision.into(),
                until_s: s.until,
            })
            .collect();
        v.sort_by_key(|s| s.until_s);
        v
    }

    /// SOS: our exact position to every share and every group, every 2 minutes.
    pub fn set_sos(&self, on: bool, now_ms: u64) {
        let mut i = self.lock();
        i.core.set_sos(on, now_ms);
        i.persist();
    }

    pub fn sos(&self) -> bool {
        self.lock().core.sos()
    }

    /// The positions to show, one per sender, extrapolated to `now_ms`.
    pub fn positions(&self, now_ms: u64) -> Vec<PeerPosition> {
        let i = self.lock();
        let now_s = i
            .core
            .unix_s(now_ms)
            .unwrap_or_else(|| u64::try_from(unix_ms() / 1000).unwrap_or(0));
        let all = i
            .store
            .as_ref()
            .and_then(|s| s.positions().ok())
            .unwrap_or_default();
        best_per_sender(all)
            .iter()
            .map(|p| PeerPosition::new(p, now_s))
            .collect()
    }

    pub fn mark_read(&self, chat: ChatRef) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        if let Some(s) = self.lock().store.as_ref() {
            let _ = s.mark_read(&c);
        }
        Ok(())
    }
}

/// The seed of a 24-word recovery phrase, to restore an identity (§2).
#[uniffi::export]
pub fn seed_from_phrase(words: Vec<String>) -> Result<Vec<u8>, LichenError> {
    lc_core::identity::seed_from_phrase(&words)
        .map(|s| s.to_vec())
        .ok_or(LichenError::BadCode)
}

/// The fingerprint of an identity key (6 groups of 4 hexadecimal digits).
#[uniffi::export]
pub fn fingerprint_of(identity: Vec<u8>) -> Result<Vec<String>, LichenError> {
    let k = key(&identity)?;
    Ok(lc_core::identity::fingerprint(&k).to_vec())
}

/// A voice message ready to send.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PreparedVoice {
    pub mode: u8,
    pub frames: u16,
    pub duration_ms: u32,
    pub data: Vec<u8>,
}

/// Encodes 8 kHz mono samples (at most 15 s are kept).
#[uniffi::export]
pub fn prepare_voice(pcm: Vec<i16>) -> PreparedVoice {
    let e = lc_media::voice::encode(&pcm);
    PreparedVoice {
        mode: e.mode.code(),
        frames: e.frames,
        duration_ms: lc_media::voice::duration_ms(e.frames),
        data: e.data,
    }
}

/// 8 kHz mono samples of a voice message; none when this build does not support its mode.
#[uniffi::export]
pub fn decode_voice(mode: u8, frames: u16, data: Vec<u8>) -> Option<Vec<i16>> {
    lc_media::voice::decode(mode, frames, &data)
}

/// Bytes of a VOICE datagram for `voice` (for estimates).
#[uniffi::export]
pub fn voice_datagram_len(voice: &PreparedVoice) -> u32 {
    8 + voice.data.len() as u32
}

/// An image made into a thumbnail: a preview (sent) and a full variant (on request).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PreparedImage {
    pub width: u16,
    pub height: u16,
    pub preview: Vec<u8>,
    pub full: Vec<u8>,
}

/// Thumbnails of an RGBA8 image (`width × height × 4` bytes).
#[uniffi::export]
pub fn prepare_image(rgba: Vec<u8>, width: u32, height: u32) -> Result<PreparedImage, LichenError> {
    let t = lc_media::image::thumbnail(&rgba, width, height).map_err(|_| LichenError::BadImage)?;
    Ok(PreparedImage {
        width: t.width,
        height: t.height,
        preview: t.preview,
        full: t.full,
    })
}

/// Bytes of the FILE datagram carrying `image`'s preview (for estimates).
#[uniffi::export]
pub fn image_datagram_len(image: &PreparedImage) -> u32 {
    21 + image.preview.len() as u32
}

/// UUIDs of the MeshCore companion's Bluetooth service (Nordic UART).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BleUuids {
    pub service: String,
    /// The app writes command frames here.
    pub rx: String,
    /// The companion notifies frames here.
    pub tx: String,
}

#[uniffi::export]
pub fn companion_ble_uuids() -> BleUuids {
    BleUuids {
        service: "6E400001-B5A3-F393-E0A9-E50E24DCCA9E".into(),
        rx: "6E400002-B5A3-F393-E0A9-E50E24DCCA9E".into(),
        tx: "6E400003-B5A3-F393-E0A9-E50E24DCCA9E".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_and_connects() {
        let core = LichenCore::new("test".into(), vec![1; 32], vec![2; 32], None).unwrap();
        core.connect(0);
        let first = core.poll_companion(0).unwrap();
        assert_eq!(first[0], 1, "APP_START comes first");
        assert!(LichenCore::new("x".into(), vec![1; 31], vec![2; 32], None).is_err());
        assert!(LichenCore::new("x".into(), vec![1; 32], vec![2; 31], None).is_err());
        assert_eq!(
            core.send_text(vec![0; 3], "x".into(), 0, 0),
            Err(LichenError::InvalidKey)
        );
        assert_eq!(
            core.send_text(vec![0; 32], "x".into(), 0, 0),
            Err(LichenError::UnknownContact)
        );
    }
}

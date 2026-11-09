//! UniFFI bindings of the lichen core (`lc-core`) for the iOS and Android apps.
//!
//! The app is the runner: it passes companion frames and the time in, writes the frames
//! `poll_companion` returns to the companion (BLE or USB), calls `tick` at `next_wakeup`, and
//! reads events. Keys and ids cross the boundary as byte arrays.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use lc_core::content::{MessageRef, REACTION_MAX, message_ref};
use lc_core::location::{self as loc, EXTRAPOLATE_MAX_S};
use lc_core::store::{self, Chat, Status, Store, StoredPosition};
use lc_core::{
    Config, Content, Core, Event, FailReason as CoreFail, Fix, GroupId, Outbound, Precision,
    PubKey, Stage,
};
use lc_proto::location::{ALT_UNKNOWN, HEADING_UNKNOWN, SPEED_UNKNOWN};
use lc_proto::search::BearingClass;

mod bridge;
mod demo;

pub use bridge::{BridgeError, LichenBridge, Offered, OnionStatus, onion_address};
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
    /// No search runs for this member.
    NotSearching,
    /// Rate limited: possible again in `wait_s` seconds.
    TooSoon {
        wait_s: u64,
    },
    /// Our position is unknown or too old.
    NoPosition,
    /// The phone's database could not be read or written.
    Storage,
    /// A frame of the message already went out, or it is not waiting: it can no longer be
    /// cancelled (§6.1).
    NotCancellable,
    /// This message has no reference (an image from before replies existed): it cannot be
    /// answered or reacted to (§6.2).
    NotReferable,
    /// The phone's clock is not known yet.
    NoClock,
    /// The invitation has expired (§10.5).
    Expired,
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
            lc_core::Error::NotSearching => Self::NotSearching,
            lc_core::Error::TooSoon { wait_ms } => Self::TooSoon {
                wait_s: wait_ms.div_ceil(1000),
            },
            lc_core::Error::NoPosition => Self::NoPosition,
            lc_core::Error::Started | lc_core::Error::NotQueued => Self::NotCancellable,
            lc_core::Error::NoClock => Self::NoClock,
            lc_core::Error::Expired => Self::Expired,
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
    /// The nickname the user gave, else the node's name.
    pub name: String,
    /// The name the node advertises.
    pub node_name: String,
    /// Repeaters on the known path, if any.
    pub hops: Option<u8>,
    /// The phone identity, once known.
    pub identity: Option<Vec<u8>>,
    /// Compared in person (QR code or safety number).
    pub verified: bool,
    /// A person's companion: someone to talk to. Repeaters, room servers and sensors are not.
    pub person: bool,
}

impl ContactInfo {
    fn of(c: &lc_core::Contact, nicknames: &HashMap<PubKey, String>) -> Self {
        Self {
            key: c.key.to_vec(),
            name: nicknames
                .get(&c.key)
                .cloned()
                .unwrap_or_else(|| c.name.clone()),
            node_name: c.name.clone(),
            hops: c.out_path.as_ref().map(lc_core::OwnedPath::hops),
            identity: c.identity.map(|i| i.to_vec()),
            verified: c.verified,
            person: c.is_person(),
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

/// A search we run (protocol.md §11): the target's node (the app's handle) and identity.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SearchState {
    pub gid: Vec<u8>,
    pub node: Vec<u8>,
    pub identity: Vec<u8>,
    pub measurements: u32,
    /// From the target's last BEACON.
    pub tx_dbm: Option<i8>,
    pub battery: Option<u8>,
    /// Unix seconds of the newest reception (ours or shared).
    pub last_heard_s: Option<u64>,
    pub last_fix: Option<SearchFix>,
    /// How long until a ping may go (0: now).
    pub ping_in_ms: u64,
}

/// A position the target reported.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SearchFix {
    pub at_s: u64,
    pub lat: f64,
    pub lon: f64,
    pub radius_m: f64,
}

/// One reception of the target, by us (`mine`) or another searcher (`by`: its node when known,
/// else its identity).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SearchMeasurement {
    pub by: Vec<u8>,
    pub mine: bool,
    pub at_s: u64,
    pub lat: f64,
    pub lon: f64,
    pub accuracy_m: f64,
    /// dBm; `None` for a bearing alone.
    pub rssi: Option<i8>,
    pub snr_db: f32,
    pub noise: Option<i8>,
    /// RSSI estimated from noise + SNR; saturated: the true RSSI is higher.
    pub estimated: bool,
    pub saturated: bool,
    pub bearing_deg: Option<u16>,
    /// The bearing came from a directional antenna (else body shadowing or other).
    pub bearing_yagi: bool,
}

/// Where the target probably is: the heat grid (row-major from the north-west corner, 255 the
/// densest cell) over its bounds, and the 68 % / 95 % ellipses.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SearchEstimate {
    pub lat: f64,
    pub lon: f64,
    pub major68_m: f64,
    pub minor68_m: f64,
    pub major95_m: f64,
    pub minor95_m: f64,
    /// The major axis, degrees from north.
    pub azimuth_deg: f64,
    pub heat_north: f64,
    pub heat_south: f64,
    pub heat_west: f64,
    pub heat_east: f64,
    pub heat_cols: u16,
    pub heat_rows: u16,
    pub heat: Vec<u8>,
    pub observations: u32,
    /// The path loss was fitted to the target's own GPS positions.
    pub calibrated: bool,
}

/// A member searching for us (the last ten minutes).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SearchedBy {
    pub gid: Vec<u8>,
    pub by: Vec<u8>,
}

/// What our SOS is doing (the app adds the radio link and the location permission).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SosStatus {
    pub on: bool,
    pub fix_fresh: bool,
    /// No fresh fix: BEACONs without a position go to the groups.
    pub beacon_only: bool,
    pub recipients: u32,
    /// When the last SOS position or BEACON was handed to the radio queue, Unix seconds.
    pub last_queued_s: Option<u64>,
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
    /// Content offered over the Internet bridge (§14.1): fetch it with `LichenBridge`.
    Announce {
        sent_at: u32,
        media_id: Vec<u8>,
        /// 1 image, 2 voice, 3 file.
        kind: u8,
        hash: Vec<u8>,
        size: u32,
        providers: Vec<Provider>,
        name: String,
    },
    Other {
        content_type: u8,
        body: Vec<u8>,
    },
}

/// Where announced content can be fetched: transport 1 iroh, 2 Tor.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Provider {
    pub transport: u8,
    pub key: Vec<u8>,
}

/// How long a received position stays on the map, in seconds.
const POSITION_KEEP_S: u64 = 24 * 3600;

impl From<Content> for Message {
    fn from(c: Content) -> Self {
        match c {
            // A reply is a text; its quote comes with the stored message (§6.2).
            Content::Text { sent_at, text } | Content::Reply { sent_at, text, .. } => {
                Self::Text { sent_at, text }
            }
            // Reactions are state, never shown as messages.
            c @ Content::Reaction { .. } => {
                let mut body = c.encode();
                let content_type = body.remove(0);
                Self::Other { content_type, body }
            }
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
            Content::Announce(a) => Self::Announce {
                sent_at: a.sent_at,
                media_id: a.media_id.to_vec(),
                kind: a.kind,
                hash: a.hash.to_vec(),
                size: a.size,
                providers: a
                    .providers
                    .into_iter()
                    .map(|(transport, key)| Provider {
                        transport,
                        key: key.to_vec(),
                    })
                    .collect(),
                name: a.name,
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
    /// The contact, verified, now presents another identity: accept or reject it first.
    IdentityChanged,
    /// Not delivered within its expiry (24 h, §6.1).
    Expired,
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
            CoreFail::IdentityChanged => Self::IdentityChanged,
            CoreFail::Expired => Self::Expired,
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
    /// Contact `node`, verified, presented another identity, not adopted: ask the user to
    /// compare safety numbers, then `accept_identity` or `reject_identity`.
    IdentityConflict {
        node: Vec<u8>,
        identity: Vec<u8>,
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
    /// Group message `id` went out (its first copy, §10.3).
    GroupSent {
        id: u64,
    },
    /// `by` (a node in a direct conversation, an identity in a group) reacted to a message
    /// (§6.2); empty `emoji`: removed. `target`: the message, when it is on this phone.
    Reacted {
        chat: ChatRef,
        by: Vec<u8>,
        emoji: String,
        target: Option<StoredMessage>,
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
    /// Node `node` joined group `gid`, which we administer, with an invitation (§10.5).
    MemberJoined {
        gid: Vec<u8>,
        node: Vec<u8>,
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
    /// Our search for `target` (identity) changed: read the estimate again (§11).
    SearchUpdated {
        target: Vec<u8>,
    },
    /// Member `by` (node) of group `gid` is searching for us, and we answer.
    Searched {
        gid: Vec<u8>,
        by: Vec<u8>,
    },
    /// A BEACON from member `from` (node) of `gid`: `sos` while it is in SOS, with or without a
    /// position (an SOS without GPS); clear when it left SOS.
    Beacon {
        gid: Vec<u8>,
        from: Vec<u8>,
        sos: bool,
        battery: Option<u8>,
    },
    /// Our radio's TX power is now `dbm` (`previous`: lowered by a search, to restore).
    TxPower {
        dbm: i8,
        previous: Option<i8>,
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
            Event::IdentityConflict { node, identity } => Self::IdentityConflict {
                node: node.to_vec(),
                identity: identity.to_vec(),
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
            Event::GroupSent { id } => Self::GroupSent { id },
            Event::GroupJoined { gid, name, admin } => Self::GroupJoined {
                gid: gid.to_vec(),
                name,
                admin: admin_node(&gid, &admin).to_vec(),
            },
            Event::GroupUpdated { gid } => Self::GroupUpdated { gid: gid.to_vec() },
            Event::GroupLeft { gid } => Self::GroupLeft { gid: gid.to_vec() },
            Event::MemberJoined { gid, node } => Self::MemberJoined {
                gid: gid.to_vec(),
                node: node.to_vec(),
            },
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
            Event::SearchUpdated { target } => Self::SearchUpdated {
                target: target.to_vec(),
            },
            Event::Searched { gid, by } => Self::Searched {
                gid: gid.to_vec(),
                by: by.to_vec(),
            },
            Event::TxPower { dbm, previous } => Self::TxPower { dbm, previous },
            Event::Beacon {
                gid,
                from,
                sos,
                battery,
            } => Self::Beacon {
                gid: gid.to_vec(),
                from: from.to_vec(),
                sos,
                battery,
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

/// Why a message we sent is waiting (§6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum WaitReason {
    /// The radio is disconnected.
    Radio,
    /// Earlier messages to the same person go first.
    Earlier,
    /// Looking for a path to them.
    Path,
    /// Opening a secure session with them.
    Handshake,
    /// The airtime budget holds it until about `at_ms` (Unix milliseconds).
    Budget { at_ms: i64 },
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MessageStatus {
    Received,
    /// Accepted and waiting (§6.1). `started`: a frame of it already went out, so it may have
    /// arrived and can no longer be cancelled. It stops at `expires_at_ms` (Unix ms) unless it
    /// is being transmitted then.
    Waiting {
        reason: WaitReason,
        started: bool,
        expires_at_ms: i64,
    },
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
    /// Seen by the user (ours always are): the unread divider goes before the first one that
    /// is not.
    pub read: bool,
    /// It has a reference: it can be answered and reacted to (§6.2).
    pub referable: bool,
    /// The message it answers, for a reply.
    pub quote: Option<Quote>,
    /// Reactions to it, the most frequent first.
    pub reactions: Vec<ReactionCount>,
}

/// What kind of message a reply answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum QuoteKind {
    Text,
    Voice,
    Photo,
    Other,
}

/// The message a reply answers (§6.2), as far as this phone knows it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Quote {
    /// The original in this conversation's history; `None`: not on this phone.
    pub message_id: Option<i64>,
    /// Ours.
    pub outgoing: bool,
    /// Its author's identity, in a group.
    pub author: Option<Vec<u8>>,
    pub kind: QuoteKind,
    /// The start of its text.
    pub excerpt: String,
}

/// One emoji's reactions to a message.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ReactionCount {
    pub emoji: String,
    pub count: u32,
    /// Ours is among them.
    pub mine: bool,
}

/// A conversation's local preferences: pinned (since when, Unix ms) and muted (until when).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ChatPrefs {
    pub chat: ChatRef,
    pub pinned_at_ms: Option<i64>,
    pub muted_until_ms: Option<i64>,
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
                // Without the outbox's view (it is filled in by `Inner::present`).
                Status::Queued => MessageStatus::Waiting {
                    reason: WaitReason::Radio,
                    started: false,
                    expires_at_ms: 0,
                },
                Status::Sending { sent, total } => MessageStatus::Sending { sent, total },
                Status::Delivered => MessageStatus::Delivered,
                Status::Failed(r) => MessageStatus::Failed { reason: r.into() },
                Status::Flooded => MessageStatus::Flooded,
            },
            fragments: m.fragments,
            airtime_ms: m.airtime_ms,
            snr_db: m.snr4.map(snr),
            read: m.read,
            referable: m.msg_ref.is_some(),
            quote: None,
            reactions: Vec::new(),
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
    /// Names the user gave contacts on this phone (node → nickname).
    nicknames: HashMap<PubKey, String>,
    /// Unfinished texts (§6.1), by row id.
    outbox: HashMap<i64, Pending>,
    /// How long a text waits before it expires (ms).
    outbox_expiry_ms: i64,
    /// Unix ms minus `now_ms`, from `set_clock`.
    clock_offset: Option<i64>,
    /// Our reactions being sent: core id → conversation and target (§6.2).
    reaction_inflight: HashMap<u64, (Chat, MessageRef)>,
}

/// How long a text waits for the radio and its recipient (§6.1).
const OUTBOX_EXPIRY_MS: i64 = 24 * 3_600_000;

/// An unfinished text in the outbox (§6.1).
#[derive(Clone, Debug)]
struct Pending {
    core_id: u64,
    chat: Chat,
    /// Unix ms.
    expires_ms: i64,
    /// Its status in the database: queued, or sending once a frame went out.
    stored: Status,
    fragments: u8,
    /// The status the app was last told.
    presented: MessageStatus,
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
        self.nicknames = store.nicknames().unwrap_or_default();
        for c in store.contacts().unwrap_or_default() {
            self.core.restore_contact(c);
        }
        for snap in store.groups().unwrap_or_default() {
            self.core.restore_group(&snap);
        }
        for (chat, precision, until) in store.shares().unwrap_or_default() {
            let _ = self.core.restore_share(chat, precision, until);
        }
        for gid in store.search_consents().unwrap_or_default() {
            self.core.restore_search_consent(&gid);
        }
        // A TX power a search lowered before the app stopped: the radio kept it.
        if let Ok(Some(v)) = store.setting(TX_POWER_RESTORE)
            && let [dbm] = v.as_slice()
        {
            self.core.restore_tx_power(*dbm as i8);
        }
        let now_s = u64::try_from(unix_ms() / 1000).unwrap_or(0);
        let _ = store.prune_positions(now_s.saturating_sub(POSITION_KEEP_S));
        self.store = Some(store);
        self.backfill_refs();
        self.resume_outbox();
        self.resume_reactions();
    }

    /// Texts unfinished when the app stopped are submitted again with their stored plaintext;
    /// media unfinished fail (`Interrupted`), §6.1.
    fn resume_outbox(&mut self) {
        let Some(rows) = self.store.as_ref().and_then(|s| s.resume_unfinished().ok()) else {
            return;
        };
        for (m, expires) in rows {
            let expires = expires.unwrap_or(m.at_ms + self.outbox_expiry_ms);
            let datagram = m.content.encode();
            let now = self.last_now;
            let id = match m.chat {
                Chat::Contact(k) => self.core.send_datagram(&k, datagram, now),
                Chat::Group(g) => self.core.send_group_datagram(&g, &datagram, now),
            };
            match id {
                Ok(id) => self.track(m.id, id, m.chat, expires, m.status, m.fragments),
                Err(_) => {
                    if let Some(st) = &self.store {
                        let _ = st.set_status(m.id, Status::Failed(CoreFail::Interrupted));
                    }
                }
            }
        }
    }

    /// Follows row `row`, sent as core message `core_id`, in the outbox.
    fn track(
        &mut self,
        row: i64,
        core_id: u64,
        chat: Chat,
        expires_ms: i64,
        stored: Status,
        fragments: u8,
    ) {
        self.inflight.insert(core_id, row);
        let mut p = Pending {
            core_id,
            chat,
            expires_ms,
            stored,
            fragments,
            presented: MessageStatus::Received,
        };
        p.presented = self.live_status(&p, self.core.outgoing(core_id, self.last_now));
        self.outbox.insert(row, p);
    }

    /// Unix ms minus the app's clock.
    fn offset(&self) -> i64 {
        self.clock_offset
            .unwrap_or_else(|| unix_ms() - i64::try_from(self.last_now).unwrap_or(0))
    }

    /// What the app should show for an unfinished text, from the core's view (§6.1).
    fn live_status(&self, p: &Pending, live: Option<Outbound>) -> MessageStatus {
        let sending = match p.stored {
            Status::Sending { sent, total } => MessageStatus::Sending { sent, total },
            _ => MessageStatus::Sending {
                sent: 0,
                total: p.fragments,
            },
        };
        let Some(o) = live else {
            return sending;
        };
        let reason = match o.stage {
            Stage::Transmitting => return sending,
            Stage::Offline => WaitReason::Radio,
            Stage::Behind => WaitReason::Earlier,
            Stage::FindingPath => WaitReason::Path,
            Stage::Connecting => WaitReason::Handshake,
            Stage::Budget { until } => WaitReason::Budget {
                at_ms: i64::try_from(until).unwrap_or(i64::MAX) + self.offset(),
            },
        };
        MessageStatus::Waiting {
            reason,
            started: o.started || !matches!(p.stored, Status::Queued),
            expires_at_ms: p.expires_ms,
        }
    }

    /// A stored message as the app should see it: an unfinished text with its live status, a
    /// reply with what it answers, the reactions to it.
    fn present(&self, m: store::Message) -> StoredMessage {
        let row = m.id;
        let quote = match &m.content {
            Content::Reply { target, .. } => Some(self.quote(&m.chat, target)),
            _ => None,
        };
        let reactions = m
            .msg_ref
            .map(|r| self.reaction_counts(&m.chat, &r))
            .unwrap_or_default();
        let mut out = StoredMessage::from(m);
        if let Some(p) = self.outbox.get(&row) {
            out.status = self.live_status(p, self.core.outgoing(p.core_id, self.last_now));
        }
        out.quote = quote;
        out.reactions = reactions;
        out
    }

    /// What reference `target` of `chat` points at (§6.2).
    fn quote(&self, chat: &Chat, target: &MessageRef) -> Quote {
        let Some(m) = self
            .store
            .as_ref()
            .and_then(|s| s.message_by_ref(chat, target).ok().flatten())
        else {
            return Quote {
                message_id: None,
                outgoing: false,
                author: None,
                kind: QuoteKind::Other,
                excerpt: String::new(),
            };
        };
        let (kind, excerpt) = match &m.content {
            Content::Text { text, .. } | Content::Reply { text, .. } => {
                (QuoteKind::Text, text.chars().take(100).collect())
            }
            Content::Voice { .. } => (QuoteKind::Voice, String::new()),
            Content::Image { .. } | Content::Announce(_) => (QuoteKind::Photo, String::new()),
            _ => (QuoteKind::Other, String::new()),
        };
        Quote {
            message_id: Some(m.id),
            outgoing: m.outgoing,
            author: m.author.map(|a| a.to_vec()),
            kind,
            excerpt,
        }
    }

    /// The reactions to message `target` of `chat`, by emoji, the most frequent first.
    fn reaction_counts(&self, chat: &Chat, target: &MessageRef) -> Vec<ReactionCount> {
        let Some(st) = &self.store else {
            return Vec::new();
        };
        let me = self.core.key().copied();
        let mut out: Vec<ReactionCount> = Vec::new();
        for (reactor, emoji) in st.reactions_for(chat, target).unwrap_or_default() {
            let mine = Some(reactor) == me;
            match out.iter_mut().find(|c| c.emoji == emoji) {
                Some(c) => {
                    c.count += 1;
                    c.mine |= mine;
                }
                None => out.push(ReactionCount {
                    emoji,
                    count: 1,
                    mine,
                }),
            }
        }
        out.sort_by_key(|c| std::cmp::Reverse(c.count));
        out
    }

    /// The identity of `chat`'s author of a message: ours, a group member's, or the contact's
    /// (§6.2).
    fn author_identity(
        &self,
        chat: &Chat,
        outgoing: bool,
        author: Option<PubKey>,
    ) -> Option<PubKey> {
        if outgoing {
            return self.core.key().copied();
        }
        match chat {
            Chat::Group(_) => author,
            Chat::Contact(k) => self
                .core
                .contacts()
                .find(|c| c.key == *k)
                .and_then(|c| c.identity),
        }
    }

    /// The reference of a message of `chat` with `plaintext` as sent (§6.2).
    fn reference(
        &self,
        chat: &Chat,
        outgoing: bool,
        author: Option<PubKey>,
        plaintext: &[u8],
    ) -> Option<MessageRef> {
        self.author_identity(chat, outgoing, author)
            .map(|a| message_ref(&a, plaintext))
    }

    /// Fills in the references of messages from before version 9 (§6.2). An image's cannot
    /// be: the sender kept the full variant, not the preview that was sent.
    fn backfill_refs(&mut self) {
        let Some(st) = &self.store else {
            return;
        };
        loop {
            let rows = st.messages_without_ref(500).unwrap_or_default();
            if rows.is_empty() {
                break;
            }
            for m in rows {
                let r = match m.content {
                    Content::Image { .. } => None,
                    ref c => self.reference(&m.chat, m.outgoing, m.author, &c.encode()),
                };
                if st.set_ref(m.id, r.as_ref()).is_err() {
                    return;
                }
            }
        }
    }

    /// Sends our reaction to `target` in `chat` (§6.2) and follows it until delivered.
    fn submit_reaction(&mut self, chat: Chat, target: MessageRef, emoji: &str, ts: u32) {
        let datagram = Content::Reaction {
            sent_at: ts,
            target,
            emoji: emoji.to_owned(),
        }
        .encode();
        let now = self.last_now;
        let id = match chat {
            Chat::Contact(k) => self.core.send_datagram(&k, datagram, now),
            Chat::Group(g) => self.core.send_group_datagram(&g, &datagram, now),
        };
        // Not accepted now: it stays pending and goes again at the next start.
        if let Ok(id) = id {
            self.reaction_inflight.insert(id, (chat, target));
        }
    }

    /// Our reactions not delivered when the app stopped go again, unless too old (§6.2).
    fn resume_reactions(&mut self) {
        let (Some(st), Some(me)) = (&self.store, self.core.key().copied()) else {
            return;
        };
        let now = unix_ms();
        for p in st.pending_reactions(&me).unwrap_or_default() {
            if p.expires_ms.is_some_and(|e| e <= now) {
                if let Some(st) = &self.store {
                    let _ = st.reaction_done(&p.chat, &p.target, &me);
                }
            } else {
                self.submit_reaction(p.chat, p.target, &p.emoji, p.ts);
            }
        }
    }

    /// A reaction from `by` (`by_wire`: as the app knows the sender), §6.2.
    fn on_reaction(
        &mut self,
        chat: Chat,
        target: MessageRef,
        by: PubKey,
        by_wire: &PubKey,
        emoji: &str,
        ts: u32,
    ) {
        let applied = self.store.as_ref().is_some_and(|s| {
            s.upsert_reaction(&chat, &target, &by, emoji, ts, false, None)
                .unwrap_or(false)
        });
        if !applied {
            return;
        }
        let target_row = self
            .store
            .as_ref()
            .and_then(|s| s.message_by_ref(&chat, &target).ok().flatten())
            .map(|m| self.present(m));
        if let Some(t) = &target_row {
            self.extra
                .push_back(LichenEvent::MessageChanged { message: t.clone() });
        }
        self.extra.push_back(LichenEvent::Reacted {
            chat: ChatRef::from_store(&chat),
            by: by_wire.to_vec(),
            emoji: emoji.to_owned(),
            target: target_row,
        });
    }

    /// Expires overdue texts and tells the app when an unfinished text's status changed
    /// (§6.1).
    fn refresh_outbox(&mut self) {
        if self.outbox.is_empty() {
            return;
        }
        let now_unix = unix_ms();
        let rows: Vec<i64> = self.outbox.keys().copied().collect();
        for row in rows {
            let Some(p) = self.outbox.get(&row).cloned() else {
                continue;
            };
            let live = self.core.outgoing(p.core_id, self.last_now);
            let transmitting = live.is_some_and(|o| o.stage == Stage::Transmitting);
            if p.expires_ms <= now_unix && !transmitting {
                self.outbox.remove(&row);
                // Found: its `Failed` event updates the row; otherwise do it here.
                if !self.core.abandon(p.core_id) {
                    self.inflight.remove(&p.core_id);
                    if let Some(m) = self.store.as_ref().and_then(|s| {
                        s.set_status(row, Status::Failed(CoreFail::Expired))
                            .ok()
                            .flatten()
                    }) {
                        let message = self.present(m);
                        self.extra
                            .push_back(LichenEvent::MessageChanged { message });
                    }
                }
                continue;
            }
            let status = self.live_status(&p, live);
            if status == p.presented {
                continue;
            }
            if let Some(m) = self.store.as_ref().and_then(|s| s.get(row).ok().flatten()) {
                let mut out = self.present(m);
                out.status = status.clone();
                self.extra
                    .push_back(LichenEvent::MessageChanged { message: out });
            }
            if let Some(q) = self.outbox.get_mut(&row) {
                q.presented = status;
            }
        }
    }

    /// A TEXT or VOICE already stored from the same sender in `chat`: a resend (§6.1).
    fn is_resend(&self, chat: &Chat, author: Option<&PubKey>, content: &Content) -> bool {
        matches!(
            content,
            Content::Text { .. } | Content::Reply { .. } | Content::Voice { .. }
        ) && self
            .store
            .as_ref()
            .is_some_and(|s| s.has_duplicate(chat, author, content).unwrap_or(false))
    }

    /// The node of identity `id` (the app knows people by node): a group member or a contact.
    fn node_of(&self, id: &PubKey) -> Option<PubKey> {
        if Some(id) == self.core.key() {
            return self.core.node_key().copied();
        }
        self.core
            .groups()
            .flat_map(|g| g.members.iter())
            .find(|m| m.key == *id)
            .map(|m| m.node)
            .or_else(|| {
                self.core
                    .contacts()
                    .find(|c| c.identity == Some(*id))
                    .map(|c| c.key)
            })
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

    /// Saves the groups that changed, and follows the outbox.
    fn persist(&mut self) {
        self.refresh_outbox();
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

    /// Stores a message we just queued and tracks its delivery. A text (`outbox`) waits in
    /// the outbox until it is delivered, fails or expires (§6.1).
    fn stored_outgoing(
        &mut self,
        chat: Chat,
        content: Content,
        id: u64,
        (fragments, airtime_ms): (u8, u64),
        outbox: bool,
        wire: Option<&[u8]>,
    ) -> StoredMessage {
        let msg_ref = self.reference(&chat, true, None, wire.unwrap_or(&content.encode()));
        let status = if outbox {
            Status::Queued
        } else {
            Status::Sending {
                sent: 0,
                total: fragments,
            }
        };
        let row = store::Message {
            id: 0,
            chat,
            outgoing: true,
            author: None,
            content,
            at_ms: unix_ms(),
            status,
            fragments,
            airtime_ms,
            snr4: None,
            read: true,
            msg_ref,
        };
        let stored = self
            .store
            .as_ref()
            .and_then(|s| s.insert(&row).ok())
            .unwrap_or(row);
        if stored.id != 0 {
            if outbox {
                let expires = stored.at_ms + self.outbox_expiry_ms;
                if let Some(st) = &self.store {
                    let _ = st.set_expiry(stored.id, Some(expires));
                }
                self.track(stored.id, id, chat, expires, status, fragments);
            } else {
                self.inflight.insert(id, stored.id);
            }
        }
        self.persist();
        self.present(stored)
    }

    fn record(&mut self, m: store::Message) -> Option<StoredMessage> {
        let row = self.store.as_ref()?.insert(&m).ok()?;
        let out = self.present(row);
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
            self.outbox.remove(&row);
        } else if let Some(p) = self.outbox.get_mut(&row) {
            p.stored = status;
        }
        if let Some(m) = self
            .store
            .as_ref()
            .and_then(|s| s.set_status(row, status).ok().flatten())
        {
            let out = self.present(m);
            if let Some(p) = self.outbox.get_mut(&row) {
                p.presented = out.status.clone();
            }
            self.extra
                .push_back(LichenEvent::MessageChanged { message: out });
        }
    }

    /// Records what an event means for the history. False: a resend already received, to
    /// keep from the app (§6.1).
    fn on_event(&mut self, e: &Event) -> bool {
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
                    let message = self.present(m);
                    self.extra
                        .push_back(LichenEvent::MessageChanged { message });
                }
            }
            Event::Received {
                from,
                content:
                    Content::Reaction {
                        sent_at,
                        target,
                        emoji,
                    },
                ..
            } => {
                let chat = Chat::Contact(*from);
                if let Some(by) = self.author_identity(&chat, false, None) {
                    self.on_reaction(chat, *target, by, from, emoji, *sent_at);
                }
                return false;
            }
            Event::GroupMessage {
                gid,
                from,
                content:
                    Content::Reaction {
                        sent_at,
                        target,
                        emoji,
                    },
                ..
            } => {
                self.on_reaction(Chat::Group(*gid), *target, *from, from, emoji, *sent_at);
                return false;
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
                let chat = Chat::Contact(*from);
                if self.is_resend(&chat, None, content) {
                    return false;
                }
                let msg_ref = self.reference(&chat, false, None, &content.encode());
                self.record(store::Message {
                    id: 0,
                    chat,
                    outgoing: false,
                    author: None,
                    content: content.clone(),
                    at_ms: unix_ms(),
                    status: Status::Received,
                    fragments: 0,
                    airtime_ms: 0,
                    snr4: Some(*snr4),
                    read: false,
                    msg_ref,
                });
            }
            Event::GroupMessage {
                gid,
                from,
                content,
                snr4,
                ..
            } => {
                let chat = Chat::Group(*gid);
                if self.is_resend(&chat, Some(from), content) {
                    return false;
                }
                let msg_ref = self.reference(&chat, false, Some(*from), &content.encode());
                self.record(store::Message {
                    id: 0,
                    chat,
                    outgoing: false,
                    author: Some(*from),
                    content: content.clone(),
                    at_ms: unix_ms(),
                    status: Status::Received,
                    fragments: 0,
                    airtime_ms: 0,
                    snr4: Some(*snr4),
                    read: false,
                    msg_ref,
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
            Event::TxPower { previous, .. } => {
                if let Some(st) = &self.store {
                    let v = previous.map(|p| [p as u8]);
                    let _ = st.set_setting(TX_POWER_RESTORE, v.as_ref().map(|v| v.as_slice()));
                }
            }
            Event::Delivered { id } | Event::GroupSent { id }
                if self.reaction_inflight.contains_key(id) =>
            {
                if let (Some((chat, target)), Some(me), Some(st)) = (
                    self.reaction_inflight.remove(id),
                    self.core.key().copied(),
                    &self.store,
                ) {
                    let _ = st.reaction_done(&chat, &target, &me);
                }
                return false;
            }
            // A reaction that failed stays pending: it goes again at the next start (§6.2).
            Event::Failed { id, .. } if self.reaction_inflight.contains_key(id) => {
                self.reaction_inflight.remove(id);
                return false;
            }
            Event::Delivered { id } => self.update(*id, Status::Delivered, true),
            Event::Failed { id, reason } => self.update(*id, Status::Failed(*reason), true),
            Event::GroupSent { id } => self.update(*id, Status::Flooded, true),
            _ => {}
        }
        true
    }
}

/// Store setting: the TX power to restore (one i8), while a search's POWER holds it.
const TX_POWER_RESTORE: &str = "tx_power_restore";

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
            nicknames: HashMap::new(),
            extra: VecDeque::new(),
            last_now: 0,
            outbox: HashMap::new(),
            outbox_expiry_ms: OUTBOX_EXPIRY_MS,
            clock_offset: None,
            reaction_inflight: HashMap::new(),
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
        i.last_now = now_ms;
        i.core.connect(now_ms);
        i.persist();
    }

    /// The companion is gone (Bluetooth or USB lost): nothing goes out, texts wait and a
    /// transfer in progress pauses until `connect` (§6.1).
    pub fn disconnect(&self, now_ms: u64) {
        let mut i = self.lock();
        i.last_now = now_ms;
        i.core.disconnect(now_ms);
        i.persist();
    }

    /// Cancels a text that is still waiting and none of whose frames went out; it is deleted
    /// (§6.1).
    pub fn cancel_message(&self, message_id: i64) -> Result<(), LichenError> {
        let mut i = self.lock();
        let p = i
            .outbox
            .get(&message_id)
            .cloned()
            .filter(|p| p.stored == Status::Queued)
            .ok_or(LichenError::NotCancellable)?;
        i.core.cancel(p.core_id)?;
        i.outbox.remove(&message_id);
        i.inflight.remove(&p.core_id);
        if let Some(st) = &i.store {
            st.delete_message(message_id)
                .map_err(|_| LichenError::Storage)?;
        }
        Ok(())
    }

    /// Sends a failed text or voice message again: the same message (row and plaintext), so
    /// that a receiver who already has it drops the copy (§6.1). A text waits again for 24 h;
    /// a voice message is refused when over budget.
    pub fn retry_message(
        &self,
        message_id: i64,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let mut i = self.lock();
        i.last_now = now_ms;
        let m = i
            .store
            .as_ref()
            .and_then(|s| s.get(message_id).ok().flatten())
            .ok_or(LichenError::Storage)?;
        if !m.outgoing || !matches!(m.status, Status::Failed(_)) {
            return Err(LichenError::NotAllowed);
        }
        let datagram = m.content.encode();
        let text = matches!(m.content, Content::Text { .. } | Content::Reply { .. });
        let id = match (&m.chat, &m.content) {
            (Chat::Contact(k), Content::Text { .. } | Content::Reply { .. }) => {
                i.core.send_datagram(k, datagram, now_ms)?
            }
            (Chat::Group(g), Content::Text { .. } | Content::Reply { .. }) => {
                i.core.send_group_datagram(g, &datagram, now_ms)?
            }
            (Chat::Contact(k), Content::Voice { .. }) => i.core.send_media(k, datagram, now_ms)?,
            _ => return Err(LichenError::NotAllowed),
        };
        let status = if text {
            Status::Queued
        } else {
            Status::Sending {
                sent: 0,
                total: m.fragments,
            }
        };
        let expires = text.then(|| unix_ms() + i.outbox_expiry_ms);
        if let Some(st) = &i.store {
            st.set_status(m.id, status)
                .map_err(|_| LichenError::Storage)?;
            st.set_expiry(m.id, expires)
                .map_err(|_| LichenError::Storage)?;
        }
        match expires {
            Some(e) => i.track(m.id, id, m.chat, e, status, m.fragments),
            None => {
                i.inflight.insert(id, m.id);
            }
        }
        i.persist();
        let row = i
            .store
            .as_ref()
            .and_then(|s| s.get(m.id).ok().flatten())
            .ok_or(LichenError::Storage)?;
        Ok(i.present(row))
    }

    /// Tests: how long a text waits before it expires (default 24 h).
    pub fn set_outbox_expiry(&self, ms: u64) {
        self.lock().outbox_expiry_ms = i64::try_from(ms).unwrap_or(i64::MAX);
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
        let i = self.lock();
        let core = i.core.next_wakeup();
        // The next text to expire (§6.1); one already overdue waits for its transfer to end.
        let now_unix = unix_ms();
        let offset = i.offset();
        let expiry = i
            .outbox
            .values()
            .map(|p| p.expires_ms)
            .filter(|&e| e > now_unix)
            .min()
            .map(|e| u64::try_from(e - offset).unwrap_or(0));
        match (core, expiry) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The next event. History changes come as `MessageChanged` right after the event that
    /// caused them.
    pub fn poll_event(&self) -> Option<LichenEvent> {
        let mut i = self.lock();
        let e = loop {
            if let Some(e) = i.extra.pop_front() {
                return Some(e);
            }
            let e = i.core.poll_event()?;
            let keep = i.on_event(&e);
            i.persist();
            if keep {
                break e;
            }
        };
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
            .map(|k| ContactInfo::of(k, &i.nicknames))
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

    /// The new identity a verified contact presented, waiting for the user.
    pub fn pending_identity(&self, node: Vec<u8>) -> Result<Option<Vec<u8>>, LichenError> {
        let k = key(&node)?;
        Ok(self.lock().core.pending_identity(&k).map(|i| i.to_vec()))
    }

    /// Accepts contact `node`'s new identity `expected`, the one whose safety number the user
    /// compared, verified when `verify`. False when another identity is pending since: show
    /// the new numbers and ask again.
    pub fn accept_identity(
        &self,
        node: Vec<u8>,
        expected: Vec<u8>,
        verify: bool,
    ) -> Result<bool, LichenError> {
        let (k, e) = (key(&node)?, key(&expected)?);
        let mut i = self.lock();
        let done = i.core.accept_identity(&k, &e, verify);
        i.save_contacts();
        Ok(done)
    }

    /// Rejects contact `node`'s new identity `expected`: the verified one stays.
    pub fn reject_identity(&self, node: Vec<u8>, expected: Vec<u8>) -> Result<(), LichenError> {
        let (k, e) = (key(&node)?, key(&expected)?);
        self.lock().core.reject_identity(&k, &e);
        Ok(())
    }

    /// The safety number between us and an identity (for a pending one, before accepting it).
    pub fn safety_number_with(&self, identity: Vec<u8>) -> Result<Vec<String>, LichenError> {
        let peer = key(&identity)?;
        let i = self.lock();
        let me = i.core.key().ok_or(LichenError::NoIdentity)?;
        Ok(lc_core::identity::safety_number(me, &peer).to_vec())
    }

    /// Marks contact `node` verified (safety numbers matched) or not (they differ); `identity`
    /// is the one whose number the user compared. False when the contact's identity changed
    /// meanwhile: nothing is verified.
    pub fn set_verified(
        &self,
        node: Vec<u8>,
        identity: Vec<u8>,
        verified: bool,
    ) -> Result<bool, LichenError> {
        let (k, id) = (key(&node)?, key(&identity)?);
        let mut i = self.lock();
        let done = i.core.set_verified(&k, &id, verified);
        i.save_contacts();
        Ok(done)
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
        let i = self.lock();
        let mut v: Vec<ContactInfo> = i
            .core
            .contacts()
            .map(|c| ContactInfo::of(c, &i.nicknames))
            .collect();
        v.sort_by_key(|c| c.name.to_lowercase());
        v
    }

    /// Names contact `node` on this phone only (`None` or blank: its node's name again).
    pub fn rename_contact(&self, node: Vec<u8>, name: Option<String>) -> Result<(), LichenError> {
        let k = key(&node)?;
        let mut i = self.lock();
        if !i.core.contacts().any(|c| c.key == k) {
            return Err(LichenError::UnknownContact);
        }
        let name = name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
        if let Some(s) = &i.store {
            let _ = s.set_nickname(&k, name.as_deref());
        }
        match name {
            Some(n) => i.nicknames.insert(k, n),
            None => i.nicknames.remove(&k),
        };
        i.extra.push_back(LichenEvent::ContactsUpdated);
        Ok(())
    }

    /// "Let this group locate me" (protocol.md §11): answer its members' pings and power
    /// requests outside SOS. Kept across restarts.
    pub fn set_search_consent(
        &self,
        gid: Vec<u8>,
        on: bool,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let g = self::gid(&gid)?;
        let mut i = self.lock();
        if !i.core.groups().any(|x| x.gid == g) {
            return Err(LichenError::UnknownGroup);
        }
        if let Some(st) = &i.store {
            let _ = st.set_search_consent(&g, on);
        }
        i.core.set_search_consent(&g, on, now_ms);
        i.persist();
        Ok(())
    }

    pub fn search_consent(&self, gid: Vec<u8>) -> bool {
        self::gid(&gid).is_ok_and(|g| self.lock().core.search_consent(&g))
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
                            name: i
                                .nicknames
                                .get(&m.node)
                                .cloned()
                                .unwrap_or_else(|| m.name.clone()),
                            node_name: m.name.clone(),
                            hops: None,
                            identity: Some(m.key.to_vec()),
                            verified: contact
                                .is_some_and(|c| c.verified && c.identity == Some(m.key)),
                            person: true,
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

    /// What a group text of `text_len` bytes costs us: fragments and the airtime of our own
    /// floods (both copies). Relays spend theirs, and nobody acknowledges (§10.3).
    pub fn estimate_group_text(&self, text_len: u32) -> Estimate {
        let (fragments, airtime_ms) = self.lock().core.estimate_group_send(5 + text_len as usize);
        Estimate {
            fragments,
            airtime_ms,
        }
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
        i.last_now = now_ms;
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
        Ok(i.stored_outgoing(Chat::Contact(k), content, id, estimate, true, None))
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
        Ok(i.stored_outgoing(Chat::Contact(k), content, id, estimate, false, None))
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
        let id = i.core.send_media(&k, datagram.clone(), now_ms)?;
        if let Some(st) = &i.store {
            let _ = st.put_media(&media_id, &Chat::Contact(k), &full.encode());
        }
        // What we keep in our history is the full image; its reference, the preview sent.
        Ok(i.stored_outgoing(Chat::Contact(k), full, id, estimate, false, Some(&datagram)))
    }

    /// Announces content offered over the bridge (§14.1) to a contact or a group. `media_id`
    /// links it to a LoRa preview sent before (empty: a new one).
    #[allow(clippy::too_many_arguments)]
    pub fn send_announce(
        &self,
        chat: ChatRef,
        media_id: Vec<u8>,
        kind: u8,
        hash: Vec<u8>,
        size: u64,
        providers: Vec<Provider>,
        name: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let c = chat.to_store()?;
        let mut i = self.lock();
        i.last_now = now_ms;
        let media_id: [u8; 8] = match <[u8; 8]>::try_from(media_id.as_slice()) {
            Ok(m) => m,
            Err(_) => i.core.random_id(),
        };
        let content = Content::Announce(lc_core::Announce {
            sent_at,
            media_id,
            kind,
            hash: hash
                .as_slice()
                .try_into()
                .map_err(|_| LichenError::InvalidKey)?,
            size: u32::try_from(size).map_err(|_| LichenError::TooLarge)?,
            providers: providers
                .iter()
                .map(|p| key(&p.key).map(|k| (p.transport, k)))
                .collect::<Result<_, _>>()?,
            name,
        });
        let datagram = content.encode();
        match c {
            Chat::Contact(k) => {
                let estimate = i.core.estimate_send(&k, datagram.len());
                let id = i.core.send_datagram(&k, datagram, now_ms)?;
                Ok(i.stored_outgoing(Chat::Contact(k), content, id, estimate, false, None))
            }
            Chat::Group(g) => {
                i.core.send_group_datagram(&g, &datagram, now_ms)?;
                let msg_ref = i.reference(&Chat::Group(g), true, None, &datagram);
                let row = store::Message {
                    id: 0,
                    chat: Chat::Group(g),
                    outgoing: true,
                    author: None,
                    content,
                    at_ms: unix_ms(),
                    status: Status::Flooded,
                    fragments: 1,
                    airtime_ms: 0,
                    snr4: None,
                    read: true,
                    msg_ref,
                };
                let stored = i
                    .store
                    .as_ref()
                    .and_then(|s| s.insert(&row).ok())
                    .unwrap_or(row);
                i.persist();
                Ok(i.present(stored))
            }
        }
    }

    /// Answers message `to_message_id` of `chat` (§6.2): a text that waits in the outbox like
    /// any other (§6.1). Refused for a message without a reference.
    pub fn send_reply(
        &self,
        chat: ChatRef,
        to_message_id: i64,
        text: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let c = chat.to_store()?;
        if text.is_empty() {
            return Err(LichenError::Empty);
        }
        let mut i = self.lock();
        i.last_now = now_ms;
        let target = i
            .store
            .as_ref()
            .and_then(|s| s.get(to_message_id).ok().flatten())
            .filter(|m| m.chat == c)
            .and_then(|m| m.msg_ref)
            .ok_or(LichenError::NotReferable)?;
        let content = Content::Reply {
            sent_at,
            target,
            text,
        };
        let datagram = content.encode();
        let (id, estimate) = match c {
            Chat::Contact(k) => (
                i.core.send_datagram(&k, datagram.clone(), now_ms)?,
                i.core.estimate_send(&k, datagram.len()),
            ),
            Chat::Group(g) => (
                i.core.send_group_datagram(&g, &datagram, now_ms)?,
                i.core.estimate_group_send(datagram.len()),
            ),
        };
        Ok(i.stored_outgoing(c, content, id, estimate, true, None))
    }

    /// Reacts to message `message_id` with `emoji` (empty removes our reaction), §6.2: shown
    /// at once, sent, and sent again after a restart until delivered (24 h at most). Returns
    /// the message with its reactions.
    pub fn send_reaction(
        &self,
        message_id: i64,
        emoji: String,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        if emoji.len() > REACTION_MAX {
            return Err(LichenError::TooLarge);
        }
        let mut i = self.lock();
        i.last_now = now_ms;
        let me = *i.core.key().ok_or(LichenError::NoIdentity)?;
        let m = i
            .store
            .as_ref()
            .and_then(|s| s.get(message_id).ok().flatten())
            .ok_or(LichenError::Storage)?;
        let target = m.msg_ref.ok_or(LichenError::NotReferable)?;
        let chat = m.chat;
        // Each change goes after the previous one, even within the same second.
        let previous = i
            .store
            .as_ref()
            .and_then(|s| s.reaction(&chat, &target, &me).ok().flatten())
            .map_or(0, |(_, ts)| ts);
        let ts = u32::try_from(unix_ms() / 1000)
            .unwrap_or(u32::MAX)
            .max(previous.saturating_add(1));
        let expires = unix_ms() + i.outbox_expiry_ms;
        if let Some(st) = &i.store {
            st.upsert_reaction(&chat, &target, &me, &emoji, ts, true, Some(expires))
                .map_err(|_| LichenError::Storage)?;
        }
        i.submit_reaction(chat, target, &emoji, ts);
        i.persist();
        let row = i
            .store
            .as_ref()
            .and_then(|s| s.get(message_id).ok().flatten())
            .ok_or(LichenError::Storage)?;
        Ok(i.present(row))
    }

    /// What a reaction costs us in `chat` (§6.2): fragments and the airtime of our own
    /// transmissions (a group's two floods); relays and acknowledgements add theirs.
    pub fn estimate_reaction(&self, chat: ChatRef) -> Result<Estimate, LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        // ts, reference and a 4-byte emoji.
        let len = 1 + 4 + 8 + 4;
        let (fragments, airtime_ms) = match c {
            Chat::Contact(k) => i.core.estimate_send(&k, len),
            Chat::Group(_) => i.core.estimate_group_send(len),
        };
        Ok(Estimate {
            fragments,
            airtime_ms,
        })
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

    /// Floods a text to the group and returns it as stored: waiting until its first copy has
    /// left (then `Flooded`, without acknowledgement), §6.1.
    pub fn send_group_text(
        &self,
        gid: Vec<u8>,
        text: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<StoredMessage, LichenError> {
        let g = self::gid(&gid)?;
        let mut i = self.lock();
        i.last_now = now_ms;
        let id = i.core.send_group_text(&g, &text, sent_at, now_ms)?;
        let content = Content::Text { sent_at, text };
        let estimate = i.core.estimate_group_send(content.encode().len());
        Ok(i.stored_outgoing(Chat::Group(g), content, id, estimate, true, None))
    }

    /// Invites contacts (nodes whose identity is known) into a group we administer (ADR-0017).
    pub fn add_members(
        &self,
        gid: Vec<u8>,
        members: Vec<Vec<u8>>,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let g = self::gid(&gid)?;
        let ms = members
            .iter()
            .map(|m| key(m))
            .collect::<Result<Vec<_>, _>>()?;
        let mut i = self.lock();
        i.core.add_members(&g, &ms, now_ms)?;
        i.persist();
        Ok(())
    }

    /// Leaves a group: a member asks the admin to remove it and forgets the group at once; the
    /// admin closes it for everyone (ADR-0017). The conversation stays on the phone.
    pub fn leave_group(&self, gid: Vec<u8>, now_ms: u64) -> Result<(), LichenError> {
        let g = self::gid(&gid)?;
        let mut i = self.lock();
        i.core.leave_group(&g, now_ms)?;
        i.persist();
        Ok(())
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
        // No database (the demo, a throwaway core): no history. A database that fails: say so,
        // not an empty conversation.
        let Some(store) = i.store.as_ref() else {
            return Ok(Vec::new());
        };
        Ok(store
            .history(&c, before_id, limit)
            .map_err(|_| LichenError::Storage)?
            .into_iter()
            .map(|m| i.present(m))
            .collect())
    }

    /// Demo and screenshots only: `count` older text messages in `chat`, alternately ours and
    /// theirs, a minute apart and before everything else, the last five of theirs unread.
    /// Refused outside the in-memory database of the demo (no data dir).
    pub fn demo_fill_history(&self, chat: ChatRef, count: u32) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        if i.data_dir.is_some() {
            return Err(LichenError::NotAllowed);
        }
        let store = i.store.as_ref().ok_or(LichenError::Storage)?;
        let start = unix_ms() - i64::from(count) * 60_000 - 3_600_000;
        for n in 0..count {
            let at_ms = start + i64::from(n) * 60_000;
            let outgoing = n % 2 == 0;
            let content = Content::Text {
                sent_at: u32::try_from(at_ms / 1000).unwrap_or(0),
                text: format!("Earlier message {}", n + 1),
            };
            let msg_ref = i.reference(&c, outgoing, None, &content.encode());
            store
                .insert(&store::Message {
                    id: 0,
                    chat: c,
                    outgoing,
                    author: None,
                    content,
                    at_ms,
                    status: if outgoing {
                        Status::Delivered
                    } else {
                        Status::Received
                    },
                    fragments: 1,
                    airtime_ms: 0,
                    snr4: None,
                    // The last five of theirs are left unread (the divider).
                    read: outgoing || n + 10 < count,
                    msg_ref,
                })
                .map_err(|_| LichenError::Storage)?;
        }
        Ok(())
    }

    /// Text messages containing `query`, case and accents aside, newest first, in every
    /// conversation (on this phone only). Under 2 characters: nothing.
    pub fn search_messages(&self, query: String, limit: u32) -> Vec<StoredMessage> {
        if query.trim().chars().count() < 2 {
            return Vec::new();
        }
        let i = self.lock();
        i.store
            .as_ref()
            .and_then(|s| s.search(&query, limit as usize).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|m| i.present(m))
            .collect()
    }

    /// Pinned and muted conversations.
    pub fn chat_prefs(&self) -> Vec<ChatPrefs> {
        self.lock()
            .store
            .as_ref()
            .and_then(|s| s.chat_prefs().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|p| ChatPrefs {
                chat: ChatRef::from_store(&p.chat),
                pinned_at_ms: p.pinned_at,
                muted_until_ms: p.muted_until,
            })
            .collect()
    }

    /// Pins a conversation at the top of the list (the latest pinned first), or unpins it.
    pub fn set_pinned(&self, chat: ChatRef, pinned: bool) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        let st = i.store.as_ref().ok_or(LichenError::Storage)?;
        st.set_pinned(&c, pinned.then(unix_ms))
            .map_err(|_| LichenError::Storage)
    }

    /// Mutes a conversation's notifications until `until_ms` (Unix ms, `i64::MAX`: until the
    /// user turns them back on), or unmutes it (`None`). SOS alerts are never muted.
    pub fn set_muted(&self, chat: ChatRef, until_ms: Option<i64>) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let i = self.lock();
        let st = i.store.as_ref().ok_or(LichenError::Storage)?;
        st.set_muted(&c, until_ms).map_err(|_| LichenError::Storage)
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
                last: i.present(m),
                unread,
            })
            .collect()
    }

    // ----- location (protocol.md §12) ---------------------------------------------------

    /// The wall clock: `unix_ms` is the current Unix time in milliseconds at `now_ms`. Call it
    /// at start and whenever the clock may have jumped; positions wait for it.
    pub fn set_clock(&self, unix_ms: u64, now_ms: u64) {
        let mut i = self.lock();
        i.clock_offset = Some(
            i64::try_from(unix_ms)
                .unwrap_or(i64::MAX)
                .saturating_sub(i64::try_from(now_ms).unwrap_or(0)),
        );
        i.core.set_clock(unix_ms, now_ms);
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

    /// What our SOS is doing now.
    pub fn sos_status(&self, now_ms: u64) -> SosStatus {
        let st = self.lock().core.sos_status(now_ms);
        SosStatus {
            on: st.on,
            fix_fresh: st.fix_fresh,
            beacon_only: st.beacon_only,
            recipients: st.recipients,
            last_queued_s: st.last_queued,
        }
    }

    // ----- search (protocol.md §11) -----

    /// Starts searching for member `target` (node or identity) of group `gid`.
    pub fn start_search(
        &self,
        gid: Vec<u8>,
        target: Vec<u8>,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let (g, k) = (self::gid(&gid)?, key(&target)?);
        let mut i = self.lock();
        i.core.start_search(&g, &k, now_ms)?;
        i.persist();
        Ok(())
    }

    pub fn stop_search(&self, target: Vec<u8>) -> Result<(), LichenError> {
        let k = key(&target)?;
        self.lock().core.stop_search(&k);
        Ok(())
    }

    pub fn searches(&self, now_ms: u64) -> Vec<SearchState> {
        self.lock()
            .core
            .searches(now_ms)
            .into_iter()
            .map(|s| SearchState {
                gid: s.gid.to_vec(),
                node: s.node.to_vec(),
                identity: s.target.to_vec(),
                measurements: u32::try_from(s.measurements).unwrap_or(u32::MAX),
                tx_dbm: s.tx_dbm,
                battery: s.battery,
                last_heard_s: s.last_heard,
                last_fix: s.last_fix.map(|f| SearchFix {
                    at_s: f.at,
                    lat: f.lat,
                    lon: f.lon,
                    radius_m: f.radius_m,
                }),
                ping_in_ms: s.ping_in_ms,
            })
            .collect()
    }

    /// Asks the target to transmit now (`TooSoon` with the wait otherwise).
    pub fn search_ping(&self, target: Vec<u8>, now_ms: u64) -> Result<(), LichenError> {
        let k = key(&target)?;
        let mut i = self.lock();
        i.core.search_ping(&k, now_ms)?;
        i.persist();
        Ok(())
    }

    /// A bearing to the target from here, degrees from true north; `yagi`: taken with a
    /// directional antenna (else by turning around, the body shadowing the signal).
    pub fn search_bearing(
        &self,
        target: Vec<u8>,
        deg: f64,
        yagi: bool,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let k = key(&target)?;
        let class = if yagi {
            BearingClass::Yagi
        } else {
            BearingClass::Body
        };
        let mut i = self.lock();
        i.core.search_bearing(&k, deg, class, now_ms)?;
        i.persist();
        Ok(())
    }

    /// Asks the target to set its TX power to `dbm` for `minutes` (close range).
    pub fn search_lower_power(
        &self,
        target: Vec<u8>,
        dbm: i8,
        minutes: u8,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let k = key(&target)?;
        let mut i = self.lock();
        i.core.search_lower_power(&k, dbm, minutes, now_ms)?;
        i.persist();
        Ok(())
    }

    pub fn search_estimate(&self, target: Vec<u8>) -> Option<SearchEstimate> {
        let k = key(&target).ok()?;
        let e = self.lock().core.search_estimate(&k)?;
        Some(SearchEstimate {
            lat: e.lat,
            lon: e.lon,
            major68_m: e.major68_m,
            minor68_m: e.minor68_m,
            major95_m: e.major95_m,
            minor95_m: e.minor95_m,
            azimuth_deg: e.azimuth_deg,
            heat_north: e.heat.north,
            heat_south: e.heat.south,
            heat_west: e.heat.west,
            heat_east: e.heat.east,
            heat_cols: e.heat.cols,
            heat_rows: e.heat.rows,
            heat: e.heat.cells,
            observations: u32::try_from(e.observations).unwrap_or(u32::MAX),
            calibrated: e.model != lc_core::PathLoss::DEFAULT,
        })
    }

    /// Every reception of the target, newest first.
    pub fn search_measurements(&self, target: Vec<u8>) -> Vec<SearchMeasurement> {
        let Ok(k) = key(&target) else {
            return Vec::new();
        };
        let i = self.lock();
        let me = i.core.key().copied();
        let mut v: Vec<SearchMeasurement> = i
            .core
            .search_measurements(&k)
            .iter()
            .map(|m| SearchMeasurement {
                by: i.node_of(&m.by).unwrap_or(m.by).to_vec(),
                mine: Some(m.by) == me,
                at_s: m.at,
                lat: m.lat,
                lon: m.lon,
                accuracy_m: m.accuracy_m,
                rssi: m.rssi,
                snr_db: snr(m.snr4),
                noise: m.noise,
                estimated: m.estimated,
                saturated: m.saturated,
                bearing_deg: m.bearing.map(|b| b.deg),
                bearing_yagi: m.bearing.is_some_and(|b| b.class == BearingClass::Yagi),
            })
            .collect();
        v.reverse();
        v
    }

    /// The target's receptions as CSV (field tests, ADR-0016), oldest first.
    pub fn search_csv(&self, target: Vec<u8>) -> String {
        let Ok(k) = key(&target) else {
            return String::new();
        };
        let i = self.lock();
        let me = i.core.key().copied();
        let mut out = String::from(
            "unix_s,searcher,mine,lat,lon,accuracy_m,rssi_dbm,snr_db,noise_dbm,estimated,saturated,\
             bearing_deg,bearing_class,target_tx_dbm\n",
        );
        let opt = |v: Option<i8>| v.map_or(String::new(), |x| x.to_string());
        for m in i.core.search_measurements(&k) {
            let by = i.node_of(&m.by).unwrap_or(m.by);
            let (deg, class) = m.bearing.map_or((String::new(), ""), |b| {
                let c = match b.class {
                    BearingClass::Yagi => "yagi",
                    BearingClass::Body => "body",
                    BearingClass::Other => "other",
                };
                (b.deg.to_string(), c)
            });
            out.push_str(&format!(
                "{},{},{},{:.6},{:.6},{:.0},{},{:.2},{},{},{},{},{},{}\n",
                m.at,
                by.iter()
                    .take(4)
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
                u8::from(Some(m.by) == me),
                m.lat,
                m.lon,
                m.accuracy_m,
                opt(m.rssi),
                f32::from(m.snr4) / 4.0,
                opt(m.noise),
                u8::from(m.estimated),
                u8::from(m.saturated),
                deg,
                class,
                opt(m.tx_dbm),
            ));
        }
        out
    }

    /// Members searching for us (the last ten minutes).
    pub fn searched_by(&self, now_ms: u64) -> Vec<SearchedBy> {
        self.lock()
            .core
            .searched_by(now_ms)
            .into_iter()
            .map(|(g, by)| SearchedBy {
                gid: g.to_vec(),
                by: by.to_vec(),
            })
            .collect()
    }

    /// The phone's battery level (0..=100), sent in our BEACONs.
    pub fn set_battery(&self, percent: Option<u8>) {
        self.lock().core.set_battery(percent);
    }

    pub fn mark_read(&self, chat: ChatRef) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        if let Some(s) = self.lock().store.as_ref() {
            let _ = s.mark_read(&c);
        }
        Ok(())
    }

    /// Deletes a conversation's messages (and the media kept for it). The contact or the
    /// group stays: a new message starts the conversation again.
    pub fn delete_conversation(&self, chat: ChatRef) -> Result<(), LichenError> {
        let c = chat.to_store()?;
        let mut i = self.lock();
        // Its waiting texts stop too.
        let waiting: Vec<(i64, u64)> = i
            .outbox
            .iter()
            .filter(|(_, p)| p.chat == c)
            .map(|(row, p)| (*row, p.core_id))
            .collect();
        for (row, core_id) in waiting {
            i.outbox.remove(&row);
            i.inflight.remove(&core_id);
            if i.core.cancel(core_id).is_err() {
                i.core.abandon(core_id);
            }
        }
        if let Some(s) = i.store.as_ref() {
            let _ = s.delete_chat(&c);
        }
        Ok(())
    }

    /// Announces our node (a MeshCore advert) so that nearby radios add it as a contact;
    /// `flood` lets repeaters relay it.
    pub fn send_advert(&self, flood: bool, now_ms: u64) {
        let mut i = self.lock();
        i.core.send_advert(flood, now_ms);
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

/// Encodes 8 kHz mono samples (at most 15 s are kept) in Codec2 `mode` (§13.1: 0 = 700C,
/// 2 = 1600, 3 = 2400; an unknown mode gives the default, 1600). The speech is prepared first
/// (level, rumble), so the app hands in the microphone's samples as they are.
#[uniffi::export]
pub fn prepare_voice(pcm: Vec<i16>, mode: u8) -> PreparedVoice {
    let mode = lc_media::voice::Mode::from_code(mode).unwrap_or(lc_media::voice::ENCODE_MODE);
    let e = lc_media::voice::encode_mode(&pcm, mode);
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

/// Bytes of a VOICE datagram of `duration_ms` in `mode` (the cost while recording).
#[uniffi::export]
pub fn voice_datagram_len_for(mode: u8, duration_ms: u32) -> u32 {
    let mode = lc_media::voice::Mode::from_code(mode).unwrap_or(lc_media::voice::ENCODE_MODE);
    let frames = duration_ms
        .div_ceil(lc_media::voice::FRAME_MS)
        .min(u32::from(lc_media::voice::MAX_FRAMES)) as u16;
    8 + lc_media::voice::data_len(mode, frames) as u32
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

/// A QR code's modules, row by row (`width` × `width`, true = dark), error correction M: the
/// contact card's code, drawn by each app (§4.7).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct QrCode {
    pub width: u32,
    pub dark: Vec<bool>,
}

#[uniffi::export]
pub fn qr_code(text: String) -> Result<QrCode, LichenError> {
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M)
        .map_err(|_| LichenError::TooLarge)?;
    Ok(QrCode {
        width: u32::try_from(code.width()).unwrap_or(0),
        dark: code
            .to_colors()
            .into_iter()
            .map(|c| c == qrcode::Color::Dark)
            .collect(),
    })
}

/// A companion frame wrapped for the radio's USB serial port (`'<' ‖ len ‖ frame`), for apps
/// that reach the radio over USB (Android OTG). Over BLE, frames go as they are.
#[uniffi::export]
pub fn usb_encode(frame: Vec<u8>) -> Result<Vec<u8>, LichenError> {
    let mut out = [0u8; lc_meshcore::FRAME_MAX + 3];
    let n = lc_meshcore::usb::encode(&frame, &mut out).map_err(|_| LichenError::TooLarge)?;
    Ok(out.get(..n).unwrap_or(&[]).to_vec())
}

/// Splits the bytes read from the radio's USB serial port (`'>' ‖ len ‖ frame`) into companion
/// frames.
#[derive(uniffi::Object, Default, Debug)]
pub struct UsbDecoder {
    inner: Mutex<lc_meshcore::usb::Decoder>,
}

#[uniffi::export]
impl UsbDecoder {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self::default()
    }

    /// The frames completed by `bytes`, in order (bytes outside frames are skipped).
    pub fn push(&self, bytes: Vec<u8>) -> Vec<Vec<u8>> {
        let mut d = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut frames = Vec::new();
        for b in bytes {
            if d.push(b) {
                frames.push(d.frame().to_vec());
            }
        }
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long conversation pages back to its first message (audit M06).
    #[test]
    fn history_pages_back_to_the_oldest() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![101; 32], None).unwrap();
        let chat = ChatRef::Contact { key: vec![9; 32] };
        core.demo_fill_history(chat.clone(), 250).unwrap();
        let mut all: Vec<StoredMessage> = Vec::new();
        let mut before = None;
        loop {
            let page = core.history(chat.clone(), before, 100).unwrap();
            let n = page.len();
            before = page.first().map(|m| m.id);
            all.splice(0..0, page);
            if n < 100 {
                break;
            }
        }
        assert_eq!(all.len(), 250);
        assert!(all.windows(2).all(|w| w[0].id < w[1].id));
        assert!(
            matches!(&all[0].message, Message::Text { text, .. } if text == "Earlier message 1")
        );
        // On a real database, refused.
        let dir = std::env::temp_dir().join(format!("lichen-fill-{}", std::process::id()));
        let real = LichenCore::new(
            "t".into(),
            vec![2; 32],
            vec![102; 32],
            Some(dir.display().to_string()),
        )
        .unwrap();
        assert!(matches!(
            real.demo_fill_history(chat, 1),
            Err(LichenError::NotAllowed)
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

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

    #[test]
    fn usb_framing_round_trip() {
        let frame = vec![0x84, 1, 2, 3];
        let wire = usb_encode(frame.clone()).unwrap();
        assert_eq!(&wire[..3], &[b'<', 4, 0]);
        assert!(usb_encode(vec![]).is_err());
        assert!(usb_encode(vec![0; 177]).is_err());
        // The radio's direction, split across reads, with noise before the first frame.
        let d = UsbDecoder::new();
        assert!(d.push(vec![0xAA, b'>', 4]).is_empty());
        let mut rest = vec![0];
        rest.extend_from_slice(&frame);
        rest.extend_from_slice(&[b'>', 1, 0, 0x88]);
        assert_eq!(d.push(rest), vec![frame, vec![0x88]]);
    }

    #[test]
    fn qr_codes() {
        let q = qr_code("lichen:1:test".into()).unwrap();
        assert!(q.width >= 21);
        assert_eq!(q.dark.len(), (q.width * q.width) as usize);
        // The finder pattern's corner is dark.
        assert!(q.dark[0]);
    }
}

#[cfg(test)]
mod essentials {
    use super::*;

    /// Search across conversations, the unread flag, pins and mutes (chat essentials).
    #[test]
    fn search_unread_pins_and_mutes() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![121; 32], None).unwrap();
        let chat = ChatRef::Contact { key: vec![9; 32] };
        core.demo_fill_history(chat.clone(), 30).unwrap();
        let found = core.search_messages("EARLIER message 12".into(), 50);
        assert_eq!(found.len(), 1);
        assert!(
            matches!(&found[0].message, Message::Text { text, .. } if text == "Earlier message 12")
        );
        assert_eq!(core.search_messages("earlier".into(), 50).len(), 30);
        assert_eq!(core.search_messages("earlier".into(), 7).len(), 7);
        assert!(core.search_messages("e".into(), 50).is_empty());

        let unread = |c: &LichenCore| {
            c.history(chat.clone(), None, 100)
                .unwrap()
                .iter()
                .filter(|m| !m.read)
                .count()
        };
        assert_eq!(unread(&core), 5);
        core.mark_read(chat.clone()).unwrap();
        assert_eq!(unread(&core), 0);

        core.set_pinned(chat.clone(), true).unwrap();
        core.set_muted(chat.clone(), Some(i64::MAX)).unwrap();
        let prefs = core.chat_prefs();
        assert_eq!(prefs.len(), 1);
        assert!(prefs[0].pinned_at_ms.is_some());
        assert_eq!(prefs[0].muted_until_ms, Some(i64::MAX));
        core.set_pinned(chat.clone(), false).unwrap();
        core.set_muted(chat, None).unwrap();
        assert!(core.chat_prefs().is_empty());
    }
}

#[cfg(test)]
mod reactions {
    use super::*;

    /// A reaction may arrive before the message it reacts to (§6.2): it shows once that
    /// message is there, and it does not appear as a message itself.
    #[test]
    fn a_reaction_before_its_target() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![140; 32], None).unwrap();
        let mut i = core.lock();
        i.core.restore_contact(lc_core::Contact {
            key: [9; 32],
            name: "Bob".into(),
            out_path: None,
            identity: Some([7; 32]),
            verified: false,
            kind: lc_core::contact_kind::CHAT,
        });
        let text = Content::Text {
            sent_at: 5,
            text: "hi".into(),
        };
        let target = message_ref(&[7; 32], &text.encode());
        let reaction = Event::Received {
            from: [9; 32],
            content: Content::Reaction {
                sent_at: 6,
                target,
                emoji: "👍".into(),
            },
            snr4: 0,
            rssi: 0,
        };
        assert!(!i.on_event(&reaction), "not a message");
        assert!(i.extra.iter().any(|e| matches!(e,
            LichenEvent::Reacted { emoji, target: None, .. } if emoji == "👍")));
        assert!(i.on_event(&Event::Received {
            from: [9; 32],
            content: text,
            snr4: 0,
            rssi: 0,
        }));
        drop(i);
        let h = core
            .history(ChatRef::Contact { key: vec![9; 32] }, None, 10)
            .unwrap();
        assert_eq!(h.len(), 1);
        assert!(h[0].referable);
        assert_eq!(
            h[0].reactions,
            vec![ReactionCount {
                emoji: "👍".into(),
                count: 1,
                mine: false
            }]
        );
    }
}

#[cfg(test)]
mod receiver {
    use super::*;

    /// A copy of a message already stored is dropped, direct or group (§6.1).
    #[test]
    fn resends_are_dropped() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![120; 32], None).unwrap();
        let text = |t: &str| Content::Text {
            sent_at: 5,
            text: t.into(),
        };
        let mut i = core.lock();
        let direct = Event::Received {
            from: [9; 32],
            content: text("hi"),
            snr4: 0,
            rssi: 0,
        };
        assert!(i.on_event(&direct));
        assert!(!i.on_event(&direct));
        let group = |from: u8| Event::GroupMessage {
            gid: [3; 16],
            from: [from; 32],
            content: text("hi"),
            snr4: 0,
            rssi: 0,
        };
        assert!(i.on_event(&group(1)));
        assert!(!i.on_event(&group(1)));
        // Another member's identical text, or another second, is another message.
        assert!(i.on_event(&group(2)));
        assert!(i.on_event(&Event::Received {
            from: [9; 32],
            content: Content::Text {
                sent_at: 6,
                text: "hi".into()
            },
            snr4: 0,
            rssi: 0,
        }));
        drop(i);
        let h = core
            .history(ChatRef::Contact { key: vec![9; 32] }, None, 10)
            .unwrap();
        assert_eq!(h.len(), 2);
    }
}

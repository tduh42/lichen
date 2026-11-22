//! Mesh tools (protocol §16, ADR-0041): the nodes the radio knows or heard, paths and traces.

use super::*;

/// A MeshCore node's type, as its adverts say.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NodeKind {
    Unknown,
    /// A person's radio.
    Companion,
    Repeater,
    RoomServer,
    Sensor,
}

impl NodeKind {
    /// Its MeshCore type (1 to 4); none for an unknown one.
    pub(crate) const fn code(self) -> Option<u8> {
        match self {
            Self::Companion => Some(lc_core::contact_kind::CHAT),
            Self::Repeater => Some(lc_core::contact_kind::REPEATER),
            Self::RoomServer => Some(lc_core::contact_kind::ROOM),
            Self::Sensor => Some(lc_core::contact_kind::SENSOR),
            Self::Unknown => None,
        }
    }

    pub(crate) const fn of(kind: u8) -> Self {
        match kind {
            lc_core::contact_kind::CHAT => Self::Companion,
            lc_core::contact_kind::REPEATER => Self::Repeater,
            lc_core::contact_kind::ROOM => Self::RoomServer,
            lc_core::contact_kind::SENSOR => Self::Sensor,
            _ => Self::Unknown,
        }
    }
}

/// A node the radio knows or heard (§16.1).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct NodeInfo {
    /// The node key.
    pub key: Vec<u8>,
    /// The nickname the user gave, else the name it advertises.
    pub name: String,
    pub kind: NodeKind,
    /// Repeaters on the radio's path to it; `None` when the radio floods to reach it.
    pub hops: Option<u8>,
    /// When the radio last heard from it (Unix seconds, the radio's clock).
    pub last_heard_s: Option<u64>,
    /// The position its adverts carry, when it advertises one.
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// In the radio's contacts; false: heard, not added (manual add or a full table).
    pub on_radio: bool,
    /// A lichen user: their identity is known.
    pub lichen: bool,
}

impl NodeInfo {
    pub(crate) fn of(n: &lc_core::Node, i: &Inner) -> Self {
        let contact = i.core.contacts().find(|c| c.key == n.key);
        Self {
            key: n.key.to_vec(),
            name: i
                .nicknames
                .get(&n.key)
                .cloned()
                .unwrap_or_else(|| n.name.clone()),
            kind: NodeKind::of(n.kind),
            hops: n.out_path.as_ref().map(lc_core::OwnedPath::hops),
            last_heard_s: (n.last_heard != 0).then_some(u64::from(n.last_heard)),
            lat: n.position.map(|(lat, _)| f64::from(lat) / 1e6),
            lon: n.position.map(|(_, lon)| f64::from(lon) / 1e6),
            on_radio: n.on_radio,
            lichen: contact.is_some_and(|c| c.identity.is_some()),
        }
    }
}

/// One hop of a path (§16.2): its hash (hex) and the repeaters it may be.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PathHop {
    pub hash: String,
    /// Names of the repeaters the radio knows whose key starts with the hash: none when
    /// unknown, several when it is ambiguous.
    pub names: Vec<String>,
}

impl From<&lc_core::Hop> for PathHop {
    fn from(h: &lc_core::Hop) -> Self {
        Self {
            hash: hex(&h.hash),
            names: h.names.clone(),
        }
    }
}

pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Where a trace stands (§16.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TraceState {
    Sending,
    Done,
    /// It did not come back in time.
    NoAnswer,
    /// The radio refused it.
    Refused,
}

/// One transmission of a trace: `from` heard by `to` (`None` is our radio).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TraceLeg {
    pub from: Option<PathHop>,
    pub to: Option<PathHop>,
    /// Once the trace is back.
    pub snr_db: Option<f32>,
}

/// A trace along the radio's path to a node and back (§16.3).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TraceInfo {
    pub state: TraceState,
    /// Out and back: our radio to the first hop, …, the last hop to our radio.
    pub legs: Vec<TraceLeg>,
    pub rtt_ms: Option<u64>,
    /// Airtime of our transmission.
    pub airtime_ms: u64,
}

impl From<&lc_core::Trace> for TraceInfo {
    fn from(t: &lc_core::Trace) -> Self {
        let hops: Vec<Option<PathHop>> = t.hops.iter().map(|h| Some(PathHop::from(h))).collect();
        let ends: Vec<Option<PathHop>> = std::iter::once(None)
            .chain(hops.iter().cloned())
            .chain(std::iter::once(None))
            .collect();
        let snrs: Vec<Option<i8>> = t
            .snrs
            .iter()
            .map(|&s| Some(s))
            .chain(std::iter::once(t.final_snr4))
            .collect();
        let legs = ends
            .windows(2)
            .enumerate()
            .map(|(i, w)| TraceLeg {
                from: w.first().cloned().flatten(),
                to: w.get(1).cloned().flatten(),
                snr_db: snrs.get(i).copied().flatten().map(snr),
            })
            .collect();
        Self {
            state: match t.status {
                lc_core::TraceStatus::Sending => TraceState::Sending,
                lc_core::TraceStatus::Done => TraceState::Done,
                lc_core::TraceStatus::NoAnswer => TraceState::NoAnswer,
                lc_core::TraceStatus::Refused => TraceState::Refused,
            },
            legs,
            rtt_ms: t.rtt_ms,
            airtime_ms: t.airtime_ms,
        }
    }
}

/// A channel read from a QR code (§15.3), before joining it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ChannelCode {
    pub name: String,
    /// Its secret, 32 hex digits (`join_private_channel` takes it).
    pub secret_hex: String,
    pub kind: ChannelKind,
    /// The radio already has it.
    pub joined: bool,
}

/// A node as the stock MeshCore app's contact code names it (§15.3).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MeshCoreContact {
    pub name: String,
    /// Its MeshCore public key (32 bytes): the node key.
    pub key: Vec<u8>,
    pub kind: NodeKind,
}

/// A node's contact code in the stock MeshCore app's format (§15.3, MeshCore's
/// `docs/qr_codes.md`): `meshcore://contact/add?name=…&public_key=…&type=…`, for a QR code
/// that a stock app scans to add the node (the radio's own: its name, `Companion`).
/// `InvalidKey` for a key that is not 32 bytes, `InvalidSetting` for `NodeKind::Unknown`.
#[uniffi::export]
pub fn meshcore_contact_code(
    name: String,
    key: Vec<u8>,
    kind: NodeKind,
) -> Result<String, LichenError> {
    let k = self::key(&key)?;
    let kind = kind.code().ok_or(LichenError::InvalidSetting)?;
    Ok(lc_core::plain::contact_code(&name, &k, kind))
}

/// Reads a stock MeshCore contact code (§15.3); `BadCode` when it is not one, its key not 64
/// hex digits, its type not 1 to 4 or its name empty or longer than a MeshCore name.
#[uniffi::export]
pub fn read_meshcore_contact_code(text: String) -> Result<MeshCoreContact, LichenError> {
    let (name, key, kind) =
        lc_core::plain::parse_contact_code(&text).ok_or(LichenError::BadCode)?;
    Ok(MeshCoreContact {
        name,
        key: key.to_vec(),
        kind: NodeKind::of(kind),
    })
}

/// A MeshCore node read from a link the stock app shares (§15.3): its signed advert, or a
/// contact code.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MeshCoreLink {
    pub name: String,
    /// Its MeshCore public key (32 bytes): the node key.
    pub key: Vec<u8>,
    pub kind: NodeKind,
    /// The position its advert carries, when it has one (degrees).
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Signed by the node itself (a shared advert); false for a contact code, which anyone can
    /// write for any key under any name.
    pub signed: bool,
    /// Hops of the path the advert came by to the radio that shared it (0: heard directly, or
    /// a contact code).
    pub hops: u8,
    /// That path's hashes, `hops` of them, each `path.len() / hops` bytes (1 to 3): the first
    /// bytes of the repeaters' keys, from the node outwards. Empty for 0 hops.
    pub path: Vec<u8>,
}

impl From<&lc_core::MeshLink> for MeshCoreLink {
    fn from(l: &lc_core::MeshLink) -> Self {
        Self {
            name: l.name.clone(),
            key: l.key.to_vec(),
            kind: NodeKind::of(l.kind),
            lat: l.position.map(|(lat, _)| f64::from(lat) / 1e6),
            lon: l.position.map(|(_, lon)| f64::from(lon) / 1e6),
            signed: l.signed(),
            hops: l.hops,
            path: l.path.clone(),
        }
    }
}

/// What `add_meshcore_contact` did (§16.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeshCoreAdd {
    /// Sent to the radio. The contact then comes as any other (`ContactsUpdated`: among the
    /// people for a companion, the nodes for the others), or the radio's refusal comes as a
    /// refused setting: a `RadioOutcome` for `RadioSetting::Contact`, `Refused`.
    Sent,
    /// The radio already has it: nothing was sent; open its chat.
    AlreadyOnTheRadio,
    /// The link is the connected radio's own.
    OwnRadio,
}

fn read_link(text: &str) -> Result<lc_core::MeshLink, LichenError> {
    lc_core::plain::read_meshcore_link(text).map_err(|e| match e {
        lc_core::LinkError::NotALink => LichenError::BadCode,
        lc_core::LinkError::BadSignature => LichenError::BadSignature,
    })
}

/// Reads a MeshCore contact link (§15.3, ADR-0047): `meshcore://` and the hex of a node's
/// signed advert, as the stock app shares it, or a contact code
/// (`meshcore://contact/add?…`). `BadCode` when it is neither, or not one a MeshCore node
/// writes; `BadSignature` for an advert whose signature does not verify (changed after its
/// node signed it, or made up).
#[uniffi::export]
pub fn read_meshcore_link(text: String) -> Result<MeshCoreLink, LichenError> {
    Ok(MeshCoreLink::from(&read_link(&text)?))
}

#[uniffi::export]
impl LichenCore {
    /// Adds the node of a MeshCore contact link to the radio (§16.6): the errors of
    /// `read_meshcore_link`, and `NoIdentity` when no radio is connected.
    pub fn add_meshcore_contact(&self, text: String) -> Result<MeshCoreAdd, LichenError> {
        let link = read_link(&text)?;
        let mut inner = self.lock();
        let added = inner.core.add_meshcore_contact(&link)?;
        Ok(match added {
            lc_core::MeshAdd::Sent => MeshCoreAdd::Sent,
            lc_core::MeshAdd::AlreadyOnTheRadio => MeshCoreAdd::AlreadyOnTheRadio,
            lc_core::MeshAdd::OwnRadio => MeshCoreAdd::OwnRadio,
        })
    }
}

#[uniffi::export]
impl LichenCore {
    /// The radio's path to `node`, hop by hop (§16.2): `None` when it floods to reach it (or
    /// does not know it), empty when it hears it directly.
    pub fn node_path(&self, node: Vec<u8>) -> Result<Option<Vec<PathHop>>, LichenError> {
        let k = key(&node)?;
        Ok(self
            .lock()
            .core
            .path_hops(&k)
            .map(|hops| hops.iter().map(PathHop::from).collect()))
    }

    /// Traces the radio's path to `node` and back (§16.3); `NodeUpdated` follows as it goes.
    /// `NoPath` when there is no repeater on the way.
    pub fn start_trace(&self, node: Vec<u8>, now_ms: u64) -> Result<(), LichenError> {
        let k = key(&node)?;
        let mut i = self.lock();
        i.last_now = now_ms;
        Ok(i.core.trace(&k, now_ms)?)
    }

    /// The QR code of channel `id`, in the stock app's format (§15.3): whoever scans it can
    /// read and post on the channel.
    pub fn channel_qr(&self, id: Vec<u8>) -> Result<String, LichenError> {
        let id = channel(&id)?;
        let i = self.lock();
        let c = i
            .core
            .channels()
            .iter()
            .find(|c| c.id() == id)
            .cloned()
            .or_else(|| {
                i.store
                    .as_ref()
                    .and_then(|s| s.channels().ok())
                    .and_then(|v| v.into_iter().find(|c| c.id() == id))
            })
            .ok_or(LichenError::UnknownGroup)?;
        Ok(lc_core::plain::channel_qr(&c.name, &c.secret))
    }

    /// Reads a channel QR code from the stock app (§15.3); `BadCode` when it is not one.
    pub fn parse_channel_qr(&self, text: String) -> Result<ChannelCode, LichenError> {
        let (name, secret) = lc_core::plain::parse_channel_qr(&text).ok_or(LichenError::BadCode)?;
        let c = lc_core::Channel {
            index: 0,
            name: name.clone(),
            secret,
        };
        let joined = self.lock().core.channels().iter().any(|x| x.id() == c.id());
        Ok(ChannelCode {
            name,
            secret_hex: hex(&secret),
            kind: MeshChannel::from(&c).kind,
            joined,
        })
    }

    /// The last trace to `node`.
    pub fn trace(&self, node: Vec<u8>) -> Option<TraceInfo> {
        let k = key(&node).ok()?;
        self.lock().core.trace_of(&k).map(TraceInfo::from)
    }

    /// The nodes the radio reported since it connected, the most recently heard first: its
    /// contacts (people, repeaters, room servers, sensors) and the nodes it heard without
    /// adding them. Empty while the radio is away. `ContactsUpdated` follows each change.
    pub fn nodes(&self) -> Vec<NodeInfo> {
        let i = self.lock();
        i.core.nodes().iter().map(|n| NodeInfo::of(n, &i)).collect()
    }
}

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
    /// The last time our radio heard it directly (§16.1, ADR-0049): SNR, RSSI (none when the
    /// radio does not measure it) and when (Unix seconds, the phone's clock). All `None` for a
    /// node heard only through repeaters, or not since this radio connected and before.
    pub snr_db: Option<f32>,
    pub rssi_dbm: Option<i16>,
    pub signal_at_s: Option<u64>,
    /// That reading's rating under the current preset; `None` without one.
    pub signal: SignalRating,
    /// When it last answered this radio with its readings (§16.7, Unix seconds, the phone's
    /// clock): see `node_readings`.
    pub readings_at_s: Option<u64>,
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
            snr_db: n.signal.map(|r| snr(r.snr4)),
            rssi_dbm: n.signal.and_then(|r| r.rssi).map(i16::from),
            signal_at_s: n.signal.map(|r| r.at_s),
            signal: i.core.signal_rating(n.signal.as_ref()).into(),
            readings_at_s: None,
        }
    }
}

/// What a reading measures (§16.7), for its label; `unit` says in what.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ReadingKind {
    AnalogInput,
    /// A sensor's value without a unit.
    Generic,
    Illuminance,
    Temperature,
    Humidity,
    Pressure,
    Voltage,
    Current,
    Frequency,
    Percentage,
    Altitude,
    Power,
    Distance,
    /// A position the node's owner lets it send.
    Latitude,
    Longitude,
}

/// One labelled value of a node's readings.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReadingValue {
    pub kind: ReadingKind,
    /// 1 is the node itself; sensors are numbered from 2.
    pub channel: u8,
    pub value: f64,
    /// "V", "°C", "%", "hPa", "A", "Hz", "m", "W", "lx", "°", or empty.
    pub unit: String,
}

/// One answer of a node with its readings (§16.7).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct NodeReading {
    /// When it came (Unix seconds, the phone's clock).
    pub at_s: u64,
    /// Its battery (the voltage of channel 1), and its charge by the radio's curve.
    pub battery_mv: Option<u32>,
    pub battery_percent: Option<u8>,
    /// Its temperature (channel 1's, else the first), °C.
    pub temperature_c: Option<f64>,
    /// Everything else it sent, in order.
    pub values: Vec<ReadingValue>,
}

impl NodeReading {
    pub(crate) fn of(r: &lc_core::Readings) -> Self {
        use lc_meshcore::lpp::{Kind, SELF_CHANNEL, Value};
        let mut battery_seen = false;
        let mut temperature_seen = false;
        let mut values = Vec::new();
        for item in &r.items {
            match item.value {
                Value::Scalar { kind, value } => {
                    // The battery and the temperature shown apart, once each.
                    if kind == Kind::Voltage
                        && item.channel == SELF_CHANNEL
                        && !battery_seen
                        && r.battery_mv == Some(value)
                    {
                        battery_seen = true;
                        continue;
                    }
                    if kind == Kind::Temperature
                        && !temperature_seen
                        && r.temperature_dc == Some(value)
                    {
                        temperature_seen = true;
                        continue;
                    }
                    let (k, scale, unit) = match kind {
                        Kind::AnalogInput => (ReadingKind::AnalogInput, 100.0, ""),
                        Kind::Generic => (ReadingKind::Generic, 1.0, ""),
                        Kind::Illuminance => (ReadingKind::Illuminance, 1.0, "lx"),
                        Kind::Temperature => (ReadingKind::Temperature, 10.0, "°C"),
                        Kind::Humidity => (ReadingKind::Humidity, 10.0, "%"),
                        Kind::Pressure => (ReadingKind::Pressure, 10.0, "hPa"),
                        Kind::Voltage => (ReadingKind::Voltage, 1000.0, "V"),
                        Kind::Current => (ReadingKind::Current, 1000.0, "A"),
                        Kind::Frequency => (ReadingKind::Frequency, 1.0, "Hz"),
                        Kind::Percentage => (ReadingKind::Percentage, 1.0, "%"),
                        Kind::Altitude => (ReadingKind::Altitude, 1.0, "m"),
                        Kind::Power => (ReadingKind::Power, 1.0, "W"),
                        Kind::Distance => (ReadingKind::Distance, 1000.0, "m"),
                    };
                    values.push(ReadingValue {
                        kind: k,
                        channel: item.channel,
                        value: value as f64 / scale,
                        unit: unit.to_owned(),
                    });
                }
                Value::Gps {
                    lat_e4,
                    lon_e4,
                    alt_cm,
                } => {
                    for (kind, value, unit) in [
                        (ReadingKind::Latitude, f64::from(lat_e4) / 1e4, "°"),
                        (ReadingKind::Longitude, f64::from(lon_e4) / 1e4, "°"),
                        (ReadingKind::Altitude, f64::from(alt_cm) / 100.0, "m"),
                    ] {
                        values.push(ReadingValue {
                            kind,
                            channel: item.channel,
                            value,
                            unit: unit.to_owned(),
                        });
                    }
                }
            }
        }
        Self {
            at_s: r.at_s,
            battery_mv: r.battery_mv.and_then(|mv| u32::try_from(mv).ok()),
            battery_percent: r.battery_percent(),
            temperature_c: r.temperature_dc.map(|t| t as f64 / 10.0),
            values,
        }
    }
}

/// Where asking a node for its readings stands (§16.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ReadingsState {
    /// Not asked since the radio connected.
    Idle,
    Asking,
    Answered,
    /// It answered without a reading lichen reads.
    AnsweredEmpty,
    /// No answer in time: it does not answer us (its owner's choice; a repeater answers only
    /// those logged in), or it was out of reach.
    NoAnswer,
    /// The radio refused to ask: the node is not one of its contacts.
    Refused,
}

impl From<lc_core::ReadingsState> for ReadingsState {
    fn from(s: lc_core::ReadingsState) -> Self {
        match s {
            lc_core::ReadingsState::Idle => Self::Idle,
            lc_core::ReadingsState::Asking => Self::Asking,
            lc_core::ReadingsState::Answered => Self::Answered,
            lc_core::ReadingsState::AnsweredEmpty => Self::AnsweredEmpty,
            lc_core::ReadingsState::NoAnswer => Self::NoAnswer,
            lc_core::ReadingsState::Refused => Self::Refused,
        }
    }
}

/// How good a reading is (§16.1, ADR-0049): its SNR against the current preset's demodulation
/// limit, and its RSSI; the worse of the two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SignalRating {
    Good,
    Fair,
    Bad,
    /// No reading.
    None,
}

impl From<lc_core::SignalRating> for SignalRating {
    fn from(r: lc_core::SignalRating) -> Self {
        match r {
            lc_core::SignalRating::Good => Self::Good,
            lc_core::SignalRating::Fair => Self::Fair,
            lc_core::SignalRating::Bad => Self::Bad,
            lc_core::SignalRating::None => Self::None,
        }
    }
}

/// Where the ratings start under the current preset (§16.1), for the bands of a signal chart:
/// a reading at a limit or above it is rated above it, as long as the other figure allows.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignalLimits {
    /// SNR from which a reading is Good, and Fair (dB).
    pub good_snr_db: f32,
    pub fair_snr_db: f32,
    /// RSSI from which a reading is Good, and Fair (dBm).
    pub good_rssi_dbm: i16,
    pub fair_rssi_dbm: i16,
}

/// One time our radio heard a node directly (§16.1).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignalReading {
    /// Unix seconds, the phone's clock.
    pub at_s: u64,
    pub snr_db: f32,
    /// `None` when the radio does not measure it.
    pub rssi_dbm: Option<i16>,
    pub signal: SignalRating,
}

/// A stop on the route to a node (§16.3): our radio first, then each repeater of its path,
/// then the node.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RouteStop {
    /// Its node key, when known: our radio's, the node's, or the one repeater the hash names.
    pub key: Option<Vec<u8>>,
    /// A repeater's hash on the path (hex); `None` for our radio and for the node.
    pub hash: Option<String>,
    /// Its name: our radio's, the node's (the nickname the user gave), or the repeaters the
    /// hash may be (none when unknown, several when ambiguous).
    pub names: Vec<String>,
    /// The position its adverts carry. None for our radio: show it where the phone is.
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

/// A leg between two stops: the last SNR measured on it towards us (the nearer stop hearing
/// the farther one), or nothing when nothing measured it (§16.3).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RouteLeg {
    pub snr_db: Option<f32>,
    /// From the frames heard; a trace gives none.
    pub rssi_dbm: Option<i16>,
    pub signal: SignalRating,
    /// When it was measured (Unix seconds, the phone's clock).
    pub at_s: Option<u64>,
    /// Measured by the last trace, not by the frames heard.
    pub traced: bool,
}

/// The route to a node (§16.3): `legs[i]` joins `stops[i]` and `stops[i + 1]`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct NodeRoute {
    pub stops: Vec<RouteStop>,
    pub legs: Vec<RouteLeg>,
}

impl NodeRoute {
    fn of(r: &lc_core::Route, i: &Inner) -> Self {
        let last = r.stops.len().saturating_sub(1);
        let stops = r
            .stops
            .iter()
            .enumerate()
            .map(|(n, s)| {
                // The node itself is shown under the name the user gave it.
                let nickname = (n == last && n > 0)
                    .then(|| s.key.and_then(|k| i.nicknames.get(&k).cloned()))
                    .flatten();
                RouteStop {
                    key: s.key.map(|k| k.to_vec()),
                    hash: (!s.hash.is_empty()).then(|| hex(&s.hash)),
                    names: nickname.map_or_else(|| s.names.clone(), |n| vec![n]),
                    lat: s.position.map(|(lat, _)| f64::from(lat) / 1e6),
                    lon: s.position.map(|(_, lon)| f64::from(lon) / 1e6),
                }
            })
            .collect();
        let legs = r
            .legs
            .iter()
            .map(|l| RouteLeg {
                snr_db: l.reading.map(|r| snr(r.snr4)),
                rssi_dbm: l.reading.and_then(|r| r.rssi).map(i16::from),
                signal: i.core.signal_rating(l.reading.as_ref()).into(),
                at_s: l.reading.map(|r| r.at_s),
                traced: l.traced,
            })
            .collect();
        Self { stops, legs }
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
    /// adding them. Empty while the radio is away. `ContactsUpdated` follows each change, a
    /// node's new signal reading included.
    pub fn nodes(&self) -> Vec<NodeInfo> {
        let i = self.lock();
        let answered: HashMap<PubKey, u64> = match (&i.store, i.core.node_key()) {
            (Some(st), Some(radio)) => st
                .last_readings_times(radio)
                .unwrap_or_default()
                .into_iter()
                .collect(),
            _ => HashMap::new(),
        };
        i.core
            .nodes()
            .iter()
            .map(|n| NodeInfo {
                readings_at_s: answered.get(&n.key).copied(),
                ..NodeInfo::of(n, &i)
            })
            .collect()
    }

    /// Asks `node` for its readings (§16.7): its battery, its temperature, what its sensors
    /// measure. `NodeUpdated` follows each change of `node_readings_state`, and the answer
    /// (`node_readings`). `TooSoon` while one request to it is out; `UnknownContact` for a node
    /// not in the radio's contacts; `NoIdentity` without a radio; `OverBudget` when the request
    /// does not fit the airtime left.
    pub fn request_node_readings(&self, node: Vec<u8>, now_ms: u64) -> Result<(), LichenError> {
        let k = key(&node)?;
        let mut i = self.lock();
        i.last_now = now_ms;
        Ok(i.core.request_readings(&k, now_ms)?)
    }

    /// Where asking `node` for its readings stands.
    pub fn node_readings_state(&self, node: Vec<u8>) -> ReadingsState {
        let Ok(k) = key(&node) else {
            return ReadingsState::Idle;
        };
        self.lock().core.readings_state(&k).into()
    }

    /// What `node` answered this radio (or the last one) over the last 7 days, oldest first,
    /// 100 answers at most (§16.7, ADR-0050). Empty for a key that is not 32 bytes, without a
    /// store or before any radio.
    pub fn node_readings(&self, node: Vec<u8>) -> Vec<NodeReading> {
        let Ok(k) = key(&node) else {
            return Vec::new();
        };
        let i = self.lock();
        let (Some(st), Some(radio)) = (&i.store, i.core.node_key()) else {
            return Vec::new();
        };
        let now_s = u64::try_from(i.wall_ms() / 1000).unwrap_or(0);
        let since = now_s.saturating_sub(lc_core::readings::KEEP_S);
        st.readings(radio, &k, since)
            .unwrap_or_default()
            .iter()
            .map(|(at, data)| NodeReading::of(&lc_core::Readings::decode(data, *at)))
            .collect()
    }

    /// The rating limits under the current preset (§16.1), for the bands of a node's signal
    /// chart; `None` before a radio reported its preset. Read them again on `Connected` and
    /// `RadioSettings` (sent after a preset changed).
    pub fn signal_limits(&self) -> Option<SignalLimits> {
        self.lock().core.signal_limits().map(|l| SignalLimits {
            good_snr_db: f32::from(l.good_snr4) / 4.0,
            fair_snr_db: f32::from(l.fair_snr4) / 4.0,
            good_rssi_dbm: i16::from(l.good_rssi),
            fair_rssi_dbm: i16::from(l.fair_rssi),
        })
    }

    /// What the connected radio (or the last one) heard of `node` directly over the last 7
    /// days, oldest first, 500 readings at most (§16.1, ADR-0049). Empty for a key that is not
    /// 32 bytes, without a store or before any radio. `NodeUpdated` follows each new reading.
    pub fn node_signal_history(&self, node: Vec<u8>) -> Vec<SignalReading> {
        let Ok(k) = key(&node) else {
            return Vec::new();
        };
        let i = self.lock();
        let (Some(st), Some(radio)) = (&i.store, i.core.node_key()) else {
            return Vec::new();
        };
        let now_s = u64::try_from(i.wall_ms() / 1000).unwrap_or(0);
        let since = now_s.saturating_sub(lc_core::signal::HISTORY_KEEP_S);
        st.signal_history(radio, &k, since)
            .unwrap_or_default()
            .iter()
            .map(|r| SignalReading {
                at_s: r.at_s,
                snr_db: snr(r.snr4),
                rssi_dbm: r.rssi.map(i16::from),
                signal: i.core.signal_rating(Some(r)).into(),
            })
            .collect()
    }

    /// The route to `node` (§16.3): our radio, the repeaters of its path and the node, with the
    /// last SNR of each leg and the stops' advertised positions. `None` when the radio floods
    /// to reach it, does not know it, or the key is not 32 bytes. `NodeUpdated` follows a
    /// trace's progress and each new reading.
    pub fn node_route(&self, node: Vec<u8>) -> Option<NodeRoute> {
        let k = key(&node).ok()?;
        let i = self.lock();
        i.core.route(&k).map(|r| NodeRoute::of(&r, &i))
    }
}

//! Records the app reads: the radio, contacts, groups, search, estimates.

use super::*;

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
    pub(crate) fn of(c: &lc_core::Contact, nicknames: &HashMap<PubKey, String>) -> Self {
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

/// A group that lists us and waits for the user's answer (§10.1).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct GroupInvitation {
    pub gid: Vec<u8>,
    pub name: String,
    /// The admin's node, and the name we know it by.
    pub admin: Vec<u8>,
    pub admin_name: String,
    /// The other members' names, as the admin lists them.
    pub members: Vec<String>,
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

/// An invitation to show as a QR code (protocol.md §10.5).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct InviteCode {
    /// What the QR code carries.
    pub text: String,
    /// To revoke it.
    pub token: Vec<u8>,
    /// Unix seconds.
    pub expires: u64,
}

/// What a scanned invitation says, before joining (protocol.md §10.5).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct InviteInfo {
    pub group_name: String,
    pub admin_name: String,
    /// The admin's node: the contact it becomes, and `admin` in `GroupJoined`.
    pub admin: Vec<u8>,
    /// Unix seconds.
    pub expires: u64,
    /// Past its time by this phone's clock (2 minutes allowed for clocks that differ).
    pub expired: bool,
    /// We already belong to a group of that admin with that name.
    pub member: bool,
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
    /// When the last SOS position or BEACON was handed to the radio queue, Unix seconds: queued,
    /// not sent; nobody confirms reception.
    pub last_queued_s: Option<u64>,
    /// Conversations whose last SOS position could not be queued.
    pub refused: u32,
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

/// A small file as it would go over LoRa (§13.2, ADR-0023): compressed when that is smaller.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LoraFile {
    /// Bytes of the datagram on the air (name and header included).
    pub wire_bytes: u32,
    pub compressed: bool,
    pub fragments: u8,
    pub airtime_ms: u64,
}

/// The FILE datagram for `name` and `data` when it can go over LoRa: at most 8 KB of data on the
/// air, 64 KiB once inflated (§13.2). The name is fitted to 64 bytes, its extension kept.
pub(crate) fn small_file(name: String, data: Vec<u8>, sent_at: u32) -> Option<(Content, Vec<u8>)> {
    if data.is_empty() || data.len() > lc_core::content::FILE_INFLATED_MAX {
        return None;
    }
    let name = fit_file_name(name, lc_core::content::FILE_NAME_MAX as u32);
    let content = Content::File {
        sent_at,
        name: if name.is_empty() { "file".into() } else { name },
        data,
    };
    let datagram = content.encode();
    // What a receiver accepts is exactly what fits.
    matches!(Content::decode(&datagram), Some(Content::File { .. })).then_some((content, datagram))
}

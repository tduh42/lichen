//! Positions and shares.

use super::*;

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
    pub(crate) fn new(p: &StoredPosition, now_s: u64) -> Self {
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
pub(crate) fn best_per_sender(all: Vec<StoredPosition>) -> Vec<StoredPosition> {
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

//! Location sharing (protocol.md §12, ADR-0011): degrees and units, fixed grids, dead
//! reckoning, shares that expire, SOS.

use std::collections::{HashMap, HashSet, VecDeque};

use lc_proto::Millis;
use lc_proto::location::{
    ALT_UNKNOWN, FLAG_END, FLAG_SOS, HEADING_UNKNOWN, I24_MAX, I24_MIN, Position, SPEED_UNKNOWN,
    UNITS, unwrap_time,
};

use crate::content::Content;
use crate::core::{Core, Error, Event, MsgId, PubKey};
use crate::group::GroupId;
use crate::store::Chat;

/// Metres per degree of latitude (and of longitude at the equator).
const M_PER_DEG: f64 = 111_320.0;
/// An exact share sends when the position is this far from the extrapolation.
pub const DEVIATION_M: f64 = 50.0;
/// Heartbeats while stationary, in seconds.
pub const HEARTBEAT_CONTACT_S: u64 = 15 * 60;
pub const HEARTBEAT_GROUP_S: u64 = 30 * 60;
/// Fewest seconds between two positions to the same target.
pub const MIN_GAP_CONTACT_S: u64 = 30;
pub const MIN_GAP_GROUP_S: u64 = 5 * 60;
/// SOS period, every target.
pub const SOS_PERIOD_S: u64 = 120;
/// A fix older than this is not sent as current (no heartbeat on a stale fix).
pub const FIX_MAX_AGE_S: u64 = 30 * 60;
/// Extrapolation stops after this long (the marker then waits for the next position).
pub const EXTRAPOLATE_MAX_S: u64 = HEARTBEAT_CONTACT_S;

/// A position from the phone, in degrees and metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fix {
    pub lat: f64,
    pub lon: f64,
    pub alt_m: Option<f64>,
    /// Radius of uncertainty (68 %).
    pub accuracy_m: f64,
    /// Course over ground, degrees from north.
    pub heading_deg: Option<f64>,
    pub speed_mps: Option<f64>,
    /// When it was measured, Unix seconds.
    pub unix_s: u64,
}

/// What a share reveals (§12.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Precision {
    Exact,
    M100,
    Km1,
    Km10,
}

impl Precision {
    /// Grid cell size in metres; `None` for exact.
    pub const fn cell_m(self) -> Option<f64> {
        match self {
            Precision::Exact => None,
            Precision::M100 => Some(100.0),
            Precision::Km1 => Some(1_000.0),
            Precision::Km10 => Some(10_000.0),
        }
    }

    pub const fn code(self) -> u8 {
        match self {
            Precision::Exact => 0,
            Precision::M100 => 1,
            Precision::Km1 => 2,
            Precision::Km10 => 3,
        }
    }

    pub const fn from_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Precision::Exact),
            1 => Some(Precision::M100),
            2 => Some(Precision::Km1),
            3 => Some(Precision::Km10),
            _ => None,
        }
    }
}

/// A position shared with a contact or a group until a given time.
#[derive(Clone, Debug, PartialEq)]
pub struct Share {
    pub chat: Chat,
    pub precision: Precision,
    /// End of the share, Unix seconds.
    pub until: u64,
    /// The last position sent, and when (what the receiver extrapolates from).
    last: Option<(Position, u64)>,
    /// Movement asked for a position the minimum gap held back.
    wanted: bool,
}

impl Share {
    /// A share nothing was sent for yet.
    pub const fn new(chat: Chat, precision: Precision, until: u64) -> Self {
        Self {
            chat,
            precision,
            until,
            last: None,
            wanted: false,
        }
    }
}

/// Location state of the core.
#[derive(Debug, Default)]
pub(crate) struct Locations {
    /// `unix_ms − now`, from [`Core::set_clock`].
    offset_ms: Option<i64>,
    fix: Option<Fix>,
    shares: Vec<Share>,
    sos: bool,
    /// Targets that got SOS positions without a share.
    sos_extra: HashSet<Chat>,
    /// Last SOS position per share target.
    sos_at: HashMap<Chat, u64>,
    /// Newest position time per sender and conversation (§12.1 freshness).
    heard: HashMap<(PubKey, Option<GroupId>), u64>,
    /// Direct positions not finished yet (queued or in flight), and their peer: never
    /// resent, and the share is reset when the newest one fails.
    pub(crate) in_flight: HashMap<MsgId, PubKey>,
}

// ----- units, grids, distances -----------------------------------------------------------------

fn clamp24(v: f64) -> i32 {
    v.round().clamp(f64::from(I24_MIN), f64::from(I24_MAX)) as i32
}

/// Degrees to on-air units.
pub fn to_units(lat: f64, lon: f64) -> (i32, i32) {
    (
        clamp24(lat * f64::from(UNITS) / 90.0),
        clamp24(lon * f64::from(UNITS) / 180.0),
    )
}

/// On-air units to degrees.
pub fn to_degrees(lat: i32, lon: i32) -> (f64, f64) {
    (
        f64::from(lat) * 90.0 / f64::from(UNITS),
        f64::from(lon) * 180.0 / f64::from(UNITS),
    )
}

/// The `prec` byte for a radius: `ceil(16 × log2(r))`, never below 1 m.
pub fn prec_byte(radius_m: f64) -> u8 {
    (16.0 * radius_m.max(1.0).log2()).ceil().clamp(0.0, 255.0) as u8
}

/// The radius a `prec` byte stands for.
pub fn radius_m(prec: u8) -> f64 {
    (f64::from(prec) / 16.0).exp2()
}

/// The centre of the fixed grid cell holding `(lat, lon)` (units), for cells of `cell_m`
/// metres (§12.2): rows of constant latitude height, columns as wide as the row's latitude
/// makes `cell_m`.
pub fn snap(lat: i32, lon: i32, cell_m: f64) -> (i32, i32) {
    let unit_lat_m = 90.0 / f64::from(UNITS) * M_PER_DEG;
    let unit_lon_m = 180.0 / f64::from(UNITS) * M_PER_DEG;
    let h = ((cell_m / unit_lat_m).round() as i64).max(1);
    let row = i64::from(lat).div_euclid(h);
    let lat_c = (row * h + h / 2).clamp(i64::from(I24_MIN), i64::from(I24_MAX));
    let phi = (lat_c as f64 * 90.0 / f64::from(UNITS)).to_radians();
    let w = ((cell_m / (unit_lon_m * phi.cos().max(1e-6))).round() as i64).clamp(1, 1 << 24);
    let col = i64::from(lon).div_euclid(w);
    let lon_c = (col * w + w / 2).clamp(i64::from(I24_MIN), i64::from(I24_MAX));
    (lat_c as i32, lon_c as i32)
}

/// Distance in metres between two points in degrees (equirectangular: exact enough at a few
/// kilometres).
pub fn distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let mean = ((a.0 + b.0) / 2.0).to_radians();
    let dy = (b.0 - a.0) * M_PER_DEG;
    let mut dlon = b.1 - a.1;
    if dlon > 180.0 {
        dlon -= 360.0;
    } else if dlon < -180.0 {
        dlon += 360.0;
    }
    let dx = dlon * M_PER_DEG * mean.cos();
    dx.hypot(dy)
}

/// Where a position sent at `at` should be at `t`, following its heading and speed (for at
/// most [`EXTRAPOLATE_MAX_S`]); sender and receiver compute the same thing.
pub fn extrapolate(p: &Position, at: u64, t: u64) -> (f64, f64) {
    let (lat, lon) = to_degrees(p.lat, p.lon);
    if p.heading == HEADING_UNKNOWN || p.speed == SPEED_UNKNOWN || p.speed == 0 {
        return (lat, lon);
    }
    let dt = t.saturating_sub(at).min(EXTRAPOLATE_MAX_S) as f64;
    let d = f64::from(p.speed) * 0.5 * dt;
    let hd = f64::from(p.heading).to_radians();
    let dlat = d * hd.cos() / M_PER_DEG;
    let dlon = d * hd.sin() / (M_PER_DEG * lat.to_radians().cos().max(1e-6));
    (lat + dlat, lon + dlon)
}

/// The position a share sends for `fix`, stamped `t` (§12.2): exact, or the cell centre
/// without altitude, heading and speed.
pub fn position_for(fix: &Fix, precision: Precision, t: u64, flags: u8) -> Position {
    let (lat, lon) = to_units(fix.lat, fix.lon);
    match precision.cell_m() {
        None => Position {
            lat,
            lon,
            alt: fix.alt_m.map_or(ALT_UNKNOWN, |a| {
                a.round()
                    .clamp(f64::from(i16::MIN + 1), f64::from(i16::MAX)) as i16
            }),
            t16: t as u16,
            heading: fix
                .heading_deg
                .filter(|h| h.is_finite() && *h >= 0.0)
                .map_or(HEADING_UNKNOWN, |h| (h.round() as u16) % 360),
            speed: fix
                .speed_mps
                .filter(|s| s.is_finite() && *s >= 0.0)
                .map_or(SPEED_UNKNOWN, |s| (s * 2.0).round().min(126.0) as u8),
            prec: prec_byte(fix.accuracy_m),
            flags,
        },
        Some(cell) => {
            let (lat, lon) = snap(lat, lon, cell);
            Position {
                lat,
                lon,
                alt: ALT_UNKNOWN,
                t16: t as u16,
                heading: HEADING_UNKNOWN,
                speed: SPEED_UNKNOWN,
                prec: prec_byte(fix.accuracy_m.max(cell * std::f64::consts::FRAC_1_SQRT_2)),
                flags,
            }
        }
    }
}

// ----- the core's side ---------------------------------------------------------------------

impl Core {
    /// Sets the wall clock: `unix_ms` is the current Unix time in milliseconds at `now`.
    /// Positions are neither sent nor accepted before it is known.
    pub fn set_clock(&mut self, unix_ms: u64, now: Millis) {
        let unix = i64::try_from(unix_ms).unwrap_or(i64::MAX);
        self.loc.offset_ms = Some(unix.saturating_sub(i64::try_from(now).unwrap_or(i64::MAX)));
    }

    /// Unix seconds at `now`.
    pub fn unix_s(&self, now: Millis) -> Option<u64> {
        self.loc.unix_s(now)
    }

    /// `now` of a Unix time in seconds.
    pub(crate) fn mono_ms(&self, unix_s: u64) -> Option<Millis> {
        let off = self.loc.offset_ms?;
        let ms = i64::try_from(unix_s.saturating_mul(1000)).unwrap_or(i64::MAX);
        Some(u64::try_from(ms.saturating_sub(off)).unwrap_or(0))
    }

    /// Our current position, from the phone.
    pub fn update_location(&mut self, fix: Fix, now: Millis) {
        if fix.lat.is_finite() && fix.lon.is_finite() {
            self.loc.fix = Some(fix);
        }
        self.tick(now);
    }

    /// Starts (or changes) sharing our position with `chat` until `until` (Unix seconds).
    pub fn start_share(
        &mut self,
        chat: Chat,
        precision: Precision,
        until: u64,
        now: Millis,
    ) -> Result<(), Error> {
        self.restore_share(chat, precision, until)?;
        self.tick(now);
        Ok(())
    }

    /// A share saved by the app, back after a restart (no tick: the clock may not be set yet).
    pub fn restore_share(
        &mut self,
        chat: Chat,
        precision: Precision,
        until: u64,
    ) -> Result<(), Error> {
        self.check_target(&chat)?;
        self.loc.shares.retain(|s| s.chat != chat);
        self.loc.shares.push(Share::new(chat, precision, until));
        Ok(())
    }

    /// Stops sharing with `chat`: one last position with the end flag (§12.2).
    pub fn stop_share(&mut self, chat: &Chat, now: Millis) {
        if let Some(i) = self.loc.shares.iter().position(|s| s.chat == *chat) {
            let s = self.loc.shares.swap_remove(i);
            self.send_end(&s.chat, s.precision, s.last.is_some(), now);
        }
        self.tick(now);
    }

    pub fn shares(&self) -> &[Share] {
        &self.loc.shares
    }

    /// Turns SOS on or off (§12.2).
    pub fn set_sos(&mut self, on: bool, now: Millis) {
        if self.loc.sos == on {
            return;
        }
        self.loc.sos = on;
        if !on {
            // Targets without a share forget us; the others get a normal position again.
            let extra: Vec<Chat> = self.loc.sos_extra.drain().collect();
            for c in extra {
                self.send_end(&c, Precision::Exact, true, now);
            }
            for s in &mut self.loc.shares {
                s.last = None;
            }
            self.loc.sos_at.clear();
        }
        self.tick(now);
    }

    pub const fn sos(&self) -> bool {
        self.loc.sos
    }

    fn check_target(&self, chat: &Chat) -> Result<(), Error> {
        match chat {
            Chat::Contact(k) if self.contacts.contains_key(k) => Ok(()),
            Chat::Group(g) if self.groups.contains_key(g) => Ok(()),
            Chat::Contact(_) => Err(Error::UnknownContact),
            Chat::Group(_) => Err(Error::UnknownGroup),
        }
    }

    /// Sends `position` to `chat` (a datagram or a group flood).
    fn send_position(&mut self, chat: &Chat, position: &Position, now: Millis) {
        let body = Content::Location(*position).encode();
        match chat {
            Chat::Contact(k) => {
                let id = self.enqueue_location(k, body);
                self.loc.in_flight.insert(id, *k);
            }
            Chat::Group(g) => {
                let _ = self.send_group(g, &body, now);
            }
        }
    }

    /// The end flag to `chat`, on our last fix at `precision` (when anything was sent).
    fn send_end(&mut self, chat: &Chat, precision: Precision, sent: bool, now: Millis) {
        let (Some(fix), Some(t), true) = (self.loc.fix, self.unix_s(now), sent) else {
            return;
        };
        if self.check_target(chat).is_ok() {
            let p = position_for(&fix, precision, t, FLAG_END);
            self.send_position(chat, &p, now);
        }
    }

    /// A direct position was delivered or failed: when the newest one to that peer failed,
    /// the next update sends again.
    pub(crate) fn location_done(&mut self, id: MsgId, failed: bool) {
        let Some(peer) = self.loc.in_flight.remove(&id) else {
            return;
        };
        let newer = self
            .loc
            .in_flight
            .iter()
            .any(|(&i, p)| *p == peer && i > id);
        if failed
            && !newer
            && let Some(s) = self
                .loc
                .shares
                .iter_mut()
                .find(|s| s.chat == Chat::Contact(peer))
        {
            s.last = None;
        }
    }

    pub(crate) fn is_location(&self, id: MsgId) -> bool {
        self.loc.in_flight.contains_key(&id)
    }

    /// Expiries, heartbeats, movement, SOS.
    pub(crate) fn tick_locations(&mut self, now: Millis) {
        let Some(t) = self.unix_s(now) else {
            return;
        };
        // Shares of contacts or groups we no longer know, and expired shares.
        let mut ended = Vec::new();
        let mut changed = false;
        let shares = std::mem::take(&mut self.loc.shares);
        for s in shares {
            if self.check_target(&s.chat).is_err() {
                changed = true;
            } else if t >= s.until {
                ended.push(s);
                changed = true;
            } else {
                self.loc.shares.push(s);
            }
        }
        for s in ended {
            self.send_end(&s.chat, s.precision, s.last.is_some(), now);
        }
        if changed {
            self.events.push_back(Event::SharesChanged);
        }

        let Some(fix) = self.loc.fix.filter(|f| f.unix_s + FIX_MAX_AGE_S >= t) else {
            return;
        };
        if self.loc.sos {
            self.tick_sos(&fix, t, now);
            return;
        }
        let mut out = Vec::new();
        for s in &mut self.loc.shares {
            let group = matches!(s.chat, Chat::Group(_));
            let p = position_for(&fix, s.precision, t, 0);
            let (gap, beat) = if group {
                (MIN_GAP_GROUP_S, HEARTBEAT_GROUP_S)
            } else {
                (MIN_GAP_CONTACT_S, HEARTBEAT_CONTACT_S)
            };
            let due = match &s.last {
                None => true,
                Some((last, at)) => {
                    let moved = match s.precision {
                        Precision::Exact => {
                            distance_m(extrapolate(last, *at, t), (fix.lat, fix.lon)) > DEVIATION_M
                        }
                        _ => (last.lat, last.lon) != (p.lat, p.lon),
                    };
                    s.wanted |= moved;
                    t >= at + beat || (s.wanted && t >= at + gap)
                }
            };
            if due {
                s.last = Some((p, t));
                s.wanted = false;
                out.push((s.chat, p));
            }
        }
        for (chat, p) in out {
            self.send_position(&chat, &p, now);
        }
    }

    /// Exact position with the SOS flag to every share and every group, every 2 minutes.
    fn tick_sos(&mut self, fix: &Fix, t: u64, now: Millis) {
        let p = position_for(fix, Precision::Exact, t, FLAG_SOS);
        let mut targets: Vec<Chat> = self.loc.shares.iter().map(|s| s.chat).collect();
        for g in self.groups.keys() {
            let c = Chat::Group(*g);
            if !targets.contains(&c) {
                targets.push(c);
                self.loc.sos_extra.insert(c);
            }
        }
        for c in targets {
            let last = self.loc.sos_at.get(&c).copied();
            if last.is_none_or(|at| t >= at + SOS_PERIOD_S) {
                self.loc.sos_at.insert(c, t);
                if let Some(s) = self.loc.shares.iter_mut().find(|s| s.chat == c) {
                    s.last = Some((p, t));
                }
                self.send_position(&c, &p, now);
            }
        }
    }

    /// When location work is due next.
    pub(crate) fn next_location_wakeup(&self) -> Option<Millis> {
        let fix = self.loc.fix?;
        let mut next: Option<u64> = None;
        let mut consider = |x: u64| next = Some(next.map_or(x, |y: u64| y.min(x)));
        for s in &self.loc.shares {
            consider(s.until);
            match (&s.last, matches!(s.chat, Chat::Group(_))) {
                (None, _) => consider(fix.unix_s),
                (Some((_, at)), group) => {
                    let (gap, beat) = if group {
                        (MIN_GAP_GROUP_S, HEARTBEAT_GROUP_S)
                    } else {
                        (MIN_GAP_CONTACT_S, HEARTBEAT_CONTACT_S)
                    };
                    consider(at + beat);
                    if s.wanted {
                        consider(at + gap);
                    }
                }
            }
        }
        if self.loc.sos {
            for at in self.loc.sos_at.values() {
                consider(at + SOS_PERIOD_S);
            }
        }
        next.and_then(|t| self.mono_ms(t))
    }

    /// A position from `from`, direct or in group `group`: fresh, then reported (§12.1).
    pub(crate) fn on_position(
        &mut self,
        from: PubKey,
        group: Option<GroupId>,
        position: Position,
        snr4: i8,
        now: Millis,
    ) {
        let at = receive(
            &mut self.loc,
            &mut self.events,
            from,
            group,
            position,
            snr4,
            now,
        );
        // A target's own fix, for its searchers (§11).
        if let (Some(at), Some(id)) = (at, self.identity_of(&from)) {
            let (lat, lon) = to_degrees(position.lat, position.lon);
            self.search_fix(&id, lat, lon, position.prec, at);
        }
    }
}

/// [`Core::on_position`] on the fields it needs (callable while a group is borrowed): the
/// position's full time when it is fresh and reported.
pub(crate) fn receive(
    loc: &mut Locations,
    events: &mut VecDeque<Event>,
    from: PubKey,
    group: Option<GroupId>,
    position: Position,
    snr4: i8,
    now: Millis,
) -> Option<u64> {
    let t = loc.unix_s(now)?;
    let at = unwrap_time(position.t16, t)?;
    let key = (from, group);
    if loc.heard.get(&key).is_some_and(|&last| at <= last) {
        return None;
    }
    loc.heard.insert(key, at);
    events.push_back(Event::Location {
        from,
        group,
        position,
        at,
        snr4,
    });
    Some(at)
}

impl Locations {
    pub(crate) const fn fix(&self) -> Option<Fix> {
        self.fix
    }

    pub(crate) const fn sos_on(&self) -> bool {
        self.sos
    }

    fn unix_s(&self, now: Millis) -> Option<u64> {
        let off = self.offset_ms?;
        let now = i64::try_from(now).unwrap_or(i64::MAX);
        u64::try_from(now.saturating_add(off))
            .ok()
            .map(|ms| ms / 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_round_trip() {
        for (lat, lon) in [(45.0, 5.5), (-33.9, 151.2), (89.99, -179.99), (0.0, 0.0)] {
            let (a, b) = to_units(lat, lon);
            let (la, lo) = to_degrees(a, b);
            assert!(distance_m((lat, lon), (la, lo)) < 2.0, "{lat} {lon}");
        }
        // The poles and the antimeridian clamp instead of wrapping.
        assert_eq!(to_units(90.0, 180.0), (I24_MAX, I24_MAX));
        assert_eq!(to_units(-90.0, -180.0), (I24_MIN, I24_MIN));
    }

    #[test]
    fn prec_scale() {
        assert_eq!(prec_byte(1.0), 0);
        assert_eq!(prec_byte(0.2), 0);
        assert_eq!(prec_byte(1000.0), 160);
        assert!(radius_m(prec_byte(37.0)) >= 37.0);
        assert!(radius_m(prec_byte(37.0)) < 37.0 * 1.05);
        assert_eq!(prec_byte(1e9), 255);
    }

    /// Every point of a cell snaps to the same centre, the centre lies in the cell, and the
    /// cell has the requested size.
    #[test]
    fn grid_is_fixed() {
        for cell in [100.0, 1_000.0, 10_000.0] {
            let (lat0, lon0) = (45.1, 5.5);
            let c = snap(to_units(lat0, lon0).0, to_units(lat0, lon0).1, cell);
            let centre = to_degrees(c.0, c.1);
            assert!(distance_m(centre, (lat0, lon0)) <= cell * 0.75, "{cell}");
            // Walk a fine lattice around the point: two distinct points snap to the same
            // centre exactly when they are within the same cell, and the cells are about
            // `cell` wide.
            let mut centres = HashMap::new();
            let step = cell / 7.0 / M_PER_DEG;
            for i in -20..20 {
                for j in -20..20 {
                    let lat = lat0 + f64::from(i) * step;
                    let lon = lon0 + f64::from(j) * step / lat0.to_radians().cos();
                    let (a, b) = to_units(lat, lon);
                    let s = snap(a, b, cell);
                    assert_eq!(snap(s.0, s.1, cell), s, "a centre snaps to itself");
                    let d = distance_m(to_degrees(s.0, s.1), (lat, lon));
                    assert!(d <= cell * 0.72, "{cell}: {d}");
                    *centres.entry(s).or_insert(0) += 1;
                }
            }
            // About 40/7 ≈ 5.7 cells per side.
            assert!(
                (25..=49).contains(&centres.len()),
                "{cell}: {}",
                centres.len()
            );
        }
    }

    /// The same fix always gives the same coarse position: averaging gains nothing.
    #[test]
    fn coarse_never_noisy() {
        let fix = Fix {
            lat: 45.1234,
            lon: 5.5678,
            alt_m: Some(1200.0),
            accuracy_m: 5.0,
            heading_deg: Some(90.0),
            speed_mps: Some(1.4),
            unix_s: 1_793_000_000,
        };
        let a = position_for(&fix, Precision::Km1, 1, 0);
        let b = position_for(
            &Fix {
                lat: fix.lat + 0.0001,
                ..fix
            },
            Precision::Km1,
            2,
            0,
        );
        assert_eq!((a.lat, a.lon), (b.lat, b.lon));
        assert_eq!(a.alt, ALT_UNKNOWN);
        assert_eq!((a.heading, a.speed), (HEADING_UNKNOWN, SPEED_UNKNOWN));
        assert!(radius_m(a.prec) >= 707.0);
        let e = position_for(&fix, Precision::Exact, 1, 0);
        assert_eq!((e.alt, e.heading, e.speed), (1200, 90, 3));
    }

    /// §12.1: a position older than (or as old as) the last one from the same sender in the
    /// same conversation is dropped; so is one outside the time window or before the clock.
    #[test]
    fn stale_and_replayed_positions_are_dropped() {
        use crate::core::Config;
        let mut core = Core::new(Config::new("t", [2; 32], [1; 32]));
        let fix = Fix {
            lat: 45.0,
            lon: 5.0,
            alt_m: None,
            accuracy_m: 5.0,
            heading_deg: None,
            speed_mps: None,
            unix_s: 0,
        };
        let at = |t: u64| position_for(&fix, Precision::Exact, t, 0);
        let (alice, gid) = ([7; 32], [9; 16]);
        let count = |core: &mut Core| {
            let mut n = 0;
            while let Some(e) = core.poll_event() {
                n += usize::from(matches!(e, Event::Location { .. }));
            }
            n
        };
        // No wall clock yet: nothing is accepted.
        core.on_position(alice, None, at(1_000), 0, 0);
        assert_eq!(count(&mut core), 0);
        let now_s = 1_793_000_000u64;
        core.set_clock(now_s * 1000, 0);
        core.on_position(alice, None, at(now_s - 60), 0, 0);
        assert_eq!(count(&mut core), 1);
        // Replayed, then older: dropped.
        core.on_position(alice, None, at(now_s - 60), 0, 0);
        core.on_position(alice, None, at(now_s - 120), 0, 0);
        assert_eq!(count(&mut core), 0);
        // The same time in a group is another conversation.
        core.on_position(alice, Some(gid), at(now_s - 60), 0, 0);
        assert_eq!(count(&mut core), 1);
        // Newer: accepted; from the future beyond 10 minutes: dropped.
        core.on_position(alice, None, at(now_s), 0, 0);
        core.on_position(alice, None, at(now_s + 700), 0, 0);
        assert_eq!(count(&mut core), 1);
        // The full time is rebuilt across a wrap of the 16-bit counter.
        let wrap = (now_s | 0xFFFF) + 1;
        core.set_clock(wrap * 1000 + 30_000, 0);
        core.on_position(alice, None, at(wrap - 10), 0, 0);
        match core.poll_event() {
            Some(Event::Location { at: t, .. }) => assert_eq!(t, wrap - 10),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn dead_reckoning() {
        let fix = Fix {
            lat: 45.0,
            lon: 5.0,
            alt_m: None,
            accuracy_m: 5.0,
            heading_deg: Some(90.0),
            speed_mps: Some(10.0),
            unix_s: 0,
        };
        let p = position_for(&fix, Precision::Exact, 0, 0);
        // 60 s east at 10 m/s: 600 m.
        let e = extrapolate(&p, 100, 160);
        let d = distance_m((45.0, 5.0), e);
        assert!((d - 600.0).abs() < 2.0, "{d}");
        assert!(e.1 > 5.0 && (e.0 - 45.0).abs() < 1e-5);
        // Capped at 15 minutes.
        let far = extrapolate(&p, 0, 100_000);
        assert!((distance_m((45.0, 5.0), far) - 9_000.0).abs() < 10.0);
    }
}

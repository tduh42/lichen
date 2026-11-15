//! Search mode (protocol.md §11, ADR-0016).
//!
//! Searcher side: every group envelope of a member heard **directly** (0 hops) is a reception,
//! measured with our position; while a search runs, the best reception of each message is
//! shared in the group (MEASURE), and every searcher fuses everyone's receptions, bearings and
//! the target's own fixes in the [`solver`](crate::solver). Target side: PING and POWER are
//! answered only in SOS or with the group's consent; BEACON carries the TX power (and the
//! position when there is a fix); in SOS without a fix, a BEACON replaces the SOS position.

use std::collections::{HashMap, HashSet, VecDeque};

use lc_meshcore::command::Command;
use lc_proto::Millis;
use lc_proto::content;
use lc_proto::group;
use lc_proto::location::{FLAG_SOS, unwrap_time};
use lc_proto::search::{
    BATTERY_UNKNOWN, Bearing, BearingClass, MAX_LEN, Measure, NOISE_UNKNOWN, POWER_MINUTES_MAX,
    Search,
};
use rand_core::Rng;

use crate::core::{Core, Error, Event, Expect, MsgId, PubKey};
use crate::group::GroupId;
use crate::location::{
    self, FIX_MAX_AGE_S, Fix, Precision, distance_m, position_for, prec_byte, radius_m, to_degrees,
    to_units,
};
use crate::solver::{self, Estimate, Kind, Observation, Params, PathLoss};

/// A searcher pings at most this often (§11.3)...
pub const PING_GAP_MS: Millis = 30_000;
/// ...and only when nothing of the target was heard for this long.
pub const HEARD_FRESH_MS: Millis = 60_000;
/// The target answers pings at most this often, per group.
pub const BEACON_GAP_MS: Millis = 20_000;
/// In SOS without a fix, a BEACON every this many seconds, per group.
pub const SOS_BEACON_S: u64 = location::SOS_PERIOD_S;
/// A searcher shares a MEASURE at most this often per target (§11.2)...
pub const MEASURE_GAP_MS: Millis = 60_000;
/// ...unless it moved this far since its last one.
pub const MEASURE_MOVE_M: f64 = 50.0;
/// A bearing is shared at most this often.
pub const BEARING_GAP_MS: Millis = 10_000;
/// The noise floor is asked this often while a search runs.
pub const STATS_PERIOD_MS: Millis = 30_000;
/// SNR (× 4) from which an estimated RSSI is only a lower bound.
pub const SATURATION_SNR4: i8 = 40;
/// Lowest TX power stock firmware accepts.
pub const MIN_TX_DBM: i8 = -9;
/// TX power assumed until a BEACON tells it (the stock default).
pub const DEFAULT_TX_DBM: i8 = 22;
/// Copies of a message are about ten frame airtimes apart (§10.3): the best direct copy of a
/// message is chosen this long after the first one.
const SHARE_WAIT_MS: Millis = 30_000;
/// Unattributed direct copies are kept this long, and this many.
const DIRECT_KEEP_MS: Millis = 60_000;
const DIRECT_MAX: usize = 64;
/// Without a search, a member's receptions are kept this long, and this many (a search started
/// later begins with them).
const IDLE_KEEP_S: u64 = 15 * 60;
const IDLE_MAX: usize = 32;
/// During a search, at most this many measurements per target (the oldest go first).
const MEASUREMENTS_MAX: usize = 400;
const FIXES_MAX: usize = 100;
/// Our own reception needs a fix at most this old.
const OWN_FIX_S: u64 = 60;
/// Noise floor assumed before `GET_STATS` answers (SF8, 62.5 kHz).
const DEFAULT_NOISE_DBM: i8 = -118;
/// Extra uncertainty of an RSSI estimated from noise + SNR, and of an unknown TX power, dB.
const ESTIMATED_SIGMA_DB: f64 = 3.0;
const UNKNOWN_TX_SIGMA_DB: f64 = 4.0;
/// A reception pairs with a fix of the target this close in time (calibration).
const PAIR_S: u64 = 60;
/// "You are being searched" is reported again after this long.
const SEARCHED_REPEAT_MS: Millis = 60_000;
/// MEASURE's `rssi` for a bearing alone.
pub const RSSI_NONE: i8 = i8::MIN;

/// One reception of a target by a searcher (us or another member), where the searcher was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measurement {
    /// The searcher's identity (ours for our own receptions).
    pub by: PubKey,
    /// Unix seconds.
    pub at: u64,
    pub lat: f64,
    pub lon: f64,
    /// The searcher's position uncertainty, metres.
    pub accuracy_m: f64,
    /// dBm; `None` for a bearing alone.
    pub rssi: Option<i8>,
    /// SNR × 4.
    pub snr4: i8,
    /// The searcher's noise floor, dBm, when known.
    pub noise: Option<i8>,
    /// RSSI estimated as noise + SNR (the radio reported none).
    pub estimated: bool,
    /// Estimated at an SNR where LoRa saturates: the true RSSI is higher.
    pub saturated: bool,
    pub bearing: Option<Bearing>,
    /// The target's TX power then, when a BEACON told it.
    pub tx_dbm: Option<i8>,
}

/// A position the target reported (§12, or in a BEACON).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetFix {
    /// Unix seconds.
    pub at: u64,
    pub lat: f64,
    pub lon: f64,
    pub radius_m: f64,
}

/// A search we run, for the app.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchInfo {
    pub gid: GroupId,
    /// The target's identity and node.
    pub target: PubKey,
    pub node: PubKey,
    pub measurements: usize,
    /// From the target's last BEACON.
    pub tx_dbm: Option<i8>,
    pub battery: Option<u8>,
    /// Unix seconds of the newest reception (ours or shared) and of the newest fix.
    pub last_heard: Option<u64>,
    pub last_fix: Option<TargetFix>,
    /// How long until a PING may go (0: now).
    pub ping_in_ms: Millis,
}

/// Envelope identity in the clear header: `(gid_hash, sender, key_epoch, counter)`.
type CopyKey = (u16, u8, u8, u32);

#[derive(Clone, Debug)]
struct Direct {
    key: CopyKey,
    /// The copy as heard: it counts only once it verifies under the member's key (SR-16).
    payload: Vec<u8>,
    snr4: i8,
    rssi: i8,
    at: Millis,
}

/// What we know of a member as a target.
#[derive(Debug)]
struct Track {
    node: PubKey,
    /// Sorted by time.
    measurements: Vec<Measurement>,
    fixes: Vec<TargetFix>,
    tx_dbm: Option<i8>,
    battery: Option<u8>,
    /// Newest reception or BEACON (ours or shared), for the PING rule.
    last_heard: Option<Millis>,
    run: Option<Run>,
}

/// A search we run.
#[derive(Debug)]
struct Run {
    gid: GroupId,
    seed: u64,
    last_ping: Option<Millis>,
    /// When and where our last MEASURE went.
    last_shared: Option<(Millis, f64, f64)>,
    last_bearing: Option<Millis>,
    /// Our best direct reception of the target's current message, and when to share it.
    pending: Option<(CopyKey, Measurement, Millis)>,
    cache: Option<Estimate>,
}

/// The TX power a POWER request set, and what to restore.
#[derive(Clone, Copy, Debug)]
struct PowerOverride {
    previous: i8,
    until: Millis,
    /// The group whose member asked (consent is checked on it); `None` after a restart.
    gid: Option<GroupId>,
}

/// Search state of the core.
#[derive(Debug, Default)]
pub(crate) struct Searches {
    /// Groups allowed to locate us ("let this group locate me").
    consent: HashSet<GroupId>,
    /// Targets by identity: searched, or recently heard.
    tracks: HashMap<PubKey, Track>,
    /// Direct copies waiting for their message to be decrypted.
    direct: VecDeque<Direct>,
    /// Messages already attributed, for their later copies.
    attributed: VecDeque<(CopyKey, PubKey, Millis)>,
    /// Our radio's noise floor, dBm (`GET_STATS`).
    noise: Option<i8>,
    next_stats: Millis,
    /// Our radio's TX power and its maximum (`SELF_INFO`).
    pub(crate) tx_dbm: Option<i8>,
    pub(crate) max_tx_dbm: Option<i8>,
    /// The phone's battery, 0..=100.
    battery: Option<u8>,
    /// Pings to answer, per group (the newest id).
    answer: HashMap<GroupId, u16>,
    last_beacon: HashMap<GroupId, Millis>,
    sos_beacon_at: HashMap<GroupId, u64>,
    searched_by: HashMap<(GroupId, PubKey), Millis>,
    power: Option<PowerOverride>,
}

impl Track {
    const fn new(node: PubKey) -> Self {
        Self {
            node,
            measurements: Vec::new(),
            fixes: Vec::new(),
            tx_dbm: None,
            battery: None,
            last_heard: None,
            run: None,
        }
    }

    fn add(&mut self, m: Measurement) {
        if self.measurements.contains(&m) {
            return;
        }
        let i = self.measurements.partition_point(|x| x.at <= m.at);
        self.measurements.insert(i, m);
        let max = if self.run.is_some() {
            MEASUREMENTS_MAX
        } else {
            IDLE_MAX
        };
        if self.measurements.len() > max {
            let extra = self.measurements.len() - max;
            self.measurements.drain(..extra);
        }
        self.changed();
    }

    fn add_fix(&mut self, f: TargetFix) {
        let i = self.fixes.partition_point(|x| x.at <= f.at);
        self.fixes.insert(i, f);
        if self.fixes.len() > FIXES_MAX {
            self.fixes.remove(0);
        }
        self.changed();
    }

    fn changed(&mut self) {
        if let Some(r) = self.run.as_mut() {
            r.cache = None;
        }
    }

    /// The path loss fitted to the receptions made while the target reported fixes.
    fn model(&self) -> PathLoss {
        let mut pairs = Vec::new();
        for m in &self.measurements {
            let (Some(rssi), false) = (m.rssi, m.saturated) else {
                continue;
            };
            let near = self
                .fixes
                .iter()
                .filter(|f| f.at.abs_diff(m.at) <= PAIR_S && f.radius_m <= 50.0)
                .min_by_key(|f| f.at.abs_diff(m.at));
            if let Some(f) = near {
                let d = distance_m((m.lat, m.lon), (f.lat, f.lon));
                let tx = m.tx_dbm.unwrap_or(DEFAULT_TX_DBM);
                pairs.push((d, f64::from(rssi) - f64::from(tx)));
            }
        }
        solver::calibrate(&pairs, PathLoss::DEFAULT).unwrap_or(PathLoss::DEFAULT)
    }

    /// Everything the solver uses, sorted by time.
    fn observations(&self) -> Vec<Observation> {
        let mut obs = Vec::with_capacity(self.measurements.len() + self.fixes.len());
        for m in &self.measurements {
            let t = m.at as f64;
            if let Some(rssi) = m.rssi {
                let extra = if m.estimated { ESTIMATED_SIGMA_DB } else { 0.0 }.hypot(
                    if m.tx_dbm.is_some() {
                        0.0
                    } else {
                        UNKNOWN_TX_SIGMA_DB
                    },
                );
                obs.push(Observation {
                    t,
                    lat: m.lat,
                    lon: m.lon,
                    kind: Kind::Rssi {
                        rssi: f64::from(rssi),
                        tx_dbm: f64::from(m.tx_dbm.unwrap_or(DEFAULT_TX_DBM)),
                        extra_sigma_db: extra,
                        lower_bound: m.saturated,
                    },
                });
            }
            if let Some(b) = m.bearing {
                obs.push(Observation {
                    t,
                    lat: m.lat,
                    lon: m.lon,
                    kind: Kind::Bearing {
                        deg: f64::from(b.deg),
                        sigma_deg: bearing_sigma(b.class),
                    },
                });
            }
        }
        for f in &self.fixes {
            obs.push(Observation {
                t: f.at as f64,
                lat: f.lat,
                lon: f.lon,
                kind: Kind::Fix {
                    radius_m: f.radius_m,
                },
            });
        }
        obs.sort_by(|a, b| a.t.total_cmp(&b.t));
        obs
    }
}

/// How far a bearing of each class is trusted (§11.1), degrees.
pub const fn bearing_sigma(class: BearingClass) -> f64 {
    match class {
        BearingClass::Yagi => 15.0,
        BearingClass::Body => 40.0,
        BearingClass::Other => 60.0,
    }
}

fn prefix(k: &PubKey) -> [u8; 4] {
    [k[0], k[1], k[2], k[3]]
}

impl Core {
    // ----- searcher side ---------------------------------------------------------------------

    /// Starts searching for member `target` (identity or node key) of group `gid`.
    pub fn start_search(
        &mut self,
        gid: &GroupId,
        target: &PubKey,
        now: Millis,
    ) -> Result<(), Error> {
        let g = self.groups.get(gid).ok_or(Error::UnknownGroup)?;
        let m = g
            .members
            .iter()
            .find(|m| m.key == *target || m.node == *target)
            .ok_or(Error::UnknownContact)?;
        if Some(m.key) == self.key {
            return Err(Error::NotAllowed);
        }
        let (key, node) = (m.key, m.node);
        let seed = self.rng.next_u64();
        let t = self
            .search
            .tracks
            .entry(key)
            .or_insert_with(|| Track::new(node));
        t.node = node;
        match t.run.as_mut() {
            Some(r) => r.gid = *gid,
            None => {
                t.run = Some(Run {
                    gid: *gid,
                    seed,
                    last_ping: None,
                    last_shared: None,
                    last_bearing: None,
                    pending: None,
                    cache: None,
                });
            }
        }
        self.search.next_stats = now;
        self.events.push_back(Event::SearchUpdated { target: key });
        self.tick(now);
        Ok(())
    }

    /// Stops searching for `target` (identity or node key).
    pub fn stop_search(&mut self, target: &PubKey) {
        if let Some(t) = self.track_mut(target) {
            t.run = None;
            if t.measurements.len() > IDLE_MAX {
                let extra = t.measurements.len() - IDLE_MAX;
                t.measurements.drain(..extra);
            }
        }
    }

    /// The searches we run.
    pub fn searches(&self, now: Millis) -> Vec<SearchInfo> {
        let mut v: Vec<SearchInfo> = self
            .search
            .tracks
            .iter()
            .filter_map(|(k, t)| {
                let r = t.run.as_ref()?;
                Some(SearchInfo {
                    gid: r.gid,
                    target: *k,
                    node: t.node,
                    measurements: t.measurements.len(),
                    tx_dbm: t.tx_dbm,
                    battery: t.battery,
                    last_heard: t
                        .measurements
                        .iter()
                        .filter(|m| m.rssi.is_some())
                        .map(|m| m.at)
                        .max(),
                    last_fix: t.fixes.last().copied(),
                    ping_in_ms: ping_wait(t, r, now),
                })
            })
            .collect();
        v.sort_by_key(|s| s.target);
        v
    }

    /// Asks the target to transmit now (§11.3): refused while it was heard less than a minute
    /// ago, or pinged less than 30 s ago.
    pub fn search_ping(&mut self, target: &PubKey, now: Millis) -> Result<MsgId, Error> {
        let (key, gid, wait) = {
            let (key, t) = self.track_entry(target).ok_or(Error::NotSearching)?;
            let r = t.run.as_ref().ok_or(Error::NotSearching)?;
            (key, r.gid, ping_wait(t, r, now))
        };
        if wait > 0 {
            return Err(Error::TooSoon { wait_ms: wait });
        }
        let id = (self.rng.next_u32() as u16).max(1);
        let msg = self.send_search(
            &gid,
            &Search::Ping {
                target: prefix(&key),
                id,
            },
            now,
        )?;
        if let Some(r) = self.track_mut(&key).and_then(|t| t.run.as_mut()) {
            r.last_ping = Some(now);
        }
        Ok(msg)
    }

    /// A bearing to the target from here, degrees from true north (the phone pointed where the
    /// signal is strongest): used at once and shared, at most every 10 s.
    pub fn search_bearing(
        &mut self,
        target: &PubKey,
        deg: f64,
        class: BearingClass,
        now: Millis,
    ) -> Result<(), Error> {
        let me = self.key.ok_or(Error::NoIdentity)?;
        let (fix, t_unix) = self.own_fix(now).ok_or(Error::NoPosition)?;
        let (key, gid, tx) = {
            let (key, t) = self.track_entry(target).ok_or(Error::NotSearching)?;
            let r = t.run.as_ref().ok_or(Error::NotSearching)?;
            if let Some(at) = r.last_bearing
                && now < at + BEARING_GAP_MS
            {
                return Err(Error::TooSoon {
                    wait_ms: at + BEARING_GAP_MS - now,
                });
            }
            (key, r.gid, t.tx_dbm)
        };
        if !deg.is_finite() {
            return Err(Error::Empty);
        }
        let bearing = Bearing {
            deg: (deg.rem_euclid(360.0).round() as u16) % 360,
            class,
        };
        let m = Measurement {
            by: me,
            at: t_unix,
            lat: fix.lat,
            lon: fix.lon,
            accuracy_m: fix.accuracy_m,
            rssi: None,
            snr4: 0,
            noise: None,
            estimated: false,
            saturated: false,
            bearing: Some(bearing),
            tx_dbm: tx,
        };
        let wire = measure_of(&key, &m);
        self.send_search(&gid, &Search::Measure(wire), now)?;
        if let Some(t) = self.track_mut(&key) {
            t.add(m);
            if let Some(r) = t.run.as_mut() {
                r.last_bearing = Some(now);
            }
        }
        self.events.push_back(Event::SearchUpdated { target: key });
        Ok(())
    }

    /// Asks the target to set its TX power to `dbm` for `minutes` (close range, where the
    /// receivers saturate).
    pub fn search_lower_power(
        &mut self,
        target: &PubKey,
        dbm: i8,
        minutes: u8,
        now: Millis,
    ) -> Result<MsgId, Error> {
        let (key, gid) = {
            let (key, t) = self.track_entry(target).ok_or(Error::NotSearching)?;
            (key, t.run.as_ref().ok_or(Error::NotSearching)?.gid)
        };
        self.send_search(
            &gid,
            &Search::Power {
                target: prefix(&key),
                dbm,
                minutes: minutes.clamp(1, POWER_MINUTES_MAX),
            },
            now,
        )
    }

    /// Where the target probably is (`None` before anything was heard).
    pub fn search_estimate(&mut self, target: &PubKey) -> Option<Estimate> {
        let key = self.track_entry(target)?.0;
        let t = self.search.tracks.get_mut(&key)?;
        let (obs, model) = (t.observations(), t.model());
        let r = t.run.as_mut()?;
        if let Some(e) = &r.cache {
            return Some(e.clone());
        }
        let e = solver::solve(&obs, &Params::new(model, r.seed))?;
        r.cache = Some(e.clone());
        Some(e)
    }

    /// Every measurement of the target, ours and shared (for the list and the CSV export).
    pub fn search_measurements(&self, target: &PubKey) -> Vec<Measurement> {
        self.track_entry(target)
            .map(|(_, t)| t.measurements.clone())
            .unwrap_or_default()
    }

    /// The target's reported positions.
    pub fn search_fixes(&self, target: &PubKey) -> Vec<TargetFix> {
        self.track_entry(target)
            .map(|(_, t)| t.fixes.clone())
            .unwrap_or_default()
    }

    // ----- target side -----------------------------------------------------------------------

    /// "Let this group locate me" (§11): off by default, kept by the app.
    pub fn set_search_consent(&mut self, gid: &GroupId, on: bool, now: Millis) {
        if on {
            self.search.consent.insert(*gid);
        } else {
            self.search.consent.remove(gid);
        }
        self.tick(now);
    }

    /// A consent saved by the app, back after a restart (no tick).
    pub fn restore_search_consent(&mut self, gid: &GroupId) {
        self.search.consent.insert(*gid);
    }

    pub fn search_consent(&self, gid: &GroupId) -> bool {
        self.search.consent.contains(gid)
    }

    /// The phone's battery level, sent in BEACONs.
    pub fn set_battery(&mut self, percent: Option<u8>) {
        self.search.battery = percent.map(|p| p.min(100));
    }

    /// A TX power a POWER request set before the app stopped (the radio keeps it): restored
    /// once the companion is connected.
    pub fn restore_tx_power(&mut self, previous: i8) {
        self.search.power = Some(PowerOverride {
            previous,
            until: 0,
            gid: None,
        });
    }

    /// Members searching for us in the last ten minutes: (group, node).
    pub fn searched_by(&self, now: Millis) -> Vec<(GroupId, PubKey)> {
        let mut v: Vec<(GroupId, PubKey)> = self
            .search
            .searched_by
            .iter()
            .filter(|(_, at)| **at + 10 * 60_000 > now)
            .map(|(k, _)| *k)
            .collect();
        v.sort();
        v
    }

    /// SOS turned off: the groups that only got position-less BEACONs get one more with the
    /// SOS bit clear (an end position needs a fix, §11.3).
    pub(crate) fn end_sos_beacons(&mut self, now: Millis) {
        let groups: Vec<GroupId> = self.search.sos_beacon_at.drain().map(|(g, _)| g).collect();
        for g in groups {
            if self.groups.contains_key(&g) {
                self.send_beacon(&g, 0, now);
            }
        }
    }

    /// When the last position-less SOS BEACON was queued, Unix seconds.
    pub(crate) fn last_sos_beacon(&self) -> Option<u64> {
        self.search.sos_beacon_at.values().max().copied()
    }

    /// Whether `gid` may make us transmit (§11 consent).
    fn may_locate(&self, gid: &GroupId) -> bool {
        self.loc.sos_on() || self.search.consent.contains(gid)
    }

    // ----- radio side ------------------------------------------------------------------------

    /// A direct copy (0 hops) of a group envelope, before any decryption (§11.2).
    pub(crate) fn search_direct(&mut self, payload: &[u8], snr4: i8, rssi: i8, now: Millis) {
        let Some((h, _)) = group::parse(payload) else {
            return;
        };
        let key = (h.gid_hash, h.sender, h.key_epoch, h.counter);
        let known = self
            .search
            .attributed
            .iter()
            .find(|a| a.0 == key)
            .map(|a| a.1);
        if let Some(member) = known {
            if self.copy_is_from(&member, payload) {
                self.search_reception(member, key, snr4, rssi, now);
            }
            return;
        }
        let d = &mut self.search.direct;
        while d.front().is_some_and(|x| x.at + DIRECT_KEEP_MS <= now) || d.len() >= DIRECT_MAX {
            d.pop_front();
        }
        d.push_back(Direct {
            key,
            payload: payload.to_vec(),
            snr4,
            rssi,
            at: now,
        });
    }

    /// A message of `member` decrypted: the direct copies of its envelope were receptions of
    /// that member.
    pub(crate) fn search_attribute(
        &mut self,
        h: &group::GroupHeader,
        member: PubKey,
        node: PubKey,
        now: Millis,
    ) {
        let key = (h.gid_hash, h.sender, h.key_epoch, h.counter);
        let a = &mut self.search.attributed;
        while a.front().is_some_and(|x| x.2 + DIRECT_KEEP_MS <= now) || a.len() >= DIRECT_MAX {
            a.pop_front();
        }
        if !a.iter().any(|x| x.0 == key) {
            a.push_back((key, member, now));
        }
        self.search
            .tracks
            .entry(member)
            .or_insert_with(|| Track::new(node))
            .node = node;
        let copies: Vec<Direct> = self
            .search
            .direct
            .iter()
            .filter(|d| d.key == key)
            .cloned()
            .collect();
        self.search.direct.retain(|d| d.key != key);
        for c in copies {
            if self.copy_is_from(&member, &c.payload) {
                self.search_reception(member, key, c.snr4, c.rssi, now);
            }
        }
    }

    /// One of our direct receptions of `member`.
    fn search_reception(&mut self, member: PubKey, key: CopyKey, snr4: i8, rssi: i8, now: Millis) {
        let Some(me) = self.key else {
            return;
        };
        let Some((fix, t_unix)) = self.own_fix(now) else {
            return;
        };
        let noise = self.search.noise;
        let (value, estimated) = if rssi == 0 {
            let n = i16::from(noise.unwrap_or(DEFAULT_NOISE_DBM));
            let v = (n + i16::from(snr4) / 4).clamp(i16::from(i8::MIN) + 1, 0);
            (v as i8, true)
        } else {
            (rssi, false)
        };
        let Some(t) = self.search.tracks.get_mut(&member) else {
            return;
        };
        let m = Measurement {
            by: me,
            at: t_unix,
            lat: fix.lat,
            lon: fix.lon,
            accuracy_m: fix.accuracy_m,
            rssi: Some(value),
            snr4,
            noise,
            estimated,
            saturated: estimated && snr4 >= SATURATION_SNR4,
            bearing: None,
            tx_dbm: t.tx_dbm,
        };
        t.add(m);
        t.last_heard = Some(now);
        if let Some(r) = t.run.as_mut() {
            match r.pending.as_mut() {
                Some((k, best, _)) if *k == key => {
                    if m.snr4 > best.snr4 {
                        *best = m;
                    }
                }
                _ => {
                    // Another message: the previous one's best goes first.
                    let before = r.pending.replace((key, m, now + SHARE_WAIT_MS));
                    if let Some((_, b, _)) = before {
                        self.share_measure(member, b, now);
                    }
                }
            }
            self.events
                .push_back(Event::SearchUpdated { target: member });
        }
    }

    /// A SEARCH message from `from` (identity, node `from_node`) in group `gid`.
    pub(crate) fn on_search(
        &mut self,
        gid: GroupId,
        from: PubKey,
        from_node: PubKey,
        s: Search,
        now: Millis,
    ) {
        let Some(me) = self.key else {
            return;
        };
        match s {
            Search::Ping { target, id } => {
                if target == prefix(&me) && self.may_locate(&gid) {
                    self.search.answer.insert(gid, id);
                    self.searched(gid, from_node, now);
                }
            }
            Search::Beacon {
                tx_dbm,
                battery,
                sos,
                position,
                ..
            } => {
                let battery = (battery != BATTERY_UNKNOWN).then_some(battery.min(100));
                self.events.push_back(Event::Beacon {
                    gid,
                    from: from_node,
                    sos,
                    battery,
                });
                let at = position.and_then(|p| {
                    location::receive(
                        &mut self.loc,
                        &mut self.events,
                        from_node,
                        Some(gid),
                        p,
                        0,
                        now,
                    )
                    .map(|at| (p, at))
                });
                let t = self
                    .search
                    .tracks
                    .entry(from)
                    .or_insert_with(|| Track::new(from_node));
                t.tx_dbm = Some(tx_dbm);
                t.battery = battery;
                t.last_heard = Some(now);
                if let Some((p, at)) = at {
                    let (lat, lon) = to_degrees(p.lat, p.lon);
                    t.add_fix(TargetFix {
                        at,
                        lat,
                        lon,
                        radius_m: radius_m(p.prec),
                    });
                }
                t.changed();
                if t.run.is_some() {
                    self.events.push_back(Event::SearchUpdated { target: from });
                }
            }
            Search::Measure(m) => {
                if m.target == prefix(&me) {
                    self.searched(gid, from_node, now);
                    return;
                }
                let Some(target) = self.groups.get(&gid).and_then(|g| {
                    g.members
                        .iter()
                        .find(|x| prefix(&x.key) == m.target && x.key != from)
                        .map(|x| (x.key, x.node))
                }) else {
                    return;
                };
                let Some(at) = self.unix_s(now).and_then(|t| unwrap_time(m.t16, t)) else {
                    return;
                };
                let (lat, lon) = to_degrees(m.lat, m.lon);
                let t = self
                    .search
                    .tracks
                    .entry(target.0)
                    .or_insert_with(|| Track::new(target.1));
                let rssi = (m.rssi != RSSI_NONE).then_some(m.rssi);
                if rssi.is_some() {
                    t.last_heard = Some(now);
                }
                t.add(Measurement {
                    by: from,
                    at,
                    lat,
                    lon,
                    accuracy_m: radius_m(m.prec),
                    rssi,
                    snr4: m.snr4,
                    noise: (m.noise != NOISE_UNKNOWN).then_some(m.noise),
                    estimated: m.estimated,
                    saturated: m.saturated,
                    bearing: m.bearing,
                    tx_dbm: t.tx_dbm,
                });
                if t.run.is_some() {
                    self.events
                        .push_back(Event::SearchUpdated { target: target.0 });
                }
            }
            Search::Power {
                target,
                dbm,
                minutes,
            } => {
                if target == prefix(&me) && self.may_locate(&gid) {
                    self.apply_power(gid, dbm, minutes, now);
                    self.searched(gid, from_node, now);
                }
            }
        }
    }

    /// A position from member `from` (identity) reached the location module: a fix of the
    /// target for its searchers.
    pub(crate) fn search_fix(&mut self, from: &PubKey, lat: f64, lon: f64, prec: u8, at: u64) {
        if let Some(t) = self.search.tracks.get_mut(from) {
            t.add_fix(TargetFix {
                at,
                lat,
                lon,
                radius_m: radius_m(prec),
            });
            if t.run.is_some() {
                self.events
                    .push_back(Event::SearchUpdated { target: *from });
            }
        }
    }

    /// The node's TX power changed (our POWER handling or its restoration).
    pub(crate) fn search_power_applied(&mut self, dbm: i8) {
        self.search.tx_dbm = Some(dbm);
        let previous = self.search.power.map(|p| p.previous);
        self.events.push_back(Event::TxPower { dbm, previous });
        // The searchers learn the new power from a BEACON.
        if let Some(PowerOverride { gid: Some(g), .. }) = self.search.power {
            self.search.answer.entry(g).or_insert(0);
        }
    }

    /// The companion refused a TX power.
    pub(crate) fn search_power_refused(&mut self, dbm: i8) {
        if self.search.power.is_some_and(|p| p.previous != dbm) {
            self.search.power = None;
        }
    }

    /// The radio's noise floor (`GET_STATS`), dBm.
    pub(crate) fn search_noise(&mut self, noise: i16) {
        if (-150..0).contains(&noise) {
            self.search.noise = Some(noise as i8);
        }
    }

    fn searched(&mut self, gid: GroupId, by: PubKey, now: Millis) {
        let last = self.search.searched_by.get(&(gid, by)).copied();
        if last.is_none_or(|t| now >= t + SEARCHED_REPEAT_MS) {
            self.search.searched_by.insert((gid, by), now);
            self.events.push_back(Event::Searched { gid, by });
        }
    }

    fn apply_power(&mut self, gid: GroupId, dbm: i8, minutes: u8, now: Millis) {
        let Some(current) = self.search.tx_dbm else {
            return;
        };
        let max = self.search.max_tx_dbm.unwrap_or(DEFAULT_TX_DBM);
        let dbm = dbm.clamp(MIN_TX_DBM, max.max(MIN_TX_DBM));
        let previous = self.search.power.map_or(current, |p| p.previous);
        self.search.power = Some(PowerOverride {
            previous,
            until: now + Millis::from(minutes.clamp(1, POWER_MINUTES_MAX)) * 60_000,
            gid: Some(gid),
        });
        self.command(&Command::SetRadioTxPower { dbm }, Expect::TxPower(dbm));
    }

    fn restore_power(&mut self) {
        if let Some(p) = self.search.power.take() {
            self.command(
                &Command::SetRadioTxPower { dbm: p.previous },
                Expect::TxPower(p.previous),
            );
        }
    }

    // ----- timers ----------------------------------------------------------------------------

    pub(crate) fn tick_search(&mut self, now: Millis) {
        let groups: Vec<GroupId> = self.groups.keys().copied().collect();
        // Searches in groups we left, members gone.
        for t in self.search.tracks.values_mut() {
            if let Some(r) = &t.run
                && !groups.contains(&r.gid)
            {
                t.run = None;
            }
        }
        // Idle receptions expire.
        if let Some(t_unix) = self.unix_s(now) {
            self.search.tracks.retain(|_, t| {
                if t.run.is_none() {
                    t.measurements.retain(|m| m.at + IDLE_KEEP_S > t_unix);
                    t.fixes.retain(|f| f.at + IDLE_KEEP_S > t_unix);
                }
                t.run.is_some() || !t.measurements.is_empty() || !t.fixes.is_empty()
            });
        }
        // Our best receptions, shared once their message's copies are in.
        let due: Vec<(PubKey, Measurement)> = self
            .search
            .tracks
            .iter_mut()
            .filter_map(|(k, t)| {
                let r = t.run.as_mut()?;
                if r.pending.is_some_and(|p| p.2 <= now) {
                    r.pending.take().map(|(_, m, _)| (*k, m))
                } else {
                    None
                }
            })
            .collect();
        for (k, m) in due {
            self.share_measure(k, m, now);
        }
        // The noise floor, while we search.
        let searching = self.search.tracks.values().any(|t| t.run.is_some());
        if searching && now >= self.search.next_stats {
            self.search.next_stats = now + STATS_PERIOD_MS;
            self.command(&Command::GetStats { kind: 1 }, Expect::Stats);
        }

        // Target side: answer pings.
        let answers: Vec<(GroupId, u16)> = self
            .search
            .answer
            .iter()
            .filter(|(g, _)| {
                self.search
                    .last_beacon
                    .get(*g)
                    .is_none_or(|t| now >= t + BEACON_GAP_MS)
            })
            .map(|(g, id)| (*g, *id))
            .collect();
        for (g, id) in answers {
            self.search.answer.remove(&g);
            if self.may_locate(&g) && groups.contains(&g) {
                self.send_beacon(&g, id, now);
            }
        }
        // In SOS without a fix, a BEACON in place of the SOS position.
        if let Some(t_unix) = self.unix_s(now) {
            let fix = self
                .loc
                .fix()
                .filter(|f| f.unix_s + FIX_MAX_AGE_S >= t_unix);
            if self.loc.sos_on() && fix.is_none() {
                for g in &groups {
                    let last = self.search.sos_beacon_at.get(g).copied();
                    if last.is_none_or(|at| t_unix >= at + SOS_BEACON_S) {
                        self.search.sos_beacon_at.insert(*g, t_unix);
                        self.send_beacon(g, 0, now);
                    }
                }
            } else {
                self.search.sos_beacon_at.clear();
            }
        }
        // A lowered TX power goes back when it expires, or when consent goes.
        if let Some(p) = self.search.power {
            let allowed = p.gid.is_some_and(|g| self.may_locate(&g));
            if now >= p.until || !allowed {
                self.restore_power();
            }
        }
    }

    pub(crate) fn next_search_wakeup(&self) -> Option<Millis> {
        let mut t: Option<Millis> = None;
        let mut consider = |x: Millis| t = Some(t.map_or(x, |y: Millis| y.min(x)));
        let mut searching = false;
        for tr in self.search.tracks.values() {
            if let Some(r) = &tr.run {
                searching = true;
                if let Some((_, _, due)) = r.pending {
                    consider(due);
                }
            }
        }
        if searching {
            consider(self.search.next_stats);
        }
        for g in self.search.answer.keys() {
            consider(
                self.search
                    .last_beacon
                    .get(g)
                    .map_or(0, |t| t + BEACON_GAP_MS),
            );
        }
        if let Some(p) = self.search.power {
            consider(p.until);
        }
        if self.loc.sos_on() {
            // SOS beacons follow the location timers' 2-minute rhythm.
            consider(
                self.search
                    .sos_beacon_at
                    .values()
                    .min()
                    .and_then(|at| self.mono_ms(at + SOS_BEACON_S))
                    .unwrap_or(0),
            );
        }
        t
    }

    // ----- helpers ---------------------------------------------------------------------------

    fn send_search(&mut self, gid: &GroupId, s: &Search, now: Millis) -> Result<MsgId, Error> {
        let mut body = [0u8; MAX_LEN];
        let n = s.encode(&mut body);
        let mut datagram = Vec::with_capacity(1 + n);
        datagram.push(content::SEARCH);
        datagram.extend_from_slice(body.get(..n).unwrap_or(&[]));
        self.send_group(gid, &datagram, false, now)
    }

    fn send_beacon(&mut self, gid: &GroupId, id: u16, now: Millis) {
        let position = self.unix_s(now).and_then(|t| {
            self.loc
                .fix()
                .filter(|f| f.unix_s + FIX_MAX_AGE_S >= t)
                .map(|f| {
                    let flags = if self.loc.sos_on() { FLAG_SOS } else { 0 };
                    position_for(&f, Precision::Exact, t, flags)
                })
        });
        let s = Search::Beacon {
            id,
            tx_dbm: self.search.tx_dbm.unwrap_or(DEFAULT_TX_DBM),
            battery: self.search.battery.unwrap_or(BATTERY_UNKNOWN),
            sos: self.loc.sos_on(),
            position,
        };
        if self.send_search(gid, &s, now).is_ok() {
            self.search.last_beacon.insert(*gid, now);
        }
    }

    /// Shares our reception `m` of `target` unless one went less than a minute ago from about
    /// the same place.
    fn share_measure(&mut self, target: PubKey, m: Measurement, now: Millis) {
        let Some(gid) = self
            .search
            .tracks
            .get(&target)
            .and_then(|t| t.run.as_ref())
            .map(|r| r.gid)
        else {
            return;
        };
        let last = self
            .search
            .tracks
            .get(&target)
            .and_then(|t| t.run.as_ref())
            .and_then(|r| r.last_shared);
        if let Some((at, lat, lon)) = last
            && now < at + MEASURE_GAP_MS
            && distance_m((lat, lon), (m.lat, m.lon)) <= MEASURE_MOVE_M
        {
            return;
        }
        if self
            .send_search(&gid, &Search::Measure(measure_of(&target, &m)), now)
            .is_ok()
            && let Some(r) = self
                .search
                .tracks
                .get_mut(&target)
                .and_then(|t| t.run.as_mut())
        {
            r.last_shared = Some((now, m.lat, m.lon));
        }
    }

    /// Our fix if fresh enough to measure with, and the Unix time.
    fn own_fix(&self, now: Millis) -> Option<(Fix, u64)> {
        let t = self.unix_s(now)?;
        let f = self.loc.fix()?;
        (f.unix_s + OWN_FIX_S >= t).then_some((f, t))
    }

    /// The tracked target matching `k` (identity or node).
    fn track_entry(&self, k: &PubKey) -> Option<(PubKey, &Track)> {
        self.search
            .tracks
            .get_key_value(k)
            .or_else(|| self.search.tracks.iter().find(|(_, t)| t.node == *k))
            .map(|(k, t)| (*k, t))
    }

    fn track_mut(&mut self, k: &PubKey) -> Option<&mut Track> {
        let key = self.track_entry(k)?.0;
        self.search.tracks.get_mut(&key)
    }
}

/// How long until a PING may go.
fn ping_wait(t: &Track, r: &Run, now: Millis) -> Millis {
    let a = r.last_ping.map_or(0, |at| at + PING_GAP_MS);
    let b = t.last_heard.map_or(0, |at| at + HEARD_FRESH_MS);
    a.max(b).saturating_sub(now)
}

/// The wire form of a measurement of `target`.
fn measure_of(target: &PubKey, m: &Measurement) -> Measure {
    let (lat, lon) = to_units(m.lat, m.lon);
    Measure {
        target: prefix(target),
        t16: m.at as u16,
        lat,
        lon,
        prec: prec_byte(m.accuracy_m),
        rssi: m.rssi.unwrap_or(RSSI_NONE),
        snr4: m.snr4,
        noise: m.noise.unwrap_or(NOISE_UNKNOWN),
        estimated: m.estimated,
        saturated: m.saturated,
        bearing: m.bearing,
    }
}

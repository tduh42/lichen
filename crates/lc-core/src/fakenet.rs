//! A fake MeshCore network to test the whole stack without radios.
//!
//! Discrete-event simulation of stock MeshCore v1.18 behaviour, limited to what lichen uses
//! (`oxymesh/docs/spec/{air,companion}.md`):
//! - **companions** answer the app's commands (`APP_START`, `DEVICE_QUERY`,
//!   `EXPORT_PRIVATE_KEY`, `GET_CONTACTS`, `GET_CONTACT_BY_KEY`, `SEND_RAW_DATA`,
//!   `SEND_PATH_DISCOVERY_REQ`, `SEND_RAW_PACKET`), transmit after the direct delay
//!   `rand[0, 5·t·0.2]`, and push `RAW_DATA` for a `RAW_CUSTOM` that arrives with an empty path;
//! - **repeaters** forward a direct packet whose first path hash is theirs, after
//!   `rand[0, 5·t·0.3]`, and every new flood packet with their hash appended, after
//!   `rand[0, 5·t·0.5]`;
//! - companions push `LOG_RX_DATA` for every frame they hear, before the duplicate filter;
//! - every node keeps the 160-entry duplicate filter on `SHA256(type ‖ payload)[0..8]`, its own
//!   transmissions included;
//! - radios are half duplex, listen before talking (retry after 120, 240 or 360 ms), and lose a
//!   frame when two neighbours' transmissions overlap at the receiver, or at random with the
//!   per-link loss rate.
//!
//! **Geometry** (optional, for search): nodes placed on a plane ([`Net::place`]) hear each
//! other through a log-distance path loss with shadowing ([`Geometry`]): RSSI and SNR follow
//! distance, a frame below the demodulation floor is lost, `GET_STATS` gives the noise floor and
//! `SET_RADIO_TX_POWER` changes what the others hear. A node can report RSSI 0, like the stock
//! Heltec V4. Unplaced nodes keep fixed readings (SNR 10 dB, RSSI −90 dBm).
//!
//! `SEND_PATH_DISCOVERY_REQ` gets `SENT` and no answer, as with stock firmware whose owner did
//! not allow telemetry (the default): lichen discovers paths itself (§4.5).
//!
//! Debugging: `LICHEN_TRACE=1` prints the lichen frames each companion hands over and receives,
//! `LICHEN_TRACE_AIR=1` the fate of every reception (ok, lost, collision, half duplex).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use lc_meshcore::air::{Packet, PayloadType, Route};
use lc_meshcore::airtime::LoRaParams;
use lc_meshcore::codes::{cmd, resp};
use lc_meshcore::ecdh::{expand_seed, public_key};
use lc_meshcore::path::{PATH_UNKNOWN, Path};
use lc_proto::Millis;
use rand_chacha::ChaCha8Rng;
use rand_core::{Rng, SeedableRng};

use crate::core::{Config, Core, Event, OwnedPath, PubKey, contact_kind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Companion,
    Repeater,
}

/// One simulated node.
pub struct Node {
    pub key: PubKey,
    prv: [u8; 64],
    pub name: String,
    pub role: Role,
    /// The app attached to a companion (absent for repeaters).
    pub core: Option<Core>,
    /// Whether the app is connected (pushes are lost otherwise).
    pub connected: bool,
    /// Key, name, path, MeshCore contact type (`contact_kind`).
    contacts: Vec<(PubKey, String, Option<OwnedPath>, u8)>,
    dedup: VecDeque<[u8; 8]>,
    txq: VecDeque<(Millis, Vec<u8>)>,
    tx_until: Millis,
    /// Earliest `AppWake` already scheduled.
    wake_at: Option<Millis>,
    pub events: Vec<Event>,
    pub tx_frames: u64,
    pub tx_airtime_ms: Millis,
    /// `RAW_DATA` pushes delivered to the app.
    pub raw_pushed: u64,
    /// The app is outside the simulation (a real app driving a simulated companion): its
    /// frames come through [`Net::external_write`] and go out through [`Net::external_read`].
    external: bool,
    outbox: Vec<Vec<u8>>,
    /// Position on the plane (east, north metres), TX power, noise floor; RSSI reported as 0.
    pub pos: Option<(f64, f64)>,
    pub tx_dbm: i8,
    pub noise_dbm: i16,
    pub rssi_zero: bool,
    /// Last reception (for `GET_STATS`).
    last_rx: (i8, i8),
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("hash", &self.key[0])
            .field("name", &self.name)
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ev {
    AppWake(usize),
    TryTx(usize),
    TxEnd(u64),
}

#[derive(Clone, Debug)]
struct Tx {
    id: u64,
    from: usize,
    start: Millis,
    end: Millis,
    frame: Vec<u8>,
}

/// Counters over a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub transmissions: u64,
    pub receptions: u64,
    pub collisions: u64,
    pub random_losses: u64,
    pub half_duplex_losses: u64,
}

/// The simulated network.
pub struct Net {
    pub now: Millis,
    radio: LoRaParams,
    loss_pct: u32,
    rng: ChaCha8Rng,
    pub nodes: Vec<Node>,
    neighbors: Vec<Vec<usize>>,
    queue: BinaryHeap<Reverse<(Millis, u64, Ev)>>,
    seq: u64,
    air: Vec<Tx>,
    next_tx_id: u64,
    pub stats: Stats,
    /// Path loss between placed nodes.
    pub geometry: Geometry,
    /// Frames a node does not hear, by receiver and raw frame: a scripted loss (the demo's
    /// lost group post, §10.6).
    pub deaf: Option<Deaf>,
}

/// See [`Net::deaf`].
pub type Deaf = Box<dyn FnMut(usize, &[u8]) -> bool + Send>;

/// Path loss between placed nodes: `RSSI = tx + a − 10·n·log10(d) + N(0, σ)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub a_db: f64,
    pub n: f64,
    pub sigma_db: f64,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            a_db: -32.0,
            n: 2.8,
            sigma_db: 6.0,
        }
    }
}

/// Plane origin of placed nodes (their GPS fixes), degrees.
pub const ORIGIN: (f64, f64) = (45.19, 5.72);
/// SNR readings saturate there, dB.
const SNR_MAX_DB: f64 = 13.0;

/// A plane point (east, north metres) in degrees.
pub fn latlon(east: f64, north: f64) -> (f64, f64) {
    (
        ORIGIN.0 + north / 111_320.0,
        ORIGIN.1 + east / (111_320.0 * ORIGIN.0.to_radians().cos()),
    )
}

impl std::fmt::Debug for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Net")
            .field("now", &self.now)
            .field("nodes", &self.nodes)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

const DEDUP_SIZE: usize = 160;
/// Wall clock of every app at simulated time 0 (Unix milliseconds, 2026-10-26).
pub const EPOCH_MS: u64 = 1_793_000_000_000;
const FREQ_KHZ: u32 = 869_618;

impl Net {
    /// An empty network with the given preset, per-link loss (percent) and random seed.
    pub fn new(radio: LoRaParams, loss_pct: u32, seed: u64) -> Self {
        Self {
            now: 0,
            radio,
            loss_pct,
            rng: ChaCha8Rng::seed_from_u64(seed),
            nodes: Vec::new(),
            neighbors: Vec::new(),
            queue: BinaryHeap::new(),
            seq: 0,
            air: Vec::new(),
            next_tx_id: 0,
            stats: Stats::default(),
            geometry: Geometry::default(),
            deaf: None,
        }
    }

    fn add(&mut self, seed: u8, name: &str, role: Role) -> usize {
        // MeshCore routes by 1-byte hashes: keep them unique in the simulation.
        let mut s = [seed; 32];
        let (prv, key) = loop {
            let prv = expand_seed(&s);
            let key = public_key(&prv);
            if key[0] != 0 && key[0] != 0xFF && self.nodes.iter().all(|n: &Node| n.key[0] != key[0])
            {
                break (prv, key);
            }
            s[31] = s[31].wrapping_add(1);
        };
        self.nodes.push(Node {
            key,
            prv,
            name: name.to_owned(),
            role,
            core: None,
            connected: false,
            contacts: Vec::new(),
            dedup: VecDeque::new(),
            txq: VecDeque::new(),
            tx_until: 0,
            wake_at: None,
            events: Vec::new(),
            tx_frames: 0,
            tx_airtime_ms: 0,
            raw_pushed: 0,
            external: false,
            outbox: Vec::new(),
            pos: None,
            tx_dbm: 22,
            noise_dbm: -118,
            rssi_zero: false,
            last_rx: (0, 0),
        });
        self.neighbors.push(Vec::new());
        self.nodes.len() - 1
    }

    pub fn add_repeater(&mut self, seed: u8, name: &str) -> usize {
        self.add(seed, name, Role::Repeater)
    }

    /// A companion with an app attached (not connected yet).
    pub fn add_companion(&mut self, seed: u8, name: &str) -> usize {
        let i = self.add(seed, name, Role::Companion);
        self.attach_app(i, seed);
        i
    }

    /// A companion driven by an app outside the simulation.
    pub fn add_external_companion(&mut self, seed: u8, name: &str) -> usize {
        let i = self.add(seed, name, Role::Companion);
        if let Some(n) = self.nodes.get_mut(i) {
            n.external = true;
            n.connected = true;
        }
        i
    }

    /// A frame from the external app to companion `i`; the answers go to its outbox.
    pub fn external_write(&mut self, i: usize, frame: &[u8]) {
        let replies = self.companion_command(i, frame);
        if let Some(n) = self.nodes.get_mut(i) {
            n.outbox.extend(replies);
        }
    }

    /// Frames from companion `i` to the external app.
    pub fn external_read(&mut self, i: usize) -> Vec<Vec<u8>> {
        self.nodes
            .get_mut(i)
            .map(|n| std::mem::take(&mut n.outbox))
            .unwrap_or_default()
    }

    /// Replaces the companion's app by a fresh one with a **new identity** and no stored
    /// contacts (the app reinstalled without its recovery phrase, ADR-0013).
    pub fn reinstall_app(&mut self, i: usize, seed: u8) {
        self.attach_app(i, seed);
        let mut rng_seed = [seed; 32];
        rng_seed[1] ^= 0x33;
        let mut id_seed = self.nodes.get(i).map_or([seed; 32], |n| n.key);
        id_seed[0] ^= 0xA5;
        id_seed[1] ^= seed | 1;
        let mut core = Core::new(Config::new("lichen-sim", id_seed, rng_seed));
        core.set_clock(EPOCH_MS + self.now, self.now);
        if let Some(n) = self.nodes.get_mut(i) {
            n.core = Some(core);
        }
    }

    /// Replaces the companion's app by a fresh one (an app restart: sessions are lost).
    pub fn attach_app(&mut self, i: usize, seed: u8) {
        let mut rng_seed = [seed; 32];
        rng_seed[0] ^= 0x5A;
        rng_seed[1] = self.rng.next_u32() as u8;
        // The phone identity (ADR-0013) depends on the node only: a restarted app keeps it, as
        // the real app keeps its seed in the Keychain.
        let mut id_seed = self.nodes.get(i).map_or([seed; 32], |n| n.key);
        id_seed[0] ^= 0xA5;
        let mut core = Core::new(Config::new("lichen-sim", id_seed, rng_seed));
        core.set_clock(EPOCH_MS + self.now, self.now);
        if let Some(n) = self.nodes.get_mut(i) {
            n.core = Some(core);
            n.connected = false;
            n.wake_at = None;
        }
    }

    pub fn link(&mut self, a: usize, b: usize) {
        if let Some(v) = self.neighbors.get_mut(a)
            && !v.contains(&b)
        {
            v.push(b);
        }
        if let Some(v) = self.neighbors.get_mut(b)
            && !v.contains(&a)
        {
            v.push(a);
        }
    }

    /// Removes every link of node `i`.
    pub fn unlink_all(&mut self, i: usize) {
        if let Some(v) = self.neighbors.get_mut(i) {
            v.clear();
        }
        for v in &mut self.neighbors {
            v.retain(|&x| x != i);
        }
    }

    /// Links consecutive nodes.
    pub fn chain(&mut self, nodes: &[usize]) {
        for w in nodes.windows(2) {
            if let [a, b] = w {
                self.link(*a, *b);
            }
        }
    }

    /// Places node `i` at `east`, `north` metres (readings then follow distance).
    pub fn place(&mut self, i: usize, east: f64, north: f64) {
        if let Some(n) = self.nodes.get_mut(i) {
            n.pos = Some((east, north));
        }
    }

    /// Links every pair of placed nodes whose mean SNR is above −5 dB.
    pub fn link_in_range(&mut self) {
        let placed: Vec<usize> = (0..self.nodes.len())
            .filter(|&i| self.nodes.get(i).is_some_and(|n| n.pos.is_some()))
            .collect();
        for &a in &placed {
            for &b in &placed {
                if a < b && self.mean_snr(a, b).is_some_and(|s| s > -5.0) {
                    self.link(a, b);
                }
            }
        }
    }

    /// Gives the app of node `i` a GPS fix at its position (5 m).
    pub fn locate(&mut self, i: usize) {
        let Some((e, n)) = self.nodes.get(i).and_then(|n| n.pos) else {
            return;
        };
        let (lat, lon) = latlon(e, n);
        let unix_s = (EPOCH_MS + self.now) / 1000;
        self.with_core(i, |c, now| {
            c.update_location(
                crate::location::Fix {
                    lat,
                    lon,
                    alt_m: None,
                    accuracy_m: 5.0,
                    heading_deg: None,
                    speed_mps: None,
                    unix_s,
                },
                now,
            );
        });
    }

    /// Mean RSSI at `r` of a transmission from `t`, when both are placed.
    fn mean_rssi(&self, t: usize, r: usize) -> Option<f64> {
        let (a, b) = (self.nodes.get(t)?, self.nodes.get(r)?);
        let ((x0, y0), (x1, y1)) = (a.pos?, b.pos?);
        let d = (x1 - x0).hypot(y1 - y0).max(1.0);
        let g = &self.geometry;
        Some(f64::from(a.tx_dbm) + g.a_db - 10.0 * g.n * d.log10())
    }

    fn mean_snr(&self, t: usize, r: usize) -> Option<f64> {
        Some(self.mean_rssi(t, r)? - f64::from(self.nodes.get(r)?.noise_dbm))
    }

    /// One reception's readings `(snr4, rssi)` at `r` from `t`, or `None` when it is below the
    /// demodulation floor (SF8: −10 dB).
    fn readings(&mut self, t: usize, r: usize) -> Option<(i8, i8)> {
        let Some(mean) = self.mean_rssi(t, r) else {
            return Some((40, -90));
        };
        let u = ((self.rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64).max(1e-300);
        let v = (self.rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        let z = (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos();
        let rssi = mean + self.geometry.sigma_db * z;
        let node = self.nodes.get(r)?;
        let snr = rssi - f64::from(node.noise_dbm);
        if snr < -10.0 {
            return None;
        }
        let snr4 = (snr.min(SNR_MAX_DB) * 4.0).round().clamp(-128.0, 127.0) as i8;
        let rssi = if node.rssi_zero {
            0
        } else {
            rssi.round().clamp(-127.0, -1.0) as i8
        };
        Some((snr4, rssi))
    }

    pub fn key(&self, i: usize) -> PubKey {
        self.nodes.get(i).map_or([0; 32], |n| n.key)
    }

    /// Shortest repeater path from `a` to `b` (hashes of the intermediate nodes).
    pub fn route(&self, a: usize, b: usize) -> Option<OwnedPath> {
        let mut prev = vec![usize::MAX; self.nodes.len()];
        let mut q = VecDeque::from([a]);
        prev[a] = a;
        while let Some(x) = q.pop_front() {
            if x == b {
                break;
            }
            for &y in self.neighbors.get(x)? {
                // Only repeaters forward.
                let forwards = y == b || self.nodes.get(y)?.role == Role::Repeater;
                if prev[y] == usize::MAX && forwards {
                    prev[y] = x;
                    q.push_back(y);
                }
            }
        }
        if prev.get(b).copied()? == usize::MAX {
            return None;
        }
        let mut hops = Vec::new();
        let mut x = *prev.get(b)?;
        while x != a {
            hops.push(self.nodes.get(x)?.key[0]);
            x = *prev.get(x)?;
        }
        hops.reverse();
        Some(OwnedPath {
            len_byte: hops.len() as u8,
            bytes: hops,
        })
    }

    /// Puts `b` in `a`'s companion contact table, with its path when `with_path`.
    pub fn know(&mut self, a: usize, b: usize, with_path: bool) {
        let path = if with_path { self.route(a, b) } else { None };
        let (key, name) = (self.key(b), self.nodes.get(b).map(|n| n.name.clone()));
        if let Some(n) = self.nodes.get_mut(a) {
            n.contacts.retain(|(k, ..)| *k != key);
            n.contacts
                .push((key, name.unwrap_or_default(), path, contact_kind::CHAT));
        }
    }

    /// Puts a node outside the simulation in `a`'s companion contact table (no path): a
    /// repeater or another companion sharing a hash byte with a simulated node.
    pub fn know_other(&mut self, a: usize, key: PubKey, name: &str, kind: u8) {
        if let Some(n) = self.nodes.get_mut(a) {
            n.contacts.retain(|(k, ..)| *k != key);
            n.contacts.push((key, name.to_owned(), None, kind));
        }
    }

    /// The apps of `a` and `b` learn each other's identity, verified, as if they had scanned
    /// each other's contact card in person (§4.7). Call once both are ready.
    pub fn introduce(&mut self, a: usize, b: usize) {
        let id = |net: &Self, i: usize| {
            net.nodes
                .get(i)
                .and_then(|n| n.core.as_ref())
                .and_then(|c| c.key().copied())
        };
        let (Some(ia), Some(ib)) = (id(self, a), id(self, b)) else {
            return;
        };
        for (me, other, other_id) in [(a, b, ib), (b, a, ia)] {
            let node = self.key(other);
            let name = self
                .nodes
                .get(other)
                .map(|n| n.name.clone())
                .unwrap_or_default();
            let path = self.route(me, other);
            self.with_core(me, |c, _| {
                if !c.contacts().any(|k| k.key == node) {
                    c.add_contact(node, &name, path);
                }
                c.set_identity(&node, other_id, true);
            });
        }
    }

    /// Connects the app of companion `i` (runs `Core::connect`).
    pub fn connect(&mut self, i: usize) {
        let now = self.now;
        if let Some(n) = self.nodes.get_mut(i) {
            n.connected = true;
            if let Some(c) = n.core.as_mut() {
                c.connect(now);
            }
        }
        self.wake(i, now);
    }

    /// The app of companion `i` loses its companion (Bluetooth out of range): pushes are lost,
    /// commands go nowhere, and the app is told (`Core::disconnect`).
    pub fn disconnect(&mut self, i: usize) {
        let now = self.now;
        if let Some(n) = self.nodes.get_mut(i) {
            n.connected = false;
            if let Some(c) = n.core.as_mut() {
                c.disconnect(now);
            }
        }
        self.wake(i, now);
    }

    /// Moves the app of companion `from` to companion `to` (the phone paired with another
    /// radio), disconnected; [`Net::connect`] it there.
    pub fn move_app(&mut self, from: usize, to: usize) {
        let now = self.now;
        let core = self.nodes.get_mut(from).and_then(|n| {
            n.connected = false;
            n.wake_at = None;
            n.core.take()
        });
        if let Some(mut c) = core {
            c.disconnect(now);
            if let Some(n) = self.nodes.get_mut(to) {
                n.core = Some(c);
                n.connected = false;
                n.wake_at = None;
            }
        }
    }

    /// Runs `f` on the app of companion `i`, then lets it act.
    pub fn with_core<T>(&mut self, i: usize, f: impl FnOnce(&mut Core, Millis) -> T) -> Option<T> {
        let now = self.now;
        let r = self.nodes.get_mut(i)?.core.as_mut().map(|c| f(c, now));
        self.wake(i, now);
        r
    }

    /// Events reported by the app of companion `i` since the last call.
    /// Reads the app of node `i`.
    pub fn with_core_ref<T>(&self, i: usize, f: impl FnOnce(&Core) -> T) -> Option<T> {
        self.nodes.get(i)?.core.as_ref().map(f)
    }

    pub fn take_events(&mut self, i: usize) -> Vec<Event> {
        self.nodes
            .get_mut(i)
            .map(|n| std::mem::take(&mut n.events))
            .unwrap_or_default()
    }

    fn schedule(&mut self, at: Millis, ev: Ev) {
        self.seq += 1;
        self.queue.push(Reverse((at, self.seq, ev)));
    }

    fn wake(&mut self, i: usize, at: Millis) {
        let Some(n) = self.nodes.get_mut(i) else {
            return;
        };
        if n.wake_at.is_some_and(|w| w <= at) {
            return;
        }
        n.wake_at = Some(at);
        self.schedule(at, Ev::AppWake(i));
    }

    /// Runs the simulation until `deadline`, or until `stop` returns true (checked after every
    /// app wake-up). Returns whether `stop` fired.
    pub fn run_until(&mut self, deadline: Millis, mut stop: impl FnMut(&mut Net) -> bool) -> bool {
        while let Some(Reverse((t, _, ev))) = self.queue.pop() {
            if t > deadline {
                self.queue.push(Reverse((t, 0, ev)));
                self.now = deadline;
                return false;
            }
            self.now = self.now.max(t);
            match ev {
                Ev::AppWake(i) => {
                    if self.nodes.get(i).and_then(|n| n.wake_at) != Some(t) {
                        continue;
                    }
                    self.app_wake(i);
                    if stop(self) {
                        return true;
                    }
                }
                Ev::TryTx(i) => self.try_tx(i),
                Ev::TxEnd(id) => self.tx_end(id),
            }
        }
        self.now = deadline;
        false
    }

    // ----- app side --------------------------------------------------------------------

    fn app_wake(&mut self, i: usize) {
        let now = self.now;
        let Some(mut core) = self.nodes.get_mut(i).and_then(|n| {
            n.wake_at = None;
            n.core.take()
        }) else {
            return;
        };
        core.tick(now);
        let connected = self.nodes.get(i).is_some_and(|n| n.connected);
        while let Some(frame) = core.poll_companion(now) {
            // A disconnected companion never gets the command.
            if !connected {
                continue;
            }
            for reply in self.companion_command(i, &frame) {
                core.on_companion_frame(&reply, now);
            }
        }
        let next = core.next_wakeup();
        if let Some(n) = self.nodes.get_mut(i) {
            while let Some(e) = core.poll_event() {
                n.events.push(e);
            }
            n.core = Some(core);
        }
        if let Some(t) = next {
            self.wake(i, t.max(now + 1));
        }
    }

    /// Pushes a frame to the app of companion `i` if it is connected.
    fn push_to_app(&mut self, i: usize, frame: &[u8]) {
        let now = self.now;
        let Some(n) = self.nodes.get_mut(i) else {
            return;
        };
        if !n.connected {
            return;
        }
        if n.external {
            n.outbox.push(frame.to_vec());
            return;
        }
        if let Some(c) = n.core.as_mut() {
            c.on_companion_frame(frame, now);
        }
        self.wake(i, now);
    }

    fn contact_frame(
        code: u8,
        key: &PubKey,
        name: &str,
        path: Option<&OwnedPath>,
        kind: u8,
    ) -> Vec<u8> {
        let mut v = vec![code];
        v.extend_from_slice(key);
        v.extend_from_slice(&[kind, 0]);
        let mut raw_path = [0u8; 64];
        match path {
            Some(p) => {
                v.push(p.len_byte);
                raw_path[..p.bytes.len()].copy_from_slice(&p.bytes);
            }
            None => v.push(PATH_UNKNOWN),
        }
        v.extend_from_slice(&raw_path);
        let mut n = [0u8; 32];
        let b = name.as_bytes();
        n[..b.len().min(31)].copy_from_slice(&b[..b.len().min(31)]);
        v.extend_from_slice(&n);
        v.extend_from_slice(&[0u8; 16]);
        v
    }

    /// The stock companion's answer to one app command.
    fn companion_command(&mut self, i: usize, frame: &[u8]) -> Vec<Vec<u8>> {
        let Some(&code) = frame.first() else {
            return vec![];
        };
        let Some(node) = self.nodes.get(i) else {
            return vec![];
        };
        let (key, prv, name) = (node.key, node.prv, node.name.clone());
        let (tx_dbm, noise, last_rx) = (node.tx_dbm, node.noise_dbm, node.last_rx);
        match code {
            cmd::APP_START => {
                let mut v = vec![resp::SELF_INFO, 1, tx_dbm as u8, 22];
                v.extend_from_slice(&key);
                v.extend_from_slice(&[0u8; 12]);
                v.extend_from_slice(&FREQ_KHZ.to_le_bytes());
                v.extend_from_slice(&self.radio.bw_hz.to_le_bytes());
                v.extend_from_slice(&[self.radio.sf, self.radio.cr]);
                v.extend_from_slice(name.as_bytes());
                vec![v]
            }
            cmd::DEVICE_QUERY => vec![vec![resp::DEVICE_INFO, 14]],
            cmd::EXPORT_PRIVATE_KEY => {
                let mut v = vec![resp::PRIVATE_KEY];
                v.extend_from_slice(&prv);
                vec![v]
            }
            cmd::GET_CONTACTS => {
                let mut out = vec![{
                    let mut v = vec![resp::CONTACTS_START];
                    v.extend_from_slice(&(node.contacts.len() as u32).to_le_bytes());
                    v
                }];
                for (k, n, p, t) in &node.contacts {
                    out.push(Self::contact_frame(resp::CONTACT, k, n, p.as_ref(), *t));
                }
                out.push(vec![resp::END_OF_CONTACTS, 0, 0, 0, 0]);
                out
            }
            cmd::GET_CONTACT_BY_KEY => {
                let k = frame.get(1..33);
                match node.contacts.iter().find(|(c, ..)| Some(&c[..]) == k) {
                    Some((c, n, p, t)) => {
                        vec![Self::contact_frame(resp::CONTACT, c, n, p.as_ref(), *t)]
                    }
                    None => vec![vec![resp::ERR, 2]],
                }
            }
            cmd::SEND_RAW_DATA => {
                let Some(&len_byte) = frame.get(1) else {
                    return vec![vec![resp::ERR, 1]];
                };
                let n = lc_meshcore::path::path_bytes(len_byte).unwrap_or(usize::MAX);
                let (Some(path), Some(payload)) = (frame.get(2..2 + n), frame.get(2 + n..)) else {
                    return vec![vec![resp::ERR, 1]];
                };
                if std::env::var_os("LICHEN_TRACE").is_some()
                    && let Ok((h, _)) = lc_proto::Header::decode(payload)
                {
                    eprintln!(
                        "{:>8} tx@{i} {:?} tag={} idx={} tot={} att={} {:02x}>{:02x}",
                        self.now, h.kind, h.tag, h.index, h.total, h.attempt, h.src, h.dst
                    );
                }
                let mut air = vec![((PayloadType::RawCustom as u8) << 2) | Route::Direct as u8];
                air.push(len_byte);
                air.extend_from_slice(path);
                air.extend_from_slice(payload);
                self.enqueue_tx(i, air, 0.2);
                vec![vec![resp::OK]]
            }
            cmd::SEND_RAW_PACKET => {
                if let Some(raw) = frame.get(2..) {
                    self.enqueue_tx(i, raw.to_vec(), 0.0);
                }
                vec![vec![resp::OK]]
            }
            cmd::SEND_SELF_ADVERT => {
                // Simplified: no airtime. A zero-hop advert reaches the neighbouring
                // companions, a flood every companion with a route back; each adds the node
                // (the stock companion's automatic adding) and tells its app.
                let flood = frame.get(1) == Some(&1);
                let hearers: Vec<usize> = (0..self.nodes.len())
                    .filter(|&j| {
                        j != i
                            && self.nodes.get(j).is_some_and(|n| n.role == Role::Companion)
                            && (flood || self.neighbors.get(i).is_some_and(|v| v.contains(&j)))
                    })
                    .collect();
                for j in hearers {
                    let Some(path) = self.route(j, i) else {
                        continue;
                    };
                    if let Some(n) = self.nodes.get_mut(j)
                        && n.contacts.iter().all(|(k, ..)| *k != key)
                    {
                        n.contacts
                            .push((key, name.clone(), Some(path), contact_kind::CHAT));
                    }
                    let mut push = vec![resp::ADVERT];
                    push.extend_from_slice(&key);
                    self.push_to_app(j, &push);
                }
                vec![vec![resp::OK]]
            }
            cmd::GET_DEFAULT_FLOOD_SCOPE => vec![vec![resp::DEFAULT_FLOOD_SCOPE]],
            cmd::GET_STATS if frame.get(1) == Some(&1) => {
                let mut v = vec![resp::STATS, 1];
                v.extend_from_slice(&noise.to_le_bytes());
                v.extend_from_slice(&[last_rx.1 as u8, last_rx.0 as u8]);
                v.extend_from_slice(&[0u8; 8]);
                vec![v]
            }
            cmd::SET_RADIO_TX_POWER => match frame.get(1).map(|&b| b as i8) {
                Some(dbm @ -9..=22) => {
                    if let Some(n) = self.nodes.get_mut(i) {
                        n.tx_dbm = dbm;
                    }
                    vec![vec![resp::OK]]
                }
                _ => vec![vec![resp::ERR, 6]],
            },
            cmd::SEND_PATH_DISCOVERY_REQ => {
                let mut v = vec![resp::SENT, 1];
                v.extend_from_slice(&[1, 2, 3, 4]);
                v.extend_from_slice(&20_000u32.to_le_bytes());
                vec![v]
            }
            _ => vec![vec![resp::ERR, 1]],
        }
    }

    // ----- radio side ------------------------------------------------------------------

    fn airtime(&self, len: usize) -> Millis {
        self.radio.airtime_us(len) / 1000
    }

    /// Queues a frame for transmission after MeshCore's random delay `rand[0, 5·t·factor]`.
    fn enqueue_tx(&mut self, i: usize, frame: Vec<u8>, factor: f64) {
        let t = self.airtime(frame.len());
        let max = (5.0 * t as f64 * factor) as u64;
        let delay = if max == 0 {
            0
        } else {
            u64::from(self.rng.next_u32()) % max
        };
        let ready = self.now + delay;
        if let Ok(p) = Packet::decode(&frame) {
            let h = p.dedup_hash();
            if let Some(n) = self.nodes.get_mut(i) {
                remember(&mut n.dedup, h);
            }
        }
        if let Some(n) = self.nodes.get_mut(i) {
            n.txq.push_back((ready, frame));
        }
        self.schedule(ready, Ev::TryTx(i));
    }

    fn channel_busy(&self, i: usize, at: Millis) -> bool {
        let Some(nb) = self.neighbors.get(i) else {
            return false;
        };
        self.air
            .iter()
            .any(|tx| nb.contains(&tx.from) && tx.start <= at && at < tx.end)
    }

    fn try_tx(&mut self, i: usize) {
        let now = self.now;
        let Some(n) = self.nodes.get(i) else {
            return;
        };
        let Some(&(ready, _)) = n.txq.front() else {
            return;
        };
        if ready > now {
            self.schedule(ready, Ev::TryTx(i));
            return;
        }
        if n.tx_until > now {
            let at = n.tx_until;
            self.schedule(at, Ev::TryTx(i));
            return;
        }
        if self.channel_busy(i, now) {
            let backoff = 120 * (1 + Millis::from(self.rng.next_u32() % 3));
            self.schedule(now + backoff, Ev::TryTx(i));
            return;
        }
        let Some((_, frame)) = self.nodes.get_mut(i).and_then(|n| n.txq.pop_front()) else {
            return;
        };
        let airtime = self.airtime(frame.len());
        let id = self.next_tx_id;
        self.next_tx_id += 1;
        self.air.retain(|tx| tx.end + 10_000 > now);
        self.air.push(Tx {
            id,
            from: i,
            start: now,
            end: now + airtime,
            frame,
        });
        if let Some(n) = self.nodes.get_mut(i) {
            n.tx_until = now + airtime;
            n.tx_frames += 1;
            n.tx_airtime_ms += airtime;
        }
        self.stats.transmissions += 1;
        self.schedule(now + airtime, Ev::TxEnd(id));
    }

    fn tx_end(&mut self, id: u64) {
        let Some(tx) = self.air.iter().find(|t| t.id == id).cloned() else {
            return;
        };
        let receivers = self.neighbors.get(tx.from).cloned().unwrap_or_default();
        for r in receivers {
            let overlaps = |o: &Tx| o.id != tx.id && o.start < tx.end && tx.start < o.end;
            let trace = std::env::var_os("LICHEN_TRACE_AIR").is_some();
            let label = || {
                Packet::decode(&tx.frame)
                    .ok()
                    .and_then(|p| lc_proto::Header::decode(p.payload).ok())
                    .map(|(h, _)| format!("{:?} idx={} att={}", h.kind, h.index, h.attempt))
                    .unwrap_or_default()
            };
            if self.air.iter().any(|o| o.from == r && overlaps(o)) {
                self.stats.half_duplex_losses += 1;
                if trace {
                    eprintln!("{:>8} {}->{} HALF-DUPLEX {}", self.now, tx.from, r, label());
                }
                continue;
            }
            let nb = self.neighbors.get(r).cloned().unwrap_or_default();
            if self.air.iter().any(|o| nb.contains(&o.from) && overlaps(o)) {
                self.stats.collisions += 1;
                if trace {
                    eprintln!("{:>8} {}->{} COLLISION {}", self.now, tx.from, r, label());
                }
                continue;
            }
            let Some((snr4, rssi)) = self.readings(tx.from, r) else {
                self.stats.random_losses += 1;
                continue;
            };
            if self.deaf.as_mut().is_some_and(|deaf| deaf(r, &tx.frame)) {
                self.stats.random_losses += 1;
                if trace {
                    eprintln!("{:>8} {}->{} missed {}", self.now, tx.from, r, label());
                }
                continue;
            }
            if self.rng.next_u32() % 100 < self.loss_pct {
                self.stats.random_losses += 1;
                if trace {
                    eprintln!("{:>8} {}->{} lost {}", self.now, tx.from, r, label());
                }
                continue;
            }
            if trace {
                eprintln!("{:>8} {}->{} ok {}", self.now, tx.from, r, label());
            }
            self.stats.receptions += 1;
            if let Some(n) = self.nodes.get_mut(r) {
                n.last_rx = (snr4, rssi);
            }
            self.receive(r, &tx.frame, snr4, rssi);
        }
        if self.nodes.get(tx.from).is_some_and(|n| !n.txq.is_empty()) {
            self.schedule(self.now, Ev::TryTx(tx.from));
        }
    }

    fn receive(&mut self, r: usize, frame: &[u8], snr4: i8, rssi: i8) {
        let Ok(p) = Packet::decode(frame) else {
            return;
        };
        if self.nodes.get(r).is_some_and(|n| n.role == Role::Companion) && frame.len() + 3 <= 176 {
            let mut log = vec![resp::LOG_RX_DATA, snr4 as u8, rssi as u8];
            log.extend_from_slice(frame);
            self.push_to_app(r, &log);
        }
        let h = p.dedup_hash();
        let Some(node) = self.nodes.get_mut(r) else {
            return;
        };
        if node.dedup.contains(&h) {
            return;
        }
        remember(&mut node.dedup, h);
        let (role, my_hash) = (node.role, node.key[0]);
        let direct = matches!(p.route, Route::Direct | Route::TransportDirect);
        if direct && !p.path.bytes().is_empty() {
            if role == Role::Repeater && p.path.bytes().first() == Some(&my_hash) {
                let rest = p.path.bytes().get(1..).unwrap_or(&[]);
                let Ok(path) = Path::new(p.path.len_byte() - 1, rest) else {
                    return;
                };
                let fwd = Packet { path, ..p };
                let mut out = [0u8; 255];
                if let Ok(n) = fwd.encode(&mut out) {
                    self.enqueue_tx(r, out[..n].to_vec(), 0.3);
                }
            }
            return;
        }
        if !direct && role == Role::Repeater && p.path.hash_size() == 1 && p.path.hops() < 63 {
            let mut bytes = p.path.bytes().to_vec();
            bytes.push(my_hash);
            let Ok(path) = Path::new(p.path.len_byte() + 1, &bytes) else {
                return;
            };
            let fwd = Packet { path, ..p };
            let mut out = [0u8; 255];
            if let Ok(n) = fwd.encode(&mut out) {
                self.enqueue_tx(r, out[..n].to_vec(), 0.5);
            }
            return;
        }
        if direct && role == Role::Companion && p.ptype == PayloadType::RawCustom {
            if let Some(n) = self.nodes.get_mut(r) {
                n.raw_pushed += 1;
            }
            if std::env::var_os("LICHEN_TRACE").is_some()
                && let Ok((h, _)) = lc_proto::Header::decode(p.payload)
            {
                eprintln!(
                    "{:>8} rx@{r} {:?} tag={} idx={} tot={} att={} {:02x}>{:02x}",
                    self.now, h.kind, h.tag, h.index, h.total, h.attempt, h.src, h.dst
                );
            }
            let mut push = vec![resp::RAW_DATA, snr4 as u8, rssi as u8, 0xFF];
            push.extend_from_slice(p.payload);
            self.push_to_app(r, &push);
        }
    }
}

fn remember(ring: &mut VecDeque<[u8; 8]>, h: [u8; 8]) {
    if ring.len() >= DEDUP_SIZE {
        ring.pop_front();
    }
    ring.push_back(h);
}

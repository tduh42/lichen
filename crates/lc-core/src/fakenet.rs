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

use crate::core::{Config, Core, Event, OwnedPath, PubKey};

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
    contacts: Vec<(PubKey, String, Option<OwnedPath>)>,
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

    /// Replaces the companion's app by a fresh one (an app restart: sessions are lost).
    pub fn attach_app(&mut self, i: usize, seed: u8) {
        let mut rng_seed = [seed; 32];
        rng_seed[0] ^= 0x5A;
        rng_seed[1] = self.rng.next_u32() as u8;
        let core = Core::new(Config::new("lichen-sim", rng_seed));
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

    /// Links consecutive nodes.
    pub fn chain(&mut self, nodes: &[usize]) {
        for w in nodes.windows(2) {
            if let [a, b] = w {
                self.link(*a, *b);
            }
        }
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
            n.contacts.retain(|(k, _, _)| *k != key);
            n.contacts.push((key, name.unwrap_or_default(), path));
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

    /// Runs `f` on the app of companion `i`, then lets it act.
    pub fn with_core<T>(&mut self, i: usize, f: impl FnOnce(&mut Core, Millis) -> T) -> Option<T> {
        let now = self.now;
        let r = self.nodes.get_mut(i)?.core.as_mut().map(|c| f(c, now));
        self.wake(i, now);
        r
    }

    /// Events reported by the app of companion `i` since the last call.
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
        while let Some(frame) = core.poll_companion(now) {
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

    fn contact_frame(code: u8, key: &PubKey, name: &str, path: Option<&OwnedPath>) -> Vec<u8> {
        let mut v = vec![code];
        v.extend_from_slice(key);
        v.extend_from_slice(&[1, 0]);
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
        match code {
            cmd::APP_START => {
                let mut v = vec![resp::SELF_INFO, 1, 22, 22];
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
                for (k, n, p) in &node.contacts {
                    out.push(Self::contact_frame(resp::CONTACT, k, n, p.as_ref()));
                }
                out.push(vec![resp::END_OF_CONTACTS, 0, 0, 0, 0]);
                out
            }
            cmd::GET_CONTACT_BY_KEY => {
                let k = frame.get(1..33);
                match node.contacts.iter().find(|(c, _, _)| Some(&c[..]) == k) {
                    Some((c, n, p)) => vec![Self::contact_frame(resp::CONTACT, c, n, p.as_ref())],
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
                        "{:>8} tx@{i} {:?} tag={} idx={} tot={} att={}",
                        self.now, h.kind, h.tag, h.index, h.total, h.attempt
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
            cmd::GET_DEFAULT_FLOOD_SCOPE => vec![vec![resp::DEFAULT_FLOOD_SCOPE]],
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
            self.receive(r, &tx.frame);
        }
        if self.nodes.get(tx.from).is_some_and(|n| !n.txq.is_empty()) {
            self.schedule(self.now, Ev::TryTx(tx.from));
        }
    }

    fn receive(&mut self, r: usize, frame: &[u8]) {
        let Ok(p) = Packet::decode(frame) else {
            return;
        };
        if self.nodes.get(r).is_some_and(|n| n.role == Role::Companion) && frame.len() + 3 <= 176 {
            let mut log = vec![resp::LOG_RX_DATA, 40, (-90i8) as u8];
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
                    "{:>8} rx@{r} {:?} tag={} idx={} tot={} att={}",
                    self.now, h.kind, h.tag, h.index, h.total, h.attempt
                );
            }
            let mut push = vec![resp::RAW_DATA, 40, (-90i8) as u8, 0xFF];
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

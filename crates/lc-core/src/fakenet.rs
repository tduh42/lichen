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
//! **Plain MeshCore text** (§15): `SEND_TXT_MSG` and `SEND_CHANNEL_TXT_MSG` put a `TXT_MSG` or
//! `GRP_TXT` of the real size on the air (its content is not encrypted in the simulation); the
//! recipient's companion queues it for `SYNC_NEXT_MESSAGE`, pushes `MSG_WAITING`, and answers a
//! direct message with an `ACK` that comes back as `SEND_CONFIRMED`. Channel slots answer
//! `GET_CHANNEL` and `SET_CHANNEL`. A companion without an app (someone with a stock client)
//! is driven with [`Net::send_plain_text`] and [`Net::post_on_channel`]; [`Net::hand_over`]
//! gives a companion, out of band, such a frame the air lost on its way (the demo, UI tests).
//!
//! **Mesh tools** (§15.8, §16), simplified (answers without airtime on the channel, after the
//! time of the round trip along the shortest route): `GET_CONTACTS` records carry the position
//! a node advertises ([`Node::advert_position`]) and the time; [`Net::hear_advert`] pushes
//! `NEW_ADVERT`; `SEND_TRACE_PATH` walks the links, each repeater named by the next hash
//! appending its SNR; repeaters and room servers ([`Net::add_room`]) answer `SEND_LOGIN` with
//! their admin or guest password (a wrong one gets no answer), `SEND_STATUS_REQ`,
//! `GET_NEIGHBOURS` and their admin's commands (`ver`, `clock`, `get name`, `neighbors`,
//! `advert`); a room keeps its posts, sends them to whoever logs in, acknowledges a post from
//! a client allowed to write and sends it to the others.
//!
//! Debugging: `LICHEN_TRACE=1` prints the lichen frames each companion hands over and receives,
//! `LICHEN_TRACE_AIR=1` the fate of every reception (ok, lost, collision, half duplex).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

/// Bytes a Bluetooth notification from an ESP32 companion carries: ATT MTU 176 − 3. Not
/// `lc_meshcore::BLE_FRAME_MAX`: the fake radio models the firmware, not what lichen expects.
const BLE_NOTIFY_MAX: usize = 173;

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
    /// A room server (§15.8): it keeps posts and sends them to the clients logged in.
    Room,
}

/// A fake companion's settings (protocol §1.1): what `SELF_INFO`, `GET_CUSTOM_VARS`,
/// `GET_AUTOADD_CONFIG` report and the setting commands change.
#[derive(Clone, Debug, Default)]
pub struct FakeSettings {
    /// LoRa parameters set by `SET_RADIO_PARAMS` (kHz, Hz, SF, CR); the network's otherwise.
    pub radio: Option<(u32, u32, u8, u8)>,
    pub lat_e6: i32,
    pub lon_e6: i32,
    /// `SELF_INFO` bytes 44–47: multi_acks, advert_loc_policy, telemetry, manual_add.
    pub prefs: [u8; 4],
    /// `Some` for a board with a GPS: whether it is on.
    pub gps: Option<bool>,
    pub autoadd: (u8, u8),
    /// A command code the companion answers with ERR 6 (§1.1: a setting it refuses).
    pub refuse: Option<u8>,
    /// A command code it answers OK without applying it (§1.1: the read-back still shows
    /// the old value, B-user-5).
    pub ignore: Option<u8>,
    /// A command code it never answers.
    pub silent: Option<u8>,
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
    /// The radio's clock (Unix seconds) as SET_DEVICE_TIME left it: `None` until set.
    pub radio_time: Option<u32>,
    /// When (simulated ms) SET_DEVICE_TIME set it: the clock runs on from there.
    pub radio_time_at: Millis,
    pub rssi_zero: bool,
    /// Last reception (for `GET_STATS`).
    last_rx: (i8, i8),
    /// Channel slots: name and secret (§15.3).
    pub channels: Vec<Option<(String, [u8; 16])>>,
    /// The companion's own settings (protocol §1.1), as the firmware keeps them.
    pub settings: FakeSettings,
    /// Plain messages waiting for `SYNC_NEXT_MESSAGE` (companion frames).
    pub inbox: VecDeque<Vec<u8>>,
    /// Direct messages sent and not acknowledged yet: their ACK tag and when they left.
    acks: Vec<(u32, Millis)>,
    /// The position its adverts carry (degrees × 10⁶), when it advertises one (§16.1).
    pub advert_position: Option<(i32, i32)>,
    /// A repeater's or room server's passwords: admin, guest (§16.5).
    pub passwords: Option<(String, String)>,
    /// The companions logged in to this server, with their role (0 guest, 2 read and write,
    /// 3 admin).
    pub clients: Vec<(usize, u8)>,
    /// A room server's posts: author's key, Unix seconds, text.
    pub posts: Vec<(PubKey, u32, String)>,
    /// What the companion holds of contacts added from outside the simulation (§16.6).
    pub contact_adverts: Vec<ContactAdvert>,
    /// The size of its contact table: an advert past it is not added, `ADD_UPDATE_CONTACT`
    /// answers `ERR 3`.
    pub contacts_max: usize,
    /// The readings it answers `SEND_TELEMETRY_REQ` with (CayenneLPP, §16.7): its battery and
    /// temperature, to every client of a server, and from a companion whose owner allows it;
    /// empty, a companion does not answer (telemetry off, the firmware's default).
    pub telemetry: Vec<u8>,
    /// A server's sensors, added for its clients that are not guests.
    pub sensors: Vec<u8>,
    /// The board's name a companion reports in `DEVICE_INFO` (`board.getManufacturerName()`).
    pub board: String,
    /// Packets heard, and sent and heard by route (flood, direct), for `GET_STATS` type 2.
    rx_packets: u32,
    tx_flood: u32,
    rx_flood: u32,
}

/// A contact a fake companion added from outside the simulation (§16.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContactAdvert {
    pub key: PubKey,
    /// The advert's timestamp, by its node's clock.
    pub timestamp: u32,
    /// Degrees × 10⁶.
    pub position: Option<(i32, i32)>,
    /// The companion keeps the advert itself (imported: what `EXPORT_CONTACT` would return),
    /// not only the record.
    pub kept: bool,
}

/// Channel slots of a simulated companion.
const CHANNEL_SLOTS: usize = 8;

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
    /// A push computed ahead (mesh tools: a trace, a server's answer), by its id.
    Push(u64),
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
    /// Pushes due later (`Ev::Push`): id, companion, frame; a frame queued for
    /// `SYNC_NEXT_MESSAGE` when `queued`.
    later: Vec<Later>,
    /// Fixed readings `(snr4, rssi)` of links between unplaced nodes, both ways
    /// ([`Net::set_link_readings`]); the others hear at SNR 10 dB, RSSI −90 dBm.
    link_readings: Vec<((usize, usize), (i8, i8))>,
}

#[derive(Debug)]
struct Later {
    id: u64,
    to: usize,
    frame: Vec<u8>,
    queued: bool,
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

/// A node's base readings as MeshCore writes them (CayenneLPP, protocol §16.7): channel 1's
/// voltage (`millivolts`, sent in hundredths of a volt) and temperature (tenths of a °C).
pub fn base_readings(millivolts: u16, temperature_dc: i16) -> Vec<u8> {
    let mut v = vec![1, 116];
    v.extend_from_slice(&(millivolts / 10).to_be_bytes());
    v.extend_from_slice(&[1, 103]);
    v.extend_from_slice(&temperature_dc.to_be_bytes());
    v
}

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
/// What stock v1.18 reports in `DEVICE_INFO` (`examples/companion_radio/MyMesh.h` L10-L14).
const FIRMWARE_VERSION: &str = "v1.18.0";
const FIRMWARE_BUILD_DATE: &str = "15 Oct 2026";
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
            later: Vec::new(),
            link_readings: Vec::new(),
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
            radio_time: None,
            radio_time_at: 0,
            rssi_zero: false,
            last_rx: (0, 0),
            channels: vec![None; CHANNEL_SLOTS],
            settings: FakeSettings::default(),
            inbox: VecDeque::new(),
            acks: Vec::new(),
            advert_position: None,
            passwords: None,
            clients: Vec::new(),
            posts: Vec::new(),
            contact_adverts: Vec::new(),
            telemetry: Vec::new(),
            sensors: Vec::new(),
            board: String::new(),
            rx_packets: 0,
            tx_flood: 0,
            rx_flood: 0,
            contacts_max: 350,
        });
        self.neighbors.push(Vec::new());
        self.nodes.len() - 1
    }

    pub fn add_repeater(&mut self, seed: u8, name: &str) -> usize {
        self.add(seed, name, Role::Repeater)
    }

    /// A room server (§15.8) with its admin and guest passwords.
    pub fn add_room(&mut self, seed: u8, name: &str, admin: &str, guest: &str) -> usize {
        let i = self.add(seed, name, Role::Room);
        if let Some(n) = self.nodes.get_mut(i) {
            n.passwords = Some((admin.to_owned(), guest.to_owned()));
        }
        i
    }

    /// Repeater or room server `i` takes these passwords (§16.5).
    pub fn set_passwords(&mut self, i: usize, admin: &str, guest: &str) {
        if let Some(n) = self.nodes.get_mut(i) {
            n.passwords = Some((admin.to_owned(), guest.to_owned()));
        }
    }

    /// Someone posts `text` in room `room` at `ts` (Unix seconds), as a stock client logged
    /// in there would.
    pub fn room_post(&mut self, room: usize, author: usize, text: &str, ts: u32) {
        let key = self.key(author);
        if let Some(n) = self.nodes.get_mut(room) {
            n.posts.push((key, ts, text.to_owned()));
        }
        let clients: Vec<usize> = self
            .nodes
            .get(room)
            .map(|n| {
                n.clients
                    .iter()
                    .map(|(c, _)| *c)
                    .filter(|&c| c != author)
                    .collect()
            })
            .unwrap_or_default();
        for c in clients {
            self.push_post(room, c, key, ts, text, 2_000);
        }
    }

    /// A companion with an app attached (not connected yet).
    pub fn add_companion(&mut self, seed: u8, name: &str) -> usize {
        let i = self.add(seed, name, Role::Companion);
        self.attach_app(i, seed);
        i
    }

    /// A companion with no lichen app: someone with a stock MeshCore client (§15), driven
    /// with [`Net::send_plain_text`] and [`Net::post_on_channel`].
    pub fn add_stock_companion(&mut self, seed: u8, name: &str) -> usize {
        let i = self.add(seed, name, Role::Companion);
        if let Some(n) = self.nodes.get_mut(i) {
            n.connected = true;
        }
        i
    }

    /// Node `i` sends a plain direct message to node `to`, as a stock client would.
    pub fn send_plain_text(&mut self, i: usize, to: usize, text: &str, ts: u32) {
        let key = self.key(to);
        let mut frame = [0u8; 176];
        let cmd = lc_meshcore::Command::SendTxtMsg {
            kind: 0,
            attempt: 0,
            ts,
            prefix: key[..6].try_into().unwrap_or(&[0; 6]),
            text: text.as_bytes(),
        };
        if let Ok(n) = cmd.encode(&mut frame) {
            let _ = self.companion_command(i, &frame[..n]);
        }
    }

    /// Node `i` puts a channel in slot `index` (§15.3).
    pub fn set_channel(&mut self, i: usize, index: usize, name: &str, secret: [u8; 16]) {
        if let Some(slot) = self
            .nodes
            .get_mut(i)
            .and_then(|n| n.channels.get_mut(index))
        {
            *slot = Some((name.to_owned(), secret));
        }
    }

    /// Node `i` posts on the channel of slot `index`, as a stock client would.
    pub fn post_on_channel(&mut self, i: usize, index: u8, text: &str, ts: u32) {
        let mut frame = [0u8; 176];
        let cmd = lc_meshcore::Command::SendChannelTxtMsg {
            kind: 0,
            channel: index,
            ts,
            text: text.as_bytes(),
        };
        if let Ok(n) = cmd.encode(&mut frame) {
            let _ = self.companion_command(i, &frame[..n]);
        }
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
        // Simulated people join the groups that list them and pin who introduces themselves;
        // a test asks for consent itself.
        core.set_group_consent(false);
        core.set_request_consent(false);
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
        // Simulated people join the groups that list them and pin who introduces themselves;
        // a test asks for consent itself.
        core.set_group_consent(false);
        core.set_request_consent(false);
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

    /// `a` and `b` no longer hear each other.
    pub fn unlink(&mut self, a: usize, b: usize) {
        if let Some(v) = self.neighbors.get_mut(a) {
            v.retain(|&x| x != b);
        }
        if let Some(v) = self.neighbors.get_mut(b) {
            v.retain(|&x| x != a);
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
            return Some(self.unplaced_readings(t, r));
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

    /// What `r` hears of `t` when either is unplaced: the link's fixed readings
    /// ([`Net::set_link_readings`]), else SNR 10 dB and RSSI −90 dBm. Draws nothing at random:
    /// the air's timeline stays the same.
    fn unplaced_readings(&self, t: usize, r: usize) -> (i8, i8) {
        self.link_readings
            .iter()
            .find(|((a, b), _)| (*a, *b) == (t.min(r), t.max(r)))
            .map_or((40, -90), |(_, v)| *v)
    }

    /// Gives the link between `a` and `b` fixed readings, both ways, while either is unplaced:
    /// SNR × 4 and RSSI (dBm). For receptions, traces and neighbour lists; the demo's links
    /// then differ (ADR-0049) without a draw more on the air.
    pub fn set_link_readings(&mut self, a: usize, b: usize, snr4: i8, rssi: i8) {
        let k = (a.min(b), a.max(b));
        self.link_readings.retain(|(l, _)| *l != k);
        self.link_readings.push((k, (snr4, rssi)));
    }

    /// Node `from`'s advert as it puts it on the air (A:§5, protocol §16.6): signed with its
    /// key, stamped `ts`, with its type, the position it advertises and its name; a flood, or
    /// zero-hop (direct, empty path).
    pub fn advert(&self, from: usize, ts: u32, zero_hop: bool) -> Vec<u8> {
        use lc_meshcore::advert::flag;
        let Some(n) = self.nodes.get(from) else {
            return Vec::new();
        };
        let mut app_data = vec![self.kind(from) | flag::NAME];
        if let Some((lat, lon)) = n.advert_position {
            app_data[0] |= flag::POSITION;
            app_data.extend_from_slice(&lat.to_le_bytes());
            app_data.extend_from_slice(&lon.to_le_bytes());
        }
        let room = lc_meshcore::advert::APP_DATA_MAX.saturating_sub(app_data.len());
        app_data.extend(n.name.bytes().take(room));
        let ts = ts.to_le_bytes();
        let sig = lc_meshcore::ecdh::sign(&n.prv, &[&n.key, &ts, &app_data]).unwrap_or([0; 64]);
        let mut payload = n.key.to_vec();
        payload.extend_from_slice(&ts);
        payload.extend_from_slice(&sig);
        payload.extend_from_slice(&app_data);
        let mut out = [0u8; 255];
        let len = Packet {
            route: if zero_hop {
                Route::Direct
            } else {
                Route::Flood
            },
            ptype: PayloadType::Advert,
            transport: None,
            path: Path::EMPTY,
            payload: &payload,
        }
        .encode(&mut out)
        .unwrap_or(0);
        out.get(..len).map(<[u8]>::to_vec).unwrap_or_default()
    }

    /// Companion `i`'s radio logs `frame` with these readings (`LOG_RX_DATA`, protocol §1),
    /// out of band: nothing goes on the air, and the radio does nothing else with it.
    pub fn log_rx(&mut self, i: usize, frame: &[u8], snr4: i8, rssi: i8) {
        if self.nodes.get(i).is_some_and(|n| n.role == Role::Companion) {
            let mut log = vec![resp::LOG_RX_DATA, snr4 as u8, rssi as u8];
            log.extend_from_slice(frame);
            self.push_to_app(i, &log);
        }
    }

    /// Companion `i`'s radio logs node `from`'s flooded advert as it reaches it, out of band
    /// (the demo's adverts, ADR-0049): from `from` itself when they are linked, else from the
    /// last repeater of the shortest route, with the hashes of the repeaters it went through;
    /// with that link's readings, the SNR moved by `jitter4` (× 4) and the RSSI by a quarter of
    /// it. Stamped by `i`'s radio clock. Nothing goes on the air, nothing is drawn at random,
    /// and the radio's contacts stay as they are. Whether there was a way.
    pub fn log_advert(&mut self, i: usize, from: usize, jitter4: i8) -> bool {
        let Some(route) = self.route(i, from) else {
            return false;
        };
        let last = match route.bytes.first() {
            Some(h) => self.nodes.iter().position(|n| n.key[0] == *h),
            None => Some(from),
        };
        let Some(last) = last else {
            return false;
        };
        let (snr4, rssi) = match self.mean_snr(last, i) {
            Some(snr) => (
                (snr.min(SNR_MAX_DB) * 4.0).round().clamp(-128.0, 127.0) as i8,
                self.mean_rssi(last, i)
                    .map_or(-90, |r| r.round().clamp(-127.0, -1.0) as i8),
            ),
            None => self.unplaced_readings(last, i),
        };
        let snr4 = snr4.saturating_add(jitter4);
        let rssi = if rssi == 0 {
            0
        } else {
            rssi.saturating_add(jitter4 / 4)
        };
        let ts = self.radio_unix_s(i);
        let advert = self.advert(from, ts, false);
        // The flood's path lists the repeaters from `from` on, the opposite of `route`.
        let hops: Vec<u8> = route.bytes.iter().rev().copied().collect();
        let Ok(p) = Packet::decode(&advert) else {
            return false;
        };
        let Ok(path) = Path::from_hashes(&hops) else {
            return false;
        };
        let mut out = [0u8; 255];
        let Ok(len) = (Packet { path, ..p }).encode(&mut out) else {
            return false;
        };
        let frame = out.get(..len).map(<[u8]>::to_vec).unwrap_or_default();
        self.log_rx(i, &frame, snr4, rssi);
        true
    }

    /// Node `i`'s radio clock now, Unix seconds (see [`Net::advert`]'s stamp).
    pub fn radio_clock(&self, i: usize) -> u32 {
        self.radio_unix_s(i)
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
        let kind = self.kind(b);
        if let Some(n) = self.nodes.get_mut(a) {
            n.contacts.retain(|(k, ..)| *k != key);
            n.contacts.push((key, name.unwrap_or_default(), path, kind));
        }
    }

    /// The MeshCore type node `i` advertises.
    fn kind(&self, i: usize) -> u8 {
        match self.nodes.get(i).map(|n| n.role) {
            Some(Role::Repeater) => contact_kind::REPEATER,
            Some(Role::Room) => contact_kind::ROOM,
            _ => contact_kind::CHAT,
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
                Ev::Push(id) => {
                    if let Some(pos) = self.later.iter().position(|l| l.id == id) {
                        let l = self.later.remove(pos);
                        if l.queued {
                            self.queue_plain(l.to, l.frame);
                        } else {
                            self.push_to_app(l.to, &l.frame);
                        }
                    }
                }
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

    /// Pushes a frame to the app of companion `i` if it is connected. The app is on Bluetooth,
    /// as on a phone: an ESP32 companion's notification holds 173 bytes and a longer frame
    /// arrives cut short (ADR-0031).
    fn push_to_app(&mut self, i: usize, frame: &[u8]) {
        let now = self.now;
        let Some(n) = self.nodes.get_mut(i) else {
            return;
        };
        if !n.connected {
            return;
        }
        let frame = frame.get(..BLE_NOTIFY_MAX).unwrap_or(frame);
        if n.external {
            n.outbox.push(frame.to_vec());
            return;
        }
        if let Some(c) = n.core.as_mut() {
            c.on_companion_frame(frame, now);
        }
        self.wake(i, now);
    }

    /// Node `i`'s radio clock now, Unix seconds: from the time the app set (SET_DEVICE_TIME),
    /// else the simulation's.
    fn radio_unix_s(&self, i: usize) -> u32 {
        match self
            .nodes
            .get(i)
            .and_then(|n| n.radio_time.map(|t| (t, n.radio_time_at)))
        {
            Some((t, at)) => t.saturating_add(((self.now.saturating_sub(at)) / 1000) as u32),
            None => ((EPOCH_MS + self.now) / 1000) as u32,
        }
    }

    /// A `CONTACT` (or `NEW_ADVERT`) record of companion `on`: the node's advertised position
    /// when it has one, heard and advertised now by `on`'s clock.
    fn contact_frame(
        &self,
        on: usize,
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
        let unix = self.radio_unix_s(on);
        let (lat, lon) = self
            .nodes
            .iter()
            .find(|n| n.key == *key)
            .and_then(|n| n.advert_position)
            .or_else(|| {
                let n = self.nodes.get(on)?;
                n.contact_adverts
                    .iter()
                    .find(|c| c.key == *key)
                    .and_then(|c| c.position)
            })
            .unwrap_or((0, 0));
        v.extend_from_slice(&unix.to_le_bytes());
        v.extend_from_slice(&lat.to_le_bytes());
        v.extend_from_slice(&lon.to_le_bytes());
        v.extend_from_slice(&unix.to_le_bytes());
        v
    }

    /// `DEVICE_INFO` as stock v1.18 sends it (`MyMesh.cpp` L1056-L1074): firmware code 14, the
    /// contact table / 2, the channel slots, PIN 0, the build date, the board, the version.
    fn device_info(&self, i: usize) -> Vec<u8> {
        let Some(n) = self.nodes.get(i) else {
            return vec![resp::DEVICE_INFO, 14];
        };
        let padded = |text: &str, len: usize| {
            let mut b: Vec<u8> = text.bytes().take(len - 1).collect();
            b.resize(len, 0);
            b
        };
        let mut v = vec![
            resp::DEVICE_INFO,
            14,
            u8::try_from(n.contacts_max / 2).unwrap_or(u8::MAX),
            u8::try_from(n.channels.len()).unwrap_or(u8::MAX),
        ];
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend(padded(FIRMWARE_BUILD_DATE, 12));
        v.extend(padded(&n.board, 40));
        v.extend(padded(FIRMWARE_VERSION, 20));
        v.extend_from_slice(&[0, 0]);
        v
    }

    /// Pushes `frame` to the app of companion `to` in `delay` ms (queued for
    /// `SYNC_NEXT_MESSAGE` with a `MSG_WAITING` when `queued`).
    fn push_later(&mut self, to: usize, frame: Vec<u8>, delay: Millis, queued: bool) {
        self.seq += 1;
        let id = self.seq;
        self.later.push(Later {
            id,
            to,
            frame,
            queued,
        });
        self.schedule(self.now + delay, Ev::Push(id));
    }

    /// The neighbour of `from` that a hop hash names: a repeater whose key starts with it.
    fn hop(&self, from: usize, hash: &[u8]) -> Option<usize> {
        self.neighbors.get(from)?.iter().copied().find(|&j| {
            self.nodes
                .get(j)
                .is_some_and(|n| n.role == Role::Repeater && n.key.starts_with(hash))
        })
    }

    /// SNR × 4 at `r` of a transmission from `t` (its mean on the plane; unplaced, the link's
    /// fixed reading, else 10 dB).
    fn snr4(&self, t: usize, r: usize) -> i8 {
        self.mean_snr(t, r)
            .map_or(self.unplaced_readings(t, r).0, |snr| {
                (snr.min(SNR_MAX_DB) * 4.0).round().clamp(-128.0, 127.0) as i8
            })
    }

    /// `SEND_TRACE_PATH` (§16.3), simplified: the trace walks the simulated links without
    /// airtime on the channel; each repeater named by the next hash, a neighbour of the last
    /// sender, appends the SNR it heard it at, and the end of the path is heard back by the
    /// companion when it is in range. Lost otherwise (no push).
    fn trace(&mut self, i: usize, frame: &[u8]) -> Vec<Vec<u8>> {
        let (Some(tag), Some(&flags), Some(path)) =
            (frame.get(1..5), frame.get(9), frame.get(10..))
        else {
            return vec![vec![resp::ERR, 6]];
        };
        let size = 1usize << (flags & 3);
        if path.is_empty() || path.len() % size != 0 {
            return vec![vec![resp::ERR, 6]];
        }
        let mut at = i;
        let mut snrs = Vec::new();
        let mut delay = 0;
        let mut reached = true;
        for hash in path.chunks(size) {
            delay += self.airtime(11 + path.len() + snrs.len()) + 50;
            match self.hop(at, hash) {
                Some(next) => {
                    snrs.push(self.snr4(at, next) as u8);
                    at = next;
                }
                None => {
                    reached = false;
                    break;
                }
            }
        }
        delay += self.airtime(11 + path.len() + snrs.len());
        let back = self.neighbors.get(at).is_some_and(|v| v.contains(&i));
        let mut sent = vec![resp::SENT, 0];
        sent.extend_from_slice(tag);
        sent.extend_from_slice(
            &u32::try_from(delay * 2 + 1_000)
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        if reached && back {
            let mut push = vec![resp::TRACE_DATA, 0, path.len() as u8, flags];
            push.extend_from_slice(tag);
            push.extend_from_slice(frame.get(5..9).unwrap_or(&[0; 4]));
            push.extend_from_slice(path);
            push.extend_from_slice(&snrs);
            push.push(self.snr4(at, i) as u8);
            self.push_later(i, push, delay, false);
        }
        vec![sent]
    }

    // ----- servers (§15.8, §16.5), simplified: answers without airtime ----------------

    /// The server in `i`'s contacts whose key starts with `prefix`.
    fn server(&self, i: usize, prefix: &[u8]) -> Option<usize> {
        let node = self.nodes.get(i)?;
        if !node.contacts.iter().any(|(k, ..)| k.starts_with(prefix)) || prefix.is_empty() {
            return None;
        }
        self.nodes
            .iter()
            .position(|n| n.key.starts_with(prefix) && n.role != Role::Companion)
    }

    /// Time to a server and back along the shortest route, if there is one.
    fn round_trip(&self, i: usize, j: usize) -> Option<Millis> {
        let hops = Millis::from(self.route(i, j)?.hops());
        Some((hops + 1) * 2 * (self.airtime(60) + 150))
    }

    /// `SENT` for a request to a server, with the companion's estimate.
    fn sent(&self, tag: &[u8], estimate: Option<Millis>) -> Vec<u8> {
        let mut v = vec![resp::SENT, 0];
        v.extend_from_slice(tag.get(..4).unwrap_or(&[0; 4]));
        let t = estimate.map_or(8_000, |t| t * 2 + 2_000);
        v.extend_from_slice(&u32::try_from(t).unwrap_or(u32::MAX).to_le_bytes());
        v
    }

    /// The role companion `i` has on server `j`.
    fn role_on(&self, i: usize, j: usize) -> Option<u8> {
        let n = self.nodes.get(j)?;
        n.clients.iter().find(|(c, _)| *c == i).map(|(_, r)| *r)
    }

    /// Room `room` sends a post to client `c`: a signed text, queued by its companion.
    fn push_post(
        &mut self,
        room: usize,
        c: usize,
        author: PubKey,
        ts: u32,
        text: &str,
        delay: Millis,
    ) {
        let key = self.key(room);
        let mut f = vec![resp::CONTACT_MSG, 40, 0, 0];
        f.extend_from_slice(&key[..6]);
        f.extend_from_slice(&[0xFF, lc_meshcore::codes::txt::SIGNED_PLAIN]);
        f.extend_from_slice(&ts.to_le_bytes());
        f.extend_from_slice(&author[..4]);
        f.extend_from_slice(text.as_bytes());
        self.push_later(c, f, delay, true);
    }

    fn fake_login(&mut self, i: usize, frame: &[u8]) -> Vec<Vec<u8>> {
        let (Some(k), pw) = (frame.get(1..33), frame.get(33..).unwrap_or(&[])) else {
            return vec![vec![resp::ERR, 6]];
        };
        let Some(j) = self.server(i, k) else {
            return vec![vec![resp::ERR, 2]];
        };
        let rt = self.round_trip(i, j);
        let sent = self.sent(k, rt);
        let Some(n) = self.nodes.get(j) else {
            return vec![sent];
        };
        let room = n.role == Role::Room;
        let role = match &n.passwords {
            Some((admin, _)) if admin.as_bytes() == pw => 3,
            Some((_, guest)) if guest.as_bytes() == pw => {
                if room {
                    2
                } else {
                    0
                }
            }
            // A wrong password gets no answer.
            _ => return vec![sent],
        };
        let Some(rt) = rt else {
            return vec![sent];
        };
        let (key, posts) = (n.key, n.posts.clone());
        if let Some(n) = self.nodes.get_mut(j) {
            n.clients.retain(|(c, _)| *c != i);
            n.clients.push((i, role));
        }
        let mut push = vec![resp::LOGIN_SUCCESS, u8::from(role == 3)];
        push.extend_from_slice(&key[..6]);
        push.extend_from_slice(&(((EPOCH_MS + self.now) / 1000) as u32).to_le_bytes());
        push.extend_from_slice(&[role, 2]);
        self.push_later(i, push, rt, false);
        // A room then sends the posts this client has not seen (here: all but its own).
        let me = self.key(i);
        for (n, (author, ts, text)) in posts.iter().filter(|(a, ..)| *a != me).enumerate() {
            self.push_post(j, i, *author, *ts, text, rt + 1_000 * (n as Millis + 1));
        }
        vec![sent]
    }

    /// A server's statistics, as `RepeaterStats` (56 bytes) or a room's `ServerStats` (52).
    fn fake_stats(&self, j: usize) -> Vec<u8> {
        let Some(n) = self.nodes.get(j) else {
            return Vec::new();
        };
        let mut v = Vec::new();
        v.extend_from_slice(&4_100u16.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&n.noise_dbm.to_le_bytes());
        v.extend_from_slice(&i16::from(n.last_rx.1).to_le_bytes());
        let sent = u32::try_from(n.tx_frames).unwrap_or(u32::MAX);
        let air = u32::try_from(n.tx_airtime_ms / 1000).unwrap_or(u32::MAX);
        let up = u32::try_from(self.now / 1000 + 3_600).unwrap_or(u32::MAX);
        for x in [
            sent * 3,
            sent,
            air,
            up,
            sent / 2,
            sent - sent / 2,
            sent * 2,
            sent,
        ] {
            v.extend_from_slice(&x.to_le_bytes());
        }
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&i16::from(n.last_rx.0).to_le_bytes());
        v.extend_from_slice(&[0, 0, 0, 0]);
        if n.role == Role::Room {
            let posted = u16::try_from(n.posts.len()).unwrap_or(u16::MAX);
            v.extend_from_slice(&posted.to_le_bytes());
            v.extend_from_slice(&posted.to_le_bytes());
        } else {
            v.extend_from_slice(&(air * 2).to_le_bytes());
            v.extend_from_slice(&0u32.to_le_bytes());
        }
        v
    }

    fn fake_status(&mut self, i: usize, frame: &[u8]) -> Vec<Vec<u8>> {
        let Some(k) = frame.get(1..33) else {
            return vec![vec![resp::ERR, 6]];
        };
        let Some(j) = self.server(i, k) else {
            return vec![vec![resp::ERR, 2]];
        };
        let tag = self.rng.next_u32().to_le_bytes();
        let rt = self.round_trip(i, j);
        let sent = self.sent(&tag, rt);
        if let (Some(rt), Some(_)) = (rt, self.role_on(i, j)) {
            let mut push = vec![resp::STATUS_RESPONSE, 0];
            push.extend_from_slice(&self.key(j)[..6]);
            push.extend_from_slice(&self.fake_stats(j));
            self.push_later(i, push, rt, false);
        }
        vec![sent]
    }

    /// `SEND_TELEMETRY_REQ` (§16.7): `ERR 2` for a node not in `i`'s contacts; else `SENT`, and
    /// after the round trip its readings, from a server to its clients (a guest without the
    /// sensors), from a companion that answers at all.
    fn fake_telemetry(&mut self, i: usize, frame: &[u8]) -> Vec<Vec<u8>> {
        let Some(k) = frame.get(4..36) else {
            return vec![vec![resp::ERR, 6]];
        };
        let known = self
            .nodes
            .get(i)
            .is_some_and(|n| n.contacts.iter().any(|(c, ..)| c[..] == *k));
        let Some(j) = self
            .nodes
            .iter()
            .position(|n| n.key[..] == *k)
            .filter(|_| known)
        else {
            return vec![vec![resp::ERR, 2]];
        };
        let tag = self.rng.next_u32().to_le_bytes();
        let rt = self.round_trip(i, j);
        let sent = self.sent(&tag, rt);
        let Some(target) = self.nodes.get(j) else {
            return vec![sent];
        };
        let data = if target.role == Role::Companion {
            target.telemetry.clone()
        } else {
            match self.role_on(i, j) {
                Some(0) => target.telemetry.clone(),
                Some(_) => [target.telemetry.as_slice(), target.sensors.as_slice()].concat(),
                None => Vec::new(),
            }
        };
        if let (Some(rt), false) = (rt, data.is_empty()) {
            let mut push = vec![resp::TELEMETRY_RESPONSE, 0];
            push.extend_from_slice(&self.key(j)[..6]);
            push.extend_from_slice(&data);
            self.push_later(i, push, rt, false);
        }
        vec![sent]
    }

    fn fake_binary(&mut self, i: usize, frame: &[u8]) -> Vec<Vec<u8>> {
        let (Some(k), Some(data)) = (frame.get(1..33), frame.get(33..)) else {
            return vec![vec![resp::ERR, 6]];
        };
        let Some(j) = self.server(i, k) else {
            return vec![vec![resp::ERR, 2]];
        };
        let tag = self.rng.next_u32().to_le_bytes();
        let rt = self.round_trip(i, j);
        let sent = self.sent(&tag, rt);
        let (Some(rt), Some(_)) = (rt, self.role_on(i, j)) else {
            return vec![sent];
        };
        // GET_NEIGHBOURS, version 0: count, offset, order, prefix length.
        let (Some(&6), Some(&0), Some(&count), Some(off), Some(&plen)) = (
            data.first(),
            data.get(1),
            data.get(2),
            data.get(3..5),
            data.get(6),
        ) else {
            return vec![sent];
        };
        let offset = usize::from(u16::from_le_bytes([off[0], off[1]]));
        let plen = usize::from(plen).min(32);
        let mut all: Vec<(PubKey, i8)> = self
            .neighbors
            .get(j)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|x| (self.key(x), self.snr4(x, j)))
            .collect();
        all.sort_by_key(|(_, snr)| std::cmp::Reverse(*snr));
        let mut body = Vec::new();
        let mut n = 0u16;
        for (key, snr) in all.iter().skip(offset).take(usize::from(count)) {
            if body.len() + plen + 5 > 130 {
                break;
            }
            body.extend_from_slice(&key[..plen]);
            body.extend_from_slice(&60u32.to_le_bytes());
            body.push(*snr as u8);
            n += 1;
        }
        let mut push = vec![resp::BINARY_RESPONSE, 0];
        push.extend_from_slice(&tag);
        push.extend_from_slice(&(all.len() as u16).to_le_bytes());
        push.extend_from_slice(&n.to_le_bytes());
        push.extend_from_slice(&body);
        self.push_later(i, push, rt, false);
        vec![sent]
    }

    /// A command to a server's command line: admins get a reply, queued like a message.
    fn fake_cli(&mut self, i: usize, prefix: &[u8], text: &[u8]) -> Vec<Vec<u8>> {
        let Some(j) = self.server(i, prefix) else {
            return vec![vec![resp::ERR, 2]];
        };
        let rt = self.round_trip(i, j);
        let sent = self.sent(&[0; 4], rt);
        let (Some(rt), Some(3)) = (rt, self.role_on(i, j)) else {
            return vec![sent];
        };
        let name = self
            .nodes
            .get(j)
            .map(|n| n.name.clone())
            .unwrap_or_default();
        let neighbours: Vec<String> = self
            .neighbors
            .get(j)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|x| format!("{:02X}:{}", self.key(x)[0], self.snr4(x, j)))
            .collect();
        let reply = match String::from_utf8_lossy(text).trim() {
            "ver" => "v1.18.0 (Build: 15 Oct 2026)".to_owned(),
            "get name" => format!("> {name}"),
            "clock" => format!("> {}", (EPOCH_MS + self.now) / 1000),
            "neighbors" => neighbours.join("\n"),
            "advert" => "OK - Advert sent".to_owned(),
            _ => "Unknown command".to_owned(),
        };
        let mut f = vec![resp::CONTACT_MSG, 40, 0, 0];
        f.extend_from_slice(&self.key(j)[..6]);
        f.extend_from_slice(&[0xFF, lc_meshcore::codes::txt::CLI_DATA]);
        f.extend_from_slice(&(((EPOCH_MS + self.now) / 1000) as u32).to_le_bytes());
        f.extend_from_slice(reply.as_bytes());
        self.push_later(i, f, rt + 500, true);
        vec![sent]
    }

    /// The room server a `SEND_TXT_MSG` goes to, if it goes to one.
    fn room_target(&self, i: usize, frame: &[u8]) -> Option<usize> {
        let j = self.server(i, frame.get(7..13)?)?;
        (self.nodes.get(j)?.role == Role::Room).then_some(j)
    }

    /// A post to a room server: kept, acknowledged, and sent to its other clients.
    fn fake_room_post(&mut self, i: usize, j: usize, ts: &[u8], text: &[u8]) -> Vec<Vec<u8>> {
        let tag = self.rng.next_u32().to_le_bytes();
        let rt = self.round_trip(i, j);
        let sent = self.sent(&tag, rt);
        let (Some(rt), Some(2 | 3)) = (rt, self.role_on(i, j)) else {
            return vec![sent];
        };
        let text = String::from_utf8_lossy(text).into_owned();
        let ts = ts
            .get(..4)
            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let author = self.key(i);
        if let Some(n) = self.nodes.get_mut(j) {
            n.posts.push((author, ts, text.clone()));
        }
        let mut confirmed = vec![resp::SEND_CONFIRMED];
        confirmed.extend_from_slice(&tag);
        confirmed.extend_from_slice(&u32::try_from(rt).unwrap_or(0).to_le_bytes());
        self.push_later(i, confirmed, rt, false);
        let others: Vec<usize> = self
            .nodes
            .get(j)
            .map(|n| {
                n.clients
                    .iter()
                    .map(|(c, _)| *c)
                    .filter(|&c| c != i)
                    .collect()
            })
            .unwrap_or_default();
        for c in others {
            self.push_post(j, c, author, ts, &text, rt + 2_000);
        }
        vec![sent]
    }

    /// The companion of `i` hears an advert from `from` and does not add it (manual add): it
    /// pushes `NEW_ADVERT` to its app (§16.1).
    pub fn hear_advert(&mut self, i: usize, from: usize) {
        let Some(n) = self.nodes.get(from) else {
            return;
        };
        let (key, name) = (n.key, n.name.clone());
        let kind = self.kind(from);
        let path = self.route(i, from);
        let frame = self.contact_frame(i, resp::NEW_ADVERT, &key, &name, path.as_ref(), kind);
        self.push_to_app(i, &frame);
    }

    /// The frame node `i` last queued for the air, as it will go out (the one a stock client's
    /// command just sent: [`Net::send_plain_text`], [`Net::post_on_channel`]).
    pub fn last_queued(&self, i: usize) -> Option<Vec<u8>> {
        self.nodes.get(i)?.txq.back().map(|(_, f)| f.clone())
    }

    /// Companion `i` hears `frame`, as its sender put it on the air, out of band: unless it heard
    /// it already (its duplicate filter, which then drops a copy that comes later by the air),
    /// it gets it as at its last hop (a direct packet with its path spent), with an unplaced
    /// node's readings, and handles it as any frame it hears. Nothing goes on the air but what
    /// it answers (a direct message's ACK), and nothing at all when it had heard it. Whether it
    /// was handed over. (The demo's `plain_heard`: what someone with a stock client sends
    /// reaches the app's radio however busy the air was, UI tests.)
    pub fn hand_over(&mut self, i: usize, frame: &[u8]) -> bool {
        let Ok(p) = Packet::decode(frame) else {
            return false;
        };
        if self
            .nodes
            .get(i)
            .is_none_or(|n| n.role != Role::Companion || n.dedup.contains(&p.dedup_hash()))
        {
            return false;
        }
        let arrived = if matches!(p.route, Route::Direct | Route::TransportDirect) {
            let mut out = [0u8; 255];
            let Ok(n) = (Packet {
                path: Path::EMPTY,
                ..p
            })
            .encode(&mut out) else {
                return false;
            };
            out.get(..n).map(<[u8]>::to_vec).unwrap_or_default()
        } else {
            frame.to_vec()
        };
        self.receive(i, &arrived, 40, -90);
        true
    }

    /// `IMPORT_CONTACT` (`MyMesh.cpp` L1409, `BaseChatMesh.cpp` L588): `OK` for an ADVERT
    /// packet, then the advert handled as if heard by flood (§16.6), silently: a forged one, the
    /// radio's own or one without a name is ignored, so is one no later than the contact's;
    /// with manual add and its type outside the auto-add mask, a path of as many hops as the
    /// auto-add limit or more, or a full table, it is pushed as `NEW_ADVERT` and not added; else
    /// it is added or updated (no path of its own) and pushed as `ADVERT`.
    fn import_contact(&mut self, i: usize, packet: &[u8]) -> Vec<Vec<u8>> {
        let read = lc_meshcore::Packet::decode(packet)
            .ok()
            .filter(|p| p.ptype == PayloadType::Advert && packet.len() > 2 + 32 + 64 - 1);
        let Some(p) = read else {
            return vec![vec![resp::ERR, 6]];
        };
        let mut out = vec![vec![resp::OK]];
        let Ok(a) = lc_meshcore::advert::Advert::decode(p.payload) else {
            return out;
        };
        let Some(node) = self.nodes.get(i) else {
            return out;
        };
        let name = String::from_utf8_lossy(a.name).into_owned();
        if !a.verify() || *a.key == node.key || name.is_empty() {
            return out;
        }
        let known = node.contacts.iter().any(|(k, ..)| k == a.key);
        let last = node
            .contact_adverts
            .iter()
            .find(|c| c.key == *a.key)
            .map_or(0, |c| c.timestamp);
        if known && a.timestamp <= last {
            return out;
        }
        let bit = match a.kind {
            contact_kind::CHAT => 0x02,
            contact_kind::REPEATER => 0x04,
            contact_kind::ROOM => 0x08,
            contact_kind::SENSOR => 0x10,
            _ => 0,
        };
        let manual = node.settings.prefs[3] & 1 != 0;
        let full = !known && node.contacts.len() >= node.contacts_max;
        let max_hops = node.settings.autoadd.1;
        let far = max_hops > 0 && p.path.hops() >= max_hops;
        let position = a.position.filter(|&p| p != (0, 0));
        if !known && ((manual && node.settings.autoadd.0 & bit == 0) || far || full) {
            let mut v = vec![resp::NEW_ADVERT];
            v.extend_from_slice(a.key);
            v.extend_from_slice(&[a.kind, 0, PATH_UNKNOWN]);
            v.extend_from_slice(&[0; 64]);
            let mut n = [0u8; 32];
            n[..a.name.len().min(31)].copy_from_slice(&a.name[..a.name.len().min(31)]);
            v.extend_from_slice(&n);
            let (lat, lon) = position.unwrap_or((0, 0));
            v.extend_from_slice(&a.timestamp.to_le_bytes());
            v.extend_from_slice(&lat.to_le_bytes());
            v.extend_from_slice(&lon.to_le_bytes());
            v.extend_from_slice(&self.radio_unix_s(i).to_le_bytes());
            out.push(v);
            if full {
                out.push(vec![0x90]);
            }
            return out;
        }
        self.keep_contact(i, *a.key, name, a.kind, a.timestamp, position, true);
        let mut v = vec![resp::ADVERT];
        v.extend_from_slice(a.key);
        out.push(v);
        out
    }

    /// `ADD_UPDATE_CONTACT` (`MyMesh.cpp` L1328, `updateContactFromFrame` L194): the record
    /// added or updated, `ERR 3` when the table is full.
    fn add_update_contact(&mut self, i: usize, frame: &[u8]) -> Vec<Vec<u8>> {
        let (Some(key), Some(&kind), Some(name), Some(ts)) = (
            frame.get(1..33).and_then(|k| PubKey::try_from(k).ok()),
            frame.get(33),
            frame.get(100..132),
            frame
                .get(132..136)
                .and_then(|b| <[u8; 4]>::try_from(b).ok()),
        ) else {
            return vec![vec![resp::ERR, 6]];
        };
        let position = frame
            .get(136..144)
            .map(|b| {
                let (lat, lon) = b.split_at(4);
                let int = |x: &[u8]| i32::from_le_bytes(<[u8; 4]>::try_from(x).unwrap_or_default());
                (int(lat), int(lon))
            })
            .filter(|&p| p != (0, 0));
        let name =
            String::from_utf8_lossy(name.split(|&b| b == 0).next().unwrap_or(&[])).into_owned();
        let Some(node) = self.nodes.get(i) else {
            return vec![];
        };
        if node.contacts.iter().all(|(k, ..)| *k != key) && node.contacts.len() >= node.contacts_max
        {
            return vec![vec![resp::ERR, 3]];
        }
        self.keep_contact(i, key, name, kind, u32::from_le_bytes(ts), position, false);
        vec![vec![resp::OK]]
    }

    /// Companion `i` holds `key` as a contact (no path), with what its advert said; the advert
    /// itself once `imported`.
    #[allow(clippy::too_many_arguments)]
    fn keep_contact(
        &mut self,
        i: usize,
        key: PubKey,
        name: String,
        kind: u8,
        ts: u32,
        position: Option<(i32, i32)>,
        imported: bool,
    ) {
        let Some(n) = self.nodes.get_mut(i) else {
            return;
        };
        match n.contacts.iter_mut().find(|(k, ..)| *k == key) {
            Some(c) => {
                c.1 = name;
                c.3 = kind;
            }
            None => n.contacts.push((key, name, None, kind)),
        }
        let kept = imported || n.contact_adverts.iter().any(|c| c.key == key && c.kept);
        n.contact_adverts.retain(|c| c.key != key);
        n.contact_adverts.push(ContactAdvert {
            key,
            timestamp: ts,
            position,
            kept,
        });
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
        let s = &node.settings;
        if s.refuse == Some(code) {
            return vec![vec![resp::ERR, 6]];
        }
        if s.ignore == Some(code) {
            return vec![vec![resp::OK]];
        }
        if s.silent == Some(code) {
            return vec![];
        }
        match code {
            cmd::APP_START => {
                let s = self
                    .nodes
                    .get(i)
                    .map(|n| n.settings.clone())
                    .unwrap_or_default();
                let (freq, bw, sf, cr) =
                    s.radio
                        .unwrap_or((FREQ_KHZ, self.radio.bw_hz, self.radio.sf, self.radio.cr));
                let mut v = vec![resp::SELF_INFO, 1, tx_dbm as u8, 22];
                v.extend_from_slice(&key);
                v.extend_from_slice(&s.lat_e6.to_le_bytes());
                v.extend_from_slice(&s.lon_e6.to_le_bytes());
                v.extend_from_slice(&s.prefs);
                v.extend_from_slice(&freq.to_le_bytes());
                v.extend_from_slice(&bw.to_le_bytes());
                v.extend_from_slice(&[sf, cr]);
                v.extend_from_slice(name.as_bytes());
                vec![v]
            }
            // Settings (protocol §1.1), with the firmware's checks. The simulation keeps its
            // own radio: new LoRa parameters only show in SELF_INFO.
            cmd::SET_RADIO_PARAMS => {
                let u = |at: usize| {
                    frame
                        .get(at..at + 4)
                        .and_then(|b| b.try_into().ok())
                        .map(u32::from_le_bytes)
                };
                match (u(1), u(5), frame.get(9), frame.get(10)) {
                    (
                        Some(f @ 150_000..=2_500_000),
                        Some(b @ 7_000..=500_000),
                        Some(&s @ 5..=12),
                        Some(&c @ 5..=8),
                    ) => {
                        if let Some(n) = self.nodes.get_mut(i) {
                            n.settings.radio = Some((f, b, s, c));
                        }
                        vec![vec![resp::OK]]
                    }
                    _ => vec![vec![resp::ERR, 6]],
                }
            }
            cmd::SET_ADVERT_LATLON => {
                let i32_at = |at: usize| {
                    frame
                        .get(at..at + 4)
                        .and_then(|b| b.try_into().ok())
                        .map(i32::from_le_bytes)
                };
                match (i32_at(1), i32_at(5)) {
                    (
                        Some(lat @ -90_000_000..=90_000_000),
                        Some(lon @ -180_000_000..=180_000_000),
                    ) => {
                        if let Some(n) = self.nodes.get_mut(i) {
                            (n.settings.lat_e6, n.settings.lon_e6) = (lat, lon);
                        }
                        vec![vec![resp::OK]]
                    }
                    _ => vec![vec![resp::ERR, 6]],
                }
            }
            cmd::SET_OTHER_PARAMS => {
                if let Some(n) = self.nodes.get_mut(i) {
                    // manual_add, telemetry, advert_loc_policy, multi_acks; SELF_INFO's order is
                    // multi_acks, advert_loc_policy, telemetry, manual_add.
                    let p = &mut n.settings.prefs;
                    if let Some(&m) = frame.get(1) {
                        p[3] = m;
                    }
                    if let Some(&t) = frame.get(2) {
                        p[2] = t & 0x3F;
                    }
                    if let Some(&l) = frame.get(3) {
                        p[1] = l;
                    }
                    if let Some(&a) = frame.get(4) {
                        p[0] = a;
                    }
                }
                vec![vec![resp::OK]]
            }
            cmd::GET_CUSTOM_VARS => {
                let mut v = vec![resp::CUSTOM_VARS];
                if let Some(Some(on)) = self.nodes.get(i).map(|n| n.settings.gps) {
                    v.extend_from_slice(if on { b"gps:1" } else { b"gps:0" });
                }
                vec![v]
            }
            cmd::SET_CUSTOM_VAR => {
                let text = frame.get(1..).unwrap_or_default();
                let node = self.nodes.get_mut(i);
                match (node, text) {
                    (Some(n), b"gps:1" | b"gps:0") if n.settings.gps.is_some() => {
                        n.settings.gps = Some(text == b"gps:1");
                        vec![vec![resp::OK]]
                    }
                    (Some(n), t) if n.settings.gps.is_some() && t.starts_with(b"gps_interval:") => {
                        vec![vec![resp::OK]]
                    }
                    _ => vec![vec![resp::ERR, 6]],
                }
            }
            cmd::GET_AUTOADD_CONFIG => {
                let (mask, hops) = self.nodes.get(i).map_or((0, 0), |n| n.settings.autoadd);
                vec![vec![resp::AUTOADD_CONFIG, mask, hops]]
            }
            cmd::SET_AUTOADD_CONFIG => {
                if let Some(n) = self.nodes.get_mut(i) {
                    n.settings.autoadd = (
                        frame.get(1).copied().unwrap_or(0),
                        frame.get(2).copied().unwrap_or(0).min(64),
                    );
                }
                vec![vec![resp::OK]]
            }
            cmd::GET_BATT_AND_STORAGE => {
                let mut v = vec![resp::BATT_AND_STORAGE];
                v.extend_from_slice(&4_012u16.to_le_bytes());
                v.extend_from_slice(&96u32.to_le_bytes());
                v.extend_from_slice(&1_024u32.to_le_bytes());
                vec![v]
            }
            cmd::SET_DEVICE_PIN => match frame
                .get(1..5)
                .and_then(|b| b.try_into().ok())
                .map(u32::from_le_bytes)
            {
                Some(0 | 100_000..=999_999) => vec![vec![resp::OK]],
                _ => vec![vec![resp::ERR, 6]],
            },
            cmd::DEVICE_QUERY => vec![self.device_info(i)],
            cmd::SEND_TRACE_PATH => self.trace(i, frame),
            cmd::SEND_LOGIN => self.fake_login(i, frame),
            cmd::SEND_STATUS_REQ => self.fake_status(i, frame),
            cmd::SEND_TELEMETRY_REQ => self.fake_telemetry(i, frame),
            cmd::SEND_BINARY_REQ => self.fake_binary(i, frame),
            cmd::LOGOUT => {
                let k = frame.get(1..33).unwrap_or(&[]);
                if let Some(j) = self.server(i, k)
                    && let Some(n) = self.nodes.get_mut(j)
                {
                    n.clients.retain(|(c, _)| *c != i);
                }
                vec![vec![resp::OK]]
            }
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
                    out.push(self.contact_frame(i, resp::CONTACT, k, n, p.as_ref(), *t));
                }
                out.push(vec![resp::END_OF_CONTACTS, 0, 0, 0, 0]);
                out
            }
            cmd::IMPORT_CONTACT => self.import_contact(i, frame.get(1..).unwrap_or(&[])),
            cmd::ADD_UPDATE_CONTACT => self.add_update_contact(i, frame),
            cmd::GET_CONTACT_BY_KEY => {
                let k = frame.get(1..33);
                match node.contacts.iter().find(|(c, ..)| Some(&c[..]) == k) {
                    Some((c, n, p, t)) => {
                        vec![self.contact_frame(i, resp::CONTACT, c, n, p.as_ref(), *t)]
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
                let tx_s = u32::try_from(node.tx_airtime_ms / 1000).unwrap_or(u32::MAX);
                v.extend_from_slice(&tx_s.to_le_bytes());
                v.extend_from_slice(&0u32.to_le_bytes());
                vec![v]
            }
            // Core (`MyMesh.cpp` L1916): battery, uptime, error flags, queue.
            cmd::GET_STATS if frame.get(1) == Some(&0) => {
                let mut v = vec![resp::STATS, 0];
                v.extend_from_slice(&4_012u16.to_le_bytes());
                let up = u32::try_from(self.now / 1000).unwrap_or(u32::MAX);
                v.extend_from_slice(&up.to_le_bytes());
                v.extend_from_slice(&0u16.to_le_bytes());
                v.push(u8::try_from(node.txq.len()).unwrap_or(u8::MAX));
                vec![v]
            }
            // Packets (`MyMesh.cpp` L1943): received, sent, by route, errors.
            cmd::GET_STATS if frame.get(1) == Some(&2) => {
                let sent = u32::try_from(node.tx_frames).unwrap_or(u32::MAX);
                let mut v = vec![resp::STATS, 2];
                for x in [
                    node.rx_packets,
                    sent,
                    node.tx_flood,
                    sent.saturating_sub(node.tx_flood),
                    node.rx_flood,
                    node.rx_packets.saturating_sub(node.rx_flood),
                    0,
                ] {
                    v.extend_from_slice(&x.to_le_bytes());
                }
                vec![v]
            }
            cmd::GET_STATS => vec![vec![resp::ERR, 6]],
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
            cmd::SYNC_NEXT_MESSAGE => {
                let next = self.nodes.get_mut(i).and_then(|n| n.inbox.pop_front());
                vec![next.unwrap_or_else(|| vec![resp::NO_MORE_MESSAGES])]
            }
            cmd::GET_CHANNEL => {
                let index = usize::from(frame.get(1).copied().unwrap_or(0));
                match node.channels.get(index) {
                    Some(slot) => {
                        let mut v = vec![resp::CHANNEL_INFO, index as u8];
                        let mut name = [0u8; 32];
                        let mut secret = [0u8; 16];
                        if let Some((n, s)) = slot {
                            let b = n.as_bytes();
                            name[..b.len().min(31)].copy_from_slice(&b[..b.len().min(31)]);
                            secret = *s;
                        }
                        v.extend_from_slice(&name);
                        v.extend_from_slice(&secret);
                        vec![v]
                    }
                    None => vec![vec![resp::ERR, 2]],
                }
            }
            cmd::SET_CHANNEL => {
                let (Some(&index), Some(name), Some(secret)) =
                    (frame.get(1), frame.get(2..34), frame.get(34..50))
                else {
                    return vec![vec![resp::ERR, 6]];
                };
                let name = name.iter().take_while(|&&b| b != 0).copied().collect();
                let name = String::from_utf8(name).unwrap_or_default();
                let secret: [u8; 16] = secret.try_into().unwrap_or([0; 16]);
                let value = (!name.is_empty() || secret != [0; 16]).then_some((name, secret));
                match self
                    .nodes
                    .get_mut(i)
                    .and_then(|n| n.channels.get_mut(usize::from(index)))
                {
                    Some(slot) => {
                        *slot = value;
                        vec![vec![resp::OK]]
                    }
                    None => vec![vec![resp::ERR, 2]],
                }
            }
            cmd::SET_ADVERT_NAME => {
                let name = String::from_utf8_lossy(frame.get(1..).unwrap_or(&[])).into_owned();
                if let Some(n) = self.nodes.get_mut(i) {
                    n.name = name;
                }
                vec![vec![resp::OK]]
            }
            cmd::RESET_PATH => {
                let k = frame.get(1..33);
                if let Some(n) = self.nodes.get_mut(i)
                    && let Some(c) = n.contacts.iter_mut().find(|(c, ..)| Some(&c[..]) == k)
                {
                    c.2 = None;
                }
                vec![vec![resp::OK]]
            }
            // Servers (§15.8, §16.5): the console, and a room's posts.
            cmd::SEND_TXT_MSG if frame.get(1) == Some(&lc_meshcore::codes::txt::CLI_DATA) => {
                let prefix = frame.get(7..13).unwrap_or(&[]).to_vec();
                let text = frame.get(13..).unwrap_or(&[]).to_vec();
                self.fake_cli(i, &prefix, &text)
            }
            cmd::SEND_TXT_MSG if self.room_target(i, frame).is_some() => {
                let j = self.room_target(i, frame).unwrap_or(usize::MAX);
                let ts = frame.get(3..7).unwrap_or(&[]).to_vec();
                let text = frame.get(13..).unwrap_or(&[]).to_vec();
                self.fake_room_post(i, j, &ts, &text)
            }
            cmd::SEND_TXT_MSG => {
                let (Some(&attempt), Some(ts), Some(prefix), Some(text)) = (
                    frame.get(2),
                    frame.get(3..7),
                    frame.get(7..13),
                    frame.get(13..),
                ) else {
                    return vec![vec![resp::ERR, 6]];
                };

                let Some((to, _, path, _)) =
                    node.contacts.iter().find(|(k, ..)| k.starts_with(prefix))
                else {
                    return vec![vec![resp::ERR, 2]];
                };
                let tag = plain_tag(ts, attempt, text, &key);
                // Dest hash, source hash, MAC, then the block: ts, flags (attempt), text.
                let mut payload = vec![to[0], key[0], 0, 0];
                payload.extend_from_slice(ts);
                payload.push(attempt & 3);
                payload.extend_from_slice(text);
                payload.resize(4 + (5 + text.len()).div_ceil(16) * 16, 0);
                let flood = path.is_none();
                let air = plain_packet(PayloadType::TxtMsg, path.as_ref(), &payload);
                if let Some(n) = self.nodes.get_mut(i) {
                    n.acks.push((tag, self.now));
                }
                self.enqueue_tx(i, air, 0.2);
                let mut v = vec![resp::SENT, u8::from(flood)];
                v.extend_from_slice(&tag.to_le_bytes());
                let timeout: u32 = if flood { 16_000 } else { 8_000 };
                v.extend_from_slice(&timeout.to_le_bytes());
                vec![v]
            }
            cmd::SEND_CHANNEL_TXT_MSG => {
                let (Some(&index), Some(ts), Some(text)) =
                    (frame.get(2), frame.get(3..7), frame.get(7..))
                else {
                    return vec![vec![resp::ERR, 6]];
                };
                let Some(Some((_, secret))) = node.channels.get(usize::from(index)) else {
                    return vec![vec![resp::ERR, 2]];
                };
                let id = crate::plain::channel_id(secret);
                // Channel hash, MAC (here: more of the channel id), then ts, flags, "name: text".
                let mut payload = vec![id[0], id[1], id[2]];
                payload.extend_from_slice(ts);
                payload.push(0);
                payload.extend_from_slice(name.as_bytes());
                payload.extend_from_slice(b": ");
                payload.extend_from_slice(text);
                payload.resize(3 + (5 + name.len() + 2 + text.len()).div_ceil(16) * 16, 0);
                let air = plain_packet(PayloadType::GrpTxt, None, &payload);
                self.enqueue_tx(i, air, 0.0);
                vec![vec![resp::OK]]
            }
            // The clock never goes back: an earlier time is refused (MyMesh.cpp, ERR 6).
            cmd::SET_DEVICE_TIME => {
                let Some(t) = frame
                    .get(1..5)
                    .and_then(|b| b.try_into().ok())
                    .map(u32::from_le_bytes)
                else {
                    return vec![vec![resp::ERR, 2]];
                };
                // Against the clock as it runs now, not as it was set.
                let running = self
                    .nodes
                    .get(i)
                    .and_then(|n| n.radio_time)
                    .map(|_| self.radio_unix_s(i));
                let now = self.now;
                match self.nodes.get_mut(i) {
                    Some(n) if running.is_none_or(|was| t >= was) => {
                        n.radio_time = Some(t);
                        n.radio_time_at = now;
                        vec![vec![resp::OK]]
                    }
                    _ => vec![vec![resp::ERR, 6]],
                }
            }
            _ => vec![vec![resp::ERR, 1]],
        }
    }

    /// A plain message reached companion `r` (§15): queued, and an ACK for a direct one.
    fn plain_received(&mut self, r: usize, p: &Packet<'_>, snr4: i8) {
        let path_len = if matches!(p.route, Route::Direct | Route::TransportDirect) {
            0xFF
        } else {
            p.path.len_byte()
        };
        let Some(node) = self.nodes.get(r) else {
            return;
        };
        let my_key = node.key;
        match p.ptype {
            PayloadType::TxtMsg => {
                let pl = p.payload;
                let (Some(&dst), Some(&src), Some(ts), Some(&flags)) =
                    (pl.first(), pl.get(1), pl.get(4..8), pl.get(8))
                else {
                    return;
                };
                if dst != my_key[0] {
                    return;
                }
                let Some((from, _, back, _)) =
                    node.contacts.iter().find(|(k, ..)| k[0] == src).cloned()
                else {
                    return;
                };
                let text: Vec<u8> = pl
                    .get(9..)
                    .unwrap_or(&[])
                    .iter()
                    .copied()
                    .take_while(|&b| b != 0)
                    .collect();
                let mut f = vec![resp::CONTACT_MSG, snr4 as u8, 0, 0];
                f.extend_from_slice(&from[..6]);
                f.extend_from_slice(&[path_len, 0]);
                f.extend_from_slice(ts);
                f.extend_from_slice(&text);
                self.queue_plain(r, f);
                // The ACK goes back the way the recipient knows, or floods.
                let tag = plain_tag(ts, flags & 3, &text, &from);
                let air = plain_packet(PayloadType::Ack, back.as_ref(), &tag.to_le_bytes());
                self.enqueue_tx(r, air, 0.2);
            }
            PayloadType::GrpTxt => {
                let pl = p.payload;
                let Some(index) = node.channels.iter().position(|c| {
                    c.as_ref().is_some_and(|(_, s)| {
                        let id = crate::plain::channel_id(s);
                        pl.get(..3) == Some(&id[..3])
                    })
                }) else {
                    return;
                };
                let (Some(ts), Some(body)) = (pl.get(3..7), pl.get(8..)) else {
                    return;
                };
                let text: Vec<u8> = body.iter().copied().take_while(|&b| b != 0).collect();
                let mut f = vec![
                    resp::CHANNEL_MSG,
                    snr4 as u8,
                    0,
                    0,
                    index as u8,
                    path_len,
                    0,
                ];
                f.extend_from_slice(ts);
                f.extend_from_slice(&text);
                self.queue_plain(r, f);
            }
            PayloadType::Ack => {
                let Some(tag) = p
                    .payload
                    .get(..4)
                    .and_then(|b| b.try_into().ok())
                    .map(u32::from_le_bytes)
                else {
                    return;
                };
                let now = self.now;
                let Some(n) = self.nodes.get_mut(r) else {
                    return;
                };
                let Some(pos) = n.acks.iter().position(|(t, _)| *t == tag) else {
                    return;
                };
                let (_, at) = n.acks.remove(pos);
                let mut push = vec![resp::SEND_CONFIRMED];
                push.extend_from_slice(&tag.to_le_bytes());
                push.extend_from_slice(&u32::try_from(now - at).unwrap_or(u32::MAX).to_le_bytes());
                self.push_to_app(r, &push);
            }
            _ => {}
        }
    }

    fn queue_plain(&mut self, r: usize, frame: Vec<u8>) {
        if let Some(n) = self.nodes.get_mut(r) {
            n.inbox.push_back(frame);
        }
        self.push_to_app(r, &[resp::MSG_WAITING]);
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
        let flood = frame.first().is_some_and(|h| h & 3 < 2);
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
            n.tx_flood = n.tx_flood.saturating_add(u32::from(flood));
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
            // Room servers answer without airtime (see the module's doc): they hear nothing
            // on the simulated channel, and draw nothing from its randomness.
            if self.nodes.get(r).is_some_and(|n| n.role == Role::Room) {
                continue;
            }
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
            let flood = tx.frame.first().is_some_and(|h| h & 3 < 2);
            if let Some(n) = self.nodes.get_mut(r) {
                n.last_rx = (snr4, rssi);
                n.rx_packets = n.rx_packets.saturating_add(1);
                n.rx_flood = n.rx_flood.saturating_add(u32::from(flood));
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
        if role == Role::Companion
            && matches!(
                p.ptype,
                PayloadType::TxtMsg | PayloadType::GrpTxt | PayloadType::Ack
            )
        {
            self.plain_received(r, &p, snr4);
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

/// The ACK tag of a plain direct message (MeshCore: a CRC over the timestamp, the flags, the
/// text and the sender's key; here, SHA-256).
fn plain_tag(ts: &[u8], attempt: u8, text: &[u8], sender: &PubKey) -> u32 {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(ts);
    h.update([attempt & 3]);
    h.update(text);
    h.update(sender);
    let d = h.finalize();
    u32::from_le_bytes([d[0], d[1], d[2], d[3]])
}

/// A plain packet: direct along `path`, or flooded.
fn plain_packet(ptype: PayloadType, path: Option<&OwnedPath>, payload: &[u8]) -> Vec<u8> {
    let route = if path.is_some() {
        Route::Direct
    } else {
        Route::Flood
    };
    let mut air = vec![((ptype as u8) << 2) | route as u8];
    match path {
        Some(p) => {
            air.push(p.len_byte);
            air.extend_from_slice(&p.bytes);
        }
        None => air.push(0),
    }
    air.extend_from_slice(payload);
    air
}

fn remember(ring: &mut VecDeque<[u8; 8]>, h: [u8; 8]) {
    if ring.len() >= DEDUP_SIZE {
        ring.pop_front();
    }
    ring.push_back(h);
}

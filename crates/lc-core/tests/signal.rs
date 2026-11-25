//! Each node's signal and the route to it (protocol.md §16.1, §16.3, ADR-0049): which frames
//! the radio heard count as a reception of a node, the rating (`testdata/lichen/signal.toml`, an
//! independent Python oracle), the history kept, and the route leg by leg.

use lc_core::fakenet::{EPOCH_MS, Net};
use lc_core::signal::{self, HISTORY_KEEP_S, HISTORY_MAX, Reading, SignalRating};
use lc_core::store::Store;
use lc_core::{Event, Node, PubKey, contact_kind};
use lc_meshcore::air::{Packet, PayloadType, Route};
use lc_meshcore::airtime::LoRaParams;
use lc_meshcore::path::Path;
use toml::Table;

fn radio() -> LoRaParams {
    LoRaParams::meshcore(8, 62_500, 5).unwrap()
}

/// `app` – `hub` – `crest` – `bob`, the app's radio knowing the three; `hub` advertises its
/// position. `lucie`, next to the app, is not in its contacts. Each link has its own readings.
struct Mesh {
    net: Net,
    app: usize,
    hub: usize,
    crest: usize,
    bob: usize,
    lucie: usize,
}

fn mesh(seed: u64) -> Mesh {
    let mut net = Net::new(radio(), 0, seed);
    let app = net.add_companion(1, "app");
    let hub = net.add_repeater(10, "hub");
    let crest = net.add_repeater(11, "crest");
    let bob = net.add_companion(2, "Bob");
    let lucie = net.add_stock_companion(6, "Lucie");
    net.chain(&[app, hub, crest, bob]);
    net.link(app, lucie);
    net.set_link_readings(app, hub, 30, -100);
    net.set_link_readings(hub, crest, -12, -116);
    net.set_link_readings(crest, bob, 20, -108);
    net.nodes[hub].advert_position = Some((45_123_456, 5_654_321));
    for n in [hub, crest, bob] {
        net.know(app, n, true);
    }
    net.connect(app);
    net.run_until(net.now + 10_000, |_| false);
    net.take_events(app);
    Mesh {
        net,
        app,
        hub,
        crest,
        bob,
        lucie,
    }
}

impl Mesh {
    /// The app's radio logs `frame` with these readings; the readings attributed.
    fn hear(&mut self, frame: &[u8], snr4: i8, rssi: i8) -> Vec<(PubKey, Reading)> {
        self.net.log_rx(self.app, frame, snr4, rssi);
        self.net.run_until(self.net.now + 100, |_| false);
        self.net
            .take_events(self.app)
            .into_iter()
            .filter_map(|e| match e {
                Event::NodeSignal { node, reading } => Some((node, reading)),
                _ => None,
            })
            .collect()
    }

    fn nodes(&self) -> Vec<Node> {
        self.net.with_core_ref(self.app, |c| c.nodes()).unwrap()
    }

    fn node(&self, i: usize) -> Node {
        let k = self.net.key(i);
        self.nodes().into_iter().find(|n| n.key == k).unwrap()
    }

    /// Unix seconds by the app's clock.
    fn unix_s(&self) -> u64 {
        (EPOCH_MS + self.net.now) / 1000
    }

    /// A fresh advert of `i`: later than any the radio reported.
    fn advert(&self, i: usize, zero_hop: bool) -> Vec<u8> {
        self.net
            .advert(i, self.net.radio_clock(self.app) + 1, zero_hop)
    }
}

/// `frame` with another route and path.
fn rerouted(frame: &[u8], route: Route, hops: &[u8]) -> Vec<u8> {
    let p = Packet::decode(frame).unwrap();
    let mut out = [0u8; 255];
    let n = Packet {
        route,
        path: Path::from_hashes(hops).unwrap(),
        ..p
    }
    .encode(&mut out)
    .unwrap();
    out[..n].to_vec()
}

/// A packet of type `ptype` from the node whose hash is `src` to `dest`: `dest ‖ src ‖ MAC (2)
/// ‖ 16 bytes`, as TXT_MSG, REQ, RESPONSE and PATH are (A:§3).
fn addressed(ptype: PayloadType, route: Route, hops: &[u8], dest: u8, src: u8) -> Vec<u8> {
    let mut payload = vec![dest, src, 0xAB, 0xCD];
    payload.extend_from_slice(&[0x5A; 16]);
    let mut out = [0u8; 255];
    let n = Packet {
        route,
        ptype,
        transport: None,
        path: Path::from_hashes(hops).unwrap(),
        payload: &payload,
    }
    .encode(&mut out)
    .unwrap();
    out[..n].to_vec()
}

/// A flooded advert heard with 0 hops is a reception of its node, exactly by its key; the same
/// advert relayed by a repeater measures the repeater, and is not attributed (§16.1).
#[test]
fn a_direct_advert_is_attributed_and_a_relayed_one_is_not() {
    let mut m = mesh(1);
    let (hub, crest) = (m.net.key(m.hub), m.net.key(m.crest));
    assert_eq!(m.node(m.hub).signal, None, "nothing heard yet");

    let advert = m.advert(m.hub, false);
    let t = m.unix_s();
    let got = m.hear(&advert, 30, -100);
    let want = Reading {
        snr4: 30,
        rssi: Some(-100),
        at_s: t,
    };
    assert_eq!(got, [(hub, want)]);
    assert_eq!(m.node(m.hub).signal, Some(want));

    // Crest's advert through the hub: the hub's signal, not crest's.
    let relayed = rerouted(&m.advert(m.crest, false), Route::Flood, &[hub[0]]);
    assert!(m.hear(&relayed, 30, -100).is_empty());
    assert_eq!(m.node(m.crest).signal, None);
    // Through two repeaters: still not.
    let far = rerouted(&m.advert(m.bob, false), Route::Flood, &[crest[0], hub[0]]);
    assert!(m.hear(&far, 30, -100).is_empty());

    // A zero-hop advert (direct, empty path) is heard from its node.
    let zero = m.advert(m.crest, true);
    assert_eq!(Packet::decode(&zero).unwrap().route, Route::Direct);
    let got = m.hear(&zero, -12, -116);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, crest);
    assert_eq!(m.node(m.crest).signal.map(|r| r.snr4), Some(-12));

    // A newer reading replaces the node's last.
    m.net.run_until(m.net.now + 5_000, |_| false);
    let t = m.unix_s();
    let got = m.hear(&m.advert(m.hub, false), 26, -104);
    assert_eq!(got.len(), 1);
    assert_eq!(
        m.node(m.hub).signal,
        Some(Reading {
            snr4: 26,
            rssi: Some(-104),
            at_s: t,
        })
    );
}

/// A radio that reports RSSI 0 (the Heltec V4) gives a reading without RSSI.
#[test]
fn an_rssi_of_zero_is_not_measured() {
    let mut m = mesh(2);
    let got = m.hear(&m.advert(m.hub, false), 30, 0);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1.rssi, None);
}

/// Adverts that do not prove who sent them are not attributed: a forged signature, our own
/// radio's, and one older than the last the radio reported for that node (played again).
#[test]
fn forged_own_and_replayed_adverts_are_not_attributed() {
    let mut m = mesh(3);
    let mut forged = m.advert(m.hub, false);
    let last = forged.len() - 1;
    forged[last] ^= 1;
    assert!(m.hear(&forged, 30, -100).is_empty(), "a forged signature");

    let own = m.advert(m.app, false);
    assert!(m.hear(&own, 30, -100).is_empty(), "our own radio");

    let old = m.net.advert(m.hub, m.net.radio_clock(m.app) - 3_600, false);
    assert!(
        m.hear(&old, 30, -100).is_empty(),
        "older than the radio's record"
    );
    assert_eq!(m.node(m.hub).signal, None);

    // A node the radio has not reported yet: its advert is its own, by its key.
    let got = m.hear(&m.advert(m.lucie, false), -16, -117);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, m.net.key(m.lucie));
}

/// A packet that names its sender by one byte is attributed only when exactly one known node
/// has that byte, and only when heard directly: a flood with 0 hops (§16.1).
#[test]
fn a_one_byte_hash_is_attributed_only_when_unique() {
    let mut m = mesh(4);
    let (me, bob) = (m.net.key(m.app), m.net.key(m.bob));
    for ptype in [
        PayloadType::TxtMsg,
        PayloadType::Req,
        PayloadType::Response,
        PayloadType::Path,
    ] {
        let flood = addressed(ptype, Route::Flood, &[], me[0], bob[0]);
        let got = m.hear(&flood, 20, -108);
        assert_eq!(got.len(), 1, "{ptype:?}");
        assert_eq!(got[0].0, bob);
        assert_eq!(got[0].1.snr4, 20);
    }
    // With transport codes too (route 0).
    let mut out = [0u8; 255];
    let payload = [me[0], bob[0], 1, 2, 3, 4, 5, 6];
    let n = Packet {
        route: Route::TransportFlood,
        ptype: PayloadType::TxtMsg,
        transport: Some([0x1234, 0]),
        path: Path::EMPTY,
        payload: &payload,
    }
    .encode(&mut out)
    .unwrap();
    assert_eq!(m.hear(&out[..n], 20, -108).len(), 1, "transport flood");

    // Relayed: the repeater's signal.
    let relayed = addressed(PayloadType::TxtMsg, Route::Flood, &[0x77], me[0], bob[0]);
    assert!(m.hear(&relayed, 20, -108).is_empty());
    // Direct with an empty path: the last repeater may have sent it.
    let direct = addressed(PayloadType::TxtMsg, Route::Direct, &[], me[0], bob[0]);
    assert!(m.hear(&direct, 20, -108).is_empty());
    // A byte no known node has.
    let known: Vec<u8> = m.nodes().iter().map(|n| n.key[0]).collect();
    let stranger = (1..=0xFE)
        .find(|b| !known.contains(b) && *b != me[0])
        .unwrap();
    let flood = addressed(PayloadType::TxtMsg, Route::Flood, &[], me[0], stranger);
    assert!(m.hear(&flood, 20, -108).is_empty());
    // Packets that name no node.
    for ptype in [PayloadType::GrpTxt, PayloadType::Ack, PayloadType::AnonReq] {
        let p = addressed(ptype, Route::Flood, &[], me[0], bob[0]);
        assert!(m.hear(&p, 20, -108).is_empty(), "{ptype:?}");
    }

    // Another node the radio knows shares Bob's first byte: nobody is attributed.
    let mut other = [0x33u8; 32];
    other[0] = bob[0];
    let app = m.app;
    m.net.know_other(app, other, "Other", contact_kind::CHAT);
    m.net.connect(app);
    m.net.run_until(m.net.now + 10_000, |_| false);
    m.net.take_events(app);
    assert!(m.nodes().iter().any(|n| n.key == other));
    let flood = addressed(PayloadType::TxtMsg, Route::Flood, &[], me[0], bob[0]);
    assert!(
        m.hear(&flood, 20, -108).is_empty(),
        "two known nodes share it"
    );
}

fn vectors() -> Table {
    let path = format!(
        "{}/../../testdata/lichen/signal.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap().parse().unwrap()
}

/// The rating at each spreading factor's limit and margins, and by RSSI: the oracle's vectors.
#[test]
fn ratings_match_the_vectors() {
    let t = vectors();
    let all = t["rating"].as_array().unwrap();
    assert!(all.len() > 60);
    for v in all {
        let name = v["name"].as_str().unwrap();
        let sf = u8::try_from(v["sf"].as_integer().unwrap()).unwrap();
        let snr4 = i8::try_from(v["snr4"].as_integer().unwrap()).unwrap();
        let rssi = i8::try_from(v["rssi"].as_integer().unwrap()).unwrap();
        let want = match v["rating"].as_str().unwrap() {
            "Good" => SignalRating::Good,
            "Fair" => SignalRating::Fair,
            "Bad" => SignalRating::Bad,
            other => panic!("{other}"),
        };
        let rssi = (rssi != 0).then_some(rssi);
        assert_eq!(signal::rate(snr4, rssi, sf), want, "{name}");
    }
}

/// The demodulation limits (SX1261/2 datasheet, Table 6-1), SNR × 4.
#[test]
fn demodulation_limits() {
    let limits: Vec<i16> = (5..=12).map(signal::snr_limit4).collect();
    assert_eq!(limits, [-10, -20, -30, -40, -50, -60, -70, -80]);
}

/// The core rates under the radio's preset: SF8 here.
#[test]
fn the_core_rates_under_the_radio_s_preset() {
    let mut m = mesh(5);
    m.hear(&m.advert(m.hub, false), -21, -100);
    let r = m.node(m.hub).signal;
    let rating = m
        .net
        .with_core_ref(m.app, |c| c.signal_rating(r.as_ref()))
        .unwrap();
    assert_eq!(
        rating,
        SignalRating::Bad,
        "−5.25 dB at SF8: within 5 dB of −10"
    );
    let none = m
        .net
        .with_core_ref(m.app, |c| c.signal_rating(None))
        .unwrap();
    assert_eq!(none, SignalRating::None);
}

/// The history: per radio and node, the newest 500, and none older than 7 days.
#[test]
fn the_history_is_capped_by_count_and_by_age() {
    let st = Store::in_memory().unwrap();
    let (radio, other_radio) = ([1u8; 32], [2u8; 32]);
    let (hub, crest) = ([10u8; 32], [11u8; 32]);
    let t0 = 1_793_000_000u64;
    let at = |i: u64| Reading {
        snr4: (i % 40) as i8,
        rssi: if i.is_multiple_of(2) {
            Some(-100)
        } else {
            None
        },
        at_s: t0 + i * 60,
    };
    for i in 0..(HISTORY_MAX as u64 + 20) {
        st.put_signal(&radio, &hub, &at(i)).unwrap();
    }
    st.put_signal(&radio, &crest, &at(3)).unwrap();
    st.put_signal(&other_radio, &hub, &at(5)).unwrap();

    let h = st.signal_history(&radio, &hub, 0).unwrap();
    assert_eq!(h.len(), HISTORY_MAX, "the newest 500");
    assert_eq!(h.first(), Some(&at(20)), "oldest first");
    assert_eq!(h.last(), Some(&at(HISTORY_MAX as u64 + 19)));
    assert_eq!(st.signal_history(&radio, &crest, 0).unwrap(), [at(3)]);
    assert_eq!(st.signal_history(&other_radio, &hub, 0).unwrap(), [at(5)]);
    // Since a time.
    let since = at(500).at_s;
    assert_eq!(st.signal_history(&radio, &hub, since).unwrap().len(), 20);

    // The last reading per node, for one radio.
    let mut last = st.last_signals(&radio).unwrap();
    last.sort_by_key(|(k, _)| *k);
    assert_eq!(last, [(hub, at(HISTORY_MAX as u64 + 19)), (crest, at(3))]);
    assert_eq!(st.last_signals(&other_radio).unwrap(), [(hub, at(5))]);

    // A reading 7 days later than older ones pushes them out.
    let later = Reading {
        snr4: 1,
        rssi: None,
        at_s: at(30).at_s + HISTORY_KEEP_S + 1,
    };
    st.put_signal(&radio, &hub, &later).unwrap();
    let h = st.signal_history(&radio, &hub, 0).unwrap();
    assert!(
        h.iter().all(|r| r.at_s + HISTORY_KEEP_S >= later.at_s),
        "none older than 7 days"
    );
    assert_eq!(h.first(), Some(&at(31)));
    assert_eq!(h.last(), Some(&later));

    // Pruned by age at start, for every radio and node.
    assert_eq!(st.prune_signals(at(4).at_s).unwrap(), 1, "crest's");
    assert!(st.signal_history(&radio, &crest, 0).unwrap().is_empty());
    assert_eq!(st.signal_history(&other_radio, &hub, 0).unwrap(), [at(5)]);
}

/// The route to a node: our radio, the repeaters of its path, the node; the first leg from the
/// first stop's own reading, the others from the last trace; a leg nothing measured has no
/// reading (§16.3).
#[test]
fn the_route_joins_the_path_the_readings_and_the_trace() {
    let mut m = mesh(6);
    let (app, hub, crest, bob) = (
        m.net.key(m.app),
        m.net.key(m.hub),
        m.net.key(m.crest),
        m.net.key(m.bob),
    );
    let route = |m: &Mesh, k: &PubKey| m.net.with_core_ref(m.app, |c| c.route(k)).unwrap();

    // To crest: our radio, the hub, crest; nothing measured yet.
    let r = route(&m, &crest).unwrap();
    assert_eq!(r.stops.len(), 3);
    assert_eq!(r.stops[0].key, Some(app));
    assert_eq!(r.stops[0].names, ["app"]);
    assert_eq!(
        r.stops[0].position, None,
        "the apps put our radio at the phone"
    );
    assert_eq!(r.stops[1].key, Some(hub));
    assert_eq!(r.stops[1].hash, [hub[0]]);
    assert_eq!(r.stops[1].names, ["hub"]);
    assert_eq!(r.stops[1].position, Some((45_123_456, 5_654_321)));
    assert_eq!(r.stops[2].key, Some(crest));
    assert!(r.stops[2].hash.is_empty());
    assert_eq!(r.stops[2].names, ["crest"]);
    assert_eq!(r.legs.len(), 2);
    assert!(r.legs.iter().all(|l| l.reading.is_none()), "{r:?}");

    // The hub heard directly: the first leg is measured.
    m.hear(&m.advert(m.hub, false), 30, -100);
    let r = route(&m, &crest).unwrap();
    let first = r.legs[0].reading.unwrap();
    assert_eq!((first.snr4, first.rssi), (30, Some(-100)));
    assert!(!r.legs[0].traced);
    assert_eq!(r.legs[1].reading, None);

    // A trace to crest measures the hub–crest leg too (the hub hearing crest).
    m.net.run_until(m.net.now + 2_000, |_| false);
    let now = m.net.now;
    m.net
        .with_core(m.app, |c, _| c.trace(&crest, now))
        .unwrap()
        .unwrap();
    m.net.run_until(m.net.now + 20_000, |_| false);
    let r = route(&m, &crest).unwrap();
    let second = r.legs[1].reading.unwrap();
    assert_eq!((second.snr4, second.rssi), (-12, None));
    assert!(r.legs[1].traced);
    // The trace is newer than the hub's advert: its final SNR (ours hearing the hub).
    let first = r.legs[0].reading.unwrap();
    assert_eq!(first.snr4, 30);
    assert!(r.legs[0].traced);

    // To Bob, a companion beyond crest: the trace stops at crest, the last leg unmeasured.
    let now = m.net.now;
    m.net
        .with_core(m.app, |c, _| c.trace(&bob, now))
        .unwrap()
        .unwrap();
    m.net.run_until(m.net.now + 20_000, |_| false);
    let r = route(&m, &bob).unwrap();
    let names: Vec<_> = r.stops.iter().map(|s| s.names.join("/")).collect();
    assert_eq!(names, ["app", "hub", "crest", "Bob"]);
    assert_eq!(r.legs.len(), 3);
    assert!(r.legs[0].reading.is_some() && r.legs[1].reading.is_some());
    assert_eq!(r.legs[2].reading, None, "companions do not forward");

    // The hub, heard directly: one leg, its own reading.
    let r = route(&m, &hub).unwrap();
    assert_eq!(r.stops.len(), 2);
    assert_eq!(r.legs.len(), 1);
    assert!(r.legs[0].reading.is_some());

    // A node the radio floods to, or one it does not know: no route.
    let lucie = m.net.key(m.lucie);
    let a = m.app;
    m.net.know(a, m.lucie, false);
    m.net.connect(a);
    m.net.run_until(m.net.now + 10_000, |_| false);
    assert!(m.nodes().iter().any(|n| n.key == lucie));
    assert_eq!(route(&m, &lucie), None);
    assert_eq!(route(&m, &[0x42; 32]), None);
}

/// A trace along an older path does not measure today's: its legs are left out.
#[test]
fn a_trace_along_another_path_is_left_out() {
    let mut m = mesh(7);
    let crest = m.net.key(m.crest);
    let now = m.net.now;
    m.net
        .with_core(m.app, |c, _| c.trace(&crest, now))
        .unwrap()
        .unwrap();
    m.net.run_until(m.net.now + 20_000, |_| false);
    let route = |m: &Mesh| m.net.with_core_ref(m.app, |c| c.route(&crest)).unwrap();
    assert!(route(&m).unwrap().legs[1].reading.is_some());
    // Crest moves next to the app: its path is empty now.
    let (app, hub, c) = (m.app, m.hub, m.crest);
    m.net.unlink_all(c);
    m.net.link(app, c);
    m.net.link(hub, c);
    m.net.know(app, c, true);
    m.net.connect(app);
    m.net.run_until(m.net.now + 10_000, |_| false);
    let r = route(&m).unwrap();
    assert_eq!(r.stops.len(), 2);
    assert_eq!(r.legs[0].reading, None, "the trace went through the hub");
}

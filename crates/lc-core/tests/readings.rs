//! A node's readings over the fake network (protocol.md §16.7, ADR-0050): asked through the
//! radio, answered or not, one request per node at a time, kept by age and count, and the
//! battery's charge from its volts.

use lc_core::fakenet::{Net, base_readings};
use lc_core::readings::{KEEP_MAX, KEEP_S};
use lc_core::store::Store;
use lc_core::{Error, Event, PubKey, Readings, ReadingsState, Server, server::Login};
use lc_meshcore::airtime::LoRaParams;

const S: u64 = 1_000;

/// `app` – `hub` – `crest` – `bob`, the app's radio knowing the three; `lucie`, next to the app,
/// is not in its contacts. The repeaters answer their clients: crest 3.92 V and 4 °C, the hub
/// 4.10 V and 11.5 °C plus its humidity for those who are not guests. Bob answers nobody.
struct Mesh {
    net: Net,
    app: usize,
    hub: usize,
    crest: usize,
    bob: usize,
    lucie: usize,
}

fn mesh(seed: u64) -> Mesh {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, seed);
    let app = net.add_companion(1, "app");
    let hub = net.add_repeater(10, "hub");
    let crest = net.add_repeater(11, "crest");
    let bob = net.add_companion(2, "Bob");
    let lucie = net.add_stock_companion(6, "Lucie");
    net.chain(&[app, hub, crest, bob]);
    net.link(app, lucie);
    for n in [hub, crest, bob] {
        net.know(app, n, true);
    }
    net.set_passwords(hub, "password", "hello");
    net.set_passwords(crest, "password", "hello");
    net.nodes[crest].telemetry = base_readings(3_920, 40);
    net.nodes[hub].telemetry = base_readings(4_100, 115);
    // Channel 2: relative humidity 71.5 % (type 104, half percents).
    net.nodes[hub].sensors = vec![2, 104, 143];
    net.connect(app);
    net.run_until(net.now + 10 * S, |_| false);
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
    fn key(&self, i: usize) -> PubKey {
        self.net.key(i)
    }

    fn ask(&mut self, i: usize) -> Result<(), Error> {
        let k = self.key(i);
        self.net
            .with_core(self.app, |c, now| c.request_readings(&k, now))
            .unwrap()
    }

    fn state(&self, i: usize) -> ReadingsState {
        let k = self.key(i);
        self.net
            .with_core_ref(self.app, |c| c.readings_state(&k))
            .unwrap()
    }

    fn login(&mut self, i: usize, password: &str) {
        let k = self.key(i);
        self.net
            .with_core(self.app, |c, now| c.login(&k, password, now))
            .unwrap()
            .unwrap();
        self.run(30);
        let login = self
            .net
            .with_core_ref(self.app, |c| c.server(&k).map(|s: &Server| s.login))
            .unwrap();
        assert!(matches!(login, Some(Login::In(_))), "{login:?}");
        self.net.take_events(self.app);
    }

    fn run(&mut self, secs: u64) {
        self.net.run_until(self.net.now + secs * S, |_| false);
    }

    /// The readings that came, by node.
    fn answers(&mut self) -> Vec<(PubKey, Readings)> {
        self.net
            .take_events(self.app)
            .into_iter()
            .filter_map(|e| match e {
                Event::NodeReadings { node, at_s, data } => {
                    Some((node, Readings::decode(&data, at_s)))
                }
                _ => None,
            })
            .collect()
    }
}

/// A repeater that lists us answers with its battery and temperature; crest's 3.92 V is 69 %.
#[test]
fn a_request_answered() {
    let mut m = mesh(1);
    m.login(m.crest, "hello");
    assert_eq!(m.state(m.crest), ReadingsState::Idle);
    m.ask(m.crest).unwrap();
    assert_eq!(m.state(m.crest), ReadingsState::Asking);
    m.run(30);
    assert_eq!(m.state(m.crest), ReadingsState::Answered);
    let got = m.answers();
    assert_eq!(got.len(), 1, "{got:?}");
    let (node, r) = &got[0];
    assert_eq!(*node, m.key(m.crest));
    assert_eq!(r.battery_mv, Some(3_920));
    assert_eq!(r.battery_percent(), Some(69));
    assert_eq!(r.temperature_dc, Some(40));
    assert_eq!(r.items.len(), 2);
    let unix = (lc_core::fakenet::EPOCH_MS + m.net.now) / 1000;
    assert!(unix - r.at_s < 30, "{} vs {unix}", r.at_s);
}

/// An admin gets the hub's sensors too; a guest only its battery and temperature.
#[test]
fn a_guest_gets_the_base_readings_an_admin_the_sensors() {
    let mut m = mesh(2);
    m.login(m.hub, "hello");
    m.ask(m.hub).unwrap();
    m.run(30);
    let got = m.answers();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1.items.len(), 2, "a guest: {got:?}");
    m.login(m.hub, "password");
    m.ask(m.hub).unwrap();
    m.run(30);
    let got = m.answers();
    assert_eq!(got.len(), 1);
    let r = &got[0].1;
    assert_eq!((r.battery_mv, r.temperature_dc), (Some(4_100), Some(115)));
    assert_eq!(r.items.len(), 3, "an admin: {got:?}");
}

/// No answer in time: a repeater that does not list us, a person who does not answer.
#[test]
fn a_request_timed_out() {
    let mut m = mesh(3);
    m.ask(m.crest).unwrap();
    m.run(5);
    assert_eq!(m.state(m.crest), ReadingsState::Asking);
    m.run(60);
    assert_eq!(m.state(m.crest), ReadingsState::NoAnswer, "not logged in");
    assert!(m.answers().is_empty());
    m.ask(m.bob).unwrap();
    m.run(90);
    assert_eq!(m.state(m.bob), ReadingsState::NoAnswer, "telemetry off");
    assert!(m.answers().is_empty());
    // The radio gone while it waits: no answer either.
    m.login(m.crest, "hello");
    m.ask(m.crest).unwrap();
    let app = m.app;
    m.net.disconnect(app);
    m.run(1);
    assert_eq!(m.state(m.crest), ReadingsState::NoAnswer);
}

/// One request per node at a time; others wait their turn in the radio's single queue.
#[test]
fn two_requests_to_one_node_give_one_in_flight() {
    let mut m = mesh(4);
    m.login(m.crest, "hello");
    m.login(m.hub, "hello");
    m.ask(m.crest).unwrap();
    assert_eq!(m.ask(m.crest), Err(Error::TooSoon { wait_ms: 0 }));
    // Another node, and a status of the first, queue behind it.
    m.ask(m.hub).unwrap();
    let hub = m.key(m.hub);
    m.net
        .with_core(m.app, |c, now| c.request_status(&hub, now))
        .unwrap()
        .unwrap();
    m.run(60);
    let got: Vec<PubKey> = m.answers().into_iter().map(|(k, _)| k).collect();
    assert_eq!(got, [m.key(m.crest), m.key(m.hub)]);
    assert_eq!(m.state(m.crest), ReadingsState::Answered);
    let status = m
        .net
        .with_core_ref(m.app, |c| c.server(&hub).and_then(|s| s.status))
        .unwrap();
    assert!(status.is_some(), "the status behind them");
    // Answered: it can be asked again.
    m.ask(m.crest).unwrap();
}

/// A node not in the radio's contacts is refused before anything goes out; so is a request
/// without a radio.
#[test]
fn a_node_off_the_radio_is_refused() {
    let mut m = mesh(5);
    assert_eq!(m.ask(m.lucie), Err(Error::UnknownContact));
    assert_eq!(m.ask(m.app), Err(Error::UnknownContact), "our own radio");
    let app = m.app;
    m.net.disconnect(app);
    m.run(1);
    assert_eq!(m.ask(m.crest), Err(Error::NoIdentity));
}

/// An answer with nothing lichen reads: said so, nothing kept.
#[test]
fn an_answer_without_a_reading() {
    let mut m = mesh(6);
    // Bob's radio answers, but with a type outside the subset only.
    m.net.nodes[m.bob].telemetry = vec![1, 200, 0, 0];
    m.ask(m.bob).unwrap();
    m.run(60);
    assert_eq!(m.state(m.bob), ReadingsState::AnsweredEmpty);
    assert!(m.answers().is_empty());
    // What follows an unknown type is dropped, what precedes it kept.
    m.net.nodes[m.bob].telemetry = [base_readings(3_700, -55), vec![1, 200, 0, 0]].concat();
    m.ask(m.bob).unwrap();
    m.run(60);
    let got = m.answers();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].1.temperature_dc, Some(-55));
    assert_eq!(got[0].1.items.len(), 2);
}

/// The charge comes from the volts with the radio's own curve (3.3 V empty, 4.2 V full).
#[test]
fn the_charge_from_the_volts() {
    for (mv, pct) in [
        (3_920, 69),
        (4_200, 100),
        (4_500, 100),
        (3_300, 0),
        (3_000, 0),
    ] {
        let r = Readings::decode(&base_readings(mv, 0), 0);
        assert_eq!(r.battery_percent(), Some(pct), "{mv} mV");
        assert_eq!(
            Some(lc_core::radio::battery_percent(mv)),
            r.battery_percent()
        );
    }
    // No voltage on channel 1: no battery, though a sensor's voltage is there.
    let r = Readings::decode(&[2, 116, 0x04, 0xEC], 0);
    assert_eq!((r.battery_mv, r.battery_percent()), (None, None));
    // A sensor's temperature stands in for a missing channel 1's.
    let r = Readings::decode(&[1, 116, 1, 0x88, 3, 103, 0xFF, 0x9C], 0);
    assert_eq!(r.temperature_dc, Some(-100));
}

/// The history: per radio and node, the newest 100, none older than 7 days.
#[test]
fn the_history_is_capped_by_age_and_count() {
    let st = Store::in_memory().unwrap();
    let (radio, other) = ([1u8; 32], [2u8; 32]);
    let (crest, hub) = ([11u8; 32], [10u8; 32]);
    let t0 = 1_793_000_000u64;
    let data = |i: u64| base_readings(3_300 + (i % 900) as u16, i as i16);
    for i in 0..(KEEP_MAX as u64 + 10) {
        st.put_readings(&radio, &crest, t0 + i * 600, &data(i))
            .unwrap();
    }
    st.put_readings(&radio, &hub, t0 + 5, &data(5)).unwrap();
    st.put_readings(&other, &crest, t0 + 7, &data(7)).unwrap();

    let h = st.readings(&radio, &crest, 0).unwrap();
    assert_eq!(h.len(), KEEP_MAX, "the newest 100");
    assert_eq!(h.first(), Some(&(t0 + 10 * 600, data(10))), "oldest first");
    assert_eq!(
        h.last().map(|r| r.0),
        Some(t0 + (KEEP_MAX as u64 + 9) * 600)
    );
    assert_eq!(st.readings(&other, &crest, 0).unwrap(), [(t0 + 7, data(7))]);
    assert_eq!(
        st.readings(&radio, &crest, t0 + 100 * 600).unwrap().len(),
        10
    );

    let mut last = st.last_readings_times(&radio).unwrap();
    last.sort();
    assert_eq!(
        last,
        [(hub, t0 + 5), (crest, t0 + (KEEP_MAX as u64 + 9) * 600)]
    );

    // An answer 7 days after older ones pushes them out.
    let later = t0 + 50 * 600 + KEEP_S + 1;
    st.put_readings(&radio, &crest, later, &data(0)).unwrap();
    let h = st.readings(&radio, &crest, 0).unwrap();
    assert!(h.iter().all(|(at, _)| at + KEEP_S >= later));
    assert_eq!(h.first().map(|r| r.0), Some(t0 + 51 * 600));

    // Pruned by age at start, for every radio and node.
    assert_eq!(st.prune_readings(t0 + 6).unwrap(), 1, "the hub's");
    assert!(st.readings(&radio, &hub, 0).unwrap().is_empty());
    assert_eq!(st.readings(&other, &crest, 0).unwrap().len(), 1);
}

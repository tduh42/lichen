//! Search mode over the fake network with geometry (protocol.md §11, ADR-0016): receptions
//! measured where the searchers are, shared, fused; consent; pings, beacons and TX power.

use lc_core::fakenet::{Geometry, Net, latlon};
use lc_core::solver::bearing_distance;
use lc_core::{Error, Event, GroupId, PathLoss};
use lc_meshcore::airtime::LoRaParams;
use lc_proto::search::BearingClass;

const S: u64 = 1_000;
const MIN: u64 = 60 * S;

/// The target (Julien) and three searchers, with a repeater on a hill linked to everyone.
struct Field {
    net: Net,
    julien: usize,
    searchers: [usize; 3],
    hub: usize,
    gid: GroupId,
}

impl Field {
    fn new(seed: u64, geometry: Geometry) -> Self {
        let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, seed);
        net.geometry = geometry;
        let julien = net.add_companion(1, "julien");
        let searchers = [
            net.add_companion(2, "alice"),
            net.add_companion(3, "bob"),
            net.add_companion(4, "carol"),
        ];
        let hub = net.add_repeater(50, "hub");
        net.place(hub, 0.0, 0.0);
        net.place(julien, 800.0, 1_200.0);
        net.place(searchers[0], -1_500.0, -500.0);
        net.place(searchers[1], 2_500.0, 0.0);
        net.place(searchers[2], 0.0, 3_000.0);
        net.link_in_range();
        let all = [julien, searchers[0], searchers[1], searchers[2]];
        for &a in &all {
            net.link(a, hub);
            for &b in &all {
                if a != b {
                    net.know(a, b, true);
                }
            }
        }
        for &a in &all {
            net.connect(a);
        }
        assert!(net.run_until(net.now + 20 * S, |n| {
            all.iter()
                .all(|&a| n.nodes[a].core.as_ref().is_some_and(|c| c.ready()))
        }));
        for (i, &a) in all.iter().enumerate() {
            for &b in &all[i + 1..] {
                net.introduce(a, b);
            }
        }
        let keys: Vec<[u8; 32]> = all.iter().map(|&i| net.key(i)).collect();
        let gid = net
            .with_core(searchers[0], |c, now| {
                c.create_group("Secours", &[keys[0], keys[2], keys[3]], now)
            })
            .unwrap()
            .unwrap();
        assert!(
            net.run_until(net.now + 60 * MIN, |n| {
                all.iter().all(|&m| {
                    n.nodes[m].core.as_ref().is_some_and(|c| {
                        c.groups()
                            .find(|g| g.gid == gid)
                            .is_some_and(|g| g.keys_known() == g.members.len() - 1)
                    })
                })
            }),
            "group keys not distributed"
        );
        let mut f = Self {
            net,
            julien,
            searchers,
            hub,
            gid,
        };
        f.net.run_until(f.net.now + 2 * MIN, |_| false);
        for i in all {
            f.net.take_events(i);
        }
        f
    }

    fn target(&self) -> [u8; 32] {
        self.net.key(self.julien)
    }

    fn start(&mut self, who: usize) {
        let (gid, t) = (self.gid, self.target());
        self.net.locate(who);
        self.net
            .with_core(who, |c, now| c.start_search(&gid, &t, now))
            .unwrap()
            .unwrap();
    }

    /// Distance from searcher `who`'s estimate to Julien, and its 95 % major semi-axis.
    fn error(&mut self, who: usize) -> Option<(f64, f64)> {
        let t = self.target();
        let e = self
            .net
            .with_core(who, |c, _| c.search_estimate(&t))
            .flatten()?;
        let (east, north) = self.net.nodes[self.julien].pos.unwrap();
        let truth = latlon(east, north);
        Some((bearing_distance((e.lat, e.lon), truth).1, e.major95_m))
    }

    /// Every searcher walks `step` metres toward Julien, gets a fix, and pings when allowed;
    /// then a minute passes.
    fn round(&mut self, step: f64) {
        let (je, jn) = self.net.nodes[self.julien].pos.unwrap();
        let t = self.target();
        for s in self.searchers {
            let (e, n) = self.net.nodes[s].pos.unwrap();
            let d = (je - e).hypot(jn - n);
            if d > 2.0 * step {
                self.net
                    .place(s, e + step * (je - e) / d, n + step * (jn - n) / d);
            }
            self.net.locate(s);
            let _ = self.net.with_core(s, |c, now| c.search_ping(&t, now));
            self.net.run_until(self.net.now + 20 * S, |_| false);
        }
    }
}

fn geometry(n: f64) -> Geometry {
    Geometry {
        a_db: -32.0,
        n,
        sigma_db: 6.0,
    }
}

/// Julien in SOS without GPS: his beacons (every 2 minutes, more often when pinged) are heard
/// by the three searchers (about 2 km away, shadowing 6 dB), who share what they measure;
/// within 15 minutes every searcher's estimate is within a third of that distance and its 95 %
/// ellipse holds him. (RSSI ranging at 6 dB gives ±60 % per reading: hundreds of metres at
/// 2 km, shrinking as the searchers close in.)
#[test]
fn sos_without_gps_converges() {
    let mut f = Field::new(3, geometry(PathLoss::DEFAULT.n));
    let j = f.julien;
    f.net.with_core(j, |c, now| c.set_sos(true, now));
    for s in f.searchers {
        f.start(s);
    }
    let mut trace = Vec::new();
    for _ in 0..15 {
        f.round(60.0);
        trace.push(f.error(f.searchers[0]).map(|(e, _)| e.round()));
    }
    println!("alice's error by minute: {trace:?}");
    for s in f.searchers {
        let (err, major95) = f.error(s).expect("an estimate");
        println!("searcher {s}: error {err:.0} m, 95 % major semi-axis {major95:.0} m");
        assert!(err < 700.0, "searcher {s}: {err:.0} m");
        assert!(
            err < major95,
            "searcher {s}: {err:.0} m outside {major95:.0} m"
        );
        // Everyone's receptions reached everyone.
        let tk = f.target();
        let by: std::collections::HashSet<[u8; 32]> = f
            .net
            .with_core(s, |c, _| c.search_measurements(&tk))
            .unwrap_or_default()
            .iter()
            .map(|m| m.by)
            .collect();
        assert_eq!(
            by.len(),
            3,
            "searcher {s} has measurements from {} searchers",
            by.len()
        );
    }
    let ev = f.net.take_events(j);
    assert!(
        ev.iter().any(|e| matches!(e, Event::Searched { .. })),
        "Julien was told he is searched"
    );
}

/// Without SOS and without consent, a PING goes unanswered; with "let this group locate me" it
/// is answered with a BEACON (TX power, and the position when Julien has a fix).
#[test]
fn consent_gates_pings() {
    let mut f = Field::new(5, geometry(2.8));
    let (alice, j, gid, t) = (f.searchers[0], f.julien, f.gid, f.target());
    f.start(alice);
    f.net
        .with_core(alice, |c, now| c.search_ping(&t, now))
        .unwrap()
        .unwrap();
    f.net.run_until(f.net.now + 40 * S, |_| false);
    assert!(
        !f.net
            .take_events(j)
            .iter()
            .any(|e| matches!(e, Event::Searched { .. }))
    );
    let info = f.net.with_core(alice, |c, now| c.searches(now)).unwrap();
    assert_eq!(info[0].tx_dbm, None, "no BEACON without consent");

    f.net
        .with_core(j, |c, now| c.set_search_consent(&gid, true, now));
    f.net.locate(j);
    f.net
        .with_core(alice, |c, now| c.search_ping(&t, now))
        .unwrap()
        .unwrap();
    f.net.run_until(f.net.now + 40 * S, |_| false);
    let alice_node = f.net.key(alice);
    assert!(
        f.net
            .take_events(j)
            .iter()
            .any(|e| matches!(e, Event::Searched { by, .. } if *by == alice_node))
    );
    let info = f.net.with_core(alice, |c, now| c.searches(now)).unwrap();
    assert_eq!(info[0].tx_dbm, Some(22));
    let fix = info[0].last_fix.expect("the BEACON's position");
    let (east, north) = f.net.nodes[j].pos.unwrap();
    assert!(bearing_distance((fix.lat, fix.lon), latlon(east, north)).1 < 10.0);
    // A BEACON's position is also a position from Julien.
    assert!(
        f.net
            .take_events(alice)
            .iter()
            .any(|e| matches!(e, Event::Location { group: Some(g), .. } if *g == gid))
    );
}

/// POWER lowers Julien's TX power for the minutes asked, the searchers learn it from the
/// BEACON that follows, and it comes back on its own, or when consent is withdrawn.
#[test]
fn power_is_lowered_then_restored() {
    let mut f = Field::new(7, geometry(2.8));
    let (alice, j, gid, t) = (f.searchers[0], f.julien, f.gid, f.target());
    f.net
        .with_core(j, |c, now| c.set_search_consent(&gid, true, now));
    f.start(alice);
    f.net
        .with_core(alice, |c, now| c.search_lower_power(&t, 2, 1, now))
        .unwrap()
        .unwrap();
    f.net.run_until(f.net.now + 40 * S, |_| false);
    assert_eq!(f.net.nodes[j].tx_dbm, 2);
    let ev = f.net.take_events(j);
    assert!(ev.contains(&Event::TxPower {
        dbm: 2,
        previous: Some(22)
    }));
    let info = f.net.with_core(alice, |c, now| c.searches(now)).unwrap();
    assert_eq!(info[0].tx_dbm, Some(2));
    f.net.run_until(f.net.now + 40 * S, |_| false);
    assert_eq!(f.net.nodes[j].tx_dbm, 22, "restored after a minute");
    assert!(f.net.take_events(j).contains(&Event::TxPower {
        dbm: 22,
        previous: None
    }));

    // Withdrawn consent restores at once.
    f.net
        .with_core(alice, |c, now| c.search_lower_power(&t, 0, 30, now))
        .unwrap()
        .unwrap();
    f.net.run_until(f.net.now + 40 * S, |_| false);
    assert_eq!(f.net.nodes[j].tx_dbm, 0);
    f.net
        .with_core(j, |c, now| c.set_search_consent(&gid, false, now));
    f.net.run_until(f.net.now + 5 * S, |_| false);
    assert_eq!(f.net.nodes[j].tx_dbm, 22);
}

/// A radio that reports RSSI 0 (the stock Heltec V4): the RSSI is the noise floor from
/// `GET_STATS` plus the SNR, flagged estimated, and saturated above 10 dB of SNR.
#[test]
fn rssi_estimated_from_noise_floor() {
    let mut f = Field::new(19, geometry(3.0));
    let (bob, j) = (f.searchers[1], f.julien);
    f.net.nodes[bob].rssi_zero = true;
    f.net.nodes[bob].noise_dbm = -112;
    f.net.with_core(j, |c, now| c.set_sos(true, now));
    f.start(bob);
    for _ in 0..4 {
        f.round(0.0);
    }
    let me = f.net.with_core_ref(bob, |c| *c.key().unwrap()).unwrap();
    let tk = f.target();
    let ms = f
        .net
        .with_core(bob, |c, _| c.search_measurements(&tk))
        .unwrap();
    let mine: Vec<_> = ms.iter().filter(|m| m.by == me).collect();
    assert!(!mine.is_empty(), "bob measured Julien");
    for m in mine {
        assert!(m.estimated);
        assert_eq!(m.noise, Some(-112));
        assert_eq!(i16::from(m.rssi.unwrap()), -112 + i16::from(m.snr4) / 4);
        assert_eq!(m.saturated, m.snr4 >= 40);
    }
}

/// Only direct copies count: a searcher who hears Julien only through the repeater measures
/// nothing itself, yet gets the others' measurements and an estimate.
#[test]
fn relayed_copies_do_not_count() {
    let mut f = Field::new(11, geometry(2.8));
    let (carol, j, hub) = (f.searchers[2], f.julien, f.hub);
    // Carol 9 km away, linked to the hub only.
    f.net.place(carol, 9_000.0, -1_000.0);
    f.net.unlink_all(carol);
    f.net.link(carol, hub);
    f.net.with_core(j, |c, now| c.set_sos(true, now));
    for s in f.searchers {
        f.start(s);
    }
    for _ in 0..5 {
        f.round(0.0);
    }
    let carol_id = f.net.with_core_ref(carol, |c| *c.key().unwrap()).unwrap();
    let tk = f.target();
    let ms = f
        .net
        .with_core(carol, |c, _| c.search_measurements(&tk))
        .unwrap();
    assert!(!ms.is_empty(), "carol got the others' measurements");
    assert!(
        ms.iter().all(|m| m.by != carol_id),
        "carol measured a relayed copy"
    );
    assert!(f.error(carol).is_some());
}

#[test]
fn limits() {
    let mut f = Field::new(23, geometry(2.8));
    let (alice, bob, j, gid, t) = (f.searchers[0], f.searchers[1], f.julien, f.gid, f.target());
    let me = f.net.key(alice);
    assert_eq!(
        f.net
            .with_core(alice, |c, now| c.start_search(&gid, &me, now)),
        Some(Err(Error::NotAllowed))
    );
    assert_eq!(
        f.net.with_core(alice, |c, now| c.search_ping(&t, now)),
        Some(Err(Error::NotSearching))
    );
    assert_eq!(
        f.net
            .with_core(alice, |c, now| c.start_search(&gid, &[9; 32], now)),
        Some(Err(Error::UnknownContact))
    );
    // Bob has no fix: no bearing.
    f.net
        .with_core(bob, |c, now| c.start_search(&gid, &t, now))
        .unwrap()
        .unwrap();
    assert_eq!(
        f.net.with_core(bob, |c, now| c.search_bearing(
            &t,
            40.0,
            BearingClass::Body,
            now
        )),
        Some(Err(Error::NoPosition))
    );
    f.start(alice);
    assert!(
        f.net
            .with_core(alice, |c, now| c.search_ping(&t, now))
            .unwrap()
            .is_ok()
    );
    assert!(matches!(
        f.net.with_core(alice, |c, now| c.search_ping(&t, now)),
        Some(Err(Error::TooSoon { wait_ms })) if wait_ms > 25 * S
    ));
    assert!(
        f.net
            .with_core(alice, |c, now| c.search_bearing(
                &t,
                40.0,
                BearingClass::Yagi,
                now
            ))
            .unwrap()
            .is_ok()
    );
    assert!(matches!(
        f.net.with_core(alice, |c, now| c.search_bearing(
            &t,
            41.0,
            BearingClass::Yagi,
            now
        )),
        Some(Err(Error::TooSoon { .. }))
    ));
    // The bearing reached Bob, alone (no RSSI).
    f.net.run_until(f.net.now + 30 * S, |_| false);
    let ms = f
        .net
        .with_core(bob, |c, _| c.search_measurements(&t))
        .unwrap();
    assert!(
        ms.iter()
            .any(|m| m.bearing.is_some_and(|b| b.deg == 40) && m.rssi.is_none())
    );
    // Julien in SOS: no consent needed; stopping drops the search.
    f.net.with_core(j, |c, now| c.set_sos(true, now));
    f.net.with_core(alice, |c, _| c.stop_search(&t));
    assert!(
        f.net
            .with_core(alice, |c, now| c.searches(now))
            .unwrap()
            .is_empty()
    );
}

/// While Julien reports GPS positions, the searchers' receptions at known distances fit the
/// path loss: a field whose exponent is 3.4 (not the default 2.8) is recognised.
#[test]
fn self_calibration() {
    let mut f = Field::new(17, geometry(3.4));
    let (alice, j) = (f.searchers[0], f.julien);
    f.net.locate(j);
    f.net.with_core(j, |c, now| c.set_sos(true, now));
    for s in f.searchers {
        f.start(s);
    }
    for _ in 0..14 {
        f.net.locate(j);
        f.round(150.0);
    }
    let t = f.target();
    let e = f
        .net
        .with_core(alice, |c, _| c.search_estimate(&t))
        .flatten()
        .unwrap();
    println!("fitted: {:?}", e.model);
    // The exponent and the offset trade against each other over a limited span of distances:
    // the fit is checked by what it predicts there.
    let truth = PathLoss {
        a_db: -32.0,
        n: 3.4,
        sigma_db: 6.0,
    };
    for d in [300.0, 1_000.0, 2_000.0] {
        let (fit, real) = (e.model.rssi(22.0, d), truth.rssi(22.0, d));
        assert!((fit - real).abs() < 4.0, "{d} m: {fit:.1} vs {real:.1} dBm");
    }
    let default = PathLoss::DEFAULT.rssi(22.0, 1_000.0);
    assert!(
        (default - truth.rssi(22.0, 1_000.0)).abs() > 10.0,
        "the default was far off"
    );
    let (err, _) = f.error(alice).unwrap();
    assert!(err < 30.0, "{err}");
}

/// Regression: four members in range of each other create a group (everyone sends a sender
/// key to everyone at once; HELLOs cross and are answered twice). A message in flight on a
/// replaced pending session used to stay stuck forever, and a member never got a key.
#[test]
fn group_keys_reach_everyone_under_contention() {
    for seed in 0..30 {
        let f = Field::new(1_000 + seed, geometry(2.8));
        drop(f);
    }
}

/// Figures for docs/measurements.md: the searchers' error and 95 % semi-axis over time (median
/// over seeds), Julien in SOS without GPS, three searchers about 2 km away walking toward him at
/// 1 m/s, shadowing 6 dB; with real RSSI, with RSSI estimated from noise + SNR (stock Heltec),
/// and with a body-shadow bearing (±40°) each round. Also the airtime each searcher spends.
#[test]
#[ignore = "figures: cargo test -p lc-core --release --test search -- --ignored --nocapture"]
fn figures() {
    const SEEDS: u64 = 10;
    const ROUNDS: usize = 15;
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    for (name, heltec, bearings) in [
        ("RSSI reported (M5)", false, false),
        ("RSSI = noise + SNR (Heltec)", true, false),
        ("RSSI + body bearings ±40°", false, true),
    ] {
        let mut err = vec![Vec::new(); ROUNDS];
        let mut axis = vec![Vec::new(); ROUNDS];
        let mut airtime = Vec::new();
        for seed in 0..SEEDS {
            let mut f = Field::new(500 + seed, geometry(2.8));
            let mut lcg = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let mut gauss = move || {
                let mut u = || {
                    lcg = lcg
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    ((lcg >> 11) as f64 / (1u64 << 53) as f64).max(1e-12)
                };
                (-2.0 * u().ln()).sqrt() * (std::f64::consts::TAU * u()).cos()
            };
            if heltec {
                for s in f.searchers {
                    f.net.nodes[s].rssi_zero = true;
                }
            }
            let j = f.julien;
            f.net.with_core(j, |c, now| c.set_sos(true, now));
            for s in f.searchers {
                f.start(s);
            }
            let tx0: u64 = f
                .searchers
                .iter()
                .map(|&s| f.net.nodes[s].tx_airtime_ms)
                .sum();
            let t0 = f.net.now;
            for r in 0..ROUNDS {
                f.round(60.0);
                if bearings {
                    let t = f.target();
                    let (je, jn) = f.net.nodes[j].pos.unwrap();
                    for s in f.searchers {
                        let (e, n) = f.net.nodes[s].pos.unwrap();
                        let (b, _) = bearing_distance(latlon(e, n), latlon(je, jn));
                        let noisy = (b + 40.0 * gauss()).rem_euclid(360.0);
                        let _ = f.net.with_core(s, |c, now| {
                            c.search_bearing(&t, noisy, BearingClass::Body, now)
                        });
                    }
                }
                if let Some((e, a)) = f.error(f.searchers[0]) {
                    err[r].push(e);
                    axis[r].push(a);
                }
            }
            let tx1: u64 = f
                .searchers
                .iter()
                .map(|&s| f.net.nodes[s].tx_airtime_ms)
                .sum();
            airtime.push((tx1 - tx0) as f64 / 3.0 / ((f.net.now - t0) as f64 / 60_000.0));
        }
        println!("\n{name}: minute, median error m, median 95 % semi-axis m");
        for r in (2..ROUNDS).step_by(3) {
            println!(
                "  {:>2}  {:>5.0}  {:>5.0}",
                r + 1,
                median(err[r].clone()),
                median(axis[r].clone())
            );
        }
        println!("  airtime per searcher: {:.0} ms/min", median(airtime));
    }
}

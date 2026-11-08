//! Location sharing over the fake network (protocol.md §12): dead reckoning, grids, expiry,
//! SOS to a group.

use lc_core::fakenet::{EPOCH_MS, Net};
use lc_core::location::{distance_m, extrapolate, radius_m, snap, to_degrees};
use lc_core::{Chat, Event, Fix, GroupId, Precision};
use lc_meshcore::airtime::LoRaParams;
use lc_proto::location::{ALT_UNKNOWN, Position};

const S: u64 = 1_000;
const MIN: u64 = 60 * S;
/// Metres per degree of latitude.
const M: f64 = 111_320.0;

/// Alice, Bob and Carol, each linked only to a repeater in the middle; everyone knows everyone.
fn three(seed: u64) -> (Net, [usize; 3]) {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, seed);
    let m = [
        net.add_companion(1, "alice"),
        net.add_companion(2, "bob"),
        net.add_companion(3, "carol"),
    ];
    let hub = net.add_repeater(50, "hub");
    for &a in &m {
        net.link(a, hub);
        for &b in &m {
            if a != b {
                net.know(a, b, true);
            }
        }
    }
    for &a in &m {
        net.connect(a);
    }
    assert!(net.run_until(net.now + 20 * S, |n| {
        m.iter()
            .all(|&a| n.nodes[a].core.as_ref().is_some_and(|c| c.ready()))
    }));
    // Everyone scanned everyone's code (ADR-0013): groups need known identities.
    net.introduce(m[0], m[1]);
    net.introduce(m[0], m[2]);
    net.introduce(m[1], m[2]);
    (net, m)
}

fn unix(now: u64) -> u64 {
    (EPOCH_MS + now) / 1000
}

/// A walker at (45.0, 5.5) going east, then north after `turn_s`, at `speed` m/s.
fn walker(t_s: f64, turn_s: f64, speed: f64) -> (f64, f64, f64) {
    let (lat0, lon0) = (45.0, 5.5);
    let east = speed * t_s.min(turn_s);
    let north = speed * (t_s - turn_s).max(0.0);
    let lat = lat0 + north / M;
    let lon = lon0 + east / (M * lat0.to_radians().cos());
    let heading = if t_s < turn_s { 90.0 } else { 0.0 };
    (lat, lon, heading)
}

fn fix_at(now: u64, lat: f64, lon: f64, heading: f64, speed: f64) -> Fix {
    Fix {
        lat,
        lon,
        alt_m: Some(1_100.0),
        accuracy_m: 5.0,
        heading_deg: Some(heading),
        speed_mps: Some(speed),
        unix_s: unix(now),
    }
}

/// Positions received by node `i`: (from, group, position, at).
fn positions(events: &[Event]) -> Vec<([u8; 32], Option<GroupId>, Position, u64)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Location {
                from,
                group,
                position,
                at,
                ..
            } => Some((*from, *group, *position, *at)),
            _ => None,
        })
        .collect()
}

/// 20 minutes of walking with one turn: Bob's extrapolation of Alice stays within about 50 m
/// of her, with a handful of positions.
#[test]
fn exact_share_follows_dead_reckoning() {
    let (mut net, [a, b, _]) = three(1);
    let bob = net.key(b);
    let start = net.now;
    let until = unix(start) + 3_600;
    net.with_core(a, |c, now| {
        c.start_share(Chat::Contact(bob), Precision::Exact, until, now)
    })
    .unwrap()
    .unwrap();
    let (speed, turn) = (1.4, 600.0);
    let mut received = Vec::new();
    let mut worst: f64 = 0.0;
    for step in 0..(20 * 60 / 5) {
        let t_s = step as f64 * 5.0;
        let (lat, lon, h) = walker(t_s, turn, speed);
        net.with_core(a, |c, now| {
            c.update_location(fix_at(now, lat, lon, h, speed), now)
        });
        net.run_until(start + (step + 1) * 5 * S, |_| false);
        received.extend(positions(&net.take_events(b)));
        // Where Bob believes Alice is, against where she is (after the first position).
        if let Some((_, _, p, at)) = received.last() {
            let seen = extrapolate(p, *at, unix(net.now));
            let (lat, lon, _) = walker(t_s + 5.0, turn, speed);
            worst = worst.max(distance_m(seen, (lat, lon)));
        }
    }
    eprintln!(
        "exact share, 20 min walk: {} positions at {:?} s, worst error {worst:.0} m",
        received.len(),
        received
            .iter()
            .map(|r| r.3 - unix(start))
            .collect::<Vec<_>>()
    );
    assert!(!received.is_empty());
    let first = &received[0].2;
    assert_eq!((first.heading, first.speed, first.alt), (90, 3, 1_100));
    // One at the start, one or two after the turn, plus a heartbeat at most.
    assert!(
        (2..=5).contains(&received.len()),
        "{} positions",
        received.len()
    );
    assert!(worst < 90.0, "worst error {worst:.0} m");
    // On air: route header and path length (2), one hop of path, the lichen frame (7-byte
    // header, content type, 14-byte body, 8-byte tag).
    let radio = LoRaParams::meshcore(8, 62_500, 5).unwrap();
    let frame = 2 + 1 + 7 + 1 + lc_proto::location::LEN + 8;
    eprintln!(
        "one direct position: {frame} bytes, {} ms at SF8 / 62.5 kHz / CR 4/5",
        radio.airtime_us(frame) / 1000
    );
    assert!(received.iter().all(|r| r.1.is_none() && !r.2.sos()));
}

/// A 1 km share only ever shows cell centres, and only when the cell changes.
#[test]
fn coarse_share_sends_cell_centres() {
    let (mut net, [a, _, c]) = three(2);
    let carol = net.key(c);
    let start = net.now;
    net.with_core(a, |core, now| {
        core.start_share(Chat::Contact(carol), Precision::Km1, unix(now) + 3_600, now)
    })
    .unwrap()
    .unwrap();
    // 30 minutes east at 2 m/s: 3.6 km, about four cells.
    let mut received = Vec::new();
    for step in 0..(30 * 60 / 10) {
        let t_s = step as f64 * 10.0;
        let (lat, lon, h) = walker(t_s, 1e9, 2.0);
        net.with_core(a, |core, now| {
            core.update_location(fix_at(now, lat, lon, h, 2.0), now);
        });
        net.run_until(start + (step + 1) * 10 * S, |_| false);
        received.extend(positions(&net.take_events(c)));
    }
    eprintln!(
        "1 km share, 3.6 km walk: {} positions at {:?} s",
        received.len(),
        received
            .iter()
            .map(|r| r.3 - unix(start))
            .collect::<Vec<_>>()
    );
    assert!(
        (3..=7).contains(&received.len()),
        "{} positions",
        received.len()
    );
    let mut cells = Vec::new();
    for (_, _, p, _) in &received {
        assert_eq!(
            snap(p.lat, p.lon, 1_000.0),
            (p.lat, p.lon),
            "not a cell centre"
        );
        assert_eq!(p.alt, ALT_UNKNOWN);
        assert!(radius_m(p.prec) >= 700.0);
        cells.push((p.lat, p.lon));
    }
    cells.dedup();
    assert_eq!(
        cells.len(),
        received.len(),
        "the same cell sent twice in a row"
    );
    // Neighbouring cells are about 1 km apart.
    for w in cells.windows(2) {
        let d = distance_m(to_degrees(w[0].0, w[0].1), to_degrees(w[1].0, w[1].1));
        assert!((800.0..1_300.0).contains(&d), "{d:.0} m");
    }
}

/// A share ends on its own with the end flag; stopping one does the same.
#[test]
fn shares_end_with_the_end_flag() {
    let (mut net, [a, b, c]) = three(3);
    let (bob, carol) = (net.key(b), net.key(c));
    net.with_core(a, |core, now| {
        core.update_location(fix_at(now, 45.0, 5.5, 0.0, 0.0), now);
        core.start_share(Chat::Contact(bob), Precision::Exact, unix(now) + 120, now)
            .unwrap();
        core.start_share(
            Chat::Contact(carol),
            Precision::M100,
            unix(now) + 3_600,
            now,
        )
        .unwrap();
    });
    net.run_until(net.now + MIN, |_| false);
    assert_eq!(positions(&net.take_events(b)).len(), 1);
    assert_eq!(positions(&net.take_events(c)).len(), 1);
    // Bob's share expires after 2 minutes.
    net.run_until(net.now + 2 * MIN, |_| false);
    let end = positions(&net.take_events(b));
    assert_eq!(end.len(), 1);
    assert!(end[0].2.end());
    assert!(
        net.take_events(a)
            .iter()
            .any(|e| matches!(e, Event::SharesChanged))
    );
    // Carol's is stopped by hand.
    net.with_core(a, |core, now| core.stop_share(&Chat::Contact(carol), now));
    net.run_until(net.now + MIN, |_| false);
    let end = positions(&net.take_events(c));
    assert_eq!(end.len(), 1);
    assert!(end[0].2.end());
    assert!(
        net.with_core(a, |core, _| core.shares().is_empty())
            .unwrap()
    );
    // Nothing more while stationary.
    net.run_until(net.now + 40 * MIN, |_| false);
    assert!(positions(&net.take_events(b)).is_empty());
    assert!(positions(&net.take_events(c)).is_empty());
}

/// A stationary share sends a heartbeat every 15 minutes.
#[test]
fn heartbeat_while_stationary() {
    let (mut net, [a, b, _]) = three(4);
    let bob = net.key(b);
    let start = net.now;
    net.with_core(a, |core, now| {
        core.start_share(Chat::Contact(bob), Precision::Exact, unix(now) + 7_200, now)
            .unwrap();
    });
    // The phone reports the same place every minute for 50 minutes.
    let mut received = Vec::new();
    for step in 0..50 {
        net.with_core(a, |core, now| {
            core.update_location(fix_at(now, 45.0, 5.5, 0.0, 0.0), now);
        });
        net.run_until(start + (step + 1) * MIN, |_| false);
        received.extend(positions(&net.take_events(b)));
    }
    // At 0, 15, 30 and 45 minutes.
    assert_eq!(received.len(), 4, "{received:?}");
}

/// SOS goes to every group, exact, every 2 minutes; turning it off clears the marker.
#[test]
fn sos_reaches_the_group() {
    let (mut net, [a, b, c]) = three(5);
    let (bob, carol) = (net.key(b), net.key(c));
    let gid = net
        .with_core(a, |core, now| {
            core.create_group("Rando", &[bob, carol], now)
        })
        .unwrap()
        .unwrap();
    // Let the group state and sender keys go around.
    net.run_until(net.now + 10 * MIN, |_| false);
    for i in [a, b, c] {
        net.take_events(i);
    }
    net.with_core(a, |core, now| {
        core.update_location(fix_at(now, 45.0, 5.5, 0.0, 0.0), now);
        core.set_sos(true, now);
    });
    net.run_until(net.now + 5 * MIN, |_| false);
    for i in [b, c] {
        let got = positions(&net.take_events(i));
        // At 0, 2 and 4 minutes.
        assert_eq!(got.len(), 3, "node {i}: {got:?}");
        assert!(
            got.iter()
                .all(|g| g.1 == Some(gid) && g.2.sos() && !g.2.end())
        );
        assert!(got.iter().all(|g| g.2.heading == 0 && g.2.alt == 1_100));
    }
    net.with_core(a, |core, now| core.set_sos(false, now));
    net.run_until(net.now + 2 * MIN, |_| false);
    for i in [b, c] {
        let got = positions(&net.take_events(i));
        assert_eq!(got.len(), 1);
        assert!(got[0].2.end() && !got[0].2.sos());
    }
}

/// What SOS reports to the app: position-less beacons to the groups without a fix, then the
/// position to every share and group once a fix comes, the queue time of the last one.
#[test]
fn sos_status_tells_what_goes_out() {
    let (mut net, [a, b, c]) = three(7);
    let (bob, carol) = (net.key(b), net.key(c));
    net.with_core(a, |core, now| {
        core.create_group("Rando", &[bob, carol], now)
    })
    .unwrap()
    .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    let off = net.with_core(a, |core, now| core.sos_status(now)).unwrap();
    assert!(!off.on && off.last_queued.is_none());

    // No fix: beacons without a position, to the group only.
    net.with_core(a, |core, now| core.set_sos(true, now));
    net.run_until(net.now + 5 * S, |_| false);
    let s = net.with_core(a, |core, now| core.sos_status(now)).unwrap();
    assert!(s.on && !s.fix_fresh && s.beacon_only);
    assert_eq!(s.recipients, 1);
    assert_eq!(s.last_queued, Some(unix(net.now) - 5));

    // A fix and a share with Bob: positions to the group and to Bob.
    net.with_core(a, |core, now| {
        core.start_share(
            lc_core::Chat::Contact(bob),
            Precision::Exact,
            unix(now) + 3_600,
            now,
        )
        .unwrap();
        core.update_location(fix_at(now, 45.0, 5.5, 0.0, 0.0), now);
    });
    net.run_until(net.now + 5 * S, |_| false);
    let s = net.with_core(a, |core, now| core.sos_status(now)).unwrap();
    assert!(s.fix_fresh && !s.beacon_only);
    assert_eq!(s.recipients, 2);
    assert!(s.last_queued.is_some_and(|t| t + 10 >= unix(net.now)));

    net.with_core(a, |core, now| core.set_sos(false, now));
    let s = net.with_core(a, |core, now| core.sos_status(now)).unwrap();
    assert!(!s.on && s.last_queued.is_none());
}

/// On lossy links some positions fail: none is resent stale, and the share recovers with a
/// fresh one, so Bob keeps up with a zigzag walk.
#[test]
fn lossy_links_recover_with_fresh_positions() {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 25, 6);
    let a = net.add_companion(1, "alice");
    let b = net.add_companion(2, "bob");
    let hub = net.add_repeater(50, "hub");
    net.link(a, hub);
    net.link(b, hub);
    net.know(a, b, true);
    net.know(b, a, true);
    net.connect(a);
    net.connect(b);
    net.run_until(20 * S, |_| false);
    let bob = net.key(b);
    let start = net.now;
    net.with_core(a, |c, now| {
        c.start_share(Chat::Contact(bob), Precision::Exact, unix(now) + 7_200, now)
    })
    .unwrap()
    .unwrap();
    // 40 minutes, changing direction every 4 minutes at 2 m/s.
    let (mut lat, mut lon) = (45.0f64, 5.5f64);
    let mut received = Vec::new();
    let mut late = 0;
    for step in 0..(40 * 60 / 5) {
        let heading: f64 = if (step / 48) % 2 == 0 { 45.0 } else { 135.0 };
        lat += 10.0 * heading.to_radians().cos() / M;
        lon += 10.0 * heading.to_radians().sin() / (M * lat.to_radians().cos());
        net.with_core(a, |c, now| {
            c.update_location(fix_at(now, lat, lon, heading, 2.0), now)
        });
        net.run_until(start + (step + 1) * 5 * S, |_| false);
        received.extend(positions(&net.take_events(b)));
        if let Some((_, _, p, at)) = received.last()
            && step > 24
            && distance_m(extrapolate(p, *at, unix(net.now)), (lat, lon)) > 250.0
        {
            late += 1;
        }
    }
    let times: Vec<u64> = received.iter().map(|r| r.3).collect();
    eprintln!(
        "25 % loss, 40 min zigzag: {} positions received, {late} of 455 checks more than 250 m off",
        received.len()
    );
    assert!(
        times.windows(2).all(|w| w[0] < w[1]),
        "stale position shown"
    );
    assert!(received.len() >= 8, "{}", received.len());
    assert!(late < 455 / 5, "{late}");
}

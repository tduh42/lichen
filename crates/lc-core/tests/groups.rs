//! Groups over the fake network: creation, key distribution, signed flooded messages,
//! removal and rekey (protocol.md §10).

use lc_core::fakenet::Net;
use lc_core::{Content, Error, Event, GroupId};
use lc_meshcore::airtime::LoRaParams;

const MIN: u64 = 60_000;

/// Five companions, each linked only to one repeater in the middle (none hears another).
/// Every member knows the admin (node 0) and the admin knows everyone.
fn star(seed: u64) -> (Net, Vec<usize>) {
    star_with_loss(seed, 0)
}

fn star_with_loss(seed: u64, loss: u32) -> (Net, Vec<usize>) {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), loss, seed);
    let names = ["alice", "bob", "carol", "dave", "eve"];
    let members: Vec<usize> = names
        .iter()
        .enumerate()
        .map(|(i, n)| net.add_companion(1 + i as u8, n))
        .collect();
    let hub = net.add_repeater(50, "hub");
    for &m in &members {
        net.link(m, hub);
    }
    for &m in &members[1..] {
        net.know(members[0], m, true);
        net.know(m, members[0], true);
    }
    for &m in &members {
        net.connect(m);
    }
    assert!(net.run_until(net.now + 20_000, |n| {
        members
            .iter()
            .all(|&m| n.nodes[m].core.as_ref().is_some_and(|c| c.has_identity()))
    }));
    (net, members)
}

fn keys_complete(net: &Net, members: &[usize], gid: &GroupId) -> bool {
    members.iter().all(|&m| {
        net.nodes[m].core.as_ref().is_some_and(|c| {
            c.groups()
                .find(|g| g.gid == *gid)
                .is_some_and(|g| g.keys_known() == g.members.len() - 1)
        })
    })
}

/// Runs until no frame has been transmitted for 5 minutes.
fn settle(net: &mut Net) {
    let mut last = (net.stats.transmissions, net.now);
    net.run_until(net.now + 120 * MIN, |n| {
        if n.stats.transmissions != last.0 {
            last = (n.stats.transmissions, n.now);
        }
        n.now >= last.1 + 5 * MIN
    });
}

fn group_texts(events: &[Event]) -> Vec<(u8, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::GroupMessage {
                from,
                content: Content::Text { text, .. },
                ..
            } => Some((from[0], text.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn group_of_five_then_removal() {
    let (mut net, m) = star(11);
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    let gid = net
        .with_core(m[0], |c, now| {
            c.create_group("Rando Vercors", &keys[1..], now)
        })
        .unwrap()
        .unwrap();
    assert!(
        net.run_until(net.now + 60 * MIN, |n| keys_complete(n, &m, &gid)),
        "sender keys not distributed"
    );
    let setup_s = net.now / 1000;
    settle(&mut net);
    let mut events: Vec<Vec<Event>> = m.iter().map(|_| Vec::new()).collect();
    for (i, &node) in m.iter().enumerate() {
        events[i].extend(net.take_events(node));
        assert!(
            i == 0
                || events[i].iter().any(
                    |e| matches!(e, Event::GroupJoined { name, .. } if name == "Rando Vercors")
                ),
            "member {i} did not join"
        );
        events[i].clear();
    }

    // Everyone posts once, spaced out; one of the messages needs two fragments.
    let long = "Refuge de la Pra: on part à 7 h par le sentier nord, rendez-vous au col \
                vers 10 h, prévoir de l'eau, le ruisseau est à sec depuis une semaine.";
    assert!(long.len() > 70 && long.len() <= 209);
    for (i, &node) in m.iter().enumerate() {
        let text = if i == 2 {
            long.to_owned()
        } else {
            format!("hello from {i}")
        };
        net.with_core(node, |c, now| {
            c.send_group_text(&gid, &text, 1_793_900_000, now)
        })
        .unwrap()
        .unwrap();
        net.run_until(net.now + 2 * MIN, |_| false);
    }
    net.run_until(net.now + 2 * MIN, |_| false);
    for (i, &node) in m.iter().enumerate() {
        events[i].extend(net.take_events(node));
        let got = group_texts(&events[i]);
        let mut expected: Vec<(u8, String)> = (0..5)
            .filter(|&j| j != i)
            .map(|j| {
                (
                    keys[j][0],
                    if j == 2 {
                        long.to_owned()
                    } else {
                        format!("hello from {j}")
                    },
                )
            })
            .collect();
        let mut got_sorted = got.clone();
        got_sorted.sort();
        expected.sort();
        assert_eq!(got_sorted, expected, "member {i}");
        events[i].clear();
    }

    settle(&mut net);
    // Alice removes eve: eve is told, the others rotate their keys.
    net.with_core(m[0], |c, now| c.remove_member(&gid, &keys[4], now))
        .unwrap()
        .unwrap();
    assert!(net.run_until(net.now + 60 * MIN, |n| {
        keys_complete(n, &m[..4], &gid)
            && m[..4].iter().all(|&i| {
                n.nodes[i].core.as_ref().is_some_and(|c| {
                    c.groups()
                        .any(|g| g.gid == gid && g.members.len() == 4 && g.epoch == 2)
                })
            })
    }));
    settle(&mut net);
    for (i, &node) in m.iter().enumerate() {
        events[i].extend(net.take_events(node));
    }
    assert!(events[4].contains(&Event::GroupLeft { gid }));
    assert!(
        net.nodes[m[4]]
            .core
            .as_ref()
            .unwrap()
            .groups()
            .next()
            .is_none()
    );
    for e in &mut events {
        e.clear();
    }
    net.with_core(m[1], |c, now| c.send_group_text(&gid, "eve is out", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 3 * MIN, |_| false);
    for (i, &node) in m.iter().enumerate() {
        events[i].extend(net.take_events(node));
        let got = group_texts(&events[i]);
        match i {
            1 => assert!(got.is_empty()),
            4 => assert!(got.is_empty(), "eve still reads the group"),
            _ => assert_eq!(
                got,
                vec![(keys[1][0], "eve is out".to_owned())],
                "member {i}"
            ),
        }
    }
    eprintln!(
        "group of 5 set up (keys everywhere) after {setup_s} s; stats {:?}",
        net.stats
    );
}

#[test]
fn limits() {
    let (mut net, m) = star(12);
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    let gid = net
        .with_core(m[0], |c, now| c.create_group("g", &keys[1..3], now))
        .unwrap()
        .unwrap();
    let too_long = "x".repeat(210);
    let r = net
        .with_core(m[0], |c, now| c.send_group_text(&gid, &too_long, 1, now))
        .unwrap();
    assert_eq!(r, Err(Error::TooLarge));
    let r = net
        .with_core(m[0], |c, now| c.send_group_text(&[9; 16], "x", 1, now))
        .unwrap();
    assert_eq!(r, Err(Error::UnknownGroup));
    // Only the admin removes members.
    assert!(net.run_until(net.now + 30 * MIN, |n| keys_complete(n, &m[..3], &gid)));
    let r = net
        .with_core(m[1], |c, now| c.remove_member(&gid, &keys[2], now))
        .unwrap();
    assert_eq!(r, Err(Error::NotAllowed));
}

/// `cargo test -p lc-core --release --test groups -- --ignored --nocapture`
#[test]
#[ignore = "measurement, prints numbers for docs/measurements.md"]
fn measure_group_delivery_one_or_two_copies() {
    for copies in [1u8, 2] {
        let (mut ok, mut total, mut dup) = (0usize, 0usize, 0usize);
        for seed in 0..10u64 {
            let (mut net, m) = star_with_loss(100 + seed, 10);
            for &i in &m {
                net.with_core(i, |c, _| c.set_group_copies(copies));
            }
            let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
            let gid = net
                .with_core(m[0], |c, now| c.create_group("g", &keys[1..], now))
                .unwrap()
                .unwrap();
            if !net.run_until(net.now + 90 * MIN, |n| keys_complete(n, &m, &gid)) {
                continue;
            }
            settle(&mut net);
            for &i in &m {
                net.take_events(i);
            }
            // Every member posts 8 times, one post a minute overall.
            for round in 0..8 {
                for (j, &i) in m.iter().enumerate() {
                    let text = format!("{round}-{j}");
                    net.with_core(i, |c, now| c.send_group_text(&gid, &text, 1, now));
                    net.run_until(net.now + MIN, |_| false);
                }
            }
            settle(&mut net);
            for (j, &i) in m.iter().enumerate() {
                let got = group_texts(&net.take_events(i));
                let expected = 8 * (m.len() - 1);
                total += expected;
                let mut seen = std::collections::HashSet::new();
                for (_, t) in &got {
                    if !seen.insert(t.clone()) {
                        dup += 1;
                    }
                }
                ok += seen.len();
                let _ = j;
            }
        }
        println!(
            "| {copies} | {ok}/{total} ({:.1} %) | {dup} |",
            100.0 * ok as f64 / total.max(1) as f64
        );
    }
}

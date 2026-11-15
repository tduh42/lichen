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
            .all(|&m| n.nodes[m].core.as_ref().is_some_and(|c| c.ready()))
    }));
    // The admin and each member scanned each other's code (ADR-0013); the members learn each
    // other's identities from the group state.
    for &m in &members[1..] {
        net.introduce(members[0], m);
    }
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
    assert!(long.len() > 62 && long.len() <= 193);
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

/// A group of 5 created from contacts around one repeater, over ten seeds: when every member
/// holds every key (median and worst, seconds).
#[test]
#[ignore = "measurement, prints numbers for docs/measurements.md"]
fn setup_timings() {
    let mut v = Vec::new();
    for seed in 1..=10 {
        let (mut net, m) = star(200 + seed);
        let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
        let t0 = net.now;
        let gid = net
            .with_core(m[0], |c, now| c.create_group("g", &keys[1..], now))
            .unwrap()
            .unwrap();
        net.run_until(net.now + 60 * MIN, |n| keys_complete(n, &m, &gid));
        v.push((net.now - t0) / 1000);
    }
    v.sort_unstable();
    println!(
        "group of 5 keys everywhere: median {} s, worst {} s, all {v:?}",
        v[v.len() / 2],
        v[v.len() - 1]
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

/// Posts in a lossy group (20 % per link, both copies of a post can be lost), without and with
/// recovery from the members (§10.6). Every member posts 8 times, a post a minute overall, then
/// once more (a message that reveals the last gaps).
/// `cargo test -p lc-core --release --test groups -- --ignored measure_group_recovery --nocapture`
#[test]
#[ignore = "measurement, prints numbers for docs/measurements.md"]
fn measure_group_recovery() {
    const SEEDS: u64 = 40;
    for (loss, recovery) in [(10, false), (10, true), (20, false), (20, true)] {
        let (mut ok, mut total, mut late, mut frames) = (0usize, 0usize, 0usize, 0u64);
        let mut delays: Vec<u64> = Vec::new();
        for seed in 0..SEEDS {
            let (mut net, m) = star_with_loss(300 + seed, loss);
            for &i in &m {
                net.with_core(i, |c, _| c.set_group_recovery(recovery));
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
            let before = net.stats.transmissions;
            // Events with when they came (to 10 s), and when each post went out.
            let mut got: Vec<Vec<(u64, Event)>> = m.iter().map(|_| Vec::new()).collect();
            let mut sent_at = std::collections::HashMap::new();
            let step = |net: &mut Net, got: &mut Vec<Vec<(u64, Event)>>| {
                net.run_until(net.now + MIN / 6, |_| false);
                for (k, &i) in m.iter().enumerate() {
                    let at = net.now;
                    if let Some(g) = got.get_mut(k) {
                        g.extend(net.take_events(i).into_iter().map(|e| (at, e)));
                    }
                }
            };
            for round in 0..9 {
                for (j, &i) in m.iter().enumerate() {
                    let text = format!("{round}-{j}");
                    sent_at.insert(text.clone(), net.now);
                    net.with_core(i, |c, now| c.send_group_text(&gid, &text, 1, now));
                    for _ in 0..6 {
                        step(&mut net, &mut got);
                    }
                }
            }
            // Recovery is over well within half an hour.
            for _ in 0..180 {
                step(&mut net, &mut got);
            }
            frames += net.stats.transmissions - before;
            for events in got {
                let mut seen = std::collections::HashSet::new();
                for (at, e) in events {
                    if let Event::GroupMessage {
                        content: Content::Text { text, .. },
                        late: l,
                        ..
                    } = e
                        && !text.starts_with("8-")
                        && seen.insert(text.clone())
                    {
                        ok += 1;
                        if l {
                            late += 1;
                            if let Some(t) = sent_at.get(&text) {
                                delays.push(at - t);
                            }
                        }
                    }
                }
                total += 8 * (m.len() - 1);
            }
        }
        delays.sort_unstable();
        let pct = |q: usize| match delays.get(delays.len() * q / 100) {
            Some(d) => format!("{} s", d / 1000),
            None => "—".into(),
        };
        println!(
            "| {loss} % | {} | {ok}/{total} ({:.1} %) | {late} | {} / {} | {:.0} |",
            if recovery { "with recovery" } else { "without" },
            100.0 * ok as f64 / total.max(1) as f64,
            pct(50),
            pct(90),
            frames as f64 / SEEDS as f64
        );
    }
}

/// A member's app restarts: the group comes back from its snapshot, the member draws a new
/// sender key (next epoch) and sends it, and still reads the others with their stored keys.
#[test]
fn member_restart_restores_the_group() {
    let (mut net, m) = star(13);
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    let gid = net
        .with_core(m[0], |c, now| c.create_group("g", &keys[1..4], now))
        .unwrap()
        .unwrap();
    let members = &m[..4];
    assert!(net.run_until(net.now + 60 * MIN, |n| keys_complete(n, members, &gid)));
    settle(&mut net);
    let snapshot = net.nodes[m[2]]
        .core
        .as_ref()
        .unwrap()
        .group_snapshot(&gid)
        .unwrap();

    // Carol's app restarts.
    net.attach_app(m[2], 77);
    net.with_core(m[2], |c, _| assert!(c.restore_group(&snapshot)));
    net.connect(m[2]);
    assert!(net.run_until(net.now + 60 * MIN, |n| keys_complete(n, members, &gid)));
    settle(&mut net);
    for &i in members {
        net.take_events(i);
    }
    // She reads Bob, and everyone reads her new key.
    net.with_core(m[1], |c, now| c.send_group_text(&gid, "from bob", 1, now));
    net.run_until(net.now + 3 * MIN, |_| false);
    net.with_core(m[2], |c, now| c.send_group_text(&gid, "from carol", 1, now));
    settle(&mut net);
    let carol = group_texts(&net.take_events(m[2]));
    assert_eq!(carol, vec![(keys[1][0], "from bob".to_owned())]);
    for &i in [m[0], m[1], m[3]].iter() {
        let got = group_texts(&net.take_events(i));
        assert!(
            got.contains(&(keys[2][0], "from carol".to_owned())),
            "{got:?}"
        );
    }
}

/// The group cost shown before sending (audit M15) is what the sender's floods take on air.
#[test]
fn group_estimate_matches_the_air() {
    let (mut net, m) = star(23);
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    let gid = net
        .with_core(m[0], |c, now| c.create_group("g", &keys[1..3], now))
        .unwrap()
        .unwrap();
    settle(&mut net);
    for text in ["ok", &"long message ".repeat(12)] {
        let (fragments, airtime) = net
            .with_core(m[0], |c, _| c.estimate_group_send(5 + text.len()))
            .unwrap();
        let before = net.nodes[m[0]].tx_airtime_ms;
        net.with_core(m[0], |c, now| c.send_group_text(&gid, text, 1, now))
            .unwrap()
            .unwrap();
        settle(&mut net);
        let spent = net.nodes[m[0]].tx_airtime_ms - before;
        assert!(fragments >= 1);
        assert!(
            spent.abs_diff(airtime) * 20 <= spent,
            "{text:?}: estimated {airtime} ms, spent {spent} ms"
        );
    }
}

/// ADR-0017: the admin invites Dave, who reads what follows; Carol leaves (the admin removes
/// her, keys rotate, she gets nothing more); the admin closes the group, everyone forgets it.
#[test]
fn invite_leave_close() {
    let (mut net, m) = star(31);
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    let (alice, bob, carol, dave) = (m[0], m[1], m[2], m[3]);
    let gid = net
        .with_core(alice, |c, now| c.create_group("Rando", &keys[1..3], now))
        .unwrap()
        .unwrap();
    assert!(net.run_until(net.now + 60 * MIN, |n| keys_complete(n, &m[..3], &gid)));
    settle(&mut net);
    for &i in &m {
        net.take_events(i);
    }

    // Only the admin invites.
    assert_eq!(
        net.with_core(bob, |c, now| c.add_members(&gid, &[keys[3]], now)),
        Some(Err(lc_core::Error::NotAllowed))
    );
    net.with_core(alice, |c, now| c.add_members(&gid, &[keys[3]], now))
        .unwrap()
        .unwrap();
    assert!(
        net.run_until(net.now + 60 * MIN, |n| keys_complete(
            n,
            &[alice, bob, carol, dave],
            &gid
        )),
        "Dave holds every key"
    );
    assert!(
        net.take_events(dave)
            .iter()
            .any(|e| matches!(e, Event::GroupJoined { .. }))
    );
    net.with_core(bob, |c, now| {
        c.send_group_text(&gid, "welcome Dave", 1, now)
    })
    .unwrap()
    .unwrap();
    settle(&mut net);
    assert!(
        group_texts(&net.take_events(dave))
            .iter()
            .any(|(_, t)| t == "welcome Dave")
    );

    // Carol leaves: gone on her side at once, removed by the admin, keys rotated.
    net.with_core(carol, |c, now| c.leave_group(&gid, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 1_000, |_| false);
    assert!(
        net.take_events(carol)
            .iter()
            .any(|e| matches!(e, Event::GroupLeft { .. }))
    );
    assert!(net.with_core(carol, |c, _| c.groups().count()).unwrap() == 0);
    assert!(net.run_until(net.now + 60 * MIN, |n| {
        n.nodes[alice].core.as_ref().is_some_and(|c| {
            c.groups()
                .find(|g| g.gid == gid)
                .is_some_and(|g| g.members.len() == 3)
        })
    }));
    assert!(net.run_until(net.now + 60 * MIN, |n| keys_complete(
        n,
        &[alice, bob, dave],
        &gid
    )));
    settle(&mut net);
    for &i in &m {
        net.take_events(i);
    }
    net.with_core(bob, |c, now| c.send_group_text(&gid, "after Carol", 2, now))
        .unwrap()
        .unwrap();
    settle(&mut net);
    assert!(
        group_texts(&net.take_events(dave))
            .iter()
            .any(|(_, t)| t == "after Carol")
    );
    assert!(group_texts(&net.take_events(carol)).is_empty());

    // The admin closes the group.
    net.with_core(alice, |c, now| c.leave_group(&gid, now))
        .unwrap()
        .unwrap();
    settle(&mut net);
    for i in [alice, bob, dave] {
        assert_eq!(
            net.with_core(i, |c, _| c.groups().count()),
            Some(0),
            "node {i}"
        );
        assert!(
            net.take_events(i)
                .iter()
                .any(|e| matches!(e, Event::GroupLeft { .. }))
                || i == alice
        );
    }
}

/// What a text, a reply and a reaction cost on the air (§6.2): every transmission counted,
/// relays and acknowledgements included. Direct: alice → bob through the hub (one hop), on an
/// established session. Group: alice to the five members, flooded twice through the hub.
#[test]
#[ignore = "measurement, prints numbers for docs/measurements.md"]
fn reply_and_reaction_costs() {
    use lc_core::content::message_ref;
    let (mut net, m) = star(23);
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    let gid = net
        .with_core(m[0], |c, now| c.create_group("g", &keys[1..], now))
        .unwrap()
        .unwrap();
    assert!(net.run_until(net.now + 60 * MIN, |n| keys_complete(n, &m, &gid)));
    let bob = keys[1];
    // A first message opens the session; it is not counted.
    net.with_core(m[0], |c, now| c.send_text(&bob, "hello", 1, now))
        .unwrap()
        .unwrap();
    settle(&mut net);
    let airtime = |n: &Net| -> u64 { n.nodes.iter().map(|x| x.tx_airtime_ms).sum() };
    let frames = |n: &Net| -> u64 { n.nodes.iter().map(|x| x.tx_frames).sum() };
    let me = net.with_core_ref(m[0], |c| *c.key().unwrap()).unwrap();
    let hello = Content::Text {
        sent_at: 1,
        text: "hello".into(),
    };
    let target = message_ref(&me, &hello.encode());
    let text = "See you at the hut!!";
    let contents = [
        (
            "text, 20 characters",
            Content::Text {
                sent_at: 2,
                text: text.into(),
            },
        ),
        (
            "reply, 20 characters",
            Content::Reply {
                sent_at: 3,
                target,
                text: text.into(),
            },
        ),
        (
            "reaction 👍",
            Content::Reaction {
                sent_at: 4,
                target,
                emoji: "👍".into(),
            },
        ),
    ];
    println!(
        "| Content | Plaintext | Direct, 1 hop (frames, airtime) | Group of 5, 2 copies (frames, airtime) |"
    );
    println!("|---|---|---|---|");
    for (name, c) in contents {
        let d = c.encode();
        let (f0, a0) = (frames(&net), airtime(&net));
        net.with_core(m[0], |core, now| core.send_datagram(&bob, d.clone(), now))
            .unwrap()
            .unwrap();
        settle(&mut net);
        let (f1, a1) = (frames(&net), airtime(&net));
        net.with_core(m[0], |core, now| core.send_group_datagram(&gid, &d, now))
            .unwrap()
            .unwrap();
        settle(&mut net);
        let (f2, a2) = (frames(&net), airtime(&net));
        println!(
            "| {name} | {} B | {} frames, {:.2} s | {} frames, {:.2} s |",
            d.len(),
            f1 - f0,
            (a1 - a0) as f64 / 1000.0,
            f2 - f1,
            (a2 - a1) as f64 / 1000.0
        );
    }
}

/// Notices drained from `node`'s events: (kind, actor name, subject names, text). The network
/// runs a moment first: a node's events are collected when it next wakes.
fn notices(net: &mut Net, node: usize) -> Vec<(u8, String, Vec<String>, String)> {
    net.run_until(net.now + 1_000, |_| false);
    net.take_events(node)
        .into_iter()
        .filter_map(|e| match e {
            Event::GroupChanged { notice: n, .. } => Some((
                n.kind,
                n.actor_name,
                n.subjects.into_iter().map(|(_, name)| name).collect(),
                n.text,
            )),
            _ => None,
        })
        .collect()
}

fn group_name(net: &Net, node: usize, gid: &GroupId) -> Option<String> {
    net.nodes[node]
        .core
        .as_ref()?
        .groups()
        .find(|g| g.gid == *gid)
        .map(|g| g.name.clone())
}

/// Every membership change becomes a notice on each phone concerned, with who did it
/// (ADR-0021, §10.1 cause).
#[test]
fn membership_notices() {
    use lc_core::content::notice::*;
    let (mut net, m) = star(41);
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    let (alice, bob, carol, dave) = (m[0], m[1], m[2], m[3]);
    let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
    let gid = net
        .with_core(alice, |c, now| c.create_group("Rando", &keys[1..3], now))
        .unwrap()
        .unwrap();
    assert!(net.run_until(net.now + 60 * MIN, |n| keys_complete(n, &m[..3], &gid)));
    assert_eq!(
        notices(&mut net, alice),
        vec![(CREATED, "alice".into(), vec![], String::new())]
    );
    assert_eq!(
        notices(&mut net, bob),
        vec![(ADDED_US, "alice".into(), vec![], String::new())]
    );
    assert_eq!(
        notices(&mut net, carol),
        vec![(ADDED_US, "alice".into(), vec![], String::new())]
    );

    net.with_core(alice, |c, now| c.add_members(&gid, &[keys[3]], now))
        .unwrap()
        .unwrap();
    // Everyone holds the new state (four members) and every key: a member that has not heard
    // of Dave yet also counts as having its keys complete.
    assert!(net.run_until(net.now + 60 * MIN, |n| {
        keys_complete(n, &m[..4], &gid)
            && m[..4].iter().all(|&i| {
                n.nodes[i]
                    .core
                    .as_ref()
                    .is_some_and(|c| c.groups().any(|g| g.gid == gid && g.members.len() == 4))
            })
    }));
    assert_eq!(
        notices(&mut net, alice),
        vec![(JOINED, "alice".into(), s(&["dave"]), String::new())]
    );
    assert_eq!(
        notices(&mut net, bob),
        vec![(JOINED, "alice".into(), s(&["dave"]), String::new())]
    );
    assert_eq!(
        notices(&mut net, carol),
        vec![(JOINED, "alice".into(), s(&["dave"]), String::new())]
    );
    assert_eq!(
        notices(&mut net, dave),
        vec![(ADDED_US, "alice".into(), vec![], String::new())]
    );

    // Only the admin renames.
    assert_eq!(
        net.with_core(bob, |c, now| c.rename_group(&gid, "Mine", now)),
        Some(Err(Error::NotAllowed))
    );
    net.with_core(alice, |c, now| {
        c.rename_group(&gid, "Vercors, dimanche", now)
    })
    .unwrap()
    .unwrap();
    assert!(net.run_until(net.now + 30 * MIN, |n| {
        [bob, carol]
            .iter()
            .all(|&x| group_name(n, x, &gid).as_deref() == Some("Vercors, dimanche"))
    }));
    let renamed = (
        RENAMED,
        "alice".to_owned(),
        vec![],
        "Vercors, dimanche".to_owned(),
    );
    assert_eq!(notices(&mut net, alice), vec![renamed.clone()]);
    assert_eq!(notices(&mut net, bob), vec![renamed.clone()]);
    assert_eq!(notices(&mut net, carol), vec![renamed]);

    // Carol leaves: "carol left" for the others.
    net.with_core(carol, |c, now| c.leave_group(&gid, now))
        .unwrap()
        .unwrap();
    assert_eq!(
        notices(&mut net, carol),
        vec![(WE_LEFT, "carol".into(), vec![], String::new())]
    );
    let count = |n: &Net, i: usize| {
        n.nodes[i]
            .core
            .as_ref()
            .and_then(|c| c.groups().find(|g| g.gid == gid).map(|g| g.members.len()))
    };
    assert!(net.run_until(net.now + 60 * MIN, |n| count(n, bob) == Some(3)));
    let left = (LEFT, "carol".to_owned(), s(&["carol"]), String::new());
    assert_eq!(notices(&mut net, alice), vec![left.clone()]);
    assert_eq!(notices(&mut net, bob), vec![left]);

    // Alice removes Dave: "alice removed dave"; Dave is told he was removed.
    net.with_core(alice, |c, now| c.remove_member(&gid, &keys[3], now))
        .unwrap()
        .unwrap();
    assert!(net.run_until(net.now + 60 * MIN, |n| {
        count(n, bob) == Some(2) && count(n, dave).is_none()
    }));
    let removed = (REMOVED, "alice".to_owned(), s(&["dave"]), String::new());
    assert_eq!(notices(&mut net, alice), vec![removed.clone()]);
    assert_eq!(notices(&mut net, bob), vec![removed]);
    // Dave saw the rename and Carol leave before he was removed.
    assert_eq!(
        notices(&mut net, dave),
        vec![
            (RENAMED, "alice".into(), vec![], "Vercors, dimanche".into()),
            (LEFT, "carol".into(), s(&["carol"]), String::new()),
            (REMOVED_US, "alice".into(), vec![], String::new()),
        ]
    );

    // Alice closes the group.
    net.with_core(alice, |c, now| c.leave_group(&gid, now))
        .unwrap()
        .unwrap();
    assert!(net.run_until(net.now + 60 * MIN, |n| count(n, bob).is_none()));
    assert_eq!(
        notices(&mut net, alice),
        vec![(CLOSED, "alice".into(), vec![], String::new())]
    );
    assert_eq!(
        notices(&mut net, bob),
        vec![(CLOSED, "alice".into(), vec![], String::new())]
    );
}

/// Local notices (0xE0–0xEF, §6) never come from the air: a peer that sends one, pairwise or
/// in the group, has it dropped.
#[test]
fn notices_are_never_received() {
    let (mut net, m) = star(42);
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    let gid = net
        .with_core(m[0], |c, now| c.create_group("g", &keys[1..2], now))
        .unwrap()
        .unwrap();
    assert!(net.run_until(net.now + 60 * MIN, |n| keys_complete(n, &m[..2], &gid)));
    net.take_events(m[1]);
    let fake = Content::Notice(lc_core::content::Notice {
        kind: lc_core::content::notice::REMOVED_US,
        actor: [0; 32],
        actor_name: "the admin".into(),
        subjects: vec![],
        text: String::new(),
    })
    .encode();
    let id = net
        .with_core(m[0], |c, now| c.send_datagram(&keys[1], fake.clone(), now))
        .unwrap()
        .unwrap();
    net.with_core(m[0], |c, now| c.send_group_datagram(&gid, &fake, now))
        .unwrap()
        .unwrap();
    let mut delivered = false;
    net.run_until(net.now + 10 * MIN, |n| {
        delivered |= n.take_events(m[0]).contains(&Event::Delivered { id });
        false
    });
    assert!(delivered, "the datagram did arrive");
    assert!(!net.take_events(m[1]).iter().any(|e| matches!(
        e,
        Event::Received { .. } | Event::GroupMessage { .. } | Event::GroupChanged { .. }
    )));
}

/// Review of 2026-11-15, SR-12: Carol's radio is away for two hours while the admin removes
/// Bob. The new state must reach her when she is back: before, it was dropped after five tries
/// (31 minutes), and Carol kept Bob in the group and kept sending him her keys.
#[test]
fn a_removal_reaches_a_member_who_was_away() {
    let (mut net, m) = star(12);
    let members = &m[..4];
    let keys: Vec<[u8; 32]> = members.iter().map(|&i| net.key(i)).collect();
    let gid = net
        .with_core(m[0], |c, now| c.create_group("Bivouac", &keys[1..], now))
        .unwrap()
        .unwrap();
    assert!(net.run_until(net.now + 60 * MIN, |n| keys_complete(n, members, &gid)));
    settle(&mut net);
    let bob_id = *net.nodes[m[1]].core.as_ref().unwrap().key().unwrap();
    let lists_bob = |net: &Net, i: usize| {
        net.nodes[i]
            .core
            .as_ref()
            .unwrap()
            .groups()
            .any(|g| g.gid == gid && g.members.iter().any(|x| x.key == bob_id))
    };
    assert!(lists_bob(&net, m[2]));

    net.disconnect(m[2]);
    net.with_core(m[0], |c, now| c.remove_member(&gid, &keys[1], now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 120 * MIN, |_| false);
    assert!(lists_bob(&net, m[2]), "Carol was away");
    assert!(!lists_bob(&net, m[3]));

    net.connect(m[2]);
    assert!(
        net.run_until(net.now + 90 * MIN, |n| !lists_bob(n, m[2])),
        "the removal never reached Carol"
    );
}

/// Review of 2026-11-15, SR-13: any contact whose identity we know could put us in a group,
/// import its members as contacts, and receive our SOS position. A group that lists us now
/// waits for the user; declined, nothing of it stays.
#[test]
fn a_group_waits_for_the_user() {
    let (mut net, m) = star(13);
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    // Bob's app asks, and he never verified Alice in person; the others are simulated
    // people and join at once.
    let alice_id = *net.nodes[m[0]].core.as_ref().unwrap().key().unwrap();
    net.with_core(m[1], |c, _| {
        c.set_group_consent(true);
        c.set_verified(&keys[0], &alice_id, false)
    });
    let gid = net
        .with_core(m[0], |c, now| c.create_group("Bivouac", &keys[1..3], now))
        .unwrap()
        .unwrap();
    let mut ev = Vec::new();
    assert!(net.run_until(net.now + 30 * MIN, |n| {
        ev.extend(n.take_events(m[1]));
        ev.iter().any(|e| matches!(e, Event::GroupInvited { .. }))
    }));
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::GroupInvited { gid: g, name, members: 3, .. } if *g == gid && name == "Bivouac"
    )));
    assert!(!ev.iter().any(|e| matches!(e, Event::GroupJoined { .. })));
    fn bob<'a>(net: &'a Net, m: &[usize]) -> &'a lc_core::Core {
        net.nodes[m[1]].core.as_ref().unwrap()
    }
    assert_eq!(bob(&net, &m).groups().count(), 0);
    assert_eq!(bob(&net, &m).invitations().len(), 1);
    // Carol's node is not one of Bob's contacts until he joins.
    assert!(!bob(&net, &m).contacts().any(|c| c.key == keys[2]));

    // He accepts: he is a member, gets the keys, and reads Carol.
    net.run_until(net.now + 20 * MIN, |_| false);
    net.with_core(m[1], |c, now| c.accept_group(&gid, now))
        .unwrap()
        .unwrap();
    assert!(bob(&net, &m).contacts().any(|c| c.key == keys[2]));
    assert!(
        net.run_until(net.now + 60 * MIN, |n| keys_complete(n, &m[..3], &gid)),
        "keys after accepting"
    );
    settle(&mut net);
    net.take_events(m[1]);
    net.with_core(m[2], |c, now| {
        c.send_group_text(&gid, "welcome", 1_793_900_000, now)
    })
    .unwrap()
    .unwrap();
    net.run_until(net.now + 5 * MIN, |_| false);
    assert_eq!(
        group_texts(&net.take_events(m[1])),
        vec![(keys[2][0], "welcome".to_owned())]
    );
}

#[test]
fn a_declined_group_leaves_nothing() {
    let (mut net, m) = star(14);
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    let alice_id = *net.nodes[m[0]].core.as_ref().unwrap().key().unwrap();
    net.with_core(m[1], |c, _| {
        c.set_group_consent(true);
        c.set_verified(&keys[0], &alice_id, false)
    });
    let gid = net
        .with_core(m[0], |c, now| c.create_group("Spam", &keys[1..3], now))
        .unwrap()
        .unwrap();
    assert!(net.run_until(net.now + 30 * MIN, |n| {
        n.nodes[m[1]].core.as_ref().unwrap().invitations().len() == 1
    }));
    net.with_core(m[1], |c, _| c.decline_group(&gid));
    net.run_until(net.now + 30 * MIN, |_| false);
    let bob = net.nodes[m[1]].core.as_ref().unwrap();
    assert!(bob.invitations().is_empty());
    assert_eq!(bob.groups().count(), 0);
    assert!(!bob.contacts().any(|c| c.key == keys[2]));
}

/// Someone verified in person adds us: joined at once, as in other messengers.
#[test]
fn a_verified_admin_adds_us() {
    let (mut net, m) = star(15);
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    net.with_core(m[1], |c, _| c.set_group_consent(true));
    let gid = net
        .with_core(m[0], |c, now| c.create_group("Amis", &keys[1..3], now))
        .unwrap()
        .unwrap();
    assert!(net.run_until(net.now + 30 * MIN, |n| {
        n.nodes[m[1]]
            .core
            .as_ref()
            .unwrap()
            .groups()
            .any(|g| g.gid == gid)
    }));
    assert!(
        net.nodes[m[1]]
            .core
            .as_ref()
            .unwrap()
            .invitations()
            .is_empty()
    );
}

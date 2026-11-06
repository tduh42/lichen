//! The whole stack (core → companion protocol → simulated MeshCore network → companion → core).

use lc_core::fakenet::Net;
use lc_core::{Content, Event, MsgId};
use lc_meshcore::airtime::LoRaParams;

const MIN: u64 = 60_000;

fn eu_narrow() -> LoRaParams {
    LoRaParams::meshcore(8, 62_500, 5).unwrap()
}

/// Two companions `a` and `b`, with `repeaters` repeaters between them in a chain.
fn chain(repeaters: usize, loss_pct: u32, seed: u64, with_paths: bool) -> (Net, usize, usize) {
    let mut net = Net::new(eu_narrow(), loss_pct, seed);
    let a = net.add_companion(1, "alice");
    let mut nodes = vec![a];
    for r in 0..repeaters {
        nodes.push(net.add_repeater(10 + r as u8, &format!("rpt{r}")));
    }
    let b = net.add_companion(2, "bob");
    nodes.push(b);
    net.chain(&nodes);
    net.know(a, b, with_paths);
    net.know(b, a, with_paths);
    net.connect(a);
    net.connect(b);
    assert!(net.run_until(net.now + 10_000, |n| {
        n.nodes[a].core.as_ref().is_some_and(|c| c.ready())
            && n.nodes[b].core.as_ref().is_some_and(|c| c.ready())
    }));
    net.run_until(net.now + 1_000, |_| false);
    (net, a, b)
}

fn send(net: &mut Net, from: usize, to: usize, datagram: Vec<u8>) -> MsgId {
    let key = net.key(to);
    net.with_core(from, |c, now| c.send_datagram(&key, datagram, now))
        .unwrap()
        .unwrap()
}

#[derive(Debug, Default)]
struct Seen {
    delivered: Vec<MsgId>,
    failed: Vec<MsgId>,
    received: Vec<Content>,
}

fn collect(net: &mut Net, i: usize, seen: &mut Seen) {
    for e in net.take_events(i) {
        match e {
            Event::Delivered { id } => seen.delivered.push(id),
            Event::Failed { id, .. } => seen.failed.push(id),
            Event::Received { content, .. } => seen.received.push(content),
            _ => {}
        }
    }
}

fn file(len: usize, seed: u8) -> Vec<u8> {
    let mut v = vec![0x02];
    v.extend((0..len - 1).map(|i| (i as u8).wrapping_mul(13).wrapping_add(seed)));
    v
}

#[test]
fn text_between_neighbours() {
    let (mut net, a, b) = chain(0, 0, 1, true);
    let key_b = net.key(b);
    let id = net
        .with_core(a, |c, now| {
            c.send_text(&key_b, "Je suis au refuge", 1_793_000_000, now)
        })
        .unwrap()
        .unwrap();
    let (mut sa, mut sb) = (Seen::default(), Seen::default());
    let done = net.run_until(net.now + 2 * MIN, |n| {
        collect(n, a, &mut sa);
        collect(n, b, &mut sb);
        sa.delivered.contains(&id)
    });
    assert!(done, "not delivered: {sa:?} {sb:?}");
    assert_eq!(
        sb.received,
        vec![Content::Text {
            sent_at: 1_793_000_000,
            text: "Je suis au refuge".into()
        }]
    );
}

#[test]
fn ten_kilobytes_over_three_repeaters_at_ten_percent_loss() {
    let (mut net, a, b) = chain(3, 10, 7, true);
    let data = file(9_800, 3);
    let id = send(&mut net, a, b, data.clone());
    let (mut sa, mut sb) = (Seen::default(), Seen::default());
    let t0 = net.now;
    let done = net.run_until(net.now + 60 * MIN, |n| {
        collect(n, a, &mut sa);
        collect(n, b, &mut sb);
        sa.delivered.contains(&id) || sa.failed.contains(&id)
    });
    assert!(
        done && sa.delivered.contains(&id),
        "{sa:?} stats {:?}",
        net.stats
    );
    assert_eq!(sb.received.len(), 1);
    assert_eq!(sb.received[0].encode(), data);
    eprintln!(
        "10 KB over 3 repeaters at 10 %: {} s, stats {:?}, airtime per node {:?}",
        (net.now - t0) / 1000,
        net.stats,
        net.nodes
            .iter()
            .map(|n| n.tx_airtime_ms / 1000)
            .collect::<Vec<_>>()
    );
}

#[test]
fn path_discovery_when_the_companion_has_no_path() {
    let (mut net, a, b) = chain(2, 0, 3, false);
    let key_b = net.key(b);
    let id = net
        .with_core(a, |c, now| c.send_text(&key_b, "hello", 1, now))
        .unwrap()
        .unwrap();
    let (mut sa, mut sb) = (Seen::default(), Seen::default());
    assert!(net.run_until(net.now + 10 * MIN, |n| {
        collect(n, a, &mut sa);
        collect(n, b, &mut sb);
        sa.delivered.contains(&id)
    }));
    assert_eq!(sb.received.len(), 1);
}

#[test]
fn crossing_hellos_settle_on_one_session() {
    let (mut net, a, b) = chain(1, 0, 4, true);
    let id_a = send(&mut net, a, b, file(300, 1));
    let id_b = send(&mut net, b, a, file(300, 2));
    let (mut sa, mut sb) = (Seen::default(), Seen::default());
    assert!(
        net.run_until(net.now + 20 * MIN, |n| {
            collect(n, a, &mut sa);
            collect(n, b, &mut sb);
            sa.delivered.contains(&id_a) && sb.delivered.contains(&id_b)
        }),
        "{sa:?} {sb:?}"
    );
    assert_eq!(sa.received.len(), 1);
    assert_eq!(sb.received.len(), 1);
}

#[test]
fn restart_of_the_receiver_triggers_a_new_handshake() {
    let (mut net, a, b) = chain(1, 0, 5, true);
    let id1 = send(&mut net, a, b, file(200, 1));
    let (mut sa, mut sb) = (Seen::default(), Seen::default());
    assert!(net.run_until(net.now + 10 * MIN, |n| {
        collect(n, a, &mut sa);
        n.take_events(b);
        sa.delivered.contains(&id1)
    }));
    // Bob's app restarts: its session with Alice is gone.
    net.attach_app(b, 99);
    net.connect(b);
    net.run_until(net.now + 5_000, |_| false);
    let id2 = send(&mut net, a, b, file(2_000, 2));
    assert!(net.run_until(net.now + 30 * MIN, |n| {
        collect(n, a, &mut sa);
        collect(n, b, &mut sb);
        sa.delivered.contains(&id2) || sa.failed.contains(&id2)
    }));
    assert!(sa.delivered.contains(&id2), "{sa:?}");
    assert_eq!(sb.received.len(), 1);
}

#[test]
fn unknown_contact_is_refused() {
    let (mut net, a, _) = chain(0, 0, 6, true);
    let r = net
        .with_core(a, |c, now| c.send_text(&[9; 32], "x", 1, now))
        .unwrap();
    assert_eq!(r, Err(lc_core::Error::UnknownContact));
}

/// `cargo test -p lc-core --release --test fakenet -- --ignored --nocapture`
#[test]
#[ignore = "measurement, prints numbers for docs/measurements.md"]
fn measure_ten_kilobytes_by_path_and_loss() {
    for reps in [0usize, 1, 3] {
        for loss in [0u32, 10, 15] {
            let (mut ok, mut secs, mut frames, mut coll) = (0u64, 0u64, 0u64, 0u64);
            let runs = 50;
            for seed in 0..runs {
                let (mut net, a, b) = chain(reps, loss, seed, true);
                let id = send(&mut net, a, b, file(9_800, seed as u8));
                let mut sa = Seen::default();
                let t0 = net.now;
                net.run_until(net.now + 120 * MIN, |n| {
                    collect(n, a, &mut sa);
                    n.take_events(b);
                    sa.delivered.contains(&id) || sa.failed.contains(&id)
                });
                if sa.delivered.contains(&id) {
                    ok += 1;
                    secs += (net.now - t0) / 1000;
                }
                frames += net.stats.transmissions;
                coll += net.stats.collisions;
            }
            println!(
                "| {reps} | {loss} % | {ok}/{runs} | {} s | {} | {:.1} % |",
                secs / ok.max(1),
                frames / runs,
                100.0 * coll as f64 / frames.max(1) as f64
            );
        }
    }
}

/// Whatever the losses, crossings and restarts, no message reaches the app twice, and every
/// message reported delivered was received.
#[test]
fn never_delivered_twice() {
    let mut delivered_total = 0;
    for seed in 0..30u64 {
        let loss = [0, 10, 20][(seed % 3) as usize];
        let (mut net, a, b) = chain(1 + (seed % 2) as usize, loss, seed, true);
        let mut sent_ab = Vec::new();
        let mut sent_ba = Vec::new();
        for i in 0..3u8 {
            let da = file(150 + 400 * usize::from(i), i);
            let db = file(90 + 300 * usize::from(i), 100 + i);
            sent_ab.push((send(&mut net, a, b, da.clone()), da));
            sent_ba.push((send(&mut net, b, a, db.clone()), db));
        }
        let (mut sa, mut sb) = (Seen::default(), Seen::default());
        let mut restarted = false;
        net.run_until(net.now + 90 * MIN, |n| {
            collect(n, a, &mut sa);
            collect(n, b, &mut sb);
            if !restarted && seed % 5 == 0 && n.now > 15 * MIN {
                restarted = true;
                n.attach_app(b, 77);
                n.connect(b);
            }
            sa.delivered.len() + sa.failed.len() == 3 && sb.delivered.len() + sb.failed.len() == 3
        });
        delivered_total += sa.delivered.len() + sb.delivered.len();
        for (received, sent, delivered) in [
            (&sb.received, &sent_ab, &sa.delivered),
            (&sa.received, &sent_ba, &sb.delivered),
        ] {
            let bytes: Vec<Vec<u8>> = received.iter().map(Content::encode).collect();
            for (id, data) in sent {
                let n = bytes.iter().filter(|r| *r == data).count();
                assert!(n <= 1, "seed {seed}: message received {n} times");
                if delivered.contains(id) {
                    assert_eq!(n, 1, "seed {seed}: delivered but not received");
                }
            }
        }
    }
    assert!(delivered_total >= 144, "{delivered_total}/180 delivered");
    eprintln!("never_delivered_twice: {delivered_total}/180 delivered");
}

/// An advert is one companion command (`SEND_SELF_ADVERT`, flood or not) and costs airtime.
#[test]
fn adverts_go_to_the_companion_and_cost_airtime() {
    let mut c = lc_core::Core::new(lc_core::Config::new("t", [1; 32], [2; 32]));
    let before = c.budget(0).0;
    c.send_advert(true, 0);
    assert_eq!(c.poll_companion(0), Some(vec![7, 1]));
    let after = c.budget(0).0;
    assert!(after > before, "{before} -> {after} ms");
    // The zero-hop variant, once the companion answered the first.
    c.on_companion_frame(&[0], 1);
    c.send_advert(false, 2);
    assert_eq!(c.poll_companion(2), Some(vec![7, 0]));
}

/// Sessions are renewed after their age or byte limit (§4.4): the next message waits for a
/// new handshake, then arrives.
#[test]
fn sessions_renew_after_their_limits() {
    let established = |events: &[Event]| {
        events
            .iter()
            .filter(|e| matches!(e, Event::SessionEstablished { .. }))
            .count()
    };
    for (age, bytes) in [(3 * MIN, u64::MAX), (u64::MAX, 1_000)] {
        let (mut net, a, b) = chain(1, 0, 7, true);
        net.with_core(a, |c, _| c.set_session_limits(age, bytes));
        let mut big = vec![lc_proto::content::FILE];
        big.extend(std::iter::repeat_n(7u8, 1_200));
        let first = send(&mut net, a, b, big);
        net.run_until(net.now + 10 * MIN, |_| false);
        let ea = net.take_events(a);
        assert!(
            ea.iter()
                .any(|e| matches!(e, Event::Delivered { id } if *id == first))
        );
        assert!(established(&ea) >= 1);
        net.run_until(net.now + 4 * MIN, |_| false);
        net.take_events(a);
        let second = send(&mut net, a, b, vec![lc_proto::content::FILE, 1, 2, 3]);
        net.run_until(net.now + 10 * MIN, |_| false);
        let ea = net.take_events(a);
        assert!(
            ea.iter()
                .any(|e| matches!(e, Event::Delivered { id } if *id == second)),
            "{ea:?}"
        );
        assert_eq!(established(&ea), 1, "renewed for ({age}, {bytes}): {ea:?}");
    }
    // Under both limits, the session stays.
    let (mut net, a, b) = chain(1, 0, 7, true);
    send(&mut net, a, b, vec![lc_proto::content::FILE, 1]);
    net.run_until(net.now + 10 * MIN, |_| false);
    net.take_events(a);
    send(&mut net, a, b, vec![lc_proto::content::FILE, 2]);
    net.run_until(net.now + 10 * MIN, |_| false);
    assert_eq!(established(&net.take_events(a)), 0, "no renewal");
}

//! Phone identities over the fake network (protocol.md §2, §4.6, ADR-0013): introduction on
//! first contact, a contact card scanned on one side only, an identity that changes.

use lc_core::fakenet::Net;
use lc_core::{Content, Event};
use lc_meshcore::airtime::LoRaParams;

const S: u64 = 1_000;
const MIN: u64 = 60 * S;

fn pair(seed: u64, loss: u32) -> (Net, usize, usize) {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), loss, seed);
    let a = net.add_companion(1, "alice");
    let b = net.add_companion(2, "bob");
    let hub = net.add_repeater(50, "hub");
    net.link(a, hub);
    net.link(b, hub);
    net.know(a, b, true);
    net.know(b, a, true);
    net.connect(a);
    net.connect(b);
    assert!(net.run_until(20 * S, |n| {
        [a, b]
            .iter()
            .all(|&i| n.nodes[i].core.as_ref().is_some_and(|c| c.ready()))
    }));
    (net, a, b)
}

fn identity(net: &Net, i: usize) -> [u8; 32] {
    *net.nodes[i].core.as_ref().unwrap().key().unwrap()
}

/// (identity, verified) that node `i` holds for node `other`.
fn known(net: &Net, i: usize, other: usize) -> (Option<[u8; 32]>, bool) {
    let node = net.key(other);
    net.nodes[i]
        .core
        .as_ref()
        .unwrap()
        .contacts()
        .find(|c| c.key == node)
        .map_or((None, false), |c| (c.identity, c.verified))
}

fn texts(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Received {
                content: Content::Text { text, .. },
                ..
            } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn learned(events: &[Event]) -> Vec<bool> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::IdentityLearned { changed, .. } => Some(*changed),
            _ => None,
        })
        .collect()
}

/// Strangers: the introduction delivers the message and both identities, unverified; a later
/// session (after a restart) uses HELLO.
#[test]
fn first_contact_introduces_both() {
    let (mut net, a, b) = pair(1, 0);
    let bob = net.key(b);
    assert_eq!(known(&net, a, b), (None, false));
    net.with_core(a, |c, now| c.send_text(&bob, "hello stranger", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 2 * MIN, |_| false);
    let (ea, eb) = (net.take_events(a), net.take_events(b));
    assert_eq!(texts(&eb), vec!["hello stranger"]);
    assert!(ea.iter().any(|e| matches!(e, Event::Delivered { .. })));
    assert_eq!(learned(&ea), vec![false]);
    assert_eq!(learned(&eb), vec![false]);
    assert_eq!(known(&net, a, b), (Some(identity(&net, b)), false));
    assert_eq!(known(&net, b, a), (Some(identity(&net, a)), false));
    // What an introduction costs on air over one hop (2-byte header + 1 path byte + frame).
    let radio = LoRaParams::meshcore(8, 62_500, 5).unwrap();
    for (name, len) in [
        ("INTRO", 7 + 16 + 32 + 1 + 1),
        ("INTRO_ACK", lc_proto::intro::INTRO_ACK_LEN),
        ("INTRO_FIN", lc_proto::intro::INTRO_FIN_LEN),
        ("HELLO", 7 + 16 + 32 + 2 + 1 + 8),
        ("HELLO_ACK", 7 + 16 + 33 + 8),
    ] {
        eprintln!(
            "{name}: {len} bytes, {} ms",
            radio.airtime_us(3 + len) / 1000
        );
    }

    // Alice's app restarts (identity and contacts kept): a HELLO, no new introduction.
    let contacts: Vec<_> = net.nodes[a]
        .core
        .as_ref()
        .unwrap()
        .contacts()
        .cloned()
        .collect();
    net.attach_app(a, 9);
    net.with_core(a, |c, _| {
        for k in contacts {
            c.restore_contact(k);
        }
    });
    net.connect(a);
    net.run_until(net.now + 20 * S, |_| false);
    net.with_core(a, |c, now| c.send_text(&bob, "again", 2, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 2 * MIN, |_| false);
    let (ea, eb) = (net.take_events(a), net.take_events(b));
    assert_eq!(texts(&eb), vec!["again"]);
    assert!(learned(&ea).is_empty() && learned(&eb).is_empty());
}

/// Alice scanned Bob's card, Bob did not scan hers: her HELLO is unreadable for Bob, who
/// introduces himself; the message goes through and Alice's verification stands.
#[test]
fn one_sided_card() {
    let (mut net, a, b) = pair(2, 0);
    let (bob, bob_id) = (net.key(b), identity(&net, b));
    net.with_core(a, |c, _| c.set_identity(&bob, bob_id, true));
    net.with_core(a, |c, now| c.send_text(&bob, "scanned you", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5 * MIN, |_| false);
    let (ea, eb) = (net.take_events(a), net.take_events(b));
    assert_eq!(texts(&eb), vec!["scanned you"]);
    assert!(learned(&ea).is_empty(), "same identity: nothing new");
    assert_eq!(learned(&eb), vec![false]);
    assert_eq!(known(&net, a, b), (Some(bob_id), true));
    assert_eq!(known(&net, b, a), (Some(identity(&net, a)), false));
}

/// Bob reinstalls without his phrase: a new identity. Alice is told it changed and the
/// contact is to verify again; messages flow again.
#[test]
fn changed_identity_is_reported() {
    let (mut net, a, b) = pair(3, 0);
    let bob = net.key(b);
    net.introduce(a, b);
    let old = identity(&net, b);
    net.with_core(a, |c, now| c.send_text(&bob, "before", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 2 * MIN, |_| false);
    assert_eq!(texts(&net.take_events(b)), vec!["before"]);
    net.take_events(a);

    net.reinstall_app(b, 42);
    net.connect(b);
    net.run_until(net.now + 20 * S, |_| false);
    let new = identity(&net, b);
    assert_ne!(new, old);
    net.with_core(a, |c, now| c.send_text(&bob, "after", 2, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 30 * MIN, |_| false);
    let (ea, eb) = (net.take_events(a), net.take_events(b));
    assert_eq!(learned(&ea), vec![true], "{ea:?}");
    assert_eq!(known(&net, a, b), (Some(new), false));
    assert_eq!(texts(&eb), vec!["after"]);
}

/// Strangers on lossy links: introductions retry, and every message arrives once.
#[test]
fn introduction_survives_losses() {
    for seed in 0..10 {
        let (mut net, a, b) = pair(100 + seed, 15);
        let bob = net.key(b);
        for i in 0..3 {
            net.with_core(a, |c, now| c.send_text(&bob, &format!("m{i}"), i, now))
                .unwrap()
                .unwrap();
        }
        net.run_until(net.now + 20 * MIN, |_| false);
        let got = texts(&net.take_events(b));
        assert_eq!(got, vec!["m0", "m1", "m2"], "seed {seed}");
    }
}

//! Phone identities over the fake network (protocol.md §2, §4.6, ADR-0013): introduction on
//! first contact, a contact card scanned on one side only, an identity that changes.

use lc_core::fakenet::Net;
use lc_core::{Content, Event, FailReason};
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
        ("HELLO", 7 + 16 + 32 + 2 + 1 + lc_proto::TAG_LEN),
        ("HELLO_ACK", 7 + 16 + 33 + lc_proto::TAG_LEN),
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

/// INTRO names its sender by one hash byte (§4.6). Bob's companion knows a repeater and another
/// companion sharing Alice's byte, both sorted before her: neither is taken for her (seen in
/// Paris, where a repeater shared the phone's node byte).
#[test]
fn introduction_picks_the_sender_among_hash_collisions() {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, 3);
    let a = net.add_companion(1, "alice");
    let b = net.add_companion(2, "bob");
    let hub = net.add_repeater(50, "hub");
    net.link(a, hub);
    net.link(b, hub);
    net.know(a, b, true);
    net.know(b, a, true);
    let mut repeater = net.key(a);
    repeater[1..].fill(0);
    let mut other = repeater;
    other[31] = 1;
    net.know_other(b, repeater, "repeater", lc_core::contact_kind::REPEATER);
    net.know_other(b, other, "other", lc_core::contact_kind::CHAT);
    net.connect(a);
    net.connect(b);
    assert!(net.run_until(20 * S, |n| {
        [a, b]
            .iter()
            .all(|&i| n.nodes[i].core.as_ref().is_some_and(|c| c.ready()))
    }));
    let bob = net.key(b);
    net.with_core(a, |c, now| c.send_text(&bob, "hello stranger", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 2 * MIN, |_| false);
    let eb = net.take_events(b);
    assert_eq!(texts(&eb), vec!["hello stranger"]);
    assert!(
        eb.iter()
            .any(|e| matches!(e, Event::Received { from, .. } if *from == net.key(a)))
    );
    assert_eq!(known(&net, b, a), (Some(identity(&net, a)), false));
    let core = net.nodes[b].core.as_ref().unwrap();
    for k in [repeater, other] {
        assert_eq!(core.contacts().find(|c| c.key == k).unwrap().identity, None);
    }
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

/// The identity node `i` holds as waiting for the user's decision about node `other`.
fn waiting(net: &Net, i: usize, other: usize) -> Option<[u8; 32]> {
    let node = net.key(other);
    net.nodes[i]
        .core
        .as_ref()
        .unwrap()
        .pending_identity(&node)
        .copied()
}

/// Review of 2026-11-15, SR-01. Alice only met Bob over the air (unverified). Whoever answers
/// for his node with another identity (his app reinstalled without its phrase, or anyone who
/// sends an introduction under his node's hash byte) does not replace him: the identity Alice
/// first saw stays, the new one waits for her, and nothing reaches it or comes from it.
#[test]
fn an_unverified_identity_is_pinned() {
    let (mut net, a, b) = pair(3, 0);
    let (alice, bob) = (net.key(a), net.key(b));
    let old = identity(&net, b);
    net.with_core(a, |c, now| c.send_text(&bob, "before", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 2 * MIN, |_| false);
    assert_eq!(texts(&net.take_events(b)), vec!["before"]);
    net.take_events(a);
    assert_eq!(known(&net, a, b), (Some(old), false));

    net.reinstall_app(b, 42);
    net.connect(b);
    net.run_until(net.now + 20 * S, |_| false);
    let new = identity(&net, b);
    assert_ne!(new, old);
    // The new identity writes first, as an attacker would.
    net.with_core(b, |c, now| {
        c.send_text(&alice, "it's me, new phone", 2, now)
    })
    .unwrap()
    .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    let ea = net.take_events(a);
    assert!(texts(&ea).is_empty(), "{ea:?}");
    assert!(learned(&ea).is_empty(), "{ea:?}");
    assert!(ea.iter().any(|e| matches!(
        e,
        Event::IdentityConflict { identity, .. } if *identity == new
    )));
    assert_eq!(known(&net, a, b), (Some(old), false));
    assert_eq!(waiting(&net, a, b), Some(new));

    // What Alice writes goes to the identity she knows. It no longer answers: the message
    // fails, saying why, and nothing reaches the new one.
    net.take_events(b);
    net.with_core(a, |c, now| c.send_text(&bob, "after", 3, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 30 * MIN, |_| false);
    let (ea, eb) = (net.take_events(a), net.take_events(b));
    assert!(texts(&eb).is_empty(), "{eb:?}");
    assert!(
        ea.iter().any(|e| matches!(
            e,
            Event::Failed {
                reason: FailReason::IdentityChanged,
                ..
            }
        )),
        "{ea:?}"
    );
    assert_eq!(known(&net, a, b), (Some(old), false));

    // She accepts it: unverified, and messages flow.
    assert!(
        net.with_core(a, |c, _| c.accept_identity(&bob, &new, false))
            .unwrap()
    );
    assert_eq!(known(&net, a, b), (Some(new), false));
    net.with_core(a, |c, now| c.send_text(&bob, "hello again", 4, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    assert_eq!(texts(&net.take_events(b)), vec!["hello again"]);
}

/// SR-01, the other half: an identity that waits must not cut Alice off from the one she
/// knows. Someone presents another identity for Bob's node while Bob is still there: his
/// messages and hers keep flowing, and the other identity waits until she rejects it.
#[test]
fn a_waiting_identity_does_not_block_the_known_one() {
    let (mut net, a, b) = pair(6, 0);
    let (alice, bob) = (net.key(a), net.key(b));
    net.introduce(a, b);
    let old = identity(&net, b);
    let bobs_contacts: Vec<_> = net.nodes[b]
        .core
        .as_ref()
        .unwrap()
        .contacts()
        .cloned()
        .collect();
    // Another app answers for Bob's node and writes to Alice.
    net.reinstall_app(b, 44);
    net.connect(b);
    net.run_until(net.now + 20 * S, |_| false);
    let other = identity(&net, b);
    net.with_core(b, |c, now| {
        c.send_text(&alice, "new number, who dis", 1, now)
    })
    .unwrap()
    .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    assert!(texts(&net.take_events(a)).is_empty());
    assert_eq!(waiting(&net, a, b), Some(other));

    // Bob's own app is on his radio again.
    net.attach_app(b, 2);
    net.with_core(b, |c, _| {
        for k in bobs_contacts {
            c.restore_contact(k);
        }
    });
    net.connect(b);
    net.run_until(net.now + 20 * S, |_| false);
    assert_eq!(identity(&net, b), old);
    net.take_events(b);
    net.with_core(a, |c, now| c.send_text(&bob, "still you?", 2, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    let (ea, eb) = (net.take_events(a), net.take_events(b));
    assert_eq!(texts(&eb), vec!["still you?"], "{ea:?}");
    assert!(ea.iter().any(|e| matches!(e, Event::Delivered { .. })));
    net.with_core(b, |c, now| c.send_text(&alice, "always", 3, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5 * MIN, |_| false);
    assert_eq!(texts(&net.take_events(a)), vec!["always"]);
    assert_eq!(known(&net, a, b), (Some(old), true));
    assert_eq!(waiting(&net, a, b), Some(other));
    net.with_core(a, |c, _| c.reject_identity(&bob, &other));
    assert_eq!(waiting(&net, a, b), None);
}

/// Review of 2026-11-06, PR-03: Bob was verified in person; whoever answers for his node now
/// with another identity (his reinstalled app, or an attacker) gets nothing until Alice
/// accepts it.
#[test]
fn verified_identity_is_not_replaced_without_consent() {
    let (mut net, a, b) = pair(4, 0);
    let bob = net.key(b);
    net.introduce(a, b);
    let old = identity(&net, b);
    net.reinstall_app(b, 43);
    net.connect(b);
    net.run_until(net.now + 20 * S, |_| false);
    let new = identity(&net, b);
    net.with_core(a, |c, now| c.send_text(&bob, "secret", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 30 * MIN, |_| false);
    let (ea, eb) = (net.take_events(a), net.take_events(b));
    assert!(texts(&eb).is_empty(), "nothing reached the new identity");
    assert!(ea.iter().any(|e| matches!(
        e,
        Event::IdentityConflict { identity, .. } if *identity == new
    )));
    assert!(
        ea.iter().any(|e| matches!(e, Event::Failed { .. })),
        "{ea:?}"
    );
    assert_eq!(known(&net, a, b), (Some(old), true));
    assert_eq!(
        net.nodes[a].core.as_ref().unwrap().pending_identity(&bob),
        Some(&new)
    );
    // While it waits, a new message still goes to the identity Alice knows (SR-01); nothing
    // answers for it, and the failure says why.
    net.with_core(a, |c, now| c.send_text(&bob, "still secret", 2, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 30 * MIN, |_| false);
    let ea = net.take_events(a);
    assert!(
        ea.iter().any(|e| matches!(
            e,
            Event::Failed {
                reason: FailReason::IdentityChanged,
                ..
            }
        )),
        "{ea:?}"
    );
    assert!(texts(&net.take_events(b)).is_empty());

    // Alice compares numbers with Bob and accepts: the new identity, unverified, and messages
    // flow.
    assert!(
        net.with_core(a, |c, _| c.accept_identity(&bob, &new, false))
            .unwrap()
    );
    assert_eq!(known(&net, a, b), (Some(new), false));
    net.with_core(a, |c, now| c.send_text(&bob, "hello again", 2, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    assert_eq!(texts(&net.take_events(b)), vec!["hello again"]);
}

/// Review PR-07: Alice compares the numbers of one new identity, but another arrives before
/// she approves. Her approval names the one she compared: refused, nothing adopted, nothing
/// sent; she must compare the new one.
#[test]
fn stale_approval_is_refused() {
    let (mut net, a, b) = pair(5, 0);
    let bob = net.key(b);
    net.introduce(a, b);
    let old = identity(&net, b);
    let reinstall = |net: &mut Net, seed: u8| {
        net.reinstall_app(b, seed);
        net.connect(b);
        net.run_until(net.now + 20 * S, |_| false);
    };
    // A first new identity, pending: Alice opens its numbers.
    reinstall(&mut net, 44);
    let shown = identity(&net, b);
    net.with_core(a, |c, now| c.send_text(&bob, "first", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 30 * MIN, |_| false);
    assert_eq!(
        net.nodes[a].core.as_ref().unwrap().pending_identity(&bob),
        Some(&shown)
    );
    // Before she taps "match", a second introduction replaces the pending identity.
    reinstall(&mut net, 46);
    let replacement = identity(&net, b);
    assert_ne!(replacement, shown);
    let alice = net.key(a);
    net.with_core(b, |c, now| {
        c.send_text(&alice, "second introduction", 3, now)
    })
    .unwrap()
    .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    assert_eq!(
        net.nodes[a].core.as_ref().unwrap().pending_identity(&bob),
        Some(&replacement)
    );
    net.take_events(a);
    net.take_events(b);
    // Her approval of what she saw is refused; verifying the old node without it too.
    assert!(
        !net.with_core(a, |c, _| c.accept_identity(&bob, &shown, true))
            .unwrap()
    );
    assert!(
        !net.with_core(a, |c, _| c.set_verified(&bob, &replacement, true))
            .unwrap()
    );
    assert_eq!(known(&net, a, b), (Some(old), true), "nothing adopted");
    // Rejecting what she saw leaves the newer one pending: she has not seen it.
    net.with_core(a, |c, _| c.reject_identity(&bob, &shown));
    assert_eq!(
        net.nodes[a].core.as_ref().unwrap().pending_identity(&bob),
        Some(&replacement)
    );
    net.with_core(a, |c, now| c.send_text(&bob, "still held", 4, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    assert!(texts(&net.take_events(b)).is_empty(), "nothing released");
    // She compares the new numbers and approves them: adopted, verified, and messages flow.
    assert!(
        net.with_core(a, |c, _| c.accept_identity(&bob, &replacement, true))
            .unwrap()
    );
    assert_eq!(known(&net, a, b), (Some(replacement), true));
    net.with_core(a, |c, now| c.send_text(&bob, "hello again", 5, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    assert_eq!(texts(&net.take_events(b)), vec!["hello again"]);
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

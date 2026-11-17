//! Introduction requests over the fake network (protocol.md §4.6, ADR-0046; second read of
//! 2026-11-17, N3): an introduction we did not start does not pin the identity it carries.
//!
//! The simulation keeps node hash bytes unique, so "someone using Bob's radio" is another app
//! on Bob's radio, as in `identity.rs`: on the air it is the same as any radio sending under
//! his node's hash byte.

use lc_core::fakenet::Net;
use lc_core::{Content, Event};
use lc_meshcore::airtime::LoRaParams;

const S: u64 = 1_000;
const MIN: u64 = 60 * S;

fn pair(seed: u64) -> (Net, usize, usize) {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, seed);
    let a = net.add_companion(1, "alice");
    let b = net.add_companion(2, "bob");
    let hub = net.add_repeater(50, "hub");
    net.link(a, hub);
    net.link(b, hub);
    net.know(a, b, true);
    net.know(b, a, true);
    // Alice is a person with the app: strangers' introductions are requests.
    net.with_core(a, |c, _| c.set_request_consent(true));
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

/// The identity node `i` holds for node `other`.
fn known(net: &Net, i: usize, other: usize) -> Option<[u8; 32]> {
    let node = net.key(other);
    net.nodes[i]
        .core
        .as_ref()
        .unwrap()
        .contacts()
        .find(|c| c.key == node)
        .and_then(|c| c.identity)
}

/// The requests node `i` holds: (identity, node), in identity order.
fn requests(net: &Net, i: usize) -> Vec<([u8; 32], [u8; 32])> {
    net.nodes[i]
        .core
        .as_ref()
        .unwrap()
        .requests()
        .map(|(i, n)| (*i, *n))
        .collect()
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

/// Texts that came from request `who`.
fn request_texts(events: &[Event], who: [u8; 32]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::RequestMessage {
                identity,
                content: Content::Text { text, .. },
                ..
            } if *identity == who => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn requested(events: &[Event]) -> Vec<[u8; 32]> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Requested { identity, .. } => Some(*identity),
            _ => None,
        })
        .collect()
}

/// N3: Eve writes first under Bob's radio, before Bob ever wrote to Alice. Her identity is
/// not pinned on Bob's contact: it is a request, her message kept apart. When Bob writes, his
/// introduction is not an "identity change" against Eve but a second request; Alice chooses
/// Bob, writes to him over the session he opened, and blocks Eve's identity without blocking
/// Bob's radio. (Before the fix, Eve's identity became Bob's and Bob's showed as a conflict.)
#[test]
fn a_squatter_is_a_request_and_so_is_the_real_person() {
    let (mut net, a, b) = pair(11);
    let (alice, bob) = (net.key(a), net.key(b));
    let bobs_id = identity(&net, b);
    let bobs_contacts: Vec<_> = net.nodes[b]
        .core
        .as_ref()
        .unwrap()
        .contacts()
        .cloned()
        .collect();

    // Eve, under Bob's node.
    net.reinstall_app(b, 66);
    net.connect(b);
    net.run_until(net.now + 20 * S, |_| false);
    let eve = identity(&net, b);
    assert_ne!(eve, bobs_id);
    net.with_core(b, |c, now| c.send_text(&alice, "hi, it's Bob", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5 * MIN, |_| false);
    let ea = net.take_events(a);
    assert!(
        texts(&ea).is_empty(),
        "nothing in Bob's conversation: {ea:?}"
    );
    assert_eq!(requested(&ea), vec![eve]);
    assert_eq!(request_texts(&ea, eve), vec!["hi, it's Bob"]);
    assert!(!ea.iter().any(|e| matches!(
        e,
        Event::IdentityLearned { .. } | Event::IdentityConflict { .. }
    )));
    assert_eq!(known(&net, a, b), None, "not pinned");
    assert_eq!(requests(&net, a), vec![(eve, bob)]);
    assert!(
        net.take_events(b)
            .iter()
            .any(|e| matches!(e, Event::Delivered { .. })),
        "Eve's message was delivered (to the request)"
    );

    // Bob's own app is on his radio again, and he writes.
    net.attach_app(b, 2);
    net.with_core(b, |c, _| {
        for k in bobs_contacts {
            c.restore_contact(k);
        }
    });
    net.connect(b);
    net.run_until(net.now + 20 * S, |_| false);
    assert_eq!(identity(&net, b), bobs_id);
    net.with_core(b, |c, now| c.send_text(&alice, "it's really me", 2, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5 * MIN, |_| false);
    let ea = net.take_events(a);
    assert!(texts(&ea).is_empty(), "{ea:?}");
    assert_eq!(requested(&ea), vec![bobs_id], "a second request");
    assert_eq!(request_texts(&ea, bobs_id), vec!["it's really me"]);
    assert!(
        !ea.iter()
            .any(|e| matches!(e, Event::IdentityConflict { .. })),
        "not an identity change: {ea:?}"
    );
    let mut both = vec![(eve, bob), (bobs_id, bob)];
    both.sort();
    assert_eq!(requests(&net, a), both);
    assert_eq!(known(&net, a, b), None);

    // Alice chooses Bob: pinned, unverified; the session he opened carries her answer.
    assert_eq!(
        net.with_core(a, |c, _| c.accept_request(&bobs_id)).unwrap(),
        Some(bob)
    );
    net.run_until(net.now + S, |_| false);
    let ea = net.take_events(a);
    assert!(ea.iter().any(|e| matches!(
        e,
        Event::RequestAccepted { identity, node } if *identity == bobs_id && *node == bob
    )));
    assert!(ea.iter().any(|e| matches!(
        e,
        Event::IdentityLearned { identity, changed: false, .. } if *identity == bobs_id
    )));
    assert_eq!(known(&net, a, b), Some(bobs_id));
    let verified = net.nodes[a]
        .core
        .as_ref()
        .unwrap()
        .contacts()
        .find(|c| c.key == bob)
        .unwrap()
        .verified;
    assert!(!verified);
    assert_eq!(requests(&net, a), vec![(eve, bob)], "Eve's still waits");
    net.take_events(b);
    net.with_core(a, |c, now| c.send_text(&bob, "hello Bob", 3, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5 * MIN, |_| false);
    assert_eq!(texts(&net.take_events(b)), vec!["hello Bob"]);
    net.with_core(b, |c, now| c.send_text(&alice, "got it", 4, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5 * MIN, |_| false);
    assert_eq!(texts(&net.take_events(a)), vec!["got it"]);

    // She blocks Eve's identity: Bob's radio is not blocked.
    net.with_core(a, |c, _| c.block_request(&eve));
    assert!(requests(&net, a).is_empty());
    assert!(!net.nodes[a].core.as_ref().unwrap().is_blocked(&bob));
    net.with_core(a, |c, now| c.send_text(&bob, "still there?", 5, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5 * MIN, |_| false);
    assert_eq!(texts(&net.take_events(b)), vec!["still there?"]);
}

/// N3 by another door: Bob scanned Alice's card, she knows nothing of him. His HELLO is
/// unreadable to her, so her app introduces itself to his node; whoever answers that
/// introduction is a request too, since Alice had nothing to send. Once she writes to him
/// herself, the introduction is hers again (the user chose to write).
#[test]
fn an_introduction_a_hello_prompted_is_a_request() {
    let (mut net, a, b) = pair(12);
    let (alice, bob) = (net.key(a), net.key(b));
    let (alice_id, bobs_id) = (identity(&net, a), identity(&net, b));
    net.with_core(b, |c, _| c.set_identity(&alice, alice_id, true));
    net.with_core(b, |c, now| c.send_text(&alice, "scanned your card", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5 * MIN, |_| false);
    let ea = net.take_events(a);
    assert!(texts(&ea).is_empty(), "{ea:?}");
    assert_eq!(requested(&ea), vec![bobs_id]);
    assert_eq!(request_texts(&ea, bobs_id), vec!["scanned your card"]);
    assert_eq!(known(&net, a, b), None);

    // She accepts: the request's message belongs to Bob's conversation now.
    assert_eq!(
        net.with_core(a, |c, _| c.accept_request(&bobs_id)).unwrap(),
        Some(bob)
    );
    assert_eq!(known(&net, a, b), Some(bobs_id));
    net.with_core(a, |c, now| c.send_text(&bob, "welcome", 2, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5 * MIN, |_| false);
    assert_eq!(texts(&net.take_events(b)), vec!["welcome"]);
}

/// Alice wrote to Bob's node herself: her introduction is hers, whoever answers it is pinned
/// as before (a request is only what we did not start), and a request already there for the
/// same identity is accepted with it.
#[test]
fn an_introduction_we_started_resolves_the_request() {
    let (mut net, a, b) = pair(13);
    let (alice, bob) = (net.key(a), net.key(b));
    let bobs_id = identity(&net, b);
    net.with_core(b, |c, now| c.send_text(&alice, "first", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5 * MIN, |_| false);
    assert_eq!(requests(&net, a), vec![(bobs_id, bob)]);
    net.take_events(a);

    // Alice, without accepting, writes to Bob: her own introduction goes out.
    net.with_core(a, |c, now| c.send_text(&bob, "who's this?", 2, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    let (ea, eb) = (net.take_events(a), net.take_events(b));
    assert_eq!(texts(&eb), vec!["who's this?"], "{ea:?}");
    assert!(ea.iter().any(|e| matches!(e, Event::Delivered { .. })));
    assert!(ea.iter().any(|e| matches!(
        e,
        Event::RequestAccepted { identity, .. } if *identity == bobs_id
    )));
    assert_eq!(known(&net, a, b), Some(bobs_id));
    assert!(requests(&net, a).is_empty());
}

/// A request survives a restart of Alice's app (the app keeps it, ADR-0046): the requester's
/// next session starts by HELLO, which Alice still answers, and what comes is still a
/// request's. Deleted, it is forgotten: a later introduction is a new request.
#[test]
fn a_request_is_answered_again_after_a_restart() {
    let (mut net, a, b) = pair(14);
    let (alice, bob) = (net.key(a), net.key(b));
    let bobs_id = identity(&net, b);
    net.with_core(b, |c, now| c.send_text(&alice, "one", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5 * MIN, |_| false);
    assert_eq!(request_texts(&net.take_events(a), bobs_id), vec!["one"]);

    // Alice's app restarts: contacts and requests restored, sessions lost.
    let contacts: Vec<_> = net.nodes[a]
        .core
        .as_ref()
        .unwrap()
        .contacts()
        .cloned()
        .collect();
    let kept = requests(&net, a);
    net.attach_app(a, 9);
    net.with_core(a, |c, _| {
        c.set_request_consent(true);
        for k in contacts {
            c.restore_contact(k);
        }
        for (i, n) in kept {
            c.restore_request(n, i);
        }
    });
    net.connect(a);
    net.run_until(net.now + 20 * S, |_| false);
    net.with_core(b, |c, now| c.send_text(&alice, "two", 2, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    let ea = net.take_events(a);
    assert_eq!(request_texts(&ea, bobs_id), vec!["two"], "{ea:?}");
    assert!(requested(&ea).is_empty(), "the same request");
    assert!(texts(&ea).is_empty());
    assert_eq!(known(&net, a, b), None);

    // Deleted: forgotten. Bob's next message needs a new introduction: a new request.
    assert!(net.with_core(a, |c, _| c.forget_request(&bobs_id)).unwrap());
    assert!(requests(&net, a).is_empty());
    net.run_until(net.now + 30 * MIN, |_| false);
    net.take_events(b);
    net.with_core(b, |c, now| c.send_text(&alice, "three", 3, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 60 * MIN, |_| false);
    let ea = net.take_events(a);
    assert_eq!(requested(&ea), vec![bobs_id], "{ea:?}");
    assert_eq!(request_texts(&ea, bobs_id), vec!["three"]);
    assert_eq!(requests(&net, a), vec![(bobs_id, bob)]);
}

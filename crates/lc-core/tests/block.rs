//! Blocking over the fake network (ADR-0034): what a blocked person sends no longer reaches
//! the app, lichen or MeshCore, direct or in a group, under their node or their identity; and
//! nothing tells them.

use lc_core::fakenet::Net;
use lc_core::{Content, Error, Event, FailReason, GroupId};
use lc_meshcore::airtime::LoRaParams;

const S: u64 = 1_000;
const MIN: u64 = 60 * S;

/// alice, bob, carol (lichen), marc (a stock MeshCore app) and dave (a radio whose app is not
/// running yet) around one repeater. alice's radio knows them all, bob's and carol's know
/// each other and alice.
fn net(seed: u64) -> (Net, [usize; 5]) {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, seed);
    let a = net.add_companion(1, "alice");
    let b = net.add_companion(2, "bob");
    let c = net.add_companion(3, "carol");
    let m = net.add_stock_companion(4, "Marc");
    let d = net.add_companion(5, "dave");
    let hub = net.add_repeater(50, "hub");
    for i in [a, b, c, m, d] {
        net.link(i, hub);
    }
    for (x, y) in [(a, b), (a, c), (a, m), (a, d), (b, c)] {
        net.know(x, y, true);
        net.know(y, x, true);
    }
    for i in [a, b, c] {
        net.connect(i);
    }
    assert!(net.run_until(20 * S, |n| {
        [a, b, c]
            .iter()
            .all(|&i| n.nodes[i].core.as_ref().is_some_and(|c| c.ready()))
    }));
    (net, [a, b, c, m, d])
}

fn identity(net: &Net, i: usize) -> [u8; 32] {
    *net.nodes[i].core.as_ref().unwrap().key().unwrap()
}

fn texts(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Received {
                content: Content::Text { text, .. },
                ..
            } => Some(text.clone()),
            Event::GroupMessage {
                content: Content::Text { text, .. },
                ..
            } => Some(text.clone()),
            Event::PlainReceived(m) => Some(m.text.clone()),
            _ => None,
        })
        .collect()
}

/// Sends `text` from `from` to `to` and runs for three minutes: what `to` received, and how it
/// ended for the sender.
fn say(net: &mut Net, from: usize, to: usize, text: &str) -> (Vec<String>, Vec<Event>) {
    let key = net.key(to);
    net.with_core(from, |c, now| c.send_text(&key, text, 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 3 * MIN, |_| false);
    (texts(&net.take_events(to)), net.take_events(from))
}

/// Direct messages, lichen and MeshCore, stop at the block and come back with the unblock;
/// the blocked sender's messages fail, ours to them are refused.
#[test]
fn a_blocked_contact_reaches_nothing() {
    let (mut net, [a, b, _, m, _]) = net(1);
    net.introduce(a, b);
    let bob = net.key(b);
    assert_eq!(say(&mut net, b, a, "before").0, ["before"]);

    let blocked = net.with_core(a, |c, now| c.block(&bob, now)).unwrap();
    assert_eq!(blocked, Some(identity(&net, b)));
    assert!(net.with_core_ref(a, |c| c.is_blocked(&bob)).unwrap());
    let (got, sender) = say(&mut net, b, a, "after");
    assert!(got.is_empty(), "{got:?}");
    assert!(
        sender.iter().any(|e| matches!(e, Event::Failed { .. })),
        "{sender:?}"
    );
    assert!(!sender.iter().any(|e| matches!(e, Event::Delivered { .. })));
    assert_eq!(
        net.with_core(a, |c, now| c.send_text(&bob, "to bob", 2, now))
            .unwrap(),
        Err(Error::Blocked)
    );

    // Marc's plain messages too, by his node.
    let marc = net.key(m);
    net.with_core(a, |c, now| c.block(&marc, now));
    net.send_plain_text(m, a, "Salut ?", 1_793_000_000);
    net.run_until(net.now + MIN, |_| false);
    let ev = net.take_events(a);
    assert!(
        !ev.iter().any(|e| matches!(e, Event::PlainReceived(_))),
        "{ev:?}"
    );
    assert_eq!(
        net.with_core(a, |c, now| c.send_plain(&marc, "?", 2, now))
            .unwrap(),
        Err(Error::Blocked)
    );

    // Unblocked: Bob reaches Alice again (a new session).
    net.with_core(a, |c, _| c.unblock(&bob, blocked.as_ref()));
    assert!(!net.with_core_ref(a, |c| c.is_blocked(&bob)).unwrap());
    assert_eq!(say(&mut net, b, a, "again").0, ["again"]);
    net.with_core(a, |c, _| c.unblock(&marc, None));
    net.send_plain_text(m, a, "Toujours là ?", 1_793_000_100);
    let mut got = Vec::new();
    assert!(net.run_until(net.now + MIN, |n| {
        got.extend(texts(&n.take_events(a)));
        !got.is_empty()
    }));
    assert_eq!(got, ["Toujours là ?"]);
}

/// A message to a blocked contact that was waiting fails when they are blocked.
#[test]
fn waiting_messages_fail_at_the_block() {
    let (mut net, [a, b, ..]) = net(2);
    net.introduce(a, b);
    let bob = net.key(b);
    // Bob's radio is away: Alice's message waits.
    net.disconnect(b);
    net.unlink_all(b);
    let id = net
        .with_core(a, |c, now| c.send_text(&bob, "waiting", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 5 * S, |_| false);
    net.take_events(a);
    net.with_core(a, |c, now| c.block(&bob, now));
    net.run_until(net.now + S, |_| false);
    let ev = net.take_events(a);
    assert!(
        ev.contains(&Event::Failed {
            id,
            reason: FailReason::Refused
        }),
        "{ev:?}"
    );
}

/// Bob, blocked, moves his phone to another radio: the introduction under his identity from
/// that node is dropped, and nothing of his reaches Alice.
#[test]
fn a_blocked_identity_behind_another_node() {
    let (mut net, [a, b, _, _, d]) = net(3);
    net.introduce(a, b);
    let bob = net.key(b);
    net.with_core(a, |c, now| c.block(&bob, now));
    net.move_app(b, d);
    net.connect(d);
    net.run_until(net.now + 20 * S, |_| false);
    let alice = net.key(a);
    net.with_core(d, |c, now| c.send_text(&alice, "it's me, from Dave's radio", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 10 * MIN, |_| false);
    let ev = net.take_events(a);
    assert!(texts(&ev).is_empty(), "{ev:?}");
    let dave = net.key(d);
    let known = net
        .with_core_ref(a, |c| c.contacts().find(|k| k.key == dave).and_then(|k| k.identity))
        .unwrap();
    assert_eq!(known, None, "the blocked identity is not attached to Dave's node");
    assert!(
        !net.take_events(d)
            .iter()
            .any(|e| matches!(e, Event::Delivered { .. }))
    );
}

/// In a group, a blocked member's posts are not reported; the others' are, and the blocked
/// member's still reach everyone else.
#[test]
fn a_blocked_members_group_posts_are_not_shown() {
    let (mut net, [a, b, c, ..]) = net(4);
    // Carol, the admin, met both in person.
    net.introduce(c, a);
    net.introduce(c, b);
    let members = [net.key(a), net.key(b)];
    let gid: GroupId = net
        .with_core(c, |core, now| core.create_group("Col", &members, now))
        .unwrap()
        .unwrap();
    let keys_known = |n: &mut Net| {
        [a, b, c].iter().all(|&i| {
            n.nodes[i].core.as_ref().is_some_and(|core| {
                core.groups()
                    .find(|g| g.gid == gid)
                    .is_some_and(|g| g.keys_known() == g.members.len() - 1)
            })
        })
    };
    assert!(net.run_until(net.now + 60 * MIN, keys_known));
    net.run_until(net.now + 5 * MIN, |_| false);
    for i in [a, b, c] {
        net.take_events(i);
    }
    let bob = net.key(b);
    net.with_core(a, |core, now| core.block(&bob, now));

    let post = |net: &mut Net, i: usize, text: &str| {
        net.with_core(i, |core, now| core.send_group_text(&gid, text, 1_793_900_000, now))
            .unwrap()
            .unwrap();
        net.run_until(net.now + 2 * MIN, |_| false);
    };
    post(&mut net, b, "from bob");
    post(&mut net, c, "from carol");
    let alice = texts(&net.take_events(a));
    assert_eq!(alice, ["from carol"]);
    let carol = texts(&net.take_events(c));
    assert_eq!(carol, ["from bob"]);
}

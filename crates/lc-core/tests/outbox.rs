//! The outbox over the fake network (protocol.md §6.1, ADR-0018): messages wait for the radio,
//! a transfer survives a disconnection, cancel and expiry, group messages counted as sent
//! only once they left, another radio after a reconnection.

use lc_core::fakenet::Net;
use lc_core::{Content, Error, Event, FailReason, GroupId, MsgId, Stage};
use lc_meshcore::airtime::LoRaParams;

const MIN: u64 = 60_000;

fn radio() -> LoRaParams {
    LoRaParams::meshcore(8, 62_500, 5).unwrap()
}

/// `a` – repeater – `b`, with paths and identities known; `a` connected only if `connect_a`.
fn pair(seed: u64, connect_a: bool) -> (Net, usize, usize, usize) {
    let mut net = Net::new(radio(), 0, seed);
    let a = net.add_companion(1, "alice");
    let r = net.add_repeater(10, "rpt");
    let b = net.add_companion(2, "bob");
    net.chain(&[a, r, b]);
    net.know(a, b, true);
    net.know(b, a, true);
    net.introduce(a, b);
    net.connect(b);
    if connect_a {
        net.connect(a);
    }
    net.run_until(net.now + 10_000, |_| false);
    (net, a, b, r)
}

fn text(t: &str) -> Content {
    Content::Text {
        sent_at: 1_793_000_000,
        text: t.into(),
    }
}

fn send(net: &mut Net, from: usize, to: usize, content: &Content) -> MsgId {
    let key = net.key(to);
    let datagram = content.encode();
    net.with_core(from, |c, now| c.send_datagram(&key, datagram, now))
        .unwrap()
        .unwrap()
}

fn stage(net: &Net, i: usize, id: MsgId) -> Option<(Stage, bool)> {
    let now = net.now;
    net.with_core_ref(i, |c| c.outgoing(id, now))
        .flatten()
        .map(|o| (o.stage, o.started))
}

fn received(events: &[Event]) -> Vec<Content> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Received { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

/// A long text: several fragments, so that the transfer can be cut in the middle.
fn long_text() -> Content {
    text(&"Refuge de la Pra, on monte demain par le sentier nord. ".repeat(20))
}

#[test]
fn waits_for_the_radio_then_goes() {
    let (mut net, a, b, _) = pair(1, false);
    let m = text("written before the radio was on");
    let id = send(&mut net, a, b, &m);
    assert_eq!(stage(&net, a, id), Some((Stage::Offline, false)));
    net.run_until(net.now + 10 * MIN, |_| false);
    assert_eq!(
        net.nodes[a].tx_frames, 0,
        "nothing goes out without the radio"
    );
    assert_eq!(stage(&net, a, id), Some((Stage::Offline, false)));

    net.connect(a);
    let mut ea = Vec::new();
    assert!(net.run_until(net.now + 3 * MIN, |n| {
        ea.extend(n.take_events(a));
        ea.contains(&Event::Delivered { id })
    }));
    assert_eq!(stage(&net, a, id), None);
    assert_eq!(received(&net.take_events(b)), vec![m]);
}

#[test]
fn a_transfer_resumes_after_a_disconnection() {
    let (mut net, a, b, _) = pair(2, true);
    let m = long_text();
    let id = send(&mut net, a, b, &m);
    // Cut the radio after a few fragments.
    assert!(net.run_until(net.now + 2 * MIN, |n| {
        n.take_events(a)
            .iter()
            .any(|e| matches!(e, Event::Progress { sent, .. } if *sent >= 3))
    }));
    net.disconnect(a);
    assert_eq!(stage(&net, a, id), Some((Stage::Offline, true)));
    assert_eq!(
        net.with_core(a, |c, _| c.cancel(id)).unwrap(),
        Err(Error::Started),
        "a message that started cannot be cancelled"
    );
    // Long enough for every timer and retry counter to run out, if they ran.
    net.run_until(net.now + 30 * MIN, |_| false);
    let frames = net.nodes[a].tx_frames;
    let ea = net.take_events(a);
    assert!(
        !ea.iter().any(|e| matches!(e, Event::Failed { .. })),
        "nothing fails while the radio is away: {ea:?}"
    );
    assert_eq!(stage(&net, a, id), Some((Stage::Offline, true)));

    net.connect(a);
    let mut ea = Vec::new();
    assert!(
        net.run_until(net.now + 10 * MIN, |n| {
            ea.extend(n.take_events(a));
            ea.contains(&Event::Delivered { id })
        }),
        "{ea:?}"
    );
    assert!(net.nodes[a].tx_frames > frames);
    assert_eq!(received(&net.take_events(b)), vec![m], "delivered once");
}

#[test]
fn cancel_before_anything_went_out() {
    let (mut net, a, b, _) = pair(3, false);
    let id = send(&mut net, a, b, &text("never mind"));
    let kept = send(&mut net, a, b, &text("this one goes"));
    assert_eq!(net.with_core(a, |c, _| c.cancel(id)).unwrap(), Ok(()));
    assert_eq!(stage(&net, a, id), None);
    assert_eq!(
        net.with_core(a, |c, _| c.cancel(id)).unwrap(),
        Err(Error::NotQueued)
    );
    net.connect(a);
    let mut ea = Vec::new();
    assert!(net.run_until(net.now + 3 * MIN, |n| {
        ea.extend(n.take_events(a));
        ea.contains(&Event::Delivered { id: kept })
    }));
    net.run_until(net.now + 2 * MIN, |_| false);
    assert_eq!(
        received(&net.take_events(b)),
        vec![text("this one goes")],
        "the cancelled message never left"
    );
}

#[test]
fn stages_of_a_first_message() {
    // No path and no identity yet: path discovery, then the introduction (§4.5, §4.6).
    let mut net = Net::new(radio(), 0, 4);
    let a = net.add_companion(1, "alice");
    let r = net.add_repeater(10, "rpt");
    let b = net.add_companion(2, "bob");
    net.chain(&[a, r, b]);
    net.know(a, b, false);
    net.know(b, a, false);
    net.connect(a);
    net.connect(b);
    net.run_until(net.now + 10_000, |_| false);
    let id = send(&mut net, a, b, &long_text());
    let mut seen: Vec<Stage> = Vec::new();
    let mut ea = Vec::new();
    assert!(net.run_until(net.now + 10 * MIN, |n| {
        if let Some((s, _)) = stage(n, a, id)
            && seen.last() != Some(&s)
        {
            seen.push(s);
        }
        ea.extend(n.take_events(a));
        ea.contains(&Event::Delivered { id })
    }));
    let at = |s: Stage| seen.iter().position(|x| *x == s);
    let (Some(p), Some(c), Some(t)) = (
        at(Stage::FindingPath),
        at(Stage::Connecting),
        at(Stage::Transmitting),
    ) else {
        panic!("stages seen: {seen:?}");
    };
    assert!(p < c && c < t, "stages seen: {seen:?}");
    assert_eq!(stage(&net, a, id), None);
}

#[test]
fn expiry_abandons_a_transfer() {
    let (mut net, a, b, _) = pair(5, true);
    let id = send(&mut net, a, b, &long_text());
    assert!(net.run_until(net.now + 2 * MIN, |n| {
        n.take_events(a)
            .iter()
            .any(|e| matches!(e, Event::Progress { sent, .. } if *sent >= 2))
    }));
    net.disconnect(a);
    // The expiry passed while the radio was away: the app abandons it before reconnecting.
    assert_eq!(net.with_core(a, |c, _| c.abandon(id)), Some(true));
    net.run_until(net.now + 10, |_| false);
    assert!(net.take_events(a).contains(&Event::Failed {
        id,
        reason: FailReason::Expired
    }));
    assert_eq!(stage(&net, a, id), None);
    assert_eq!(net.with_core(a, |c, _| c.abandon(id)), Some(false));

    // The session still works for the next message, and the abandoned one never completes.
    net.connect(a);
    let next = send(&mut net, a, b, &text("after"));
    let mut ea = Vec::new();
    assert!(net.run_until(net.now + 5 * MIN, |n| {
        ea.extend(n.take_events(a));
        ea.contains(&Event::Delivered { id: next })
    }));
    assert_eq!(received(&net.take_events(b)), vec![text("after")]);
}

#[test]
fn another_radio_after_reconnecting() {
    let (mut net, a, b, r) = pair(6, true);
    let id = send(&mut net, a, b, &text("from the first radio"));
    let mut ea = Vec::new();
    assert!(net.run_until(net.now + 3 * MIN, |n| {
        ea.extend(n.take_events(a));
        ea.contains(&Event::Delivered { id })
    }));
    // The phone now pairs with another radio: same app, same identity, another node.
    let a2 = net.add_companion(3, "alice-2");
    net.link(a2, r);
    net.move_app(a, a2);
    net.know(a2, b, true);
    net.know(b, a2, true);
    net.introduce(a2, b);
    let id = send(&mut net, a2, b, &text("from the second radio"));
    assert_eq!(stage(&net, a2, id), Some((Stage::Offline, false)));
    net.connect(a2);
    let mut ea = Vec::new();
    assert!(
        net.run_until(net.now + 3 * MIN, |n| {
            ea.extend(n.take_events(a2));
            ea.contains(&Event::Delivered { id })
        }),
        "{ea:?}"
    );
    let got = received(&net.take_events(b));
    assert_eq!(
        got,
        vec![text("from the first radio"), text("from the second radio")]
    );
}

/// Three companions around a hub; alice (admin) created a group with the two others and the
/// keys are out.
fn group() -> (Net, Vec<usize>, GroupId) {
    let mut net = Net::new(radio(), 0, 7);
    let m: Vec<usize> = ["alice", "bob", "carol"]
        .iter()
        .enumerate()
        .map(|(i, n)| net.add_companion(1 + i as u8, n))
        .collect();
    let hub = net.add_repeater(50, "hub");
    for &x in &m {
        net.link(x, hub);
    }
    for &x in &m[1..] {
        net.know(m[0], x, true);
        net.know(x, m[0], true);
    }
    for &x in &m {
        net.connect(x);
    }
    net.run_until(net.now + 20_000, |_| false);
    for &x in &m[1..] {
        net.introduce(m[0], x);
    }
    let keys: Vec<[u8; 32]> = m.iter().map(|&i| net.key(i)).collect();
    let gid = net
        .with_core(m[0], |c, now| c.create_group("g", &keys[1..], now))
        .unwrap()
        .unwrap();
    assert!(net.run_until(net.now + 60 * MIN, |n| {
        m.iter().all(|&x| {
            n.with_core_ref(x, |c| {
                c.groups()
                    .find(|g| g.gid == gid)
                    .is_some_and(|g| g.keys_known() == g.members.len() - 1)
            }) == Some(true)
        })
    }));
    net.run_until(net.now + 5 * MIN, |_| false);
    for &x in &m {
        net.take_events(x);
    }
    (net, m, gid)
}

fn group_texts(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::GroupMessage {
                content: Content::Text { text, .. },
                ..
            } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_group_message_is_sent_once_it_left() {
    let (mut net, m, gid) = group();
    let bob = m[1];
    net.disconnect(bob);
    let id = net
        .with_core(bob, |c, now| {
            c.send_group_text(&gid, "written offline", 1, now)
        })
        .unwrap()
        .unwrap();
    assert_eq!(stage(&net, bob, id), Some((Stage::Offline, false)));
    net.run_until(net.now + 10 * MIN, |_| false);
    assert!(
        !net.take_events(bob).contains(&Event::GroupSent { id }),
        "not sent while the radio is away"
    );
    assert!(group_texts(&net.take_events(m[0])).is_empty());

    net.connect(bob);
    let mut eb = Vec::new();
    assert!(net.run_until(net.now + 3 * MIN, |n| {
        eb.extend(n.take_events(bob));
        eb.contains(&Event::GroupSent { id })
    }));
    assert_eq!(stage(&net, bob, id), None);
    net.run_until(net.now + 2 * MIN, |_| false);
    assert_eq!(group_texts(&net.take_events(m[0])), vec!["written offline"]);
    assert_eq!(group_texts(&net.take_events(m[2])), vec!["written offline"]);

    // Cancelled before it left: nobody gets it.
    net.disconnect(bob);
    let id = net
        .with_core(bob, |c, now| c.send_group_text(&gid, "never mind", 2, now))
        .unwrap()
        .unwrap();
    assert_eq!(net.with_core(bob, |c, _| c.cancel(id)).unwrap(), Ok(()));
    net.connect(bob);
    net.run_until(net.now + 3 * MIN, |_| false);
    assert!(!net.take_events(bob).contains(&Event::GroupSent { id }));
    assert!(group_texts(&net.take_events(m[0])).is_empty());
}

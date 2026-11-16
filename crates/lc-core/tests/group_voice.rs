//! Voice notes in groups over the fake network (protocol.md §13.1, ADR-0035): up to five
//! flooded fragments, reassembled and verified by every member; texts stay at two.

use lc_core::fakenet::Net;
use lc_core::{Content, Error, Event, GroupId};
use lc_meshcore::airtime::LoRaParams;

const MIN: u64 = 60_000;

/// Three companions around one repeater; the admin (0) met the others in person.
fn trio(seed: u64) -> (Net, Vec<usize>, GroupId) {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, seed);
    let m: Vec<usize> = ["alice", "bob", "carol"]
        .iter()
        .enumerate()
        .map(|(i, n)| net.add_companion(1 + i as u8, n))
        .collect();
    let hub = net.add_repeater(50, "hub");
    for &i in &m {
        net.link(i, hub);
    }
    for &i in &m[1..] {
        net.know(m[0], i, true);
        net.know(i, m[0], true);
    }
    for &i in &m {
        net.connect(i);
    }
    assert!(net.run_until(net.now + 20_000, |n| {
        m.iter()
            .all(|&i| n.nodes[i].core.as_ref().is_some_and(|c| c.ready()))
    }));
    for &i in &m[1..] {
        net.introduce(m[0], i);
    }
    let keys = [net.key(m[1]), net.key(m[2])];
    let gid = net
        .with_core(m[0], |c, now| c.create_group("Col", &keys, now))
        .unwrap()
        .unwrap();
    let complete = |n: &mut Net| {
        m.iter().all(|&i| {
            n.nodes[i].core.as_ref().is_some_and(|c| {
                c.groups()
                    .find(|g| g.gid == gid)
                    .is_some_and(|g| g.keys_known() == g.members.len() - 1)
            })
        })
    };
    assert!(net.run_until(net.now + 60 * MIN, complete));
    net.run_until(net.now + 5 * MIN, |_| false);
    for &i in &m {
        net.take_events(i);
    }
    (net, m, gid)
}

/// Six seconds of Codec2 700C: 150 frames, 525 bytes, 5 fragments with its signature.
fn six_seconds() -> Content {
    Content::Voice {
        sent_at: 1_793_900_000,
        mode: 0,
        frames: 150,
        data: (0..525).map(|i| (i * 7 % 251) as u8).collect(),
    }
}

#[test]
fn a_voice_note_reaches_every_member() {
    let (mut net, m, gid) = trio(31);
    let voice = six_seconds();
    let datagram = voice.encode();
    let (fragments, airtime) = net
        .with_core_ref(m[1], |c| c.estimate_group_send(datagram.len()))
        .unwrap();
    assert_eq!(fragments, 5);
    eprintln!("6 s of 700C in a group: {fragments} fragments, {airtime} ms of our own floods");
    let id = net
        .with_core(m[1], |c, now| c.send_group_voice(&gid, &datagram, now))
        .unwrap()
        .unwrap();
    let mut sent = false;
    net.run_until(net.now + 3 * MIN, |n| {
        sent |= n.take_events(m[1]).contains(&Event::GroupSent { id });
        false
    });
    assert!(sent, "the first copy of every fragment left");
    for &i in [m[0], m[2]].iter() {
        let got: Vec<Content> = net
            .take_events(i)
            .into_iter()
            .filter_map(|e| match e {
                Event::GroupMessage { content, .. } => Some(content),
                _ => None,
            })
            .collect();
        assert_eq!(got, [voice.clone()], "member {i}");
    }
}

#[test]
fn what_a_group_refuses() {
    let (mut net, m, gid) = trio(32);
    // Over 5 fragments: 200 frames of 700C (8 s) is 700 bytes.
    let long = Content::Voice {
        sent_at: 1,
        mode: 0,
        frames: 200,
        data: vec![1; 700],
    };
    assert_eq!(
        net.with_core(m[1], |c, now| c.send_group_voice(&gid, &long.encode(), now))
            .unwrap(),
        Err(Error::TooLarge)
    );
    // Only a VOICE takes more than two fragments: a text does not.
    let text = "x".repeat(250);
    assert_eq!(
        net.with_core(m[1], |c, now| c.send_group_text(&gid, &text, 1, now))
            .unwrap(),
        Err(Error::TooLarge)
    );
    let not_voice = Content::Text {
        sent_at: 1,
        text: "hi".into(),
    };
    assert_eq!(
        net.with_core(m[1], |c, now| c.send_group_voice(
            &gid,
            &not_voice.encode(),
            now
        ))
        .unwrap(),
        Err(Error::Invalid)
    );
}

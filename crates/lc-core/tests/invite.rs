//! Group invitation by QR code over the fake network (protocol.md §10.5, ADR-0020): people the
//! admin does not know scan one invitation and join; expired, revoked and forged tokens admit
//! no one.

use lc_core::fakenet::Net;
use lc_core::identity::Invite;
use lc_core::{Content, Error, Event, GroupId};
use lc_meshcore::airtime::LoRaParams;

const MIN: u64 = 60_000;
/// The wall clock at the start of the run, in Unix ms.
const UNIX0: u64 = 1_793_000_000_000;

/// alice (admin), bob and carol form "Rando Vercors", each linked to a repeater in the middle.
/// `extra` more companions stand next to alice (linked to her and to the repeater), known to
/// no one: they will scan her invitation.
fn setup(seed: u64, extra: usize) -> (Net, Vec<usize>, Vec<usize>, GroupId) {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, seed);
    let members: Vec<usize> = ["alice", "bob", "carol"]
        .iter()
        .enumerate()
        .map(|(i, n)| net.add_companion(1 + i as u8, n))
        .collect();
    let joiners: Vec<usize> = ["dave", "eve", "frank"][..extra]
        .iter()
        .enumerate()
        .map(|(i, n)| net.add_companion(10 + i as u8, n))
        .collect();
    let hub = net.add_repeater(50, "hub");
    for &m in members.iter().chain(&joiners) {
        net.link(m, hub);
    }
    for &j in &joiners {
        net.link(j, members[0]);
    }
    for &m in &members[1..] {
        net.know(members[0], m, true);
        net.know(m, members[0], true);
    }
    let all: Vec<usize> = members.iter().chain(&joiners).copied().collect();
    for &m in &all {
        net.connect(m);
    }
    assert!(net.run_until(net.now + 20_000, |n| {
        all.iter()
            .all(|&m| n.nodes[m].core.as_ref().is_some_and(|c| c.ready()))
    }));
    for &m in &all {
        net.with_core(m, |c, now| c.set_clock(UNIX0 + now, now));
    }
    for &m in &members[1..] {
        net.introduce(members[0], m);
    }
    let keys: Vec<[u8; 32]> = members[1..].iter().map(|&i| net.key(i)).collect();
    let gid = net
        .with_core(members[0], |c, now| {
            c.create_group("Rando Vercors", &keys, now)
        })
        .unwrap()
        .unwrap();
    assert!(
        net.run_until(net.now + 60 * MIN, |n| complete(n, &members, &gid, 3)),
        "group not set up"
    );
    for &m in &all {
        net.take_events(m);
    }
    (net, members, joiners, gid)
}

/// Every node in `nodes` has group `gid` with `size` members and everyone's sender key.
fn complete(net: &Net, nodes: &[usize], gid: &GroupId, size: usize) -> bool {
    nodes.iter().all(|&m| {
        net.nodes[m].core.as_ref().is_some_and(|c| {
            c.groups()
                .find(|g| g.gid == *gid)
                .is_some_and(|g| g.members.len() == size && g.keys_known() == g.members.len() - 1)
        })
    })
}

fn size(net: &Net, node: usize, gid: &GroupId) -> Option<usize> {
    net.nodes[node]
        .core
        .as_ref()?
        .groups()
        .find(|g| g.gid == *gid)
        .map(|g| g.members.len())
}

fn invite(net: &mut Net, admin: usize, gid: &GroupId) -> Invite {
    let invite = net
        .with_core(admin, |c, now| c.create_invite(gid, now))
        .unwrap()
        .unwrap();
    // What the joiner's camera reads.
    Invite::decode(&invite.encode()).unwrap()
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
fn two_people_join_with_one_invitation() {
    let (mut net, m, j, gid) = setup(21, 2);
    let inv = invite(&mut net, m[0], &gid);
    assert_eq!(inv.group_name, "Rando Vercors");
    assert_eq!(inv.admin_name, "alice");
    for &x in &j {
        net.with_core(x, |c, now| c.join(&inv, now))
            .unwrap()
            .unwrap();
    }
    let everyone: Vec<usize> = m.iter().chain(&j).copied().collect();
    let started = net.now;
    assert!(net.run_until(net.now + 30 * MIN, |n| {
        j.iter().all(|&x| size(n, x, &gid) == Some(5))
    }));
    println!("two joiners admitted: {} s", (net.now - started) / 1000);
    assert!(
        net.run_until(net.now + 60 * MIN, |n| complete(n, &everyone, &gid, 5)),
        "the joiners did not get in"
    );
    let keys_s = (net.now - started) / 1000;
    println!("two joiners, group of 5 with every key: {keys_s} s");
    // The admin relays the keys (§10.2): no session between a member and a newcomer.
    assert!(keys_s < 300, "{keys_s} s");
    for &member in &m[1..] {
        let strangers: Vec<_> = j.iter().map(|&x| net.key(x)).collect();
        assert!(
            !net.take_events(member).iter().any(
                |e| matches!(e, Event::SessionEstablished { peer } if strangers.contains(peer))
            ),
            "member {member} opened a session with a newcomer"
        );
    }
    let admin = net.take_events(m[0]);
    for &x in &j {
        let node = net.key(x);
        assert!(
            admin.contains(&Event::MemberJoined { gid, node }),
            "the admin was not told"
        );
        let mine = net.take_events(x);
        assert!(mine.iter().any(
            |e| matches!(e, Event::GroupJoined { name, gid: g, .. } if name == "Rando Vercors" && *g == gid)
        ));
        // The admin is a verified contact: scanned in person.
        let c = net.nodes[x].core.as_ref().unwrap();
        let a = c.contacts().find(|k| k.key == net.key(m[0])).unwrap();
        assert!(a.verified && a.identity.is_some());
    }

    // A newcomer posts, everyone reads; the admin answers, the newcomers read it.
    for e in &everyone {
        net.take_events(*e);
    }
    net.with_core(j[0], |c, now| c.send_group_text(&gid, "hi all", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 3 * MIN, |_| false);
    net.with_core(m[0], |c, now| c.send_group_text(&gid, "welcome", 2, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 3 * MIN, |_| false);
    for &x in &everyone {
        let got = group_texts(&net.take_events(x));
        if x != j[0] {
            assert!(got.contains(&"hi all".to_owned()), "node {x}: {got:?}");
        }
        if x != m[0] {
            assert!(got.contains(&"welcome".to_owned()), "node {x}: {got:?}");
        }
    }
}

/// Runs long enough for a GROUP_JOIN to arrive, then checks that no one got in.
fn nobody_admitted(net: &mut Net, m: &[usize], joiner: usize, gid: &GroupId) {
    net.run_until(net.now + 15 * MIN, |_| false);
    assert_eq!(size(net, m[0], gid), Some(3));
    assert_eq!(size(net, joiner, gid), None);
    assert!(
        !net.take_events(m[0])
            .iter()
            .any(|e| matches!(e, Event::MemberJoined { .. }))
    );
    // The request did arrive: the admin knows the joiner's identity from the session.
    let node = net.key(joiner);
    assert!(
        net.nodes[m[0]]
            .core
            .as_ref()
            .unwrap()
            .contacts()
            .any(|c| c.key == node && c.identity.is_some())
    );
}

#[test]
fn a_revoked_invitation_admits_no_one() {
    let (mut net, m, j, gid) = setup(22, 1);
    let inv = invite(&mut net, m[0], &gid);
    net.with_core(m[0], |c, _| c.revoke_invite(&inv.token));
    net.with_core(j[0], |c, now| c.join(&inv, now))
        .unwrap()
        .unwrap();
    nobody_admitted(&mut net, &m, j[0], &gid);
}

#[test]
fn a_forged_token_admits_no_one() {
    let (mut net, m, j, gid) = setup(23, 1);
    let mut inv = invite(&mut net, m[0], &gid);
    inv.token[0] ^= 1;
    net.with_core(j[0], |c, now| c.join(&inv, now))
        .unwrap()
        .unwrap();
    nobody_admitted(&mut net, &m, j[0], &gid);
}

#[test]
fn an_expired_invitation_admits_no_one() {
    let (mut net, m, j, gid) = setup(24, 1);
    let mut inv = invite(&mut net, m[0], &gid);
    // Past its 10 minutes and the 2 minutes allowed for phone clocks.
    net.run_until(net.now + 13 * MIN, |_| false);
    // The joiner's app refuses it (its clock is right)…
    assert_eq!(
        net.with_core(j[0], |c, now| c.join(&inv, now)).unwrap(),
        Err(Error::Expired)
    );
    // …and the admin's, if the joiner's clock is wrong.
    inv.expires += 3600;
    net.with_core(j[0], |c, now| c.join(&inv, now))
        .unwrap()
        .unwrap();
    nobody_admitted(&mut net, &m, j[0], &gid);
}

/// Who holds which keys, second by second, while two people join (to tune the relay).
#[test]
#[ignore = "measurement, prints a timeline"]
fn key_timeline() {
    let (mut net, m, j, gid) = setup(21, 2);
    let inv = invite(&mut net, m[0], &gid);
    for &x in &j {
        net.with_core(x, |c, now| c.join(&inv, now))
            .unwrap()
            .unwrap();
    }
    let everyone: Vec<usize> = m.iter().chain(&j).copied().collect();
    let t0 = net.now;
    let mut last = String::new();
    println!("keys held / members − 1: alice bob carol dave eve");
    net.run_until(net.now + 10 * MIN, |n| {
        let row: Vec<String> = everyone
            .iter()
            .map(|&x| {
                n.nodes[x]
                    .core
                    .as_ref()
                    .and_then(|c| c.groups().find(|g| g.gid == gid))
                    .map_or("-".into(), |g| {
                        format!("{}/{}", g.keys_known(), g.members.len() - 1)
                    })
            })
            .collect();
        let row = row.join(" ");
        if row != last {
            println!("{:>4} s  {row}", (n.now - t0) / 1000);
            last = row;
        }
        complete(n, &everyone, &gid, 5)
    });
}

/// Two people join by invitation, over ten seeds: when both are admitted, and when the group
/// holds every key (median and worst, seconds).
#[test]
#[ignore = "measurement, prints numbers for docs/measurements.md"]
fn join_timings() {
    let (mut admitted, mut keys) = (Vec::new(), Vec::new());
    for seed in 1..=10 {
        let (mut net, m, j, gid) = setup(100 + seed, 2);
        let inv = invite(&mut net, m[0], &gid);
        for &x in &j {
            net.with_core(x, |c, now| c.join(&inv, now))
                .unwrap()
                .unwrap();
        }
        let everyone: Vec<usize> = m.iter().chain(&j).copied().collect();
        let t0 = net.now;
        net.run_until(net.now + 60 * MIN, |n| {
            j.iter().all(|&x| size(n, x, &gid) == Some(5))
        });
        admitted.push((net.now - t0) / 1000);
        net.run_until(net.now + 60 * MIN, |n| complete(n, &everyone, &gid, 5));
        keys.push((net.now - t0) / 1000);
    }
    for (what, mut v) in [("admitted", admitted), ("every key", keys)] {
        v.sort_unstable();
        println!(
            "{what}: median {} s, worst {} s, all {v:?}",
            v[v.len() / 2],
            v[v.len() - 1]
        );
    }
}

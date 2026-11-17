//! Attacks found by the second security read of 2026-11-17 on handshakes and paths, each
//! with the run that must still work without it.

use std::sync::{Arc, Mutex};

use lc_core::Event;
use lc_core::fakenet::Net;
use lc_meshcore::air::{Packet, PayloadType};
use lc_meshcore::airtime::LoRaParams;
use lc_proto::discovery::{Keying, parse_probe, seal_path_reply};
use lc_proto::intro::{Intro, open_intro, seal_intro};
use lc_proto::{Header, Kind};

const S: u64 = 1_000;
const MIN: u64 = 60 * S;
const BOGUS: [u8; 10] = [0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A];

fn net3(seed: u64, paths: bool) -> (Net, usize, usize) {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, seed);
    let a = net.add_companion(1, "alice");
    let b = net.add_companion(2, "bob");
    let hub = net.add_repeater(50, "hub");
    net.link(a, hub);
    net.link(b, hub);
    net.know(a, b, paths);
    net.know(b, a, paths);
    net.connect(a);
    net.connect(b);
    assert!(net.run_until(20 * S, |n| {
        [a, b]
            .iter()
            .all(|&i| n.nodes[i].core.as_ref().is_some_and(|c| c.ready()))
    }));
    (net, a, b)
}

fn raw_push(payload: &[u8]) -> Vec<u8> {
    let mut push = vec![0x84, 20, 0xB0, 0xFF];
    push.extend_from_slice(payload);
    push
}

fn outcome(ev: &[Event], id: u64) -> Option<String> {
    ev.iter().find_map(|e| match e {
        Event::Delivered { id: i } if *i == id => Some("delivered".to_owned()),
        Event::Failed { id: i, reason } if *i == id => Some(format!("failed {reason:?}")),
        _ => None,
    })
}

/// A forged INTRO under Bob's hash byte while Alice re-handshakes after Bob's restart.
fn restart_then(attack: bool) -> Option<String> {
    let (mut net, a, b) = net3(31, true);
    net.introduce(a, b);
    let (alice, bob) = (net.key(a), net.key(b));
    let id1 = net
        .with_core(a, |c, now| c.send_text(&bob, "first", 1, now))
        .unwrap()
        .unwrap();
    let mut ev = Vec::new();
    assert!(net.run_until(net.now + 5 * MIN, |n| {
        ev.extend(n.take_events(a));
        outcome(&ev, id1).is_some()
    }));
    assert_eq!(outcome(&ev, id1).as_deref(), Some("delivered"));
    // Bob's app restarts (same identity, kept in its store): its session is gone.
    let alice_id = net.with_core_ref(a, |c| *c.key().unwrap()).unwrap();
    net.attach_app(b, 99);
    net.connect(b);
    net.run_until(net.now + 5 * S, |_| false);
    net.with_core(b, |c, _| c.set_identity(&alice, alice_id, true));
    net.take_events(a);
    let id2 = net
        .with_core(a, |c, now| c.send_text(&bob, "second", 2, now))
        .unwrap()
        .unwrap();
    let mut ev = Vec::new();
    let t0 = net.now;
    if attack {
        // For five minutes, a forged INTRO "from Bob" every 3 s, then nothing.
        let mut k = 0u16;
        while net.now < t0 + 5 * MIN {
            let intro = Intro {
                tag: 0x2000 + k,
                attempt: 0,
                salt: [(k % 251) as u8; 16],
                ephemeral: lc_proto::kdf::x25519_public(&[(k % 250) as u8 + 1; 32]),
                return_path_len: 0,
                return_path: &[],
            };
            let mut frame = [0u8; 200];
            let n = seal_intro(bob[0], alice[0], &intro, &mut frame).unwrap();
            net.with_core(a, |c, now| {
                c.on_companion_frame(&raw_push(&frame[..n]), now)
            });
            net.run_until(net.now + 3 * S, |n| {
                ev.extend(n.take_events(a));
                false
            });
            k += 1;
        }
    }
    net.run_until(t0 + 65 * MIN, |n| {
        ev.extend(n.take_events(a));
        outcome(&ev, id2).is_some()
    });
    let r = outcome(&ev, id2);
    eprintln!(
        "restart attack={attack}: {r:?} at +{} s, transmissions {}",
        (net.now - t0) / 1000,
        net.stats.transmissions
    );
    r
}

#[test]
fn a_restart_without_attack_delivers() {
    assert_eq!(restart_then(false).as_deref(), Some("delivered"));
}

/// N2: forged INTROs under Bob's hash byte, while Alice's message waits for a new handshake
/// after Bob's restart, used to cancel that handshake and leave the message requeued on the
/// lost session for over an hour. Now an INTRO changes nothing until its INTRO_FIN.
#[test]
fn forged_intros_during_recovery() {
    assert_eq!(restart_then(true).as_deref(), Some("delivered"));
}

/// A copy of Alice's genuine INTRO with another return path, injected into Bob after the
/// genuine one: is it the path Bob adopts at INTRO_FIN?
fn intro_copy(attack: bool) -> bool {
    let (mut net, a, b) = net3(32, true);
    let (alice, bob) = (net.key(a), net.key(b));
    let air = |net: &Net| {
        net.with_core_ref(b, |c| c.estimate_send(&alice, 50).1)
            .unwrap()
    };
    let baseline = air(&net);
    let captured: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
    let cap = captured.clone();
    net.deaf = Some(Box::new(move |r, frame| {
        if r == b
            && let Ok(p) = Packet::decode(frame)
            && p.ptype == PayloadType::RawCustom
            && let Ok((h, _)) = Header::decode(p.payload)
            && h.kind == Kind::Control
            && h.index == 3
        {
            cap.lock().unwrap().push(p.payload.to_vec());
        }
        false
    }));
    let id = net
        .with_core(a, |c, now| c.send_text(&bob, "hello", 1, now))
        .unwrap()
        .unwrap();
    let cap2 = captured.clone();
    assert!(net.run_until(net.now + 5 * MIN, |_| !cap2.lock().unwrap().is_empty()));
    net.run_until(net.now + 300, |_| false);
    if attack {
        let genuine = captured.lock().unwrap()[0].clone();
        let (_, intro) = open_intro(bob[0], &genuine).unwrap();
        let forged = Intro {
            return_path_len: BOGUS.len() as u8,
            return_path: &BOGUS,
            ..intro
        };
        let mut frame = [0u8; 200];
        let n = seal_intro(alice[0], bob[0], &forged, &mut frame).unwrap();
        net.with_core(b, |c, now| {
            c.on_companion_frame(&raw_push(&frame[..n]), now)
        });
    }
    let mut ev = Vec::new();
    let moved = net.run_until(net.now + 5 * MIN, |n| {
        ev.extend(n.take_events(a));
        air(n) > baseline
    });
    net.run_until(net.now + 10 * MIN, |n| {
        ev.extend(n.take_events(a));
        outcome(&ev, id).is_some()
    });
    eprintln!(
        "intro copy attack={attack}: Bob's path moved {moved} (airtime {} vs {baseline}); Alice's text {:?}",
        air(&net),
        outcome(&ev, id)
    );
    moved
}

#[test]
fn an_intro_without_copy_keeps_the_path() {
    assert!(!intro_copy(false));
}

/// N9: a copy of Alice's genuine INTRO with another return path, heard after it, used to move
/// the path Bob adopts at INTRO_FIN. A copy now keeps the first one's path.
#[test]
fn an_intro_copy_with_another_path() {
    assert!(!intro_copy(true));
}

/// Alice knows Bob's identity but no path; Bob misses her first (keyed) probe. A forged
/// answer to her second (anonymous) probe, with a path to nowhere.
fn anonymous_reply(attack: bool) -> Option<String> {
    let (mut net, a, b) = net3(33, false);
    net.introduce(a, b);
    let (alice, bob) = (net.key(a), net.key(b));
    let seen: Arc<Mutex<Vec<[u8; 8]>>> = Arc::default();
    let s2 = seen.clone();
    net.deaf = Some(Box::new(move |r, frame| {
        let Ok(p) = Packet::decode(frame) else {
            return false;
        };
        let Some(probe) = parse_probe(p.payload) else {
            return false;
        };
        let mut v = s2.lock().unwrap();
        if !v.contains(&probe.id) {
            v.push(probe.id);
        }
        // Bob does not hear Alice's first probe.
        r == b && v.first() == Some(&probe.id)
    }));
    let id = net
        .with_core(a, |c, now| c.send_text(&bob, "hi", 1, now))
        .unwrap()
        .unwrap();
    let mut ev = Vec::new();
    let mut forged = 0usize;
    let t0 = net.now;
    net.run_until(net.now + 30 * MIN, |n| {
        ev.extend(n.take_events(a));
        if attack {
            let ids = seen.lock().unwrap().clone();
            for pid in ids.iter().skip(forged) {
                let k = Keying::anonymous(&alice, &bob);
                let mut buf = [0u8; 255];
                let len = seal_path_reply(
                    &k,
                    bob[0],
                    alice[0],
                    pid,
                    &[1; 8],
                    BOGUS.len() as u8,
                    &BOGUS,
                    &mut buf,
                )
                .unwrap();
                let push = raw_push(&buf[..len]);
                n.with_core(a, |c, now| c.on_companion_frame(&push, now));
            }
            forged = ids.len();
        }
        outcome(&ev, id).is_some()
    });
    let r = outcome(&ev, id);
    eprintln!(
        "anonymous reply attack={attack}: {r:?} after {} s, probes {}",
        (net.now - t0) / 1000,
        seen.lock().unwrap().len()
    );
    r
}

#[test]
fn a_missed_first_probe_still_delivers() {
    assert_eq!(anonymous_reply(false).as_deref(), Some("delivered"));
}

/// N4: anyone who heard an anonymous probe can answer it with a path to nowhere; Alice's text
/// used to fail "unreachable" over it. The path is now unproven, and a handshake that fails over
/// it looks for a path again, from a keyed probe, before the text fails.
#[test]
fn a_forged_answer_to_an_anonymous_probe() {
    assert_eq!(anonymous_reply(true).as_deref(), Some("delivered"));
}

/// N10: Alice knows Bob's identity, Bob does not know hers, so he cannot open her HELLO and
/// introduces himself instead. Nobody asked for that introduction and anyone can replay a
/// HELLO to start one: it sends a few INTROs, not a whole introduction's worth (8).
#[test]
fn an_unasked_introduction_sends_few_intros() {
    let (mut net, a, b) = net3(41, true);
    let (alice, bob) = (net.key(a), net.key(b));
    let bob_id = net.with_core_ref(b, |c| *c.key().unwrap()).unwrap();
    net.with_core(a, |c, _| c.set_identity(&bob, bob_id, true));
    // Alice hears none of Bob's INTROs: he sends them again until he gives up.
    let intros: Arc<Mutex<usize>> = Arc::default();
    let count = intros.clone();
    net.deaf = Some(Box::new(move |r, frame| {
        let intro = r == a
            && Packet::decode(frame).is_ok_and(|p| {
                p.ptype == PayloadType::RawCustom
                    && Header::decode(p.payload)
                        .is_ok_and(|(h, _)| h.kind == Kind::Control && h.index == 3)
            });
        if intro {
            *count.lock().unwrap() += 1;
        }
        intro
    }));
    net.with_core(a, |c, now| c.send_text(&bob, "hello", 1, now))
        .unwrap()
        .unwrap();
    net.run_until(net.now + 9 * MIN, |_| false);
    let sent = *intros.lock().unwrap();
    eprintln!("INTROs from one unasked introduction: {sent}");
    assert!((1..=3).contains(&sent), "{sent} INTROs");
    let _ = alice;
}

/// N11: a HELLO stamped 0 (its sender had no clock) used to stay fresh for ever, so a recorded
/// one could be replayed at any time. Once the receiver knows the time, it gets no answer.
#[test]
fn a_hello_stamped_zero_gets_no_answer() {
    let (mut net, a, b) = net3(42, true);
    net.introduce(a, b);
    let bob = net.key(b);
    // Alice's app does not know the time: its clock says 1970.
    net.with_core(a, |c, now| c.set_clock(0, now));
    let id = net
        .with_core(a, |c, now| c.send_text(&bob, "no clock", 1, now))
        .unwrap()
        .unwrap();
    let mut ev = Vec::new();
    net.run_until(net.now + 5 * MIN, |n| {
        ev.extend(n.take_events(a));
        outcome(&ev, id).is_some()
    });
    assert_ne!(outcome(&ev, id).as_deref(), Some("delivered"));
}

/// N15: a node's name, as its radio advertises it, loses the characters that reorder text: a
/// name cannot pose as another one.
#[test]
fn node_names_lose_bidirectional_controls() {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, 43);
    let a = net.add_companion(1, "alice");
    let b = net.add_companion(2, "Bob\u{202E}nilliM");
    let hub = net.add_repeater(50, "hub");
    net.link(a, hub);
    net.link(b, hub);
    net.know(a, b, true);
    net.connect(a);
    net.connect(b);
    let bob = net.key(b);
    assert!(net.run_until(20 * S, |n| {
        n.nodes[a]
            .core
            .as_ref()
            .is_some_and(|c| c.contacts().any(|k| k.key == bob))
    }));
    let name = net
        .with_core_ref(a, |c| {
            c.contacts().find(|k| k.key == bob).unwrap().name.clone()
        })
        .unwrap();
    assert_eq!(name, "BobnilliM");
}

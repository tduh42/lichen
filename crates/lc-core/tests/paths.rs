//! Paths and unauthenticated frames (protocol.md §4.5, §4.6; review of 2026-11-15, SR-02).
//! Anyone in range can send an anonymous PROBE or an INTRO under a contact's hash byte: such a
//! frame may be answered, but the path it carries must not replace the one that works.

use lc_core::Event;
use lc_core::fakenet::Net;
use lc_meshcore::air::{Packet, PayloadType, Route};
use lc_meshcore::airtime::LoRaParams;
use lc_meshcore::path::Path;
use lc_proto::discovery::{Keying, build_probe};
use lc_proto::intro::{Intro, seal_intro};

const S: u64 = 1_000;
const MIN: u64 = 60 * S;

/// Alice and Bob around one repeater, verified, with a working session.
fn talking(seed: u64) -> (Net, usize, usize) {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, seed);
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
    net.introduce(a, b);
    let bob = net.key(b);
    let id = net
        .with_core(a, |c, now| c.send_text(&bob, "first", 1, now))
        .unwrap()
        .unwrap();
    let mut ev = Vec::new();
    assert!(net.run_until(net.now + 5 * MIN, |n| {
        ev.extend(n.take_events(a));
        ev.contains(&Event::Delivered { id })
    }));
    net.run_until(net.now + MIN, |_| false);
    net.take_events(a);
    net.take_events(b);
    (net, a, b)
}

/// Alice writes to Bob again: delivered within ten minutes?
fn still_reaches_bob(net: &mut Net, a: usize, b: usize) -> bool {
    let bob = net.key(b);
    let id = net
        .with_core(a, |c, now| c.send_text(&bob, "still there?", 2, now))
        .unwrap()
        .unwrap();
    let mut ev = Vec::new();
    net.run_until(net.now + 10 * MIN, |n| {
        ev.extend(n.take_events(a));
        ev.contains(&Event::Delivered { id })
    })
}

/// A path through a repeater that does not exist.
const NOWHERE: [u8; 1] = [0x99];

#[test]
fn an_anonymous_probe_does_not_move_the_path() {
    let (mut net, a, b) = talking(21);
    let (alice, bob) = (net.key(a), net.key(b));
    // Someone floods a probe "from Bob", keyed by the two node keys (public), as if it had
    // come through repeater 0x99.
    let probe = build_probe(&Keying::anonymous(&bob, &alice), bob[0], alice[0], &[7; 8]);
    let packet = Packet {
        route: Route::Flood,
        ptype: PayloadType::GrpData,
        transport: None,
        path: Path::new(1, &NOWHERE).unwrap(),
        payload: &probe,
    };
    let mut raw = [0u8; 255];
    let n = packet.encode(&mut raw).unwrap();
    // LOG_RX_DATA: snr4, rssi, raw frame.
    let mut push = vec![0x88, 20, 0xB0];
    push.extend_from_slice(&raw[..n]);
    net.with_core(a, |c, now| c.on_companion_frame(&push, now));
    assert!(still_reaches_bob(&mut net, a, b));
}

#[test]
fn an_intro_does_not_move_the_path() {
    let (mut net, a, b) = talking(22);
    let (alice, bob) = (net.key(a), net.key(b));
    let intro = Intro {
        tag: 0x4242,
        attempt: 0,
        salt: [3; 16],
        ephemeral: lc_proto::kdf::x25519_public(&[4; 32]),
        return_path_len: 1,
        return_path: &NOWHERE,
    };
    let mut frame = [0u8; 200];
    let n = seal_intro(bob[0], alice[0], &intro, &mut frame).unwrap();
    // RAW_DATA: snr4, rssi, 0xFF, payload.
    let mut push = vec![0x84, 20, 0xB0, 0xFF];
    push.extend_from_slice(&frame[..n]);
    net.with_core(a, |c, now| c.on_companion_frame(&push, now));
    assert!(still_reaches_bob(&mut net, a, b));
}

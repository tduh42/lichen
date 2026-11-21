//! End-to-end transfers between two sessions over a lossy link: delivery, nonce uniqueness,
//! byte-distinct retransmissions, forged fragments.

use std::collections::HashSet;

use lc_proto::sender::{SendError, Timing};
use lc_proto::{CHUNK_MAX, FrameError, Received, Session, SessionEvent, SessionKeys};
use proptest::prelude::*;
use rand_chacha::ChaCha8Rng;
use rand_core::{Rng, SeedableRng};

const A: u8 = 0xA1;
const B: u8 = 0xB2;

fn pair() -> (Session, Session) {
    let (k1, k2) = ([1u8; 32], [2u8; 32]);
    let t = Timing::default();
    (
        Session::new(SessionKeys { send: k1, recv: k2 }, A, B, t),
        Session::new(SessionKeys { send: k2, recv: k1 }, B, A, t),
    )
}

#[derive(Debug, Default)]
struct Outcome {
    delivered: bool,
    failed: bool,
    received: Vec<Vec<u8>>,
    frames: usize,
}

/// Runs until both sides are quiet. Every emitted frame (lost or not) is checked for nonce
/// uniqueness per direction and byte uniqueness overall.
fn run(a: &mut Session, b: &mut Session, loss_pct: u32, seed: u64) -> Outcome {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut out = Outcome::default();
    let mut nonces: [HashSet<[u8; 5]>; 2] = [HashSet::new(), HashSet::new()];
    let mut frames_seen: HashSet<Vec<u8>> = HashSet::new();
    let mut now = 0u64;
    let mut buf = [0u8; 200];
    for _ in 0..200_000 {
        for (dir, seen) in nonces.iter_mut().enumerate() {
            let (from, to) = if dir == 0 {
                (&mut *a, &mut *b)
            } else {
                (&mut *b, &mut *a)
            };
            while let Some(len) = from.poll_transmit(now, &mut buf) {
                out.frames += 1;
                let frame = &mut buf[..len];
                let nonce: [u8; 5] = frame[2..7].try_into().unwrap();
                assert!(seen.insert(nonce), "nonce reused: {nonce:02x?}");
                assert!(frames_seen.insert(frame.to_vec()), "identical frame resent");
                if rng.next_u32() % 100 < loss_pct {
                    continue;
                }
                if let Ok(Received::Datagram { data, .. }) = to.handle(frame, now) {
                    out.received.push(data.to_vec());
                }
            }
        }
        while let Some(e) = a.take_event() {
            match e {
                SessionEvent::Delivered { .. } => out.delivered = true,
                SessionEvent::Failed { .. } | SessionEvent::PeerAborted { .. } => out.failed = true,
                SessionEvent::ReceiveFailed { .. } => {}
            }
        }
        while b.take_event().is_some() {}
        let next = match (a.next_wakeup(), b.next_wakeup()) {
            (Some(x), Some(y)) => x.min(y),
            (x, y) => match x.or(y) {
                Some(t) => t,
                None => return out,
            },
        };
        now = now.max(next);
    }
    panic!("transfer did not settle");
}

fn datagram(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

#[test]
fn lossless_full_datagram() {
    let (mut a, mut b) = pair();
    let data = datagram(64 * CHUNK_MAX, 7);
    a.send(&data, CHUNK_MAX, 0).unwrap();
    let out = run(&mut a, &mut b, 0, 1);
    assert!(out.delivered && !out.failed);
    assert_eq!(out.received, vec![data]);
    assert_eq!(out.frames, 64 + 1);
}

#[test]
fn too_large_and_busy() {
    let (mut a, _) = pair();
    assert_eq!(
        a.send(&datagram(64 * 100 + 1, 0), 100, 0),
        Err(SendError::TooLarge)
    );
    assert_eq!(a.send(&[], 100, 0), Err(SendError::Empty));
    assert_eq!(a.send(&[1], CHUNK_MAX + 1, 0), Err(SendError::BadChunk));
    a.send(&[1, 2, 3], 100, 0).unwrap();
    assert_eq!(a.send(&[4], 100, 0), Err(SendError::Busy));
}

#[test]
fn several_datagrams_in_a_row_with_loss() {
    let (mut a, mut b) = pair();
    let mut expected = Vec::new();
    for i in 0..20u8 {
        let data = datagram(1 + usize::from(i) * 97, i);
        a.send(&data, CHUNK_MAX, 0).unwrap();
        let out = run(&mut a, &mut b, 10, u64::from(i));
        assert!(out.delivered, "datagram {i}");
        expected.push(data);
        assert_eq!(out.received.last(), expected.last());
    }
}

#[test]
fn forged_fragment_does_not_touch_the_buffer() {
    let (mut a, mut b) = pair();
    a.send(&datagram(400, 1), 100, 0).unwrap();
    let mut buf = [0u8; 200];
    let len = a.poll_transmit(0, &mut buf).unwrap();
    let mut forged = buf;
    forged[10] ^= 0x80;
    assert_eq!(
        b.handle(&mut forged[..len], 0).unwrap_err(),
        FrameError::Auth
    );
    assert_eq!(
        b.next_wakeup(),
        None,
        "no reassembly may start from a forged fragment"
    );
    // The genuine one is accepted.
    assert!(b.handle(&mut buf[..len], 0).is_ok());
    assert!(b.next_wakeup().is_some());
    // Replaying it is harmless.
    let mut replay = buf;
    assert!(matches!(
        b.handle(&mut replay[..len], 0),
        Err(FrameError::Auth) | Ok(_)
    ));
}

#[test]
fn delivery_rate_at_ten_percent_loss() {
    let mut delivered = 0;
    for seed in 0..200 {
        let (mut a, mut b) = pair();
        a.send(&datagram(64 * CHUNK_MAX, seed as u8), CHUNK_MAX, 0)
            .unwrap();
        let out = run(&mut a, &mut b, 10, seed);
        if out.delivered {
            delivered += 1;
            assert_eq!(out.received.len(), 1);
        }
    }
    assert!(delivered >= 198, "{delivered}/200 delivered");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// Any size, chunk and loss: nonces never repeat (checked inside `run`), delivery reports
    /// match what the receiver got, and the receiver never gets a corrupted datagram.
    #[test]
    fn lossy_transfers_are_sound(
        len in 1usize..=(64 * CHUNK_MAX),
        chunk in 40usize..=CHUNK_MAX,
        loss in 0u32..60,
        seed in any::<u64>(),
    ) {
        let (mut a, mut b) = pair();
        let data = datagram(len, seed as u8);
        match a.send(&data, chunk, 0) {
            Err(SendError::TooLarge) => {
                prop_assert!(len > 64 * chunk);
                return Ok(());
            }
            r => { r.unwrap(); }
        }
        let out = run(&mut a, &mut b, loss, seed);
        prop_assert!(out.delivered ^ out.failed);
        for r in &out.received {
            prop_assert_eq!(r, &data);
        }
        prop_assert!(out.received.len() <= 1);
        if out.delivered {
            prop_assert_eq!(out.received.len(), 1);
        }
    }
}

/// `cargo test -p lc-proto --release --test transfer -- --ignored --nocapture`
#[test]
#[ignore = "measurement, prints numbers for docs/measurements.md"]
fn measure_loss_vs_cost() {
    println!("loss | delivered | frames per delivered 10 KB datagram (mean, max)");
    for loss in [0u32, 5, 10, 20, 30, 40] {
        let (mut ok, mut frames, mut max) = (0u32, 0usize, 0usize);
        for seed in 0..1000u64 {
            let (mut a, mut b) = pair();
            a.send(&datagram(64 * CHUNK_MAX, seed as u8), CHUNK_MAX, 0)
                .unwrap();
            let out = run(&mut a, &mut b, loss, seed);
            if out.delivered {
                ok += 1;
                frames += out.frames;
                max = max.max(out.frames);
            }
        }
        println!(
            "{loss:>3}% | {ok:>4}/1000 | {:.1}, {max}",
            frames as f64 / f64::from(ok.max(1))
        );
    }
}

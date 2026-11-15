//! HELLO / HELLO_ACK (protocol.md §4.1–4.3, ADR-0029).

use lc_meshcore::ecdh::{expand_seed, public_key, shared_secret};
use lc_proto::handshake::{
    Ends, Hello, HelloAck, VERSION, hello_salt, open_hello, open_hello_ack, salt_time, seal_hello,
    seal_hello_ack,
};
use lc_proto::kdf::x25519_public;
use lc_proto::{FrameError, Header, Kind};

struct Id {
    prv: [u8; 64],
    pubk: [u8; 32],
}

/// Identity keys for the derivation; their first bytes stand in for the node hashes.
fn ends<'a>(me: &'a Id, peer: &'a Id) -> Ends<'a> {
    Ends {
        me: &me.pubk,
        peer: &peer.pubk,
        me_node: me.pubk[0],
        peer_node: peer.pubk[0],
    }
}

fn id(seed: u8) -> Id {
    let prv = expand_seed(&[seed; 32]);
    Id {
        pubk: public_key(&prv),
        prv,
    }
}

const E_A: [u8; 32] = [11; 32];
const E_B: [u8; 32] = [22; 32];
const PATH: [u8; 2] = [0x42, 0x17];

fn hello() -> Hello<'static> {
    Hello {
        tag: 0xBEEF,
        attempt: 0,
        salt: hello_salt(1_793_000_000, &[3; 12]),
        ephemeral: x25519_public(&E_A),
        version: VERSION,
        return_path_len: 2,
        return_path: &PATH,
    }
}

fn ack(h: &Hello<'_>) -> HelloAck {
    HelloAck {
        tag: h.tag,
        attempt: h.attempt,
        salt: [4; 16],
        ephemeral: x25519_public(&E_B),
        version: VERSION,
    }
}

#[test]
fn full_handshake_gives_matching_keys() {
    let (alice, bob) = (id(1), id(2));
    let s_a = shared_secret(&alice.prv, &bob.pubk).unwrap();
    let s_b = shared_secret(&bob.prv, &alice.pubk).unwrap();
    assert_eq!(s_a, s_b);

    let hello = hello();
    let mut frame = [0u8; 172];
    let n = seal_hello(&s_a, &ends(&alice, &bob), &E_A, &hello, &mut frame).unwrap();
    // The same size as before ADR-0029: the ephemeral moved from the ciphertext to the clear.
    assert_eq!(n, 7 + 16 + 32 + 2 + 2 + 16);
    let (h, _) = Header::decode(&frame[..n]).unwrap();
    assert_eq!(
        (h.kind, h.index, h.total, h.tag),
        (Kind::Control, 0, 1, 0xBEEF)
    );
    assert_eq!((h.dst, h.src), (bob.pubk[0], alice.pubk[0]));
    // Ephemeral in clear, right after the salt.
    assert_eq!(frame[7 + 16..7 + 48], hello.ephemeral);

    let got = open_hello(&s_b, &ends(&bob, &alice), &bob.prv, &mut frame[..n]).unwrap();
    assert_eq!(got, hello);
    assert_eq!(salt_time(&got.salt), 1_793_000_000);

    let a = ack(&got);
    let mut frame2 = [0u8; 172];
    let (n2, kb) = seal_hello_ack(
        &s_b,
        &ends(&bob, &alice),
        &bob.prv,
        &E_B,
        &got,
        &a,
        &mut frame2,
    )
    .unwrap();
    assert_eq!(n2, 7 + 16 + 32 + 1 + 16);
    let (got_ack, ka) = open_hello_ack(
        &s_a,
        &ends(&alice, &bob),
        &alice.prv,
        &E_A,
        &hello,
        &mut frame2[..n2],
    )
    .unwrap();
    assert_eq!(got_ack, a);
    assert_eq!(ka.send, kb.recv);
    assert_eq!(ka.recv, kb.send);
    assert_ne!(ka.send, ka.recv);
}

#[test]
fn wrong_secret_or_tampering_is_rejected() {
    let (alice, bob, eve) = (id(1), id(2), id(3));
    let s_ab = shared_secret(&alice.prv, &bob.pubk).unwrap();
    let s_eb = shared_secret(&eve.prv, &bob.pubk).unwrap();
    let hello = Hello {
        return_path_len: 0xFF,
        return_path: &[],
        ..hello()
    };
    let mut frame = [0u8; 172];
    let n = seal_hello(&s_ab, &ends(&alice, &bob), &E_A, &hello, &mut frame).unwrap();

    // Eve's secret with Bob does not open Alice's HELLO.
    let mut copy = frame;
    assert_eq!(
        open_hello(&s_eb, &ends(&bob, &alice), &bob.prv, &mut copy[..n]).unwrap_err(),
        FrameError::Auth
    );
    // Any changed byte (header, salt, ephemeral, ciphertext, tag) is rejected.
    for i in 0..n {
        let mut copy = frame;
        copy[i] ^= 0x01;
        assert!(
            open_hello(&s_ab, &ends(&bob, &alice), &bob.prv, &mut copy[..n]).is_err(),
            "byte {i}"
        );
    }
    // A resend (attempt + 1) changes the bytes, so MeshCore's duplicate filter lets it through.
    let mut again = [0u8; 172];
    let resend = Hello {
        attempt: 1,
        ..hello
    };
    let n2 = seal_hello(&s_ab, &ends(&alice, &bob), &E_A, &resend, &mut again).unwrap();
    assert_eq!(n, n2);
    assert_ne!(frame[..n], again[..n2]);
    // An inconsistent return path is refused at build time.
    let bad = Hello {
        return_path_len: 3,
        return_path: &[1, 2],
        ..hello
    };
    assert!(seal_hello(&s_ab, &ends(&alice, &bob), &E_A, &bad, &mut again).is_err());
}

/// Review of 2026-11-15, SR-06: Eve stole Alice's seed. She can compute the static secret
/// between Alice and Bob, but not answer Alice's HELLO as Bob: HELLO_ACK's key needs Bob's
/// static key or Alice's ephemeral.
#[test]
fn a_stolen_seed_does_not_answer_as_the_contact() {
    let (alice, bob) = (id(1), id(2));
    let s = shared_secret(&alice.prv, &bob.pubk).unwrap();
    let hello = hello();
    let mut frame = [0u8; 172];
    let n = seal_hello(&s, &ends(&alice, &bob), &E_A, &hello, &mut frame).unwrap();
    // Eve reads the HELLO's clear part, then answers with Alice's key standing in for Bob's.
    let a = ack(&hello);
    let mut forged = [0u8; 172];
    let (m, _) = seal_hello_ack(
        &s,
        &ends(&bob, &alice),
        &alice.prv,
        &E_B,
        &hello,
        &a,
        &mut forged,
    )
    .unwrap();
    assert_eq!(
        open_hello_ack(
            &s,
            &ends(&alice, &bob),
            &alice.prv,
            &E_A,
            &hello,
            &mut forged[..m]
        )
        .unwrap_err(),
        FrameError::Auth
    );
    let _ = n;
}

/// The return path is bound into the keys (ADR-0029): a HELLO_ACK computed for another path
/// does not open.
#[test]
fn the_transcript_is_bound() {
    let (alice, bob) = (id(1), id(2));
    let s = shared_secret(&alice.prv, &bob.pubk).unwrap();
    let hello = hello();
    let other = Hello {
        return_path: &[0x99, 0x17],
        ..hello
    };
    let a = ack(&hello);
    let mut frame = [0u8; 172];
    let (m, _) = seal_hello_ack(
        &s,
        &ends(&bob, &alice),
        &bob.prv,
        &E_B,
        &other,
        &a,
        &mut frame,
    )
    .unwrap();
    assert!(
        open_hello_ack(
            &s,
            &ends(&alice, &bob),
            &alice.prv,
            &E_A,
            &hello,
            &mut frame[..m]
        )
        .is_err()
    );
}

#[test]
fn small_order_ephemeral_is_rejected() {
    let (alice, bob) = (id(1), id(2));
    let s = shared_secret(&alice.prv, &bob.pubk).unwrap();
    let hello = Hello {
        ephemeral: [0; 32],
        ..hello()
    };
    let mut frame = [0u8; 172];
    // Bob cannot answer an all-zero ephemeral: every term with it is zero.
    assert!(
        seal_hello_ack(
            &s,
            &ends(&bob, &alice),
            &bob.prv,
            &E_B,
            &hello,
            &ack(&hello),
            &mut frame
        )
        .is_err()
    );
}

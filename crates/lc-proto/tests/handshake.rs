use lc_meshcore::ecdh::{expand_seed, public_key, shared_secret};
use lc_proto::handshake::{
    Ends, Hello, HelloAck, open_hello, open_hello_ack, seal_hello, seal_hello_ack, session_keys,
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

#[test]
fn full_handshake_gives_matching_keys() {
    let (alice, bob) = (id(1), id(2));
    let s_a = shared_secret(&alice.prv, &bob.pubk).unwrap();
    let s_b = shared_secret(&bob.prv, &alice.pubk).unwrap();
    assert_eq!(s_a, s_b);

    let e_a = [11u8; 32];
    let return_path = [0x42, 0x17];
    let hello = Hello {
        tag: 0xBEEF,
        attempt: 0,
        salt: [3u8; 16],
        ephemeral: x25519_public(&e_a),
        return_path_len: 2,
        return_path: &return_path,
    };
    let mut frame = [0u8; 172];
    let n = seal_hello(&s_a, &ends(&alice, &bob), &hello, &mut frame).unwrap();
    assert_eq!(n, 7 + 16 + 32 + 2 + 2 + 16);
    let (h, _) = Header::decode(&frame[..n]).unwrap();
    assert_eq!(
        (h.kind, h.index, h.total, h.tag),
        (Kind::Control, 0, 1, 0xBEEF)
    );
    assert_eq!((h.dst, h.src), (bob.pubk[0], alice.pubk[0]));

    let got = open_hello(&s_b, &ends(&bob, &alice), &mut frame[..n]).unwrap();
    assert_eq!(got.ephemeral, hello.ephemeral);
    assert_eq!(got.return_path, &return_path);
    let salt_i = got.salt;

    let e_b = [22u8; 32];
    let ack = HelloAck {
        tag: got.tag,
        attempt: got.attempt,
        salt: [4u8; 16],
        ephemeral: x25519_public(&e_b),
    };
    let mut frame2 = [0u8; 172];
    let n2 = seal_hello_ack(&s_b, &ends(&bob, &alice), &salt_i, &ack, &mut frame2).unwrap();
    let got_ack =
        open_hello_ack(&s_a, &ends(&alice, &bob), &hello.salt, &mut frame2[..n2]).unwrap();
    assert_eq!(got_ack, ack);

    let ka = session_keys(
        &s_a,
        &e_a,
        &got_ack.ephemeral,
        &hello.salt,
        &got_ack.salt,
        &alice.pubk,
        &bob.pubk,
        true,
    )
    .unwrap();
    let kb = session_keys(
        &s_b,
        &e_b,
        &got.ephemeral,
        &salt_i,
        &ack.salt,
        &alice.pubk,
        &bob.pubk,
        false,
    )
    .unwrap();
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
        tag: 1,
        attempt: 0,
        salt: [5u8; 16],
        ephemeral: x25519_public(&[9u8; 32]),
        return_path_len: 0xFF,
        return_path: &[],
    };
    let mut frame = [0u8; 172];
    let n = seal_hello(&s_ab, &ends(&alice, &bob), &hello, &mut frame).unwrap();

    // Eve's secret with Bob does not open Alice's HELLO.
    let mut copy = frame;
    assert_eq!(
        open_hello(&s_eb, &ends(&bob, &alice), &mut copy[..n]).unwrap_err(),
        FrameError::Auth
    );
    // Any changed byte (header, salt, ciphertext, tag) is rejected.
    for i in 0..n {
        let mut copy = frame;
        copy[i] ^= 0x01;
        assert!(
            open_hello(&s_ab, &ends(&bob, &alice), &mut copy[..n]).is_err(),
            "byte {i}"
        );
    }
    // A resend (attempt + 1) changes the bytes, so MeshCore's duplicate filter lets it through.
    let mut again = [0u8; 172];
    let resend = Hello {
        attempt: 1,
        ..hello
    };
    let n2 = seal_hello(&s_ab, &ends(&alice, &bob), &resend, &mut again).unwrap();
    assert_eq!(n, n2);
    assert_ne!(frame[..n], again[..n2]);
    // An inconsistent return path is refused at build time.
    let bad = Hello {
        return_path_len: 3,
        return_path: &[1, 2],
        ..hello
    };
    assert!(seal_hello(&s_ab, &ends(&alice, &bob), &bad, &mut again).is_err());
}

#[test]
fn small_order_ephemeral_is_rejected() {
    let s = [1u8; 32];
    assert!(
        session_keys(
            &s, &[2u8; 32], &[0u8; 32], &[0; 16], &[0; 16], &[1; 32], &[2; 32], true
        )
        .is_none()
    );
}

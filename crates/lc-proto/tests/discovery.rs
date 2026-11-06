use lc_meshcore::ecdh::{expand_seed, public_key, shared_secret};
use lc_proto::discovery::{
    CHANNEL_HASH, Keying, PROBE_LEN, build_probe, open_path_reply, parse_probe, seal_path_reply,
    verify_probe,
};

fn keyed<'a>(s: &'a [u8; 32], prober: &'a [u8; 32], target: &'a [u8; 32]) -> Keying<'a> {
    Keying {
        s,
        anonymous: false,
        prober,
        target,
    }
}

fn id(seed: u8) -> ([u8; 64], [u8; 32]) {
    let prv = expand_seed(&[seed; 32]);
    (prv, public_key(&prv))
}

#[test]
fn probe_and_reply_round_trip() {
    let ((a_prv, a), (b_prv, b), (e_prv, _)) = (id(1), id(2), id(3));
    let s = shared_secret(&a_prv, &b).unwrap();
    let probe = build_probe(&keyed(&s, &a, &b), a[0], b[0], &[7; 8]);
    assert_eq!(probe.len(), PROBE_LEN);
    assert_eq!(probe[0], CHANNEL_HASH);
    // A stock node would read a 2-byte MAC then 17 bytes: not a multiple of 16.
    assert_ne!((PROBE_LEN - 1 - 2) % 16, 0);
    let p = parse_probe(&probe).unwrap();
    assert_eq!((p.dst, p.src, p.id), (b[0], a[0], [7; 8]));
    let s_b = shared_secret(&b_prv, &a).unwrap();
    assert!(verify_probe(&keyed(&s_b, &a, &b), a[0], b[0], &probe));
    // Someone else's secret, the anonymous keying, or any changed byte, fails.
    let s_e = shared_secret(&e_prv, &b).unwrap();
    assert!(!verify_probe(&keyed(&s_e, &a, &b), a[0], b[0], &probe));
    assert!(!verify_probe(
        &Keying::anonymous(&a, &b),
        a[0],
        b[0],
        &probe
    ));
    for i in 0..PROBE_LEN {
        let mut t = probe;
        t[i] ^= 1;
        assert!(
            !verify_probe(&keyed(&s_b, &a, &b), a[0], b[0], &t),
            "byte {i}"
        );
    }

    // Bob saw the probe with path [r1, r2] and answers along [r2, r1].
    let mut frame = [0u8; 172];
    let n = seal_path_reply(
        &keyed(&s_b, &a, &b),
        b[0],
        a[0],
        &p.id,
        2,
        &[0x11, 0x22],
        &mut frame,
    )
    .unwrap();
    let (got_id, len, path) =
        open_path_reply(&keyed(&s, &a, &b), a[0], b[0], &mut frame[..n]).unwrap();
    assert_eq!((got_id, len, path), ([7; 8], 2, &[0x11u8, 0x22][..]));
    let mut frame2 = [0u8; 172];
    let n = seal_path_reply(&keyed(&s_b, &a, &b), b[0], a[0], &p.id, 0, &[], &mut frame2).unwrap();
    for i in 0..n {
        let mut t = frame2;
        t[i] ^= 1;
        assert!(
            open_path_reply(&keyed(&s, &a, &b), a[0], b[0], &mut t[..n]).is_err(),
            "byte {i}"
        );
    }
}

/// Without the target's identity, the probe and its answer use the public node keys: anyone
/// can build them, and the identity keying does not accept them.
#[test]
fn anonymous_probe() {
    let ((a_prv, a), (_, b)) = (id(1), id(2));
    let anon = Keying::anonymous(&a, &b);
    let probe = build_probe(&anon, a[0], b[0], &[9; 8]);
    assert!(verify_probe(&anon, a[0], b[0], &probe));
    let s = shared_secret(&a_prv, &b).unwrap();
    assert!(!verify_probe(&keyed(&s, &a, &b), a[0], b[0], &probe));
    let mut frame = [0u8; 172];
    let n = seal_path_reply(&anon, b[0], a[0], &[9; 8], 1, &[0x33], &mut frame).unwrap();
    let mut copy = frame;
    assert!(open_path_reply(&keyed(&s, &a, &b), a[0], b[0], &mut copy[..n]).is_err());
    let (_, len, path) = open_path_reply(&anon, a[0], b[0], &mut frame[..n]).unwrap();
    assert_eq!((len, path), (1, &[0x33u8][..]));
}

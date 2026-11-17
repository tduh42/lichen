use lc_proto::group::{
    FRAGMENT_PLAIN_MAX, GroupHeader, PAYLOAD_MAX, VOICE_MAX_FRAGMENTS, fragments_for,
    fragments_up_to, open, parse, seal, stock_would_try,
};
use proptest::prelude::*;

fn header(index: u8, total: u8) -> GroupHeader {
    GroupHeader {
        gid_hash: 0xBEEF,
        sender: 0xA1,
        key_epoch: 3,
        counter: 42,
        index,
        total,
        copy: 0,
    }
}

#[test]
fn sizes() {
    // A 156-byte envelope, whole over Bluetooth after 8 hops (ADR-0031).
    assert_eq!(FRAGMENT_PLAIN_MAX, 128);
    // TEXT capacity: content type + ts + text + 64-byte signature.
    assert_eq!(FRAGMENT_PLAIN_MAX - 1 - 4 - 64, 59);
    assert_eq!(2 * FRAGMENT_PLAIN_MAX - 1 - 4 - 64, 187);
    assert_eq!(fragments_for(128), Some(1));
    assert_eq!(fragments_for(129), Some(2));
    assert_eq!(fragments_for(256), Some(2));
    assert_eq!(fragments_for(257), None);
    // A voice note in a group (ADR-0035): 5 fragments, 640 bytes.
    assert_eq!(fragments_up_to(640, VOICE_MAX_FRAGMENTS), Some(5));
    assert_eq!(fragments_up_to(641, VOICE_MAX_FRAGMENTS), None);
    assert_eq!(fragments_up_to(129, VOICE_MAX_FRAGMENTS), Some(2));
    assert_eq!(fragments_up_to(0, VOICE_MAX_FRAGMENTS), None);
    assert_eq!(
        fragments_up_to(7 * 128 + 1, 9),
        None,
        "the wire says 7 at most"
    );
}

/// ADR-0035: `total` takes 3 bits; 0, above 7, and an index past it are refused.
#[test]
fn frag_total_takes_three_bits() {
    let key = [9u8; 32];
    for total in 1..=7u8 {
        let h = header(total - 1, total);
        let mut out = [0u8; 200];
        let n = seal(&key, &h, &[1, 2, 3], &mut out).unwrap();
        assert_eq!(parse(&out[..n]).map(|(h, _)| h), Some(h));
    }
    let mut out = [0u8; 200];
    let n = seal(&key, &header(0, 1), &[1, 2, 3], &mut out).unwrap();
    for frag in [0x00, 0x10, 0x51, 0x45 + 0x10] {
        let mut p = out;
        p[10] = frag;
        assert_eq!(parse(&p[..n]), None, "frag {frag:#04x}");
    }
}

proptest! {
    #[test]
    fn round_trip_and_stock_rejection(
        plain in proptest::collection::vec(any::<u8>(), 1..=FRAGMENT_PLAIN_MAX),
        key in any::<[u8; 32]>(),
        two in any::<bool>(),
    ) {
        let h = header(u8::from(two), 1 + u8::from(two));
        let mut out = [0u8; 200];
        let n = seal(&key, &h, &plain, &mut out).unwrap();
        prop_assert!(n <= PAYLOAD_MAX);
        // A stock node never tries to decrypt it.
        prop_assert!(!stock_would_try(n));
        prop_assert_eq!(parse(&out[..n]).map(|(h, _)| h), Some(h));
        let mut buf = out;
        let (got, p) = open(&key, &mut buf[..n]).unwrap();
        prop_assert_eq!(got, h);
        prop_assert_eq!(p, plain.as_slice());
        // Wrong key, or any flipped byte (pad excepted), fails.
        let mut buf = out;
        prop_assert!(open(&[0; 32], &mut buf[..n]).is_err() || key == [0; 32]);
        for i in 0..n {
            let mut t = out;
            t[i] ^= 0x01;
            prop_assert!(open(&key, &mut t[..n]).is_err(), "byte {}", i);
        }
    }

    #[test]
    fn parse_never_panics(payload in proptest::collection::vec(any::<u8>(), 0..200)) {
        let _ = parse(&payload);
        let mut p = payload.clone();
        let _ = open(&[1; 32], &mut p);
    }
}

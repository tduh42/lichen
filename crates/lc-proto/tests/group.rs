use lc_proto::group::{
    FRAGMENT_PLAIN_MAX, GroupHeader, PAYLOAD_MAX, fragments_for, open, parse, seal, stock_would_try,
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
    assert_eq!(FRAGMENT_PLAIN_MAX, 139);
    // TEXT capacity: content type + ts + text + 64-byte signature.
    assert_eq!(FRAGMENT_PLAIN_MAX - 1 - 4 - 64, 70);
    assert_eq!(2 * FRAGMENT_PLAIN_MAX - 1 - 4 - 64, 209);
    assert_eq!(fragments_for(139), Some(1));
    assert_eq!(fragments_for(140), Some(2));
    assert_eq!(fragments_for(279), None);
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

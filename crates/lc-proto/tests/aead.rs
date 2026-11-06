#![allow(deprecated)] // the reference crate renamed its in-place API; still correct for a cross-check

use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use lc_proto::aead::{open, seal};
use proptest::prelude::*;

#[test]
fn rfc8439_2_8_2() {
    let key: [u8; 32] = core::array::from_fn(|i| 0x80 + i as u8);
    let nonce: [u8; 12] = hex::decode("070000004041424344454647")
        .unwrap()
        .try_into()
        .unwrap();
    let aad = hex::decode("50515253c0c1c2c3c4c5c6c7").unwrap();
    let mut buf = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.".to_vec();
    let tag = seal(&key, &nonce, &aad, &mut buf);
    assert_eq!(
        hex::encode(&buf),
        "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b6116"
    );
    // RFC tag 1ae10b594f09e26a7e902ecbd0600691, truncated to 8 bytes.
    assert_eq!(hex::encode(tag), "1ae10b594f09e26a");
    assert!(open(&key, &nonce, &aad, &mut buf, &tag).is_ok());
    assert!(buf.starts_with(b"Ladies"));
}

proptest! {
    #[test]
    fn matches_reference_crate(
        key in any::<[u8; 32]>(),
        nonce in any::<[u8; 12]>(),
        aad in proptest::collection::vec(any::<u8>(), 0..40),
        plain in proptest::collection::vec(any::<u8>(), 0..200),
    ) {
        let mut ours = plain.clone();
        let tag = seal(&key, &nonce, &aad, &mut ours);
        let mut theirs = plain.clone();
        let full = ChaCha20Poly1305::new((&key).into())
            .encrypt_in_place_detached(Nonce::from_slice(&nonce), &aad, &mut theirs)
            .unwrap();
        prop_assert_eq!(&ours, &theirs);
        prop_assert_eq!(&tag[..], &full[..8]);
        let mut back = ours.clone();
        prop_assert!(open(&key, &nonce, &aad, &mut back, &tag).is_ok());
        prop_assert_eq!(back, plain);
    }

    #[test]
    fn any_bit_flip_is_rejected_and_buffer_untouched(
        plain in proptest::collection::vec(any::<u8>(), 1..160),
        flip in any::<usize>(),
    ) {
        let key = [7u8; 32];
        let nonce = [1u8; 12];
        let aad = [9u8; 7];
        let mut ct = plain.clone();
        let tag = seal(&key, &nonce, &aad, &mut ct);
        let total_bits = (ct.len() + 8 + 7) * 8;
        let bit = flip % total_bits;
        let (mut ct2, mut tag2, mut aad2) = (ct.clone(), tag, aad);
        let byte = bit / 8;
        if byte < ct.len() {
            ct2[byte] ^= 1 << (bit % 8);
        } else if byte < ct.len() + 8 {
            tag2[byte - ct.len()] ^= 1 << (bit % 8);
        } else {
            aad2[byte - ct.len() - 8] ^= 1 << (bit % 8);
        }
        let before = ct2.clone();
        prop_assert!(open(&key, &nonce, &aad2, &mut ct2, &tag2).is_err());
        prop_assert_eq!(ct2, before);
    }
}

//! The AEAD of every frame is RFC 8439's ChaCha20-Poly1305 (ADR-0028): its test vector, and a
//! cross-check against an independent assembly from the `chacha20` and `poly1305` primitives.

use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use lc_proto::TAG_LEN;
use lc_proto::aead::{open, seal};
use poly1305::Poly1305;
use poly1305::universal_hash::{KeyInit, UniversalHash};
use proptest::prelude::*;

/// RFC 8439 §2.8, written out from the two primitives: the reference `seal` is checked against.
fn reference(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], buf: &mut [u8]) -> [u8; 16] {
    let mut poly_key = [0u8; 32];
    ChaCha20::new(key.into(), nonce.into()).apply_keystream(&mut poly_key);
    let mut c = ChaCha20::new(key.into(), nonce.into());
    c.seek(64u32);
    c.apply_keystream(buf);
    let mut mac = Poly1305::new((&poly_key).into());
    mac.update_padded(aad);
    mac.update_padded(buf);
    let mut lengths = [0u8; 16];
    lengths[..8].copy_from_slice(&(aad.len() as u64).to_le_bytes());
    lengths[8..].copy_from_slice(&(buf.len() as u64).to_le_bytes());
    mac.update_padded(&lengths);
    mac.finalize().into()
}

#[test]
fn the_tag_is_the_full_one() {
    assert_eq!(TAG_LEN, 16);
}

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
    assert_eq!(hex::encode(tag), "1ae10b594f09e26a7e902ecbd0600691");
    assert!(open(&key, &nonce, &aad, &mut buf, &tag).is_ok());
    assert!(buf.starts_with(b"Ladies"));
}

/// The forgery that the 8-byte tag allowed with probability 2^-52 (ADR-0028): a zero byte
/// appended to the ciphertext, the tag kept or shifted. With the full tag it is rejected like
/// any other change.
#[test]
fn an_appended_byte_is_rejected() {
    let (key, nonce, aad) = ([3u8; 32], [5u8; 12], [9u8; 7]);
    let mut ct = b"see you at the pass".to_vec();
    let tag = seal(&key, &nonce, &aad, &mut ct);
    let mut longer = ct.clone();
    longer.push(0);
    assert!(open(&key, &nonce, &aad, &mut longer, &tag).is_err());
    let mut shorter = ct[..ct.len() - 1].to_vec();
    assert!(open(&key, &nonce, &aad, &mut shorter, &tag).is_err());
    assert!(open(&key, &nonce, &aad, &mut ct, &tag).is_ok());
}

proptest! {
    #[test]
    fn matches_the_independent_assembly(
        key in any::<[u8; 32]>(),
        nonce in any::<[u8; 12]>(),
        aad in proptest::collection::vec(any::<u8>(), 0..40),
        plain in proptest::collection::vec(any::<u8>(), 0..200),
    ) {
        let mut ours = plain.clone();
        let tag = seal(&key, &nonce, &aad, &mut ours);
        let mut theirs = plain.clone();
        let full = reference(&key, &nonce, &aad, &mut theirs);
        prop_assert_eq!(&ours, &theirs);
        prop_assert_eq!(tag, full);
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
        let total_bits = (ct.len() + TAG_LEN + 7) * 8;
        let bit = flip % total_bits;
        let (mut ct2, mut tag2, mut aad2) = (ct.clone(), tag, aad);
        let byte = bit / 8;
        if byte < ct.len() {
            ct2[byte] ^= 1 << (bit % 8);
        } else if byte < ct.len() + TAG_LEN {
            tag2[byte - ct.len()] ^= 1 << (bit % 8);
        } else {
            aad2[byte - ct.len() - TAG_LEN] ^= 1 << (bit % 8);
        }
        let before = ct2.clone();
        prop_assert!(open(&key, &nonce, &aad2, &mut ct2, &tag2).is_err());
        prop_assert_eq!(ct2, before);
    }
}

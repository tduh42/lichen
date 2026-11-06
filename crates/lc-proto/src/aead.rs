//! ChaCha20-Poly1305 (RFC 8439) with the tag truncated to its first 8 bytes, compared in
//! constant time. Built from the `chacha20` and `poly1305` primitives because the AEAD crates
//! only verify full 16-byte tags.

use crate::{Key, TAG_LEN};
use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use poly1305::Poly1305;
use poly1305::universal_hash::{KeyInit, UniversalHash};
use subtle::ConstantTimeEq;

/// The tag does not verify.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthError;

fn cipher(key: &Key, nonce: &[u8; 12]) -> ChaCha20 {
    ChaCha20::new(key.into(), nonce.into())
}

fn full_tag(key: &Key, nonce: &[u8; 12], aad: &[u8], ct: &[u8]) -> [u8; 16] {
    let mut poly_key = [0u8; 32];
    cipher(key, nonce).apply_keystream(&mut poly_key);
    let mut mac = Poly1305::new((&poly_key).into());
    mac.update_padded(aad);
    mac.update_padded(ct);
    let mut lengths = [0u8; 16];
    let (a, c) = lengths.split_at_mut(8);
    a.copy_from_slice(&(aad.len() as u64).to_le_bytes());
    c.copy_from_slice(&(ct.len() as u64).to_le_bytes());
    mac.update_padded(&lengths);
    mac.finalize().into()
}

fn truncate(tag: [u8; 16]) -> [u8; TAG_LEN] {
    let mut out = [0u8; TAG_LEN];
    for (o, t) in out.iter_mut().zip(tag) {
        *o = t;
    }
    out
}

/// Encrypts `buf` in place and returns the 8-byte tag.
pub fn seal(key: &Key, nonce: &[u8; 12], aad: &[u8], buf: &mut [u8]) -> [u8; TAG_LEN] {
    let mut c = cipher(key, nonce);
    c.seek(64u32);
    c.apply_keystream(buf);
    truncate(full_tag(key, nonce, aad, buf))
}

/// Verifies `tag` over the ciphertext `buf`, then decrypts it in place. `buf` is left
/// untouched when the tag is wrong.
pub fn open(
    key: &Key,
    nonce: &[u8; 12],
    aad: &[u8],
    buf: &mut [u8],
    tag: &[u8; TAG_LEN],
) -> Result<(), AuthError> {
    let expected = truncate(full_tag(key, nonce, aad, buf));
    if !bool::from(expected.ct_eq(tag)) {
        return Err(AuthError);
    }
    let mut c = cipher(key, nonce);
    c.seek(64u32);
    c.apply_keystream(buf);
    Ok(())
}

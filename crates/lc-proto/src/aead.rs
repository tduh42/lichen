//! ChaCha20-Poly1305 as RFC 8439 defines it, with its full 16-byte tag (ADR-0028), from the
//! `chacha20poly1305` crate: in place, the tag apart, no allocation. The tag is compared in
//! constant time and nothing is decrypted before it verifies.

use crate::{Key, TAG_LEN};
use chacha20poly1305::ChaCha20Poly1305;
use chacha20poly1305::aead::{AeadInOut, KeyInit};

/// The tag does not verify.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthError;

/// Encrypts `buf` in place and returns the tag.
pub fn seal(key: &Key, nonce: &[u8; 12], aad: &[u8], buf: &mut [u8]) -> [u8; TAG_LEN] {
    match ChaCha20Poly1305::new(key.into()).encrypt_inout_detached(nonce.into(), aad, buf.into()) {
        Ok(tag) => tag.into(),
        // The AEAD only refuses a message of 256 GiB or more, before it encrypts anything;
        // lichen's buffers hold one frame. Should it ever happen, nothing readable may leave:
        // the buffer is cleared, and an all-zero tag does not verify.
        Err(_) => {
            buf.fill(0);
            [0; TAG_LEN]
        }
    }
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
    ChaCha20Poly1305::new(key.into())
        .decrypt_inout_detached(nonce.into(), aad, buf.into(), tag.into())
        .map_err(|_| AuthError)
}

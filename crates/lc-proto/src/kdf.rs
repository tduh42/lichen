//! HKDF-SHA256 (RFC 5869) and X25519 helpers.

use crate::Key;
use curve25519_dalek::montgomery::MontgomeryPoint;
use hkdf::Hkdf;
use sha2::Sha256;

/// `HKDF(salt, ikm, info_0 ‖ info_1 ‖ …, 32)`.
pub fn hkdf(salt: &[u8], ikm: &[u8], info: &[&[u8]]) -> Key {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut okm = [0u8; 32];
    // 32 bytes is always a valid HKDF-SHA256 output length.
    let _ = hk.expand_multi_info(info, &mut okm);
    okm
}

/// X25519 public key of a 32-byte secret (clamped).
pub fn x25519_public(secret: &[u8; 32]) -> [u8; 32] {
    MontgomeryPoint::mul_base_clamped(*secret).to_bytes()
}

/// X25519 shared secret; `None` for an all-zero result (small-order peer key).
pub fn x25519(secret: &[u8; 32], peer: &[u8; 32]) -> Option<[u8; 32]> {
    let shared = MontgomeryPoint(*peer).mul_clamped(*secret).to_bytes();
    if shared.iter().all(|&b| b == 0) {
        None
    } else {
        Some(shared)
    }
}

//! MeshCore identities and ECDH (`oxymesh/docs/spec/air.md` §2).
//!
//! A private key is 64 "expanded" orlp bytes: `SHA512(seed)` with the scalar clamped in
//! `[0..32]` and the nonce prefix in `[32..64]`. The shared secret is
//! `X25519(clamp(prv[0..32]), u(peer))`, `u` being the Montgomery form of the peer's Ed25519 key.

use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use sha2::{Digest, Sha512};

/// Expands a 32-byte seed into a MeshCore private key (orlp `ed25519_create_keypair`).
pub fn expand_seed(seed: &[u8; 32]) -> [u8; 64] {
    let digest = Sha512::digest(seed);
    let mut prv = [0u8; 64];
    for (o, b) in prv.iter_mut().zip(digest.iter()) {
        *o = *b;
    }
    prv[0] &= 248;
    prv[31] &= 63;
    prv[31] |= 64;
    prv
}

fn scalar_bytes(prv: &[u8; 64]) -> [u8; 32] {
    let mut s = [0u8; 32];
    for (o, b) in s.iter_mut().zip(prv.iter()) {
        *o = *b;
    }
    s
}

/// The Ed25519 public key of a MeshCore private key.
pub fn public_key(prv: &[u8; 64]) -> [u8; 32] {
    EdwardsPoint::mul_base_clamped(scalar_bytes(prv))
        .compress()
        .to_bytes()
}

/// MeshCore static shared secret with `peer` (32 bytes). `None` if `peer` is not a valid
/// Ed25519 point or the result is all zeros.
pub fn shared_secret(prv: &[u8; 64], peer: &[u8; 32]) -> Option<[u8; 32]> {
    let point = CompressedEdwardsY(*peer).decompress()?;
    let secret = point
        .to_montgomery()
        .mul_clamped(scalar_bytes(prv))
        .to_bytes();
    if secret.iter().all(|&b| b == 0) {
        None
    } else {
        Some(secret)
    }
}

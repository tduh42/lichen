//! MeshCore identities: ECDH and Ed25519 signatures (`oxymesh/docs/spec/air.md` §2).
//!
//! A private key is 64 "expanded" orlp bytes: `SHA512(seed)` with the scalar clamped in
//! `[0..32]` and the nonce prefix in `[32..64]`. The shared secret is
//! `X25519(clamp(prv[0..32]), u(peer))`, `u` being the Montgomery form of the peer's Ed25519 key.

use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use curve25519_dalek::scalar::Scalar;
use ed25519_dalek::hazmat::{ExpandedSecretKey, raw_sign_byupdate};
use ed25519_dalek::{Signature, VerifyingKey};
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

/// The X25519 secret of a private key (its clamped scalar, MeshCore's ECDH side).
pub fn x25519_secret(prv: &[u8; 64]) -> [u8; 32] {
    scalar_bytes(prv)
}

/// The X25519 public key (Montgomery `u`) of an Ed25519 public key; `None` if it does not
/// decompress.
pub fn montgomery(public: &[u8; 32]) -> Option<[u8; 32]> {
    Some(
        CompressedEdwardsY(*public)
            .decompress()?
            .to_montgomery()
            .to_bytes(),
    )
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

fn expanded(prv: &[u8; 64]) -> ExpandedSecretKey {
    let mut hash_prefix = [0u8; 32];
    for (o, b) in hash_prefix.iter_mut().zip(prv.iter().skip(32)) {
        *o = *b;
    }
    ExpandedSecretKey {
        scalar: Scalar::from_bytes_mod_order(scalar_bytes(prv)),
        hash_prefix,
    }
}

/// Ed25519 signature of the concatenated `parts` with a MeshCore private key: the scalar is
/// `prv[0..32]` and the nonce prefix `prv[32..64]`, as orlp's `ed25519_sign`.
pub fn sign(prv: &[u8; 64], parts: &[&[u8]]) -> Option<[u8; 64]> {
    let esk = expanded(prv);
    let vk = VerifyingKey::from(&esk);
    raw_sign_byupdate::<Sha512, _>(
        &esk,
        |h: &mut Sha512| {
            for p in parts {
                h.update(p);
            }
            Ok(())
        },
        &vk,
    )
    .ok()
    .map(|s| s.to_bytes())
}

/// Strict Ed25519 verification of `sig` over the concatenated `parts`.
pub fn verify(key: &[u8; 32], parts: &[&[u8]], sig: &[u8; 64]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(key) else {
        return false;
    };
    let sig = Signature::from_bytes(sig);
    // `verify_strict` needs the message in one piece; parts are small (≤ a few hundred bytes).
    let mut msg = [0u8; 512];
    let mut n = 0;
    for p in parts {
        let Some(dst) = msg.get_mut(n..n + p.len()) else {
            return false;
        };
        dst.copy_from_slice(p);
        n += p.len();
    }
    msg.get(..n)
        .is_some_and(|m| vk.verify_strict(m, &sig).is_ok())
}

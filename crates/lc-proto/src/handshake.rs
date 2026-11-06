//! Handshake (§4): HELLO and HELLO_ACK under single-use keys derived from the static MeshCore
//! secret and a random salt, then per-direction session keys with forward secrecy.
//!
//! This module only builds, opens and derives. Which handshake is pending, crossing HELLOs and
//! retries are the caller's business (`lc-core`).

use crate::header::{FrameError, Header, Kind};
use crate::kdf::{hkdf, x25519};
use crate::session::SessionKeys;
use crate::{HEADER_LEN, Key, TAG_LEN, aead};
use lc_meshcore::path::{PATH_UNKNOWN, path_bytes};

/// Salt length.
pub const SALT_LEN: usize = 16;
const EPH_LEN: usize = 32;
const AAD_LEN: usize = HEADER_LEN + SALT_LEN;

/// A HELLO, to send or as received.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hello<'a> {
    /// Handshake identifier (random).
    pub tag: u16,
    pub attempt: u8,
    pub salt: [u8; SALT_LEN],
    /// Initiator's ephemeral X25519 public key.
    pub ephemeral: [u8; EPH_LEN],
    /// Path from the responder back to the initiator (`0xFF` = unknown, no bytes).
    pub return_path_len: u8,
    pub return_path: &'a [u8],
}

/// A HELLO_ACK, to send or as received.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelloAck {
    pub tag: u16,
    pub attempt: u8,
    pub salt: [u8; SALT_LEN],
    /// Responder's ephemeral X25519 public key.
    pub ephemeral: [u8; EPH_LEN],
}

fn hello_key(s: &Key, salt_i: &[u8; SALT_LEN], pub_i: &[u8; 32], pub_r: &[u8; 32]) -> Key {
    hkdf(salt_i, s, &[b"lichen/1 hello", pub_i, pub_r])
}

fn hello_ack_key(
    s: &Key,
    salt_r: &[u8; SALT_LEN],
    pub_r: &[u8; 32],
    pub_i: &[u8; 32],
    salt_i: &[u8; SALT_LEN],
) -> Key {
    hkdf(salt_r, s, &[b"lichen/1 hello-ack", pub_r, pub_i, salt_i])
}

fn control_header(me: &[u8; 32], peer: &[u8; 32], tag: u16, index: u8, attempt: u8) -> Header {
    Header {
        dst: peer[0],
        src: me[0],
        tag,
        kind: Kind::Control,
        index,
        total: 1,
        attempt,
    }
}

/// Writes `header ‖ salt ‖ seal(plain) ‖ tag` into `out`; returns the frame length.
fn seal_control(
    key: &Key,
    header: &Header,
    salt: &[u8; SALT_LEN],
    plain: &[u8],
    out: &mut [u8],
) -> Result<usize, FrameError> {
    let len = AAD_LEN + plain.len() + TAG_LEN;
    let frame = out.get_mut(..len).ok_or(FrameError::Overflow)?;
    let (aad, rest) = frame.split_at_mut(AAD_LEN);
    let (head, salt_slot) = aad.split_at_mut(HEADER_LEN);
    head.copy_from_slice(&header.encode());
    salt_slot.copy_from_slice(salt);
    let (ct, tag_slot) = rest.split_at_mut(plain.len());
    ct.copy_from_slice(plain);
    let tag = aead::seal(key, &header.nonce(), aad, ct);
    tag_slot.copy_from_slice(&tag);
    Ok(len)
}

/// Splits a control frame into `(header, salt, plaintext)` after verifying it under the key
/// returned by `key_for(salt)`. The plaintext is decrypted in place.
fn open_control<'a>(
    frame: &'a mut [u8],
    index: u8,
    me: &[u8; 32],
    peer: &[u8; 32],
    key_for: impl FnOnce(&[u8; SALT_LEN]) -> Key,
) -> Result<(Header, [u8; SALT_LEN], &'a [u8]), FrameError> {
    let (header, _) = Header::decode(frame)?;
    if header.kind != Kind::Control || header.index != index {
        return Err(FrameError::Malformed);
    }
    if header.dst != me[0] || header.src != peer[0] {
        return Err(FrameError::NotForUs);
    }
    if frame.len() < AAD_LEN + TAG_LEN {
        return Err(FrameError::Length);
    }
    let (aad, rest) = frame.split_at_mut(AAD_LEN);
    let ct_len = rest.len() - TAG_LEN;
    let (ct, tag) = rest.split_at_mut(ct_len);
    let tag: &[u8; TAG_LEN] = (&*tag).try_into().map_err(|_| FrameError::Length)?;
    let salt: [u8; SALT_LEN] = aad
        .get(HEADER_LEN..)
        .and_then(|s| s.try_into().ok())
        .ok_or(FrameError::Length)?;
    let key = key_for(&salt);
    aead::open(&key, &header.nonce(), aad, ct, tag).map_err(|_| FrameError::Auth)?;
    Ok((header, salt, ct))
}

fn valid_return_path(len_byte: u8, bytes: &[u8]) -> bool {
    if len_byte == PATH_UNKNOWN {
        bytes.is_empty()
    } else {
        path_bytes(len_byte) == Some(bytes.len())
    }
}

/// Builds a HELLO from `me` (initiator) to `peer`.
pub fn seal_hello(
    s: &Key,
    me: &[u8; 32],
    peer: &[u8; 32],
    hello: &Hello<'_>,
    out: &mut [u8],
) -> Result<usize, FrameError> {
    if !valid_return_path(hello.return_path_len, hello.return_path) {
        return Err(FrameError::Malformed);
    }
    let mut plain = [0u8; EPH_LEN + 2 + 64];
    let plain_len = EPH_LEN + 2 + hello.return_path.len();
    let p = plain.get_mut(..plain_len).ok_or(FrameError::Malformed)?;
    let (eph, rest) = p.split_at_mut(EPH_LEN);
    eph.copy_from_slice(&hello.ephemeral);
    let (flags_len, path) = rest.split_at_mut(2);
    flags_len.copy_from_slice(&[0, hello.return_path_len]);
    path.copy_from_slice(hello.return_path);
    let header = control_header(me, peer, hello.tag, 0, hello.attempt);
    let key = hello_key(s, &hello.salt, me, peer);
    seal_control(&key, &header, &hello.salt, p, out)
}

/// Opens a HELLO sent by `peer` (initiator) to `me` (responder). The frame is decrypted in
/// place and the return path borrows from it.
pub fn open_hello<'a>(
    s: &Key,
    peer: &[u8; 32],
    me: &[u8; 32],
    frame: &'a mut [u8],
) -> Result<Hello<'a>, FrameError> {
    let (header, salt, plain) =
        open_control(frame, 0, me, peer, |salt| hello_key(s, salt, peer, me))?;
    let ephemeral: [u8; EPH_LEN] = plain
        .get(..EPH_LEN)
        .and_then(|e| e.try_into().ok())
        .ok_or(FrameError::Malformed)?;
    let return_path_len = *plain.get(EPH_LEN + 1).ok_or(FrameError::Malformed)?;
    let return_path = plain.get(EPH_LEN + 2..).ok_or(FrameError::Malformed)?;
    if !valid_return_path(return_path_len, return_path) {
        return Err(FrameError::Malformed);
    }
    Ok(Hello {
        tag: header.tag,
        attempt: header.attempt,
        salt,
        ephemeral,
        return_path_len,
        return_path,
    })
}

/// Builds a HELLO_ACK from `me` (responder) to `peer`, answering the HELLO salted `salt_i`.
pub fn seal_hello_ack(
    s: &Key,
    me: &[u8; 32],
    peer: &[u8; 32],
    salt_i: &[u8; SALT_LEN],
    ack: &HelloAck,
    out: &mut [u8],
) -> Result<usize, FrameError> {
    let mut plain = [0u8; EPH_LEN + 1];
    let (eph, _flags) = plain.split_at_mut(EPH_LEN);
    eph.copy_from_slice(&ack.ephemeral);
    let header = control_header(me, peer, ack.tag, 1, ack.attempt);
    let key = hello_ack_key(s, &ack.salt, me, peer, salt_i);
    seal_control(&key, &header, &ack.salt, &plain, out)
}

/// Opens a HELLO_ACK sent by `peer` (responder) to `me` (initiator) for our HELLO salted
/// `salt_i`.
pub fn open_hello_ack(
    s: &Key,
    peer: &[u8; 32],
    me: &[u8; 32],
    salt_i: &[u8; SALT_LEN],
    frame: &mut [u8],
) -> Result<HelloAck, FrameError> {
    let (header, salt, plain) = open_control(frame, 1, me, peer, |salt_r| {
        hello_ack_key(s, salt_r, peer, me, salt_i)
    })?;
    let ephemeral: [u8; EPH_LEN] = plain
        .get(..EPH_LEN)
        .and_then(|e| e.try_into().ok())
        .ok_or(FrameError::Malformed)?;
    Ok(HelloAck {
        tag: header.tag,
        attempt: header.attempt,
        salt,
        ephemeral,
    })
}

/// Session keys (§4.3). `initiator` tells which side we are; `None` if the ephemeral exchange
/// gives an all-zero secret.
#[allow(clippy::too_many_arguments)]
pub fn session_keys(
    s: &Key,
    my_ephemeral_secret: &[u8; 32],
    peer_ephemeral: &[u8; 32],
    salt_i: &[u8; SALT_LEN],
    salt_r: &[u8; SALT_LEN],
    pub_i: &[u8; 32],
    pub_r: &[u8; 32],
    initiator: bool,
) -> Option<SessionKeys> {
    let dh = x25519(my_ephemeral_secret, peer_ephemeral)?;
    let mut ikm = [0u8; 64];
    let (a, b) = ikm.split_at_mut(32);
    a.copy_from_slice(s);
    b.copy_from_slice(&dh);
    let mut salt = [0u8; 2 * SALT_LEN];
    let (x, y) = salt.split_at_mut(SALT_LEN);
    x.copy_from_slice(salt_i);
    y.copy_from_slice(salt_r);
    let k_ir = hkdf(&salt, &ikm, &[b"lichen/1 data", pub_i, pub_r]);
    let k_ri = hkdf(&salt, &ikm, &[b"lichen/1 data", pub_r, pub_i]);
    Some(if initiator {
        SessionKeys {
            send: k_ir,
            recv: k_ri,
        }
    } else {
        SessionKeys {
            send: k_ri,
            recv: k_ir,
        }
    })
}

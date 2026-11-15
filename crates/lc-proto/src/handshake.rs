//! Handshake (§4, ADR-0029): HELLO and HELLO_ACK, then per-direction session keys.
//!
//! Each side's ephemeral key travels in clear and every Diffie-Hellman term between the
//! static and ephemeral keys goes into the keys, with a hash of what both sides said:
//!
//! - HELLO is sealed under `S ‖ es`: only someone holding the initiator's static key and its
//!   ephemeral, or the responder's static key, can read or make it.
//! - HELLO_ACK and the session add `ee` and `se`: a stolen static key of the initiator no
//!   longer lets anyone answer as the responder (`es` needs the responder's static key or the
//!   initiator's ephemeral), and the session has forward secrecy.
//! - The transcript (salts, ephemerals, versions, the return path) is bound into HELLO_ACK's
//!   key and the session keys: nothing of it can be changed on the way.
//!
//! This module only builds, opens and derives. Which handshake is pending, crossing HELLOs,
//! freshness and retries are the caller's business (`lc-core`).

use crate::header::{FrameError, Header, Kind};
use crate::kdf::{hkdf, x25519};
use crate::session::SessionKeys;
use crate::{HEADER_LEN, Key, TAG_LEN, aead};
use lc_meshcore::ecdh::{montgomery, x25519_secret};
use lc_meshcore::path::{PATH_UNKNOWN, path_bytes};
use sha2::{Digest, Sha256};

/// Salt length. A HELLO's salt starts with its sender's Unix time (§4.1).
pub const SALT_LEN: usize = 16;
/// The protocol version this phone speaks (ADR-0029), carried in HELLO, HELLO_ACK and the
/// introduction. Version 1 was the 8-byte-tag protocol before 2026-11-15.
pub const VERSION: u8 = 2;
const EPH_LEN: usize = 32;
/// The clear part after the header: salt and ephemeral key.
const CLEAR_LEN: usize = SALT_LEN + EPH_LEN;
const AAD_LEN: usize = HEADER_LEN + CLEAR_LEN;

/// The two ends of a handshake: identity keys for the key derivation (§2), node hashes for the
/// frame header (§3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ends<'a> {
    pub me: &'a [u8; 32],
    pub peer: &'a [u8; 32],
    pub me_node: u8,
    pub peer_node: u8,
}

/// A HELLO, to send or as received.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hello<'a> {
    /// Handshake identifier (random).
    pub tag: u16,
    pub attempt: u8,
    /// `t_I u32 ‖ 12 random bytes`: the initiator's Unix seconds (0 unknown), for freshness.
    pub salt: [u8; SALT_LEN],
    /// Initiator's ephemeral X25519 public key.
    pub ephemeral: [u8; EPH_LEN],
    /// The initiator's protocol version.
    pub version: u8,
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
    /// The responder's protocol version.
    pub version: u8,
}

/// The initiator's Unix time in a HELLO's salt (0 when it did not know it).
pub const fn salt_time(salt: &[u8; SALT_LEN]) -> u32 {
    let [a, b, c, d, ..] = *salt;
    u32::from_le_bytes([a, b, c, d])
}

/// A salt for a HELLO sent at Unix time `unix_s` (0 unknown), with 12 random bytes.
pub fn hello_salt(unix_s: u32, random: &[u8; 12]) -> [u8; SALT_LEN] {
    let mut s = [0u8; SALT_LEN];
    let (t, r) = s.split_at_mut(4);
    t.copy_from_slice(&unix_s.to_le_bytes());
    r.copy_from_slice(random);
    s
}

/// `X25519(secret, u(identity))`: a Diffie-Hellman term with an identity key (§2). `None`
/// when the key does not decompress or the result is zero.
fn dh_identity(secret: &[u8; 32], identity: &[u8; 32]) -> Option<[u8; 32]> {
    x25519(secret, &montgomery(identity)?)
}

fn ikm<const N: usize>(parts: [&[u8; 32]; N]) -> [[u8; 32]; N] {
    parts.map(|p| *p)
}

fn hello_key(s: &Key, es: &[u8; 32], hello: &Hello<'_>, id_i: &[u8; 32], id_r: &[u8; 32]) -> Key {
    let ikm = ikm([s, es]);
    hkdf(
        &hello.salt,
        ikm.as_flattened(),
        &[b"lichen/2 hello", id_i, id_r, &hello.ephemeral],
    )
}

/// The transcript up to the responder's ephemeral: what HELLO_ACK's key binds.
fn transcript(
    id_i: &[u8; 32],
    id_r: &[u8; 32],
    hello: &Hello<'_>,
    salt_r: &[u8; SALT_LEN],
    e_r: &[u8; EPH_LEN],
) -> [u8; 32] {
    Sha256::new()
        .chain_update(b"lichen/2 transcript")
        .chain_update(id_i)
        .chain_update(id_r)
        .chain_update(hello.salt)
        .chain_update(hello.ephemeral)
        .chain_update([hello.version, hello.return_path_len])
        .chain_update(hello.return_path)
        .chain_update(salt_r)
        .chain_update(e_r)
        .finalize()
        .into()
}

/// The four Diffie-Hellman results of a handshake, from one side.
struct Terms {
    s: Key,
    es: [u8; 32],
    ee: [u8; 32],
    se: [u8; 32],
}

impl Terms {
    /// The initiator's: its ephemeral secret and static key, the responder's ephemeral.
    fn initiator(
        s: &Key,
        prv_i: &[u8; 64],
        eph_i: &[u8; 32],
        id_r: &[u8; 32],
        e_r: &[u8; 32],
    ) -> Option<Self> {
        Some(Self {
            s: *s,
            es: dh_identity(eph_i, id_r)?,
            ee: x25519(eph_i, e_r)?,
            se: x25519(&x25519_secret(prv_i), e_r)?,
        })
    }

    /// The responder's: its static key and ephemeral secret, the initiator's ephemeral.
    fn responder(
        s: &Key,
        prv_r: &[u8; 64],
        eph_r: &[u8; 32],
        id_i: &[u8; 32],
        e_i: &[u8; 32],
    ) -> Option<Self> {
        Some(Self {
            s: *s,
            es: x25519(&x25519_secret(prv_r), e_i)?,
            ee: x25519(eph_r, e_i)?,
            se: dh_identity(eph_r, id_i)?,
        })
    }

    fn ikm(&self) -> [[u8; 32]; 4] {
        ikm([&self.s, &self.es, &self.ee, &self.se])
    }

    fn ack_key(&self, salt_r: &[u8; SALT_LEN], h1: &[u8; 32]) -> Key {
        hkdf(
            salt_r,
            self.ikm().as_flattened(),
            &[b"lichen/2 hello-ack", h1],
        )
    }

    /// `(K_{I→R}, K_{R→I})`.
    fn session(
        &self,
        salt_i: &[u8; SALT_LEN],
        salt_r: &[u8; SALT_LEN],
        id_i: &[u8; 32],
        id_r: &[u8; 32],
        h2: &[u8; 32],
    ) -> (Key, Key) {
        let mut salt = [0u8; 2 * SALT_LEN];
        let (a, b) = salt.split_at_mut(SALT_LEN);
        a.copy_from_slice(salt_i);
        b.copy_from_slice(salt_r);
        let ikm = self.ikm();
        (
            hkdf(
                &salt,
                ikm.as_flattened(),
                &[b"lichen/2 data", id_i, id_r, h2],
            ),
            hkdf(
                &salt,
                ikm.as_flattened(),
                &[b"lichen/2 data", id_r, id_i, h2],
            ),
        )
    }
}

/// The whole transcript: `h1` and the responder's version.
fn transcript_end(h1: &[u8; 32], version_r: u8) -> [u8; 32] {
    Sha256::new()
        .chain_update(h1)
        .chain_update([version_r])
        .finalize()
        .into()
}

pub(crate) const fn control_header(
    me_node: u8,
    peer_node: u8,
    tag: u16,
    index: u8,
    attempt: u8,
) -> Header {
    Header {
        dst: peer_node,
        src: me_node,
        tag,
        kind: Kind::Control,
        index,
        total: 1,
        attempt,
    }
}

/// Writes `header ‖ salt ‖ ephemeral ‖ seal(plain) ‖ tag` into `out`; returns the length.
fn seal_control(
    key: &Key,
    header: &Header,
    salt: &[u8; SALT_LEN],
    ephemeral: &[u8; EPH_LEN],
    plain: &[u8],
    out: &mut [u8],
) -> Result<usize, FrameError> {
    let len = AAD_LEN + plain.len() + TAG_LEN;
    let frame = out.get_mut(..len).ok_or(FrameError::Overflow)?;
    let (aad, rest) = frame.split_at_mut(AAD_LEN);
    let (head, clear) = aad.split_at_mut(HEADER_LEN);
    head.copy_from_slice(&header.encode());
    let (salt_slot, eph_slot) = clear.split_at_mut(SALT_LEN);
    salt_slot.copy_from_slice(salt);
    eph_slot.copy_from_slice(ephemeral);
    let (ct, tag_slot) = rest.split_at_mut(plain.len());
    ct.copy_from_slice(plain);
    let tag = aead::seal(key, &header.nonce(), aad, ct);
    tag_slot.copy_from_slice(&tag);
    Ok(len)
}

/// An opened control frame: header, salt, ephemeral, plaintext.
type Opened<'a> = (Header, [u8; SALT_LEN], [u8; EPH_LEN], &'a [u8]);

/// Splits a control frame into `(header, salt, ephemeral, plaintext)` after verifying it under
/// the key `key_for(salt, ephemeral)` returns (`None`: refused). Decrypted in place.
fn open_control(
    frame: &mut [u8],
    index: u8,
    me_node: u8,
    peer_node: u8,
    key_for: impl FnOnce(&[u8; SALT_LEN], &[u8; EPH_LEN]) -> Option<Key>,
) -> Result<Opened<'_>, FrameError> {
    let (header, _) = Header::decode(frame)?;
    if header.kind != Kind::Control || header.index != index {
        return Err(FrameError::Malformed);
    }
    if header.dst != me_node || header.src != peer_node {
        return Err(FrameError::NotForUs);
    }
    if frame.len() < AAD_LEN + TAG_LEN {
        return Err(FrameError::Length);
    }
    let (aad, rest) = frame.split_at_mut(AAD_LEN);
    let ct_len = rest.len() - TAG_LEN;
    let (ct, tag) = rest.split_at_mut(ct_len);
    let tag: &[u8; TAG_LEN] = (&*tag).try_into().map_err(|_| FrameError::Length)?;
    let clear = aad.get(HEADER_LEN..).ok_or(FrameError::Length)?;
    let salt: [u8; SALT_LEN] = clear
        .get(..SALT_LEN)
        .and_then(|s| s.try_into().ok())
        .ok_or(FrameError::Length)?;
    let ephemeral: [u8; EPH_LEN] = clear
        .get(SALT_LEN..)
        .and_then(|e| e.try_into().ok())
        .ok_or(FrameError::Length)?;
    let key = key_for(&salt, &ephemeral).ok_or(FrameError::Auth)?;
    aead::open(&key, &header.nonce(), aad, ct, tag).map_err(|_| FrameError::Auth)?;
    Ok((header, salt, ephemeral, ct))
}

pub(crate) fn valid_return_path(len_byte: u8, bytes: &[u8]) -> bool {
    if len_byte == PATH_UNKNOWN {
        bytes.is_empty()
    } else {
        path_bytes(len_byte) == Some(bytes.len())
    }
}

/// Builds a HELLO from `ends.me` (initiator) to `ends.peer`; `eph_secret` is the secret of
/// `hello.ephemeral`.
pub fn seal_hello(
    s: &Key,
    ends: &Ends<'_>,
    eph_secret: &[u8; 32],
    hello: &Hello<'_>,
    out: &mut [u8],
) -> Result<usize, FrameError> {
    if !valid_return_path(hello.return_path_len, hello.return_path) {
        return Err(FrameError::Malformed);
    }
    let es = dh_identity(eph_secret, ends.peer).ok_or(FrameError::Malformed)?;
    let mut plain = [0u8; 2 + 64];
    let plain_len = 2 + hello.return_path.len();
    let p = plain.get_mut(..plain_len).ok_or(FrameError::Malformed)?;
    let (head, path) = p.split_at_mut(2);
    head.copy_from_slice(&[hello.version, hello.return_path_len]);
    path.copy_from_slice(hello.return_path);
    let header = control_header(ends.me_node, ends.peer_node, hello.tag, 0, hello.attempt);
    let key = hello_key(s, &es, hello, ends.me, ends.peer);
    seal_control(&key, &header, &hello.salt, &hello.ephemeral, p, out)
}

/// Opens a HELLO sent by `ends.peer` (initiator) to `ends.me` (responder), whose expanded
/// private key is `prv`. The frame is decrypted in place and the return path borrows from it.
pub fn open_hello<'a>(
    s: &Key,
    ends: &Ends<'_>,
    prv: &[u8; 64],
    frame: &'a mut [u8],
) -> Result<Hello<'a>, FrameError> {
    let (header, salt, ephemeral, plain) =
        open_control(frame, 0, ends.me_node, ends.peer_node, |salt, e_i| {
            let es = x25519(&x25519_secret(prv), e_i)?;
            let probe = Hello {
                tag: 0,
                attempt: 0,
                salt: *salt,
                ephemeral: *e_i,
                version: 0,
                return_path_len: PATH_UNKNOWN,
                return_path: &[],
            };
            Some(hello_key(s, &es, &probe, ends.peer, ends.me))
        })?;
    let [version, return_path_len, ..] = *plain else {
        return Err(FrameError::Malformed);
    };
    let return_path = plain.get(2..).ok_or(FrameError::Malformed)?;
    if !valid_return_path(return_path_len, return_path) {
        return Err(FrameError::Malformed);
    }
    Ok(Hello {
        tag: header.tag,
        attempt: header.attempt,
        salt,
        ephemeral,
        version,
        return_path_len,
        return_path,
    })
}

/// Builds a HELLO_ACK from `ends.me` (responder, expanded private key `prv`, ephemeral secret
/// `eph_secret`) answering `hello`, and returns its length with the session keys.
pub fn seal_hello_ack(
    s: &Key,
    ends: &Ends<'_>,
    prv: &[u8; 64],
    eph_secret: &[u8; 32],
    hello: &Hello<'_>,
    ack: &HelloAck,
    out: &mut [u8],
) -> Result<(usize, SessionKeys), FrameError> {
    let terms = Terms::responder(s, prv, eph_secret, ends.peer, &hello.ephemeral)
        .ok_or(FrameError::Malformed)?;
    let h1 = transcript(ends.peer, ends.me, hello, &ack.salt, &ack.ephemeral);
    let header = control_header(ends.me_node, ends.peer_node, ack.tag, 1, ack.attempt);
    let key = terms.ack_key(&ack.salt, &h1);
    let n = seal_control(
        &key,
        &header,
        &ack.salt,
        &ack.ephemeral,
        &[ack.version],
        out,
    )?;
    let h2 = transcript_end(&h1, ack.version);
    let (k_ir, k_ri) = terms.session(&hello.salt, &ack.salt, ends.peer, ends.me, &h2);
    Ok((
        n,
        SessionKeys {
            send: k_ri,
            recv: k_ir,
        },
    ))
}

/// Opens a HELLO_ACK sent by `ends.peer` (responder) to `ends.me` (initiator, expanded
/// private key `prv`) for our `hello`, whose ephemeral secret is `eph_secret`; returns it with
/// the session keys.
pub fn open_hello_ack(
    s: &Key,
    ends: &Ends<'_>,
    prv: &[u8; 64],
    eph_secret: &[u8; 32],
    hello: &Hello<'_>,
    frame: &mut [u8],
) -> Result<(HelloAck, SessionKeys), FrameError> {
    let mut terms = None;
    let mut h1 = [0u8; 32];
    let (header, salt, ephemeral, plain) =
        open_control(frame, 1, ends.me_node, ends.peer_node, |salt_r, e_r| {
            let t = Terms::initiator(s, prv, eph_secret, ends.peer, e_r)?;
            h1 = transcript(ends.me, ends.peer, hello, salt_r, e_r);
            let key = t.ack_key(salt_r, &h1);
            terms = Some(t);
            Some(key)
        })?;
    let terms = terms.ok_or(FrameError::Auth)?;
    let [version, ..] = *plain else {
        return Err(FrameError::Malformed);
    };
    let h2 = transcript_end(&h1, version);
    let (k_ir, k_ri) = terms.session(&hello.salt, &salt, ends.me, ends.peer, &h2);
    Ok((
        HelloAck {
            tag: header.tag,
            attempt: header.attempt,
            salt,
            ephemeral,
            version,
        },
        SessionKeys {
            send: k_ir,
            recv: k_ri,
        },
    ))
}

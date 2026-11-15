//! Introduction (§4.6, ADR-0013): a first contact in three messages on the Noise XX pattern.
//! Identities travel encrypted under ephemeral keys, and each side proves it holds its
//! identity key (`es` for the responder, `se` for the initiator).
//!
//! INTRO (I → R, in clear) ‖ INTRO_ACK (R → I: `e_R`, `id_R`, proof) ‖ INTRO_FIN (I → R:
//! `id_I`, proof). Both sides then hold the session keys. Which introduction is pending and
//! the retries are the caller's business (`lc-core`).

use crate::handshake::{SALT_LEN, VERSION, control_header, valid_return_path};
use crate::header::{FrameError, Header, Kind};
use crate::kdf::{hkdf, x25519, x25519_public};
use crate::session::SessionKeys;
use crate::{HEADER_LEN, Key, TAG_LEN, aead};
use lc_meshcore::ecdh::{montgomery, x25519_secret};
use sha2::{Digest, Sha256};

const EPH: usize = 32;
const ID: usize = 32;
const INTRO_HEAD: usize = HEADER_LEN + SALT_LEN + EPH;
/// INTRO_ACK body: `salt_R ‖ e_R ‖ seal(id_R) ‖ seal(flags)`.
pub const INTRO_ACK_LEN: usize = INTRO_HEAD + ID + TAG_LEN + 1 + TAG_LEN;
/// INTRO_FIN body: `seal(id_I) ‖ seal(flags)`.
pub const INTRO_FIN_LEN: usize = HEADER_LEN + ID + TAG_LEN + 1 + TAG_LEN;

/// An INTRO, to send or as received.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Intro<'a> {
    /// Introduction identifier (random).
    pub tag: u16,
    pub attempt: u8,
    pub salt: [u8; SALT_LEN],
    /// Initiator's ephemeral X25519 public key.
    pub ephemeral: [u8; EPH],
    /// Path from the responder back to the initiator (`0xFF` = unknown, no bytes).
    pub return_path_len: u8,
    pub return_path: &'a [u8],
}

/// What the initiator learns from an INTRO_ACK.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntroAck {
    pub tag: u16,
    pub attempt: u8,
    pub salt: [u8; SALT_LEN],
    pub ephemeral: [u8; EPH],
    /// The responder's identity, proven.
    pub identity: [u8; ID],
    /// The responder's protocol version (ADR-0029).
    pub version: u8,
}

/// The responder's side of an introduction it answered. Its ephemeral secret is never printed
/// and is wiped when dropped (review of 2026-11-15, SR-07).
#[derive(Clone, PartialEq, Eq)]
pub struct Answer {
    pub salt_i: [u8; SALT_LEN],
    pub e_i: [u8; EPH],
    pub salt_r: [u8; SALT_LEN],
    pub eph_secret: [u8; 32],
    /// [`intro_hash`] of the INTRO answered: its return path is bound into every key.
    pub intro_hash: [u8; 32],
}

impl core::fmt::Debug for Answer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Answer")
            .field("salt_i", &self.salt_i)
            .field("e_i", &self.e_i)
            .field("salt_r", &self.salt_r)
            .finish_non_exhaustive()
    }
}

impl Drop for Answer {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.eph_secret.zeroize();
    }
}

/// The hash of an INTRO as both sides bind it into the introduction's keys (ADR-0029): its
/// salt, ephemeral and return path. A return path changed on the way gives other keys, and the
/// introduction fails instead of leading the answer astray.
pub fn intro_hash(intro: &Intro<'_>) -> [u8; 32] {
    Sha256::new()
        .chain_update(b"lichen/2 intro")
        .chain_update(intro.salt)
        .chain_update(intro.ephemeral)
        .chain_update([intro.return_path_len])
        .chain_update(intro.return_path)
        .finalize()
        .into()
}

fn salts(salt_i: &[u8; SALT_LEN], salt_r: &[u8; SALT_LEN]) -> [u8; 2 * SALT_LEN] {
    let mut s = [0u8; 2 * SALT_LEN];
    let (a, b) = s.split_at_mut(SALT_LEN);
    a.copy_from_slice(salt_i);
    b.copy_from_slice(salt_r);
    s
}

fn concat<const N: usize>(parts: [&[u8; 32]; N]) -> [[u8; 32]; N] {
    parts.map(|p| *p)
}

/// The Diffie-Hellman results of an introduction and the keys built on them.
struct Chain {
    salt: [u8; 2 * SALT_LEN],
    e_i: [u8; EPH],
    e_r: [u8; EPH],
    ee: [u8; 32],
    /// The INTRO and the responder's ephemeral: bound into every key.
    h: [u8; 32],
}

/// `SHA-256(intro_hash ‖ salt_R ‖ e_R)`.
fn chain_hash(intro_hash: &[u8; 32], salt_r: &[u8; SALT_LEN], e_r: &[u8; EPH]) -> [u8; 32] {
    Sha256::new()
        .chain_update(intro_hash)
        .chain_update(salt_r)
        .chain_update(e_r)
        .finalize()
        .into()
}

impl Chain {
    fn k1(&self) -> Key {
        hkdf(
            &self.salt,
            &self.ee,
            &[b"lichen/2 intro r", &self.e_i, &self.e_r, &self.h],
        )
    }

    fn ikm2(&self, es: &[u8; 32]) -> [u8; 64] {
        let mut ikm = [0u8; 64];
        let (a, b) = ikm.split_at_mut(32);
        a.copy_from_slice(&self.ee);
        b.copy_from_slice(es);
        ikm
    }

    fn ikm3(&self, es: &[u8; 32], se: &[u8; 32]) -> [u8; 96] {
        let mut ikm = [0u8; 96];
        for (chunk, part) in ikm
            .as_chunks_mut::<32>()
            .0
            .iter_mut()
            .zip(concat([&self.ee, es, se]))
        {
            *chunk = part;
        }
        ikm
    }

    fn k2(&self, es: &[u8; 32], id_r: &[u8; 32]) -> Key {
        hkdf(
            &self.salt,
            &self.ikm2(es),
            &[b"lichen/2 intro r2", &self.e_i, &self.e_r, id_r, &self.h],
        )
    }

    fn k3(&self, es: &[u8; 32], id_r: &[u8; 32]) -> Key {
        hkdf(
            &self.salt,
            &self.ikm2(es),
            &[b"lichen/2 intro i", &self.e_i, &self.e_r, id_r, &self.h],
        )
    }

    fn k4(&self, es: &[u8; 32], se: &[u8; 32], id_r: &[u8; 32], id_i: &[u8; 32]) -> Key {
        hkdf(
            &self.salt,
            &self.ikm3(es, se),
            &[
                b"lichen/2 intro i2",
                &self.e_i,
                &self.e_r,
                id_r,
                id_i,
                &self.h,
            ],
        )
    }

    /// `(K_{I→R}, K_{R→I})`.
    fn session(
        &self,
        es: &[u8; 32],
        se: &[u8; 32],
        id_i: &[u8; 32],
        id_r: &[u8; 32],
    ) -> (Key, Key) {
        let ikm = self.ikm3(es, se);
        (
            hkdf(&self.salt, &ikm, &[b"lichen/2 data", id_i, id_r, &self.h]),
            hkdf(&self.salt, &ikm, &[b"lichen/2 data", id_r, id_i, &self.h]),
        )
    }
}

/// Builds an INTRO from node `me_node` to node `peer_node`. Returns the frame length.
pub fn seal_intro(
    me_node: u8,
    peer_node: u8,
    intro: &Intro<'_>,
    out: &mut [u8],
) -> Result<usize, FrameError> {
    if !valid_return_path(intro.return_path_len, intro.return_path) {
        return Err(FrameError::Malformed);
    }
    let len = INTRO_HEAD + 1 + intro.return_path.len();
    let frame = out.get_mut(..len).ok_or(FrameError::Overflow)?;
    let header = control_header(me_node, peer_node, intro.tag, 3, intro.attempt);
    let (head, rest) = frame.split_at_mut(HEADER_LEN);
    head.copy_from_slice(&header.encode());
    let (salt, rest) = rest.split_at_mut(SALT_LEN);
    salt.copy_from_slice(&intro.salt);
    let (eph, rest) = rest.split_at_mut(EPH);
    eph.copy_from_slice(&intro.ephemeral);
    let (l, path) = rest.split_at_mut(1);
    l.copy_from_slice(&[intro.return_path_len]);
    path.copy_from_slice(intro.return_path);
    Ok(len)
}

/// Reads an INTRO addressed to node `me_node`; returns the sender's node hash and the INTRO.
pub fn open_intro(me_node: u8, frame: &[u8]) -> Result<(u8, Intro<'_>), FrameError> {
    let (header, _) = Header::decode(frame)?;
    if header.kind != Kind::Control || header.index != 3 {
        return Err(FrameError::Malformed);
    }
    if header.dst != me_node {
        return Err(FrameError::NotForUs);
    }
    let salt: [u8; SALT_LEN] = frame
        .get(HEADER_LEN..HEADER_LEN + SALT_LEN)
        .and_then(|s| s.try_into().ok())
        .ok_or(FrameError::Length)?;
    let ephemeral: [u8; EPH] = frame
        .get(HEADER_LEN + SALT_LEN..INTRO_HEAD)
        .and_then(|s| s.try_into().ok())
        .ok_or(FrameError::Length)?;
    let (&return_path_len, return_path) = frame
        .get(INTRO_HEAD..)
        .and_then(<[u8]>::split_first)
        .ok_or(FrameError::Length)?;
    if !valid_return_path(return_path_len, return_path) {
        return Err(FrameError::Malformed);
    }
    Ok((
        header.src,
        Intro {
            tag: header.tag,
            attempt: header.attempt,
            salt,
            ephemeral,
            return_path_len,
            return_path,
        },
    ))
}

/// Builds the INTRO_ACK answering `intro` (responder side). `answer` holds the responder's
/// salt and ephemeral secret: the same for every resend of the same INTRO.
#[allow(clippy::too_many_arguments)]
pub fn seal_intro_ack(
    me_node: u8,
    peer_node: u8,
    my_prv: &[u8; 64],
    my_id: &[u8; 32],
    answer: &Answer,
    tag: u16,
    attempt: u8,
    out: &mut [u8],
) -> Result<usize, FrameError> {
    let e_r = x25519_public(&answer.eph_secret);
    let chain = Chain {
        salt: salts(&answer.salt_i, &answer.salt_r),
        e_i: answer.e_i,
        e_r,
        ee: x25519(&answer.eph_secret, &answer.e_i).ok_or(FrameError::Auth)?,
        h: chain_hash(&answer.intro_hash, &answer.salt_r, &e_r),
    };
    let es = x25519(&x25519_secret(my_prv), &answer.e_i).ok_or(FrameError::Auth)?;
    let frame = out.get_mut(..INTRO_ACK_LEN).ok_or(FrameError::Overflow)?;
    let header = control_header(me_node, peer_node, tag, 4, attempt);
    let (first, second) = frame.split_at_mut(INTRO_HEAD + ID + TAG_LEN);
    {
        let (aad, rest) = first.split_at_mut(INTRO_HEAD);
        let (head, rest_aad) = aad.split_at_mut(HEADER_LEN);
        head.copy_from_slice(&header.encode());
        let (salt, eph) = rest_aad.split_at_mut(SALT_LEN);
        salt.copy_from_slice(&answer.salt_r);
        eph.copy_from_slice(&e_r);
        let (ct, t) = rest.split_at_mut(ID);
        ct.copy_from_slice(my_id);
        t.copy_from_slice(&aead::seal(&chain.k1(), &header.nonce(), aad, ct));
    }
    let (ct, t) = second.split_at_mut(1);
    ct.copy_from_slice(&[VERSION]);
    t.copy_from_slice(&aead::seal(
        &chain.k2(&es, my_id),
        &header.nonce(),
        first,
        ct,
    ));
    Ok(INTRO_ACK_LEN)
}

/// Opens the INTRO_ACK from node `peer_node` answering our INTRO (`salt_i`, ephemeral
/// secret `my_eph_secret`, [`intro_hash`] `intro_hash`). The responder's identity is proven
/// on success.
pub fn open_intro_ack(
    me_node: u8,
    peer_node: u8,
    my_eph_secret: &[u8; 32],
    salt_i: &[u8; SALT_LEN],
    intro_hash: &[u8; 32],
    frame: &[u8],
) -> Result<IntroAck, FrameError> {
    let (header, _) = Header::decode(frame)?;
    if header.kind != Kind::Control || header.index != 4 {
        return Err(FrameError::Malformed);
    }
    if header.dst != me_node || header.src != peer_node {
        return Err(FrameError::NotForUs);
    }
    if frame.len() != INTRO_ACK_LEN {
        return Err(FrameError::Length);
    }
    let mut buf = [0u8; INTRO_ACK_LEN];
    buf.copy_from_slice(frame);
    let (first, second) = buf.split_at_mut(INTRO_HEAD + ID + TAG_LEN);
    let salt_r: [u8; SALT_LEN] = first
        .get(HEADER_LEN..HEADER_LEN + SALT_LEN)
        .and_then(|s| s.try_into().ok())
        .ok_or(FrameError::Length)?;
    let e_r: [u8; EPH] = first
        .get(HEADER_LEN + SALT_LEN..INTRO_HEAD)
        .and_then(|s| s.try_into().ok())
        .ok_or(FrameError::Length)?;
    let chain = Chain {
        salt: salts(salt_i, &salt_r),
        e_i: x25519_public(my_eph_secret),
        e_r,
        ee: x25519(my_eph_secret, &e_r).ok_or(FrameError::Auth)?,
        h: chain_hash(intro_hash, &salt_r, &e_r),
    };
    // Second tag first: it covers the first sealed part as sent.
    let second_aad: [u8; INTRO_HEAD + ID + TAG_LEN] =
        (&*first).try_into().map_err(|_| FrameError::Length)?;
    let identity = {
        let (aad, rest) = first.split_at_mut(INTRO_HEAD);
        let (ct, t) = rest.split_at_mut(ID);
        let t: &[u8; TAG_LEN] = (&*t).try_into().map_err(|_| FrameError::Length)?;
        aead::open(&chain.k1(), &header.nonce(), aad, ct, t).map_err(|_| FrameError::Auth)?;
        let id: [u8; ID] = (&*ct).try_into().map_err(|_| FrameError::Length)?;
        id
    };
    let u_r = montgomery(&identity).ok_or(FrameError::Auth)?;
    let es = x25519(my_eph_secret, &u_r).ok_or(FrameError::Auth)?;
    let (ct, t) = second.split_at_mut(1);
    let t: &[u8; TAG_LEN] = (&*t).try_into().map_err(|_| FrameError::Length)?;
    aead::open(
        &chain.k2(&es, &identity),
        &header.nonce(),
        &second_aad,
        ct,
        t,
    )
    .map_err(|_| FrameError::Auth)?;
    let [version] = *ct else {
        return Err(FrameError::Malformed);
    };
    Ok(IntroAck {
        tag: header.tag,
        attempt: header.attempt,
        salt: salt_r,
        ephemeral: e_r,
        identity,
        version,
    })
}

/// Initiator's DH results with a proven INTRO_ACK.
fn initiator_chain(
    my_prv: &[u8; 64],
    my_eph_secret: &[u8; 32],
    salt_i: &[u8; SALT_LEN],
    intro_hash: &[u8; 32],
    ack: &IntroAck,
) -> Option<(Chain, [u8; 32], [u8; 32])> {
    let chain = Chain {
        salt: salts(salt_i, &ack.salt),
        e_i: x25519_public(my_eph_secret),
        e_r: ack.ephemeral,
        ee: x25519(my_eph_secret, &ack.ephemeral)?,
        h: chain_hash(intro_hash, &ack.salt, &ack.ephemeral),
    };
    let es = x25519(my_eph_secret, &montgomery(&ack.identity)?)?;
    let se = x25519(&x25519_secret(my_prv), &ack.ephemeral)?;
    Some((chain, es, se))
}

/// Builds the INTRO_FIN (initiator side), try `attempt`.
#[allow(clippy::too_many_arguments)]
pub fn seal_intro_fin(
    me_node: u8,
    peer_node: u8,
    my_prv: &[u8; 64],
    my_id: &[u8; 32],
    my_eph_secret: &[u8; 32],
    salt_i: &[u8; SALT_LEN],
    intro_hash: &[u8; 32],
    ack: &IntroAck,
    attempt: u8,
    out: &mut [u8],
) -> Result<usize, FrameError> {
    let (chain, es, se) =
        initiator_chain(my_prv, my_eph_secret, salt_i, intro_hash, ack).ok_or(FrameError::Auth)?;
    let frame = out.get_mut(..INTRO_FIN_LEN).ok_or(FrameError::Overflow)?;
    let header = control_header(me_node, peer_node, ack.tag, 5, attempt);
    let (first, second) = frame.split_at_mut(HEADER_LEN + ID + TAG_LEN);
    {
        let (head, rest) = first.split_at_mut(HEADER_LEN);
        head.copy_from_slice(&header.encode());
        let (ct, t) = rest.split_at_mut(ID);
        ct.copy_from_slice(my_id);
        t.copy_from_slice(&aead::seal(
            &chain.k3(&es, &ack.identity),
            &header.nonce(),
            head,
            ct,
        ));
    }
    let (ct, t) = second.split_at_mut(1);
    ct.copy_from_slice(&[VERSION]);
    let k4 = chain.k4(&es, &se, &ack.identity, my_id);
    t.copy_from_slice(&aead::seal(&k4, &header.nonce(), first, ct));
    Ok(INTRO_FIN_LEN)
}

/// Responder's DH results for an answered INTRO.
fn responder_chain(my_prv: &[u8; 64], answer: &Answer) -> Option<(Chain, [u8; 32])> {
    let e_r = x25519_public(&answer.eph_secret);
    let chain = Chain {
        salt: salts(&answer.salt_i, &answer.salt_r),
        e_i: answer.e_i,
        e_r,
        ee: x25519(&answer.eph_secret, &answer.e_i)?,
        h: chain_hash(&answer.intro_hash, &answer.salt_r, &e_r),
    };
    let es = x25519(&x25519_secret(my_prv), &answer.e_i)?;
    Some((chain, es))
}

/// Opens the INTRO_FIN from node `peer_node` for the INTRO we answered; returns the
/// initiator's identity, proven.
pub fn open_intro_fin(
    me_node: u8,
    peer_node: u8,
    my_prv: &[u8; 64],
    my_id: &[u8; 32],
    answer: &Answer,
    frame: &[u8],
) -> Result<[u8; ID], FrameError> {
    let (header, _) = Header::decode(frame)?;
    if header.kind != Kind::Control || header.index != 5 {
        return Err(FrameError::Malformed);
    }
    if header.dst != me_node || header.src != peer_node {
        return Err(FrameError::NotForUs);
    }
    if frame.len() != INTRO_FIN_LEN {
        return Err(FrameError::Length);
    }
    let (chain, es) = responder_chain(my_prv, answer).ok_or(FrameError::Auth)?;
    let mut buf = [0u8; INTRO_FIN_LEN];
    buf.copy_from_slice(frame);
    let (first, second) = buf.split_at_mut(HEADER_LEN + ID + TAG_LEN);
    let second_aad: [u8; HEADER_LEN + ID + TAG_LEN] =
        (&*first).try_into().map_err(|_| FrameError::Length)?;
    let id_i = {
        let (head, rest) = first.split_at_mut(HEADER_LEN);
        let (ct, t) = rest.split_at_mut(ID);
        let t: &[u8; TAG_LEN] = (&*t).try_into().map_err(|_| FrameError::Length)?;
        aead::open(&chain.k3(&es, my_id), &header.nonce(), head, ct, t)
            .map_err(|_| FrameError::Auth)?;
        let id: [u8; ID] = (&*ct).try_into().map_err(|_| FrameError::Length)?;
        id
    };
    let se = x25519(
        &answer.eph_secret,
        &montgomery(&id_i).ok_or(FrameError::Auth)?,
    )
    .ok_or(FrameError::Auth)?;
    let (ct, t) = second.split_at_mut(1);
    let t: &[u8; TAG_LEN] = (&*t).try_into().map_err(|_| FrameError::Length)?;
    aead::open(
        &chain.k4(&es, &se, my_id, &id_i),
        &header.nonce(),
        &second_aad,
        ct,
        t,
    )
    .map_err(|_| FrameError::Auth)?;
    Ok(id_i)
}

/// The initiator's session keys after a proven INTRO_ACK.
pub fn initiator_keys(
    my_prv: &[u8; 64],
    my_id: &[u8; 32],
    my_eph_secret: &[u8; 32],
    salt_i: &[u8; SALT_LEN],
    intro_hash: &[u8; 32],
    ack: &IntroAck,
) -> Option<SessionKeys> {
    let (chain, es, se) = initiator_chain(my_prv, my_eph_secret, salt_i, intro_hash, ack)?;
    let (ir, ri) = chain.session(&es, &se, my_id, &ack.identity);
    Some(SessionKeys { send: ir, recv: ri })
}

/// The responder's session keys after a proven INTRO_FIN from `id_i`.
pub fn responder_keys(
    my_prv: &[u8; 64],
    my_id: &[u8; 32],
    answer: &Answer,
    id_i: &[u8; 32],
) -> Option<SessionKeys> {
    let (chain, es) = responder_chain(my_prv, answer)?;
    let se = x25519(&answer.eph_secret, &montgomery(id_i)?)?;
    let (ir, ri) = chain.session(&es, &se, id_i, my_id);
    Some(SessionKeys { send: ri, recv: ir })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lc_meshcore::ecdh::{expand_seed, public_key};

    /// A stand-in INTRO hash, where the INTRO itself does not matter.
    const H: [u8; 32] = [0x5A; 32];

    struct Side {
        prv: [u8; 64],
        id: [u8; 32],
        node: u8,
    }

    fn side(seed: u8, node: u8) -> Side {
        let prv = expand_seed(&[seed; 32]);
        Side {
            id: public_key(&prv),
            prv,
            node,
        }
    }

    /// The whole introduction; returns both sides' session keys.
    fn run(i: &Side, r: &Side) -> (SessionKeys, SessionKeys) {
        let (eph_i, salt_i) = ([11u8; 32], [12u8; SALT_LEN]);
        let path = [0x42];
        let intro = Intro {
            tag: 0xBEEF,
            attempt: 0,
            salt: salt_i,
            ephemeral: x25519_public(&eph_i),
            return_path_len: 1,
            return_path: &path,
        };
        let mut buf = [0u8; 200];
        let n = seal_intro(i.node, r.node, &intro, &mut buf).unwrap();
        let (src, got) = open_intro(r.node, &buf[..n]).unwrap();
        assert_eq!((src, got), (i.node, intro));

        let answer = Answer {
            salt_i: got.salt,
            e_i: got.ephemeral,
            salt_r: [13; SALT_LEN],
            eph_secret: [14; 32],
            intro_hash: intro_hash(&got),
        };
        let n =
            seal_intro_ack(r.node, i.node, &r.prv, &r.id, &answer, got.tag, 0, &mut buf).unwrap();
        assert_eq!(n, INTRO_ACK_LEN);
        let ack = open_intro_ack(
            i.node,
            r.node,
            &eph_i,
            &salt_i,
            &intro_hash(&intro),
            &buf[..n],
        )
        .unwrap();
        assert_eq!((ack.identity, ack.version), (r.id, VERSION));

        let n = seal_intro_fin(
            i.node,
            r.node,
            &i.prv,
            &i.id,
            &eph_i,
            &salt_i,
            &intro_hash(&intro),
            &ack,
            0,
            &mut buf,
        )
        .unwrap();
        let id_i = open_intro_fin(r.node, i.node, &r.prv, &r.id, &answer, &buf[..n]).unwrap();
        assert_eq!(id_i, i.id);
        (
            initiator_keys(&i.prv, &i.id, &eph_i, &salt_i, &intro_hash(&intro), &ack).unwrap(),
            responder_keys(&r.prv, &r.id, &answer, &id_i).unwrap(),
        )
    }

    #[test]
    fn both_sides_agree() {
        let (i, r) = (side(1, 0xA1), side(2, 0xB2));
        let (ki, kr) = run(&i, &r);
        assert_eq!(ki.send, kr.recv);
        assert_eq!(ki.recv, kr.send);
        assert_ne!(ki.send, ki.recv);
        // Other identities, other keys.
        let (kj, _) = run(&side(3, 0xA1), &r);
        assert_ne!(kj.send, ki.send);
    }

    /// Someone who knows R's identity but not its private key cannot answer as R.
    #[test]
    fn responder_must_hold_its_key() {
        let (i, r) = (side(1, 0xA1), side(2, 0xB2));
        let mallory = side(9, 0xB2);
        let eph_i = [11u8; 32];
        let salt_i = [12u8; SALT_LEN];
        let answer = Answer {
            salt_i,
            e_i: x25519_public(&eph_i),
            salt_r: [13; SALT_LEN],
            eph_secret: [14; 32],
            intro_hash: H,
        };
        let mut buf = [0u8; 200];
        // Mallory claims R's identity with her own key.
        let n =
            seal_intro_ack(r.node, i.node, &mallory.prv, &r.id, &answer, 1, 0, &mut buf).unwrap();
        assert_eq!(
            open_intro_ack(i.node, r.node, &eph_i, &salt_i, &H, &buf[..n]),
            Err(FrameError::Auth)
        );
    }

    /// Nor can someone claim I's identity in INTRO_FIN.
    #[test]
    fn initiator_must_hold_its_key() {
        let (i, r) = (side(1, 0xA1), side(2, 0xB2));
        let mallory = side(9, 0xA1);
        let (eph_i, salt_i) = ([11u8; 32], [12u8; SALT_LEN]);
        let answer = Answer {
            salt_i,
            e_i: x25519_public(&eph_i),
            salt_r: [13; SALT_LEN],
            eph_secret: [14; 32],
            intro_hash: H,
        };
        let mut buf = [0u8; 200];
        let n = seal_intro_ack(r.node, i.node, &r.prv, &r.id, &answer, 1, 0, &mut buf).unwrap();
        let ack = open_intro_ack(i.node, r.node, &eph_i, &salt_i, &H, &buf[..n]).unwrap();
        let n = seal_intro_fin(
            i.node,
            r.node,
            &mallory.prv,
            &i.id,
            &eph_i,
            &salt_i,
            &H,
            &ack,
            0,
            &mut buf,
        )
        .unwrap();
        assert_eq!(
            open_intro_fin(r.node, i.node, &r.prv, &r.id, &answer, &buf[..n]),
            Err(FrameError::Auth)
        );
    }

    #[test]
    fn tampering_and_resends() {
        let (i, r) = (side(1, 0xA1), side(2, 0xB2));
        let (eph_i, salt_i) = ([11u8; 32], [12u8; SALT_LEN]);
        let answer = Answer {
            salt_i,
            e_i: x25519_public(&eph_i),
            salt_r: [13; SALT_LEN],
            eph_secret: [14; 32],
            intro_hash: H,
        };
        let mut buf = [0u8; 200];
        let n = seal_intro_ack(r.node, i.node, &r.prv, &r.id, &answer, 1, 0, &mut buf).unwrap();
        let good = buf[..n].to_vec();
        for at in 0..n {
            let mut bad = good.clone();
            bad[at] ^= 0x10;
            assert!(
                open_intro_ack(i.node, r.node, &eph_i, &salt_i, &H, &bad).is_err(),
                "byte {at}"
            );
        }
        assert_eq!(
            open_intro_ack(i.node, r.node, &eph_i, &salt_i, &H, &good[..n - 1]),
            Err(FrameError::Length)
        );
        let ack = open_intro_ack(i.node, r.node, &eph_i, &salt_i, &H, &good).unwrap();
        // A resent INTRO_FIN (next attempt) has new bytes and still opens.
        let mut a = [0u8; INTRO_FIN_LEN];
        let mut b = [0u8; INTRO_FIN_LEN];
        let na = seal_intro_fin(
            i.node, r.node, &i.prv, &i.id, &eph_i, &salt_i, &H, &ack, 0, &mut a,
        )
        .unwrap();
        let nb = seal_intro_fin(
            i.node, r.node, &i.prv, &i.id, &eph_i, &salt_i, &H, &ack, 1, &mut b,
        )
        .unwrap();
        assert_ne!(a[..na], b[..nb]);
        for f in [&a[..na], &b[..nb]] {
            assert_eq!(
                open_intro_fin(r.node, i.node, &r.prv, &r.id, &answer, f),
                Ok(i.id)
            );
        }
        // Not for us, or from someone else.
        assert_eq!(
            open_intro_fin(0x77, i.node, &r.prv, &r.id, &answer, &a[..na]),
            Err(FrameError::NotForUs)
        );
        assert_eq!(
            open_intro(0x77, &a[..na]).map(|_| ()),
            Err(FrameError::Malformed)
        );
    }

    /// ADR-0029: the INTRO's return path is bound into the keys. Changed on the way, the
    /// responder's answer does not open for the initiator.
    #[test]
    fn the_return_path_is_bound() {
        let (i, r) = (side(1, 0xA1), side(2, 0xB2));
        let (eph_i, salt_i) = ([11u8; 32], [12u8; SALT_LEN]);
        let sent = Intro {
            tag: 7,
            attempt: 0,
            salt: salt_i,
            ephemeral: x25519_public(&eph_i),
            return_path_len: 1,
            return_path: &[0x42],
        };
        let received = Intro {
            return_path: &[0x99],
            ..sent
        };
        let answer = Answer {
            salt_i,
            e_i: sent.ephemeral,
            salt_r: [13; SALT_LEN],
            eph_secret: [14; 32],
            intro_hash: intro_hash(&received),
        };
        let mut buf = [0u8; 200];
        let n = seal_intro_ack(r.node, i.node, &r.prv, &r.id, &answer, 7, 0, &mut buf).unwrap();
        assert_eq!(
            open_intro_ack(
                i.node,
                r.node,
                &eph_i,
                &salt_i,
                &intro_hash(&sent),
                &buf[..n]
            ),
            Err(FrameError::Auth)
        );
    }

    #[test]
    fn intro_decoding() {
        let mut buf = [0u8; 200];
        let intro = Intro {
            tag: 1,
            attempt: 2,
            salt: [3; SALT_LEN],
            ephemeral: [4; 32],
            return_path_len: 0xFF,
            return_path: &[],
        };
        let n = seal_intro(1, 2, &intro, &mut buf).unwrap();
        assert_eq!(n, INTRO_HEAD + 1);
        assert_eq!(open_intro(2, &buf[..n]), Ok((1, intro)));
        assert_eq!(
            open_intro(3, &buf[..n]).map(|_| ()),
            Err(FrameError::NotForUs)
        );
        assert!(open_intro(2, &buf[..n - 1]).is_err());
        // A path length that does not match its bytes.
        let bad = Intro {
            return_path_len: 2,
            return_path: &[1],
            ..intro
        };
        assert_eq!(seal_intro(1, 2, &bad, &mut buf), Err(FrameError::Malformed));
    }
}

//! Path discovery (§4.5, ADR-0008): a PROBE flooded as `GRP_DATA` on channel hash `0x7C`, and
//! the PATH_REPLY sent back in direct mode along the reversed path.

use crate::header::{FrameError, Header, Kind};
use crate::kdf::hkdf;
use crate::{HEADER_LEN, Key, TAG_LEN, aead};
use lc_meshcore::path::{PATH_UNKNOWN, path_bytes};

/// Channel hash of lichen's flooded envelopes.
pub const CHANNEL_HASH: u8 = 0x7C;
/// PROBE version byte.
pub const PROBE_VERSION: u8 = 0x01;
/// PROBE payload length: never `2 + 16k`, so stock nodes reject it before any MAC check.
pub const PROBE_LEN: usize = 20;
const ID_LEN: usize = 8;
const PROBE_AAD: usize = PROBE_LEN - TAG_LEN;

/// A probe identifier (random, single use).
pub type ProbeId = [u8; ID_LEN];

/// The fields of a received PROBE, before authentication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    pub dst: u8,
    pub src: u8,
    pub id: ProbeId,
}

fn probe_key(s: &Key, id: &ProbeId, pub_src: &[u8; 32], pub_dst: &[u8; 32]) -> Key {
    hkdf(id, s, &[b"lichen/1 probe", pub_src, pub_dst])
}

fn reply_key(s: &Key, id: &ProbeId, pub_from: &[u8; 32], pub_to: &[u8; 32]) -> Key {
    hkdf(id, s, &[b"lichen/1 path-reply", pub_from, pub_to])
}

/// The `GRP_DATA` payload of a PROBE from `me` to `peer`.
pub fn build_probe(s: &Key, me: &[u8; 32], peer: &[u8; 32], id: &ProbeId) -> [u8; PROBE_LEN] {
    let mut p = [0u8; PROBE_LEN];
    let (aad, tag_slot) = p.split_at_mut(PROBE_AAD);
    let (head, id_slot) = aad.split_at_mut(4);
    head.copy_from_slice(&[CHANNEL_HASH, PROBE_VERSION, peer[0], me[0]]);
    id_slot.copy_from_slice(id);
    let tag = aead::seal(&probe_key(s, id, me, peer), &[0; 12], aad, &mut []);
    tag_slot.copy_from_slice(&tag);
    p
}

/// Recognises a PROBE payload (before authentication).
pub fn parse_probe(payload: &[u8]) -> Option<Probe> {
    let p: &[u8; PROBE_LEN] = payload.try_into().ok()?;
    let [CHANNEL_HASH, PROBE_VERSION, dst, src, ..] = *p else {
        return None;
    };
    Some(Probe {
        dst,
        src,
        id: p.get(4..PROBE_AAD)?.try_into().ok()?,
    })
}

/// True when `payload` is a PROBE from `peer` to `me` under the static secret `s`.
pub fn verify_probe(s: &Key, peer: &[u8; 32], me: &[u8; 32], payload: &[u8]) -> bool {
    let Some(p) = parse_probe(payload) else {
        return false;
    };
    if p.dst != me[0] || p.src != peer[0] {
        return false;
    }
    let (Some(aad), Some(tag)) = (payload.get(..PROBE_AAD), payload.get(PROBE_AAD..)) else {
        return false;
    };
    let Ok(tag) = <&[u8; TAG_LEN]>::try_from(tag) else {
        return false;
    };
    aead::open(&probe_key(s, &p.id, peer, me), &[0; 12], aad, &mut [], tag).is_ok()
}

/// Builds the PATH_REPLY from `me` to `peer` for probe `id`, carrying the path the probe took
/// (peer → me). Returns the frame length.
pub fn seal_path_reply(
    s: &Key,
    me: &[u8; 32],
    peer: &[u8; 32],
    id: &ProbeId,
    path_len: u8,
    path: &[u8],
    out: &mut [u8],
) -> Result<usize, FrameError> {
    if path_len == PATH_UNKNOWN || path_bytes(path_len) != Some(path.len()) {
        return Err(FrameError::Malformed);
    }
    let header = Header {
        dst: peer[0],
        src: me[0],
        tag: u16::from_le_bytes([id[0], id[1]]),
        kind: Kind::Control,
        index: 2,
        total: 1,
        attempt: 0,
    };
    let len = HEADER_LEN + ID_LEN + 1 + path.len() + TAG_LEN;
    let frame = out.get_mut(..len).ok_or(FrameError::Overflow)?;
    let (aad, rest) = frame.split_at_mut(HEADER_LEN + ID_LEN);
    let (head, id_slot) = aad.split_at_mut(HEADER_LEN);
    head.copy_from_slice(&header.encode());
    id_slot.copy_from_slice(id);
    let (ct, tag_slot) = rest.split_at_mut(1 + path.len());
    let (l, p) = ct.split_at_mut(1);
    l.copy_from_slice(&[path_len]);
    p.copy_from_slice(path);
    let tag = aead::seal(&reply_key(s, id, me, peer), &header.nonce(), aad, ct);
    tag_slot.copy_from_slice(&tag);
    Ok(len)
}

/// Opens a PATH_REPLY sent by `peer` to `me`. Returns the probe `id` and the path
/// (`path_len`, bytes) from `me` to `peer`, borrowed from the decrypted frame.
pub fn open_path_reply<'a>(
    s: &Key,
    peer: &[u8; 32],
    me: &[u8; 32],
    frame: &'a mut [u8],
) -> Result<(ProbeId, u8, &'a [u8]), FrameError> {
    let (header, _) = Header::decode(frame)?;
    if header.kind != Kind::Control || header.index != 2 {
        return Err(FrameError::Malformed);
    }
    if header.dst != me[0] || header.src != peer[0] {
        return Err(FrameError::NotForUs);
    }
    if frame.len() < HEADER_LEN + ID_LEN + 1 + TAG_LEN {
        return Err(FrameError::Length);
    }
    let (aad, rest) = frame.split_at_mut(HEADER_LEN + ID_LEN);
    let id: ProbeId = aad
        .get(HEADER_LEN..)
        .and_then(|b| b.try_into().ok())
        .ok_or(FrameError::Length)?;
    if header.tag != u16::from_le_bytes([id[0], id[1]]) {
        return Err(FrameError::Malformed);
    }
    let ct_len = rest.len() - TAG_LEN;
    let (ct, tag) = rest.split_at_mut(ct_len);
    let tag: &[u8; TAG_LEN] = (&*tag).try_into().map_err(|_| FrameError::Length)?;
    aead::open(&reply_key(s, &id, peer, me), &header.nonce(), aad, ct, tag)
        .map_err(|_| FrameError::Auth)?;
    let ct: &[u8] = ct;
    let (&len_byte, path) = ct.split_first().ok_or(FrameError::Malformed)?;
    if path_bytes(len_byte) != Some(path.len()) {
        return Err(FrameError::Malformed);
    }
    Ok((id, len_byte, path))
}

//! Group message envelope (§10.3, ADR-0009): flooded as `GRP_DATA` on channel hash `0x7C`,
//! encrypted under the sender's key, 1 or 2 fragments per message.

use crate::discovery::CHANNEL_HASH;
use crate::header::FrameError;
use crate::{Key, TAG_LEN, aead};

/// Envelope type without, and with, the trailing pad byte.
pub const TYPE_MESSAGE: u8 = 0x02;
pub const TYPE_MESSAGE_PADDED: u8 = 0x03;
/// Header length: channel hash, type, gid hash, sender, key epoch, counter, frag.
pub const HEADER_LEN: usize = 11;
/// Largest envelope: fits `LOG_RX_DATA` after 8 hops with transport codes (171 − 8 − 4).
pub const PAYLOAD_MAX: usize = 159;
/// Largest plaintext per fragment (room kept for the pad byte).
pub const FRAGMENT_PLAIN_MAX: usize = PAYLOAD_MAX - HEADER_LEN - TAG_LEN - 1;
/// Fragments per message.
pub const MAX_FRAGMENTS: u8 = 2;
/// Ed25519 signature length, appended to every message plaintext.
pub const SIGNATURE_LEN: usize = 64;
/// Largest message plaintext (`content_type ‖ body ‖ signature`).
pub const MESSAGE_MAX: usize = FRAGMENT_PLAIN_MAX * MAX_FRAGMENTS as usize;

/// The clear part of an envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupHeader {
    pub gid_hash: u16,
    pub sender: u8,
    pub key_epoch: u8,
    pub counter: u32,
    pub index: u8,
    pub total: u8,
}

impl GroupHeader {
    fn encode(&self, padded: bool) -> [u8; HEADER_LEN] {
        let g = self.gid_hash.to_le_bytes();
        let c = self.counter.to_le_bytes();
        [
            CHANNEL_HASH,
            if padded {
                TYPE_MESSAGE_PADDED
            } else {
                TYPE_MESSAGE
            },
            g[0],
            g[1],
            self.sender,
            self.key_epoch,
            c[0],
            c[1],
            c[2],
            c[3],
            (self.index << 4) | (self.total & 0x0F),
        ]
    }

    /// `counter ‖ key_epoch ‖ frag ‖ 0×6`.
    fn nonce(&self) -> [u8; 12] {
        let c = self.counter.to_le_bytes();
        [
            c[0],
            c[1],
            c[2],
            c[3],
            self.key_epoch,
            (self.index << 4) | (self.total & 0x0F),
            0,
            0,
            0,
            0,
            0,
            0,
        ]
    }
}

/// True when a stock node would try to decrypt a `GRP_DATA` payload of this length: after the
/// channel hash and its 2-byte MAC, a non-empty multiple of 16 bytes.
pub const fn stock_would_try(payload_len: usize) -> bool {
    payload_len > 3 && (payload_len - 3).is_multiple_of(16)
}

/// Recognises a group envelope and returns its header and whether it ends with a pad byte.
pub fn parse(payload: &[u8]) -> Option<(GroupHeader, bool)> {
    if payload.len() < HEADER_LEN + 1 + TAG_LEN || payload.len() > PAYLOAD_MAX {
        return None;
    }
    let h: &[u8; HEADER_LEN] = payload.get(..HEADER_LEN)?.try_into().ok()?;
    let [
        CHANNEL_HASH,
        kind,
        g0,
        g1,
        sender,
        key_epoch,
        c0,
        c1,
        c2,
        c3,
        frag,
    ] = *h
    else {
        return None;
    };
    let padded = match kind {
        TYPE_MESSAGE => false,
        TYPE_MESSAGE_PADDED => true,
        _ => return None,
    };
    if padded && payload.last() != Some(&0) {
        return None;
    }
    let (index, total) = (frag >> 4, frag & 0x0F);
    if total == 0 || total > MAX_FRAGMENTS || index >= total {
        return None;
    }
    Some((
        GroupHeader {
            gid_hash: u16::from_le_bytes([g0, g1]),
            sender,
            key_epoch,
            counter: u32::from_le_bytes([c0, c1, c2, c3]),
            index,
            total,
        },
        padded,
    ))
}

/// Builds one fragment envelope into `out`; returns its length.
pub fn seal(
    key: &Key,
    header: &GroupHeader,
    plain: &[u8],
    out: &mut [u8],
) -> Result<usize, FrameError> {
    if plain.is_empty() || plain.len() > FRAGMENT_PLAIN_MAX {
        return Err(FrameError::Length);
    }
    let unpadded = HEADER_LEN + plain.len() + TAG_LEN;
    let padded = stock_would_try(unpadded);
    let len = unpadded + usize::from(padded);
    let frame = out.get_mut(..len).ok_or(FrameError::Overflow)?;
    let (aad, rest) = frame.split_at_mut(HEADER_LEN);
    aad.copy_from_slice(&header.encode(padded));
    let (ct, rest) = rest.split_at_mut(plain.len());
    ct.copy_from_slice(plain);
    let tag = aead::seal(key, &header.nonce(), aad, ct);
    let (tag_slot, pad) = rest.split_at_mut(TAG_LEN);
    tag_slot.copy_from_slice(&tag);
    pad.fill(0);
    Ok(len)
}

/// Opens a fragment envelope with `key`; the plaintext is decrypted in place.
pub fn open<'a>(key: &Key, payload: &'a mut [u8]) -> Result<(GroupHeader, &'a [u8]), FrameError> {
    let (header, padded) = parse(payload).ok_or(FrameError::Malformed)?;
    let body_end = payload.len() - usize::from(padded);
    let body = payload.get_mut(..body_end).ok_or(FrameError::Length)?;
    let (aad, rest) = body.split_at_mut(HEADER_LEN);
    let ct_len = rest.len().checked_sub(TAG_LEN).ok_or(FrameError::Length)?;
    let (ct, tag) = rest.split_at_mut(ct_len);
    let tag: &[u8; TAG_LEN] = (&*tag).try_into().map_err(|_| FrameError::Length)?;
    aead::open(key, &header.nonce(), aad, ct, tag).map_err(|_| FrameError::Auth)?;
    Ok((header, ct))
}

/// Number of fragments for a message plaintext of `len` bytes, or `None` if it is too long.
pub const fn fragments_for(len: usize) -> Option<u8> {
    if len == 0 || len > MESSAGE_MAX {
        None
    } else if len <= FRAGMENT_PLAIN_MAX {
        Some(1)
    } else {
        Some(2)
    }
}

//! A node's advert (protocol §15.3, §16.6; MeshCore v1.18 at 81dbae5): the payload of an
//! ADVERT packet, `key (32) ‖ timestamp u32 ‖ signature (64) ‖ app_data` (`src/Mesh.cpp`
//! L404–L434), and the whole packet as the companion's `EXPORT_CONTACT` gives it and the stock
//! app shares it. `app_data` (`src/helpers/AdvertDataHelpers.cpp` L4–L28) is
//! `flags ‖ [lat i32 ‖ lon i32] ‖ [feature 1 u16] ‖ [feature 2 u16] ‖ name`, at most 32 bytes.

use crate::DecodeError;
use crate::air::{Packet, PayloadType, Route};
use crate::bytes::Reader;

/// Longest `app_data` (`MAX_ADVERT_DATA_SIZE`, `src/MeshCore.h` L12).
pub const APP_DATA_MAX: usize = 32;

/// `app_data` flags (`src/helpers/AdvertDataHelpers.h` L7–L17).
pub mod flag {
    /// The node's type: 1 companion, 2 repeater, 3 room server, 4 sensor.
    pub const TYPE: u8 = 0x0F;
    pub const POSITION: u8 = 0x10;
    pub const FEATURE_1: u8 = 0x20;
    pub const FEATURE_2: u8 = 0x40;
    pub const NAME: u8 = 0x80;
}

/// A decoded advert. Its fields are as the node wrote them: nothing is checked but the
/// structure (see [`Advert::verify`] for the signature).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Advert<'a> {
    pub key: &'a [u8; 32],
    /// The node's clock when it advertised (Unix seconds).
    pub timestamp: u32,
    pub signature: &'a [u8; 64],
    /// What the signature covers after the key and the timestamp.
    pub app_data: &'a [u8],
    /// `flags & 0x0F`.
    pub kind: u8,
    /// Degrees × 10⁶, when the flags announce a position.
    pub position: Option<(i32, i32)>,
    /// The two reserved fields, when present.
    pub features: [Option<u16>; 2],
    /// The rest of `app_data` when the flags announce a name, else empty.
    pub name: &'a [u8],
}

impl<'a> Advert<'a> {
    /// Decodes an ADVERT payload: `app_data` holds at least its flags, at most 32 bytes, and
    /// every field its flags announce.
    pub fn decode(payload: &'a [u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(payload);
        let key = r.array::<32>().ok_or(DecodeError::Truncated)?;
        let timestamp = r.u32().ok_or(DecodeError::Truncated)?;
        let signature = r.array::<64>().ok_or(DecodeError::Truncated)?;
        let app_data = r.rest();
        if app_data.len() > APP_DATA_MAX {
            return Err(DecodeError::Invalid);
        }
        let mut d = Reader::new(app_data);
        let flags = d.u8().ok_or(DecodeError::Truncated)?;
        let position = if flags & flag::POSITION != 0 {
            Some((
                d.i32().ok_or(DecodeError::Truncated)?,
                d.i32().ok_or(DecodeError::Truncated)?,
            ))
        } else {
            None
        };
        let mut features = [None; 2];
        for (f, bit) in features.iter_mut().zip([flag::FEATURE_1, flag::FEATURE_2]) {
            if flags & bit != 0 {
                *f = Some(d.u16().ok_or(DecodeError::Truncated)?);
            }
        }
        let rest = d.rest();
        Ok(Self {
            key,
            timestamp,
            signature,
            app_data,
            kind: flags & flag::TYPE,
            position,
            features,
            name: if flags & flag::NAME != 0 { rest } else { &[] },
        })
    }

    /// Whether the signature verifies: Ed25519 by `key` over `key ‖ timestamp ‖ app_data`
    /// (`src/Mesh.cpp` L271–L281).
    pub fn verify(&self) -> bool {
        crate::ecdh::verify(
            self.key,
            &[self.key, &self.timestamp.to_le_bytes(), self.app_data],
            self.signature,
        )
    }
}

/// A node's advert as the stock app shares it (§15.3): the packet `EXPORT_CONTACT` returns,
/// header `0x11` (ADVERT, flood, version 0), any path, then the advert. The packet and its
/// advert; nothing else is accepted.
pub fn decode_shared(raw: &[u8]) -> Result<(Packet<'_>, Advert<'_>), DecodeError> {
    let p = Packet::decode(raw)?;
    if p.route != Route::Flood || p.ptype != PayloadType::Advert {
        return Err(DecodeError::Invalid);
    }
    Ok((p, Advert::decode(p.payload)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed(seed: &[u8; 32], app_data: &[u8]) -> [u8; 256] {
        let prv = crate::ecdh::expand_seed(seed);
        let key = crate::ecdh::public_key(&prv);
        let ts = 1_793_000_000u32.to_le_bytes();
        let sig = crate::ecdh::sign(&prv, &[&key, &ts, app_data]).unwrap();
        let mut out = [0u8; 256];
        out[..2].copy_from_slice(&[0x11, 0]);
        out[2..34].copy_from_slice(&key);
        out[34..38].copy_from_slice(&ts);
        out[38..102].copy_from_slice(&sig);
        out[102..102 + app_data.len()].copy_from_slice(app_data);
        out
    }

    #[test]
    fn a_signed_advert_reads_and_verifies() {
        let data = [0x91, 1, 0, 0, 0, 2, 0, 0, 0, b'h', b'u', b'b'];
        let raw = signed(&[7; 32], &data);
        let (p, a) = decode_shared(&raw[..102 + data.len()]).unwrap();
        assert_eq!(p.path.hops(), 0);
        assert_eq!((a.kind, a.position, a.name), (1, Some((1, 2)), &b"hub"[..]));
        assert!(a.verify());
        let mut changed = raw;
        changed[102 + data.len() - 1] ^= 1;
        let (_, a) = decode_shared(&changed[..102 + data.len()]).unwrap();
        assert!(!a.verify());
    }

    #[test]
    fn fields_its_flags_announce_must_be_there() {
        let raw = signed(&[7; 32], &[0x91, 1, 2, 3]);
        assert_eq!(decode_shared(&raw[..106]), Err(DecodeError::Truncated));
        let raw = signed(&[7; 32], &[0xA1, 1]);
        assert_eq!(decode_shared(&raw[..104]), Err(DecodeError::Truncated));
        // No flags at all, or 33 bytes of app_data.
        assert_eq!(decode_shared(&raw[..102]), Err(DecodeError::Truncated));
        assert_eq!(decode_shared(&raw[..102 + 33]), Err(DecodeError::Invalid));
    }

    #[test]
    fn only_the_exported_header() {
        let mut raw = signed(&[7; 32], &[0x81, b'x']);
        for h in [0x10, 0x12, 0x13, 0x15, 0x51] {
            raw[0] = h;
            assert!(decode_shared(&raw[..104]).is_err(), "{h:#x}");
        }
    }
}

//! MeshCore paths: an encoded `path_len` byte (bits 0-5 hop count, bits 6-7 hash size − 1)
//! followed by the hashes (`oxymesh/docs/spec/air.md` §1.2).

use crate::DecodeError;

/// Maximum number of path bytes.
pub const MAX_PATH_BYTES: usize = 64;
/// `path_len` meaning "unknown path" in contact state and the companion protocol.
pub const PATH_UNKNOWN: u8 = 0xFF;

/// A valid path: its `path_len` byte and its hash bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Path<'a> {
    len_byte: u8,
    bytes: &'a [u8],
}

/// Number of path bytes announced by an encoded `path_len`, or `None` if it is invalid
/// (reserved hash size, or more than 64 bytes).
pub const fn path_bytes(len_byte: u8) -> Option<usize> {
    let size = (len_byte >> 6) as usize + 1;
    if size > 3 {
        return None;
    }
    let n = (len_byte & 0x3F) as usize * size;
    if n > MAX_PATH_BYTES { None } else { Some(n) }
}

impl<'a> Path<'a> {
    /// Empty path (zero hops, 1-byte hashes).
    pub const EMPTY: Path<'static> = Path {
        len_byte: 0,
        bytes: &[],
    };

    /// Path from an encoded `path_len` and its bytes; lengths must agree.
    pub fn new(len_byte: u8, bytes: &'a [u8]) -> Result<Self, DecodeError> {
        match path_bytes(len_byte) {
            Some(n) if n == bytes.len() => Ok(Self { len_byte, bytes }),
            _ => Err(DecodeError::Invalid),
        }
    }

    /// Path of 1-byte hashes (`path_hash_mode` 0).
    pub fn from_hashes(bytes: &'a [u8]) -> Result<Self, DecodeError> {
        let n = u8::try_from(bytes.len()).map_err(|_| DecodeError::Invalid)?;
        if n > 63 {
            return Err(DecodeError::Invalid);
        }
        Self::new(n, bytes)
    }

    /// The encoded `path_len` byte.
    pub const fn len_byte(&self) -> u8 {
        self.len_byte
    }

    /// The hash bytes.
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Number of hops (repeaters traversed).
    pub const fn hops(&self) -> u8 {
        self.len_byte & 0x3F
    }

    /// Size of one hash, in bytes (1 to 3).
    pub const fn hash_size(&self) -> usize {
        (self.len_byte >> 6) as usize + 1
    }

    /// The reversed path (to reply along it), written into `out`.
    pub fn reversed<'o>(&self, out: &'o mut [u8; MAX_PATH_BYTES]) -> Option<Path<'o>> {
        let size = self.hash_size();
        let n = self.bytes.len();
        for (i, hash) in self.bytes.chunks_exact(size).rev().enumerate() {
            let start = i.checked_mul(size)?;
            out.get_mut(start..start.checked_add(size)?)?
                .copy_from_slice(hash);
        }
        Some(Path {
            len_byte: self.len_byte,
            bytes: out.get(..n)?,
        })
    }
}

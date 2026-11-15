//! Secret bytes that print as nothing and are wiped when dropped (review of 2026-11-15, SR-07,
//! SR-34): ephemeral handshake keys and group sender keys.

use zeroize::Zeroize;

/// 32 secret bytes. Reads as the bytes (`&secret` where a `&[u8; 32]` is expected).
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Secret(pub(crate) [u8; 32]);

impl core::ops::Deref for Secret {
    type Target = [u8; 32];
    fn deref(&self) -> &[u8; 32] {
        &self.0
    }
}

impl PartialEq<[u8; 32]> for Secret {
    fn eq(&self, other: &[u8; 32]) -> bool {
        self.0 == *other
    }
}

impl core::fmt::Debug for Secret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Secret(..)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_nothing() {
        let s = Secret([0x42; 32]);
        assert_eq!(format!("{s:?}"), "Secret(..)");
        assert!(s == [0x42; 32]);
        assert_eq!(*s, [0x42; 32]);
    }
}

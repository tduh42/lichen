//! The phone identity (protocol.md §2, §4.7, ADR-0013): recovery phrase, fingerprint, safety
//! number, contact card.

use sha2::{Digest, Sha256, Sha512};

use crate::core::PubKey;

/// The 24 words of an identity seed (BIP-39, English list).
pub fn recovery_phrase(seed: &[u8; 32]) -> Vec<String> {
    bip39::Mnemonic::from_entropy(seed)
        .map(|m| m.words().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The seed of 24 words (checksum verified; case and extra spaces ignored).
pub fn seed_from_phrase(words: &[String]) -> Option<[u8; 32]> {
    let phrase = words
        .iter()
        .map(|w| w.trim().to_lowercase())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let m = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, &phrase).ok()?;
    m.to_entropy().try_into().ok()
}

/// The 12-byte fingerprint of an identity, as 6 groups of 4 hexadecimal digits.
pub fn fingerprint(id: &PubKey) -> [String; 6] {
    let d = Sha256::new()
        .chain_update(b"lichen/1 fingerprint")
        .chain_update(id)
        .finalize();
    std::array::from_fn(|i| {
        let a = d.get(2 * i).copied().unwrap_or(0);
        let b = d.get(2 * i + 1).copied().unwrap_or(0);
        format!("{a:02X}{b:02X}")
    })
}

/// The safety number of two identities: 6 groups of 5 digits, the same on both phones.
pub fn safety_number(a: &PubKey, b: &PubKey) -> [String; 6] {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let h = Sha512::new()
        .chain_update(b"lichen/1 safety")
        .chain_update(lo)
        .chain_update(hi)
        .finalize();
    std::array::from_fn(|i| {
        let v = h
            .get(5 * i..5 * i + 5)
            .unwrap_or(&[0; 5])
            .iter()
            .fold(0u64, |acc, &x| acc << 8 | u64::from(x));
        format!("{:05}", v % 100_000)
    })
}

/// A contact card: what a QR code carries (§4.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub identity: PubKey,
    pub node: PubKey,
    pub name: String,
}

const CARD_PREFIX: &str = "lichen:1:";
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

impl Card {
    /// `lichen:1:` and the unpadded base64url of `identity ‖ node ‖ name` (name cut to 32 bytes).
    pub fn encode(&self) -> String {
        let mut end = self.name.len().min(32);
        while !self.name.is_char_boundary(end) {
            end -= 1;
        }
        let mut raw = Vec::with_capacity(64 + end);
        raw.extend_from_slice(&self.identity);
        raw.extend_from_slice(&self.node);
        raw.extend_from_slice(self.name.as_bytes().get(..end).unwrap_or(&[]));
        let mut out = String::from(CARD_PREFIX);
        for chunk in raw.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |acc, (i, &b)| acc | u32::from(b) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                let c = B64
                    .get(((n >> (18 - 6 * i)) & 63) as usize)
                    .copied()
                    .unwrap_or(b'A');
                out.push(char::from(c));
            }
        }
        out
    }

    pub fn decode(text: &str) -> Option<Self> {
        let body = text.trim().strip_prefix(CARD_PREFIX)?;
        let mut raw = Vec::with_capacity(body.len() * 3 / 4);
        let mut acc = 0u32;
        let mut bits = 0;
        for c in body.bytes() {
            let v = B64.iter().position(|&x| x == c)? as u32;
            acc = acc << 6 | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                raw.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        if bits >= 6 || raw.len() < 64 || raw.len() > 96 {
            return None;
        }
        let (identity, rest) = raw.split_first_chunk::<32>()?;
        let (node, name) = rest.split_first_chunk::<32>()?;
        Some(Self {
            identity: *identity,
            node: *node,
            name: String::from_utf8(name.to_vec()).ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrase_round_trip() {
        let seed = [0x5Au8; 32];
        let words = recovery_phrase(&seed);
        assert_eq!(words.len(), 24);
        assert_eq!(seed_from_phrase(&words), Some(seed));
        // Case and spaces do not matter; a wrong word or a broken checksum does.
        let shouted: Vec<String> = words
            .iter()
            .map(|w| format!(" {} ", w.to_uppercase()))
            .collect();
        assert_eq!(seed_from_phrase(&shouted), Some(seed));
        let mut bad = words.clone();
        bad[3] = "zzzz".into();
        assert_eq!(seed_from_phrase(&bad), None);
        let mut swapped = words.clone();
        swapped.swap(0, 1);
        if swapped != words {
            assert_ne!(seed_from_phrase(&swapped), Some(seed));
        }
        // The standard vector: all-zero entropy.
        let zero = recovery_phrase(&[0; 32]);
        assert_eq!(zero[0], "abandon");
        assert_eq!(zero[23], "art");
    }

    #[test]
    fn fingerprint_and_safety_number() {
        let (a, b) = ([1u8; 32], [2u8; 32]);
        let fa = fingerprint(&a);
        assert!(
            fa.iter()
                .all(|g| g.len() == 4 && g.chars().all(|c| c.is_ascii_hexdigit()))
        );
        assert_ne!(fa, fingerprint(&b));
        let s = safety_number(&a, &b);
        assert_eq!(s, safety_number(&b, &a), "the same on both phones");
        assert!(
            s.iter()
                .all(|g| g.len() == 5 && g.chars().all(|c| c.is_ascii_digit()))
        );
        assert_ne!(s, safety_number(&a, &[3; 32]));
    }

    #[test]
    fn card_round_trip() {
        let card = Card {
            identity: [7; 32],
            node: [9; 32],
            name: "Camille Dupré".into(),
        };
        let text = card.encode();
        assert!(text.starts_with("lichen:1:"));
        assert!(text.bytes().skip(9).all(|c| B64.contains(&c)));
        assert_eq!(Card::decode(&text), Some(card.clone()));
        // A long name is cut on a character boundary.
        let long = Card {
            name: "é".repeat(40),
            ..card.clone()
        };
        assert_eq!(Card::decode(&long.encode()).unwrap().name, "é".repeat(16));
        assert_eq!(Card::decode("lichen:2:AAAA"), None);
        assert_eq!(Card::decode("lichen:1:!!"), None);
        assert_eq!(Card::decode(&text[..40]), None);
    }
}

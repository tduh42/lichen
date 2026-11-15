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

/// The safety number of two identities (§4.7, ADR-0029): 12 groups of 5 digits, the same on
/// both phones. Each identity has its own 30 digits, the lower identity's first: a man in the
/// middle must match each half with a key of his own (about 2^100 tries each). With one number
/// hashing both identities, as before, two numbers that collide were enough: about 2^50 (review
/// of 2026-11-15, SR-05).
pub fn safety_number(a: &PubKey, b: &PubKey) -> [String; 12] {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let (x, y) = (safety_half(lo), safety_half(hi));
    std::array::from_fn(|i| {
        x.get(i)
            .or_else(|| y.get(i.wrapping_sub(6)))
            .cloned()
            .unwrap_or_default()
    })
}

/// One identity's 30 digits of a safety number.
fn safety_half(id: &PubKey) -> [String; 6] {
    let h = Sha512::new()
        .chain_update(b"lichen/2 safety")
        .chain_update(id)
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
const INVITE_PREFIX: &str = "lichen:1:g:";
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
/// The longest name a card or an invitation carries, in bytes.
pub const NAME_MAX: usize = 32;

/// Unpadded base64url.
fn b64_encode(raw: &[u8], out: &mut String) {
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
}

/// Unpadded base64url; `None` on a character outside the alphabet or a dangling one.
fn b64_decode(body: &str) -> Option<Vec<u8>> {
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
    (bits < 6).then_some(raw)
}

/// `name` cut to [`NAME_MAX`] bytes on a character boundary.
fn cut(name: &str) -> &[u8] {
    let mut end = name.len().min(NAME_MAX);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name.as_bytes().get(..end).unwrap_or(&[])
}

impl Card {
    /// `lichen:1:` and the unpadded base64url of `identity ‖ node ‖ name` (name cut to 32 bytes).
    pub fn encode(&self) -> String {
        let name = cut(&self.name);
        let mut raw = Vec::with_capacity(64 + name.len());
        raw.extend_from_slice(&self.identity);
        raw.extend_from_slice(&self.node);
        raw.extend_from_slice(name);
        let mut out = String::from(CARD_PREFIX);
        b64_encode(&raw, &mut out);
        out
    }

    pub fn decode(text: &str) -> Option<Self> {
        let raw = b64_decode(text.trim().strip_prefix(CARD_PREFIX)?)?;
        if raw.len() < 64 || raw.len() > 64 + NAME_MAX {
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

/// A group invitation: what its QR code carries (§10.5). No key and no group id: the token
/// only means something to the admin's app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invite {
    /// The admin's identity and node, as on their contact card.
    pub admin: PubKey,
    pub node: PubKey,
    pub token: [u8; 16],
    /// Unix seconds.
    pub expires: u32,
    pub admin_name: String,
    pub group_name: String,
}

impl Invite {
    /// `lichen:1:g:` and the unpadded base64url of `admin ‖ node ‖ token ‖ expires u32 ‖
    /// n ‖ admin name ‖ m ‖ group name` (names cut to 32 bytes).
    pub fn encode(&self) -> String {
        let (a, g) = (cut(&self.admin_name), cut(&self.group_name));
        let mut raw = Vec::with_capacity(86 + a.len() + g.len());
        raw.extend_from_slice(&self.admin);
        raw.extend_from_slice(&self.node);
        raw.extend_from_slice(&self.token);
        raw.extend_from_slice(&self.expires.to_le_bytes());
        raw.push(a.len() as u8);
        raw.extend_from_slice(a);
        raw.push(g.len() as u8);
        raw.extend_from_slice(g);
        let mut out = String::from(INVITE_PREFIX);
        b64_encode(&raw, &mut out);
        out
    }

    pub fn decode(text: &str) -> Option<Self> {
        let raw = b64_decode(text.trim().strip_prefix(INVITE_PREFIX)?)?;
        let (admin, rest) = raw.split_first_chunk::<32>()?;
        let (node, rest) = rest.split_first_chunk::<32>()?;
        let (token, rest) = rest.split_first_chunk::<16>()?;
        let (expires, rest) = rest.split_first_chunk::<4>()?;
        let (admin_name, rest) = name_field(rest)?;
        let (group_name, rest) = name_field(rest)?;
        if !rest.is_empty() {
            return None;
        }
        Some(Self {
            admin: *admin,
            node: *node,
            token: *token,
            expires: u32::from_le_bytes(*expires),
            admin_name,
            group_name,
        })
    }
}

/// A length byte and that many bytes of UTF-8 (at most [`NAME_MAX`]); the rest.
fn name_field(raw: &[u8]) -> Option<(String, &[u8])> {
    let (&n, rest) = raw.split_first()?;
    let n = usize::from(n);
    if n > NAME_MAX {
        return None;
    }
    let name = rest.get(..n)?;
    Some((String::from_utf8(name.to_vec()).ok()?, rest.get(n..)?))
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
        // One half per identity (SR-05): a's half is the same with any other identity.
        let t = safety_number(&a, &[3; 32]);
        assert_eq!(s[..6], t[..6]);
        assert_ne!(s[6..], t[6..]);
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

    #[test]
    fn invite_round_trip() {
        let invite = Invite {
            admin: [7; 32],
            node: [9; 32],
            token: [3; 16],
            expires: 1_793_000_600,
            admin_name: "Camille".into(),
            group_name: "é".repeat(40),
        };
        let text = invite.encode();
        assert!(text.starts_with("lichen:1:g:"));
        let back = Invite::decode(&text).unwrap();
        assert_eq!(
            back.group_name,
            "é".repeat(16),
            "cut on a character boundary"
        );
        assert_eq!(
            back,
            Invite {
                group_name: "é".repeat(16),
                ..invite.clone()
            }
        );
        // A card is not an invitation, nor the other way round.
        let card = Card {
            identity: [7; 32],
            node: [9; 32],
            name: "Camille".into(),
        };
        assert_eq!(Invite::decode(&card.encode()), None);
        assert_eq!(Card::decode(&text), None);
        assert_eq!(Invite::decode(&text[..text.len() - 1]), None);
    }
}

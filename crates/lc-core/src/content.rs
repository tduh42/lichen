//! Datagram contents (§6): `content_type ‖ body`.

use lc_proto::content;
use lc_proto::location::{self, Position};

/// Identifies an image across its variants.
pub type MediaId = [u8; 8];

/// Most frames in a voice message (15 s).
pub const VOICE_MAX_FRAMES: u16 = 375;
/// Largest image variant.
pub const IMAGE_MAX: usize = 4_096;
/// FILE kind of an AVIF image.
const KIND_AVIF: u8 = 1;

/// Bits per Codec2 frame of a VOICE mode, if known.
pub const fn voice_bits(mode: u8) -> Option<usize> {
    match mode {
        0 => Some(28),
        1 => Some(48),
        _ => None,
    }
}

/// Transports of an announcement (§14.1).
pub mod transport {
    pub const IROH: u8 = 1;
    pub const TOR: u8 = 2;
}

/// Largest announced content (§14.1).
pub const ANNOUNCE_MAX_SIZE: u32 = 64 << 20;

/// An announcement (§14.1): what to fetch, from whom, verified by its BLAKE3 hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announce {
    pub sent_at: u32,
    /// The LoRa preview it completes, if any (§13.2).
    pub media_id: MediaId,
    /// 1 image, 2 voice, 3 file.
    pub kind: u8,
    pub hash: [u8; 32],
    pub size: u32,
    /// (transport, provider key), 1 or 2 of them.
    pub providers: Vec<(u8, [u8; 32])>,
    pub name: String,
}

/// A decoded datagram content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// `ts u32 ‖ UTF-8 text`.
    Text { sent_at: u32, text: String },
    /// A position (§12).
    Location(Position),
    /// A voice message (§13.1): Codec2 frames, `mode` 0 = 700C, 1 = 1200.
    Voice {
        sent_at: u32,
        mode: u8,
        frames: u16,
        data: Vec<u8>,
    },
    /// An AVIF thumbnail (§13.2), its preview or (`full`) its full variant.
    Image {
        sent_at: u32,
        media_id: MediaId,
        full: bool,
        width: u16,
        height: u16,
        /// Size of the full variant (0: none).
        full_len: u16,
        data: Vec<u8>,
    },
    /// "Send me this variant" (§13.3).
    MediaRequest { media_id: MediaId, full: bool },
    /// Content offered over the Internet bridge (§14.1).
    Announce(Announce),
    /// A type this version does not interpret.
    Other { content_type: u8, body: Vec<u8> },
}

impl Content {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Content::Text { sent_at, text } => {
                let mut v = Vec::with_capacity(5 + text.len());
                v.push(content::TEXT);
                v.extend_from_slice(&sent_at.to_le_bytes());
                v.extend_from_slice(text.as_bytes());
                v
            }
            Content::Location(p) => {
                let mut v = Vec::with_capacity(1 + location::LEN);
                v.push(content::LOCATION);
                v.extend_from_slice(&p.encode());
                v
            }
            Content::Voice {
                sent_at,
                mode,
                frames,
                data,
            } => {
                let mut v = Vec::with_capacity(8 + data.len());
                v.push(content::VOICE);
                v.extend_from_slice(&sent_at.to_le_bytes());
                v.push(*mode);
                v.extend_from_slice(&frames.to_le_bytes());
                v.extend_from_slice(data);
                v
            }
            Content::Image {
                sent_at,
                media_id,
                full,
                width,
                height,
                full_len,
                data,
            } => {
                let mut v = Vec::with_capacity(21 + data.len());
                v.push(content::FILE);
                v.extend_from_slice(&sent_at.to_le_bytes());
                v.push(KIND_AVIF);
                v.extend_from_slice(media_id);
                v.push(u8::from(*full));
                v.extend_from_slice(&width.to_le_bytes());
                v.extend_from_slice(&height.to_le_bytes());
                v.extend_from_slice(&full_len.to_le_bytes());
                v.extend_from_slice(data);
                v
            }
            Content::MediaRequest { media_id, full } => {
                let mut v = Vec::with_capacity(10);
                v.push(content::MEDIA_REQUEST);
                v.extend_from_slice(media_id);
                v.push(u8::from(*full));
                v
            }
            Content::Announce(a) => {
                let mut end = a.name.len().min(32);
                while !a.name.is_char_boundary(end) {
                    end -= 1;
                }
                let mut v = Vec::with_capacity(52 + 33 * a.providers.len() + end);
                v.push(content::ANNOUNCE);
                v.extend_from_slice(&a.sent_at.to_le_bytes());
                v.extend_from_slice(&a.media_id);
                v.push(a.kind);
                v.extend_from_slice(&a.hash);
                v.extend_from_slice(&a.size.to_le_bytes());
                v.push(a.providers.len() as u8);
                for (t, k) in &a.providers {
                    v.push(*t);
                    v.extend_from_slice(k);
                }
                v.push(end as u8);
                v.extend_from_slice(a.name.as_bytes().get(..end).unwrap_or(&[]));
                v
            }
            Content::Other { content_type, body } => {
                let mut v = Vec::with_capacity(1 + body.len());
                v.push(*content_type);
                v.extend_from_slice(body);
                v
            }
        }
    }

    pub fn decode(datagram: &[u8]) -> Option<Self> {
        let (&content_type, body) = datagram.split_first()?;
        if content_type == content::TEXT {
            let (ts, text) = body.split_at_checked(4)?;
            if text.is_empty() {
                return None;
            }
            Some(Content::Text {
                sent_at: u32::from_le_bytes(ts.try_into().ok()?),
                text: String::from_utf8_lossy(text).into_owned(),
            })
        } else if content_type == content::LOCATION {
            Position::decode(body).map(Content::Location)
        } else if content_type == content::VOICE {
            decode_voice(body)
        } else if content_type == content::FILE && body.get(4) == Some(&KIND_AVIF) {
            decode_image(body)
        } else if content_type == content::ANNOUNCE {
            decode_announce(body)
        } else if content_type == content::MEDIA_REQUEST {
            let (id, rest) = body.split_first_chunk::<8>()?;
            let full = match rest {
                [0] => false,
                [1] => true,
                _ => return None,
            };
            Some(Content::MediaRequest {
                media_id: *id,
                full,
            })
        } else {
            Some(Content::Other {
                content_type,
                body: body.to_vec(),
            })
        }
    }
}

fn decode_voice(body: &[u8]) -> Option<Content> {
    let (head, data) = body.split_first_chunk::<7>()?;
    let [t0, t1, t2, t3, mode, f0, f1] = *head;
    let frames = u16::from_le_bytes([f0, f1]);
    if frames > VOICE_MAX_FRAMES {
        return None;
    }
    // A mode this version does not know is still a voice message, shown as unsupported.
    if let Some(bits) = voice_bits(mode)
        && data.len() != (usize::from(frames) * bits).div_ceil(8)
    {
        return None;
    }
    Some(Content::Voice {
        sent_at: u32::from_le_bytes([t0, t1, t2, t3]),
        mode,
        frames,
        data: data.to_vec(),
    })
}

fn decode_announce(body: &[u8]) -> Option<Content> {
    let (ts, b) = body.split_first_chunk::<4>()?;
    let (media_id, b) = b.split_first_chunk::<8>()?;
    let (&kind, b) = b.split_first()?;
    let (hash, b) = b.split_first_chunk::<32>()?;
    let (size, b) = b.split_first_chunk::<4>()?;
    let size = u32::from_le_bytes(*size);
    let (&n, mut b) = b.split_first()?;
    if !(1..=2).contains(&n) || size > ANNOUNCE_MAX_SIZE {
        return None;
    }
    let mut providers = Vec::with_capacity(usize::from(n));
    for _ in 0..n {
        let (&t, rest) = b.split_first()?;
        let (k, rest) = rest.split_first_chunk::<32>()?;
        providers.push((t, *k));
        b = rest;
    }
    let (&len, name) = b.split_first()?;
    if usize::from(len) != name.len() || len > 32 {
        return None;
    }
    Some(Content::Announce(Announce {
        sent_at: u32::from_le_bytes(*ts),
        media_id: *media_id,
        kind,
        hash: *hash,
        size,
        providers,
        name: String::from_utf8(name.to_vec()).ok()?,
    }))
}

fn decode_image(body: &[u8]) -> Option<Content> {
    let (head, data) = body.split_first_chunk::<20>()?;
    let [
        t0,
        t1,
        t2,
        t3,
        _kind,
        i0,
        i1,
        i2,
        i3,
        i4,
        i5,
        i6,
        i7,
        variant,
        w0,
        w1,
        h0,
        h1,
        l0,
        l1,
    ] = *head;
    let full = match variant {
        0 => false,
        1 => true,
        _ => return None,
    };
    if data.is_empty() || data.len() > IMAGE_MAX {
        return None;
    }
    Some(Content::Image {
        sent_at: u32::from_le_bytes([t0, t1, t2, t3]),
        media_id: [i0, i1, i2, i3, i4, i5, i6, i7],
        full,
        width: u16::from_le_bytes([w0, w1]),
        height: u16::from_le_bytes([h0, h1]),
        full_len: u16::from_le_bytes([l0, l1]),
        data: data.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_round_trips() {
        let all = [
            Content::Voice {
                sent_at: 7,
                mode: 1,
                frames: 3,
                data: vec![1; 18],
            },
            Content::Voice {
                sent_at: 7,
                mode: 0,
                frames: 3,
                data: vec![1; 11],
            },
            Content::Voice {
                sent_at: 7,
                mode: 9,
                frames: 1,
                data: vec![1; 5],
            },
            Content::Image {
                sent_at: 9,
                media_id: [3; 8],
                full: false,
                width: 160,
                height: 120,
                full_len: 975,
                data: vec![5; 400],
            },
            Content::MediaRequest {
                media_id: [4; 8],
                full: true,
            },
            Content::Announce(Announce {
                sent_at: 5,
                media_id: [6; 8],
                kind: 1,
                hash: [7; 32],
                size: 1_800_000,
                providers: vec![(transport::IROH, [8; 32]), (transport::TOR, [9; 32])],
                name: "Photo du refuge".into(),
            }),
        ];
        for c in all {
            assert_eq!(Content::decode(&c.encode()), Some(c));
        }
    }

    #[test]
    fn announce_size_and_limits() {
        let a = Announce {
            sent_at: 1,
            media_id: [0; 8],
            kind: 1,
            hash: [1; 32],
            size: 1_000,
            providers: vec![(transport::IROH, [2; 32])],
            name: "IMG_2031.jpg".into(),
        };
        // One iroh provider and a 12-byte name: 97 bytes, one fragment on any path.
        assert_eq!(Content::Announce(a.clone()).encode().len(), 97);
        let bad = |a: Announce| Content::decode(&Content::Announce(a).encode());
        assert_eq!(
            bad(Announce {
                providers: vec![],
                ..a.clone()
            }),
            None
        );
        assert_eq!(
            bad(Announce {
                providers: vec![(1, [0; 32]); 3],
                ..a.clone()
            }),
            None
        );
        assert_eq!(
            bad(Announce {
                size: ANNOUNCE_MAX_SIZE + 1,
                ..a.clone()
            }),
            None
        );
        // An unknown transport is kept (and skipped by the app).
        let other = Announce {
            providers: vec![(9, [3; 32])],
            ..a.clone()
        };
        assert_eq!(bad(other.clone()), Some(Content::Announce(other)));
        // Truncated anywhere: refused.
        let full = Content::Announce(a).encode();
        for n in 1..full.len() {
            assert_eq!(Content::decode(&full[..n]), None, "{n}");
        }
    }

    #[test]
    fn malformed_media() {
        // Voice: wrong length for the mode, too many frames.
        let v = |mode: u8, frames: u16, len: usize| {
            Content::Voice {
                sent_at: 0,
                mode,
                frames,
                data: vec![0; len],
            }
            .encode()
        };
        assert_eq!(Content::decode(&v(1, 3, 17)), None);
        assert_eq!(Content::decode(&v(0, 2, 8)), None);
        assert_eq!(Content::decode(&v(1, 376, 2_256)), None);
        // Image: bad variant, empty, too large; another kind is "other".
        let mut img = Content::Image {
            sent_at: 0,
            media_id: [0; 8],
            full: true,
            width: 1,
            height: 1,
            full_len: 0,
            data: vec![1],
        }
        .encode();
        img[14] = 2;
        assert_eq!(Content::decode(&img), None);
        img[14] = 1;
        assert!(Content::decode(&img).is_some());
        assert_eq!(Content::decode(&img[..21]), None);
        let mut big = img[..21].to_vec();
        big.extend(std::iter::repeat_n(0, IMAGE_MAX + 1));
        assert_eq!(Content::decode(&big), None);
        img[5] = 2;
        assert!(matches!(Content::decode(&img), Some(Content::Other { .. })));
        // Request: exactly 9 bytes, variant 0 or 1.
        assert_eq!(
            Content::decode(&[content::MEDIA_REQUEST, 0, 0, 0, 0, 0, 0, 0, 0]),
            None
        );
        assert_eq!(
            Content::decode(&[content::MEDIA_REQUEST, 0, 0, 0, 0, 0, 0, 0, 0, 2]),
            None
        );
    }
}

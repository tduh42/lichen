//! Datagram contents (§6): `content_type ‖ body`.

use lc_proto::content;
use lc_proto::location::{self, Position};
use sha2::{Digest, Sha256};

/// Identifies an image across its variants.
pub type MediaId = [u8; 8];

/// Most frames in a voice message (15 s).
pub const VOICE_MAX_FRAMES: u16 = 375;
/// Largest image variant.
pub const IMAGE_MAX: usize = 4_096;
/// The most data a small file carries over LoRa (§13.2 kinds 2 and 3, ADR-0022), and its name.
pub const FILE_MAX: usize = 8_192;
pub const FILE_NAME_MAX: usize = 64;
/// The largest small file once inflated (§13.2 kind 3, ADR-0023).
pub const FILE_INFLATED_MAX: usize = 65_536;
/// FILE kind of a small file (§13.2).
const KIND_FILE: u8 = 2;
/// FILE kind of a compressed small file (§13.2, ADR-0023).
const KIND_FILE_DEFLATE: u8 = 3;
/// FILE kind of an AVIF image.
const KIND_AVIF: u8 = 1;

/// Bits per 40 ms of a VOICE mode, if known (§13.1).
pub const fn voice_bits(mode: u8) -> Option<usize> {
    match mode {
        0 => Some(28),
        1 => Some(48),
        2 => Some(64),
        3 => Some(96),
        _ => None,
    }
}

/// A message reference (§6.2): names a message of a conversation on every phone.
pub type MessageRef = [u8; 8];

/// Longest reaction, in bytes (§6.2).
pub const REACTION_MAX: usize = 32;
/// Longest sticker emoji, GIF id and GIF title, in bytes (§6.3).
pub const STICKER_MAX: usize = 32;
pub const GIF_ID_MAX: usize = 32;
pub const GIF_TITLE_MAX: usize = 64;
/// Sticker set 1: Noto Animated Emoji; GIF provider 1: GIPHY (§6.3).
pub const STICKER_SET_NOTO: u8 = 1;
pub const GIF_GIPHY: u8 = 1;

/// The reference of the message with `plaintext` (`content_type ‖ body` as sent) by `author`
/// (an identity key), §6.2.
pub fn message_ref(author: &[u8; 32], plaintext: &[u8]) -> MessageRef {
    let mut h = Sha256::new();
    h.update(b"lichen/1 ref");
    h.update(author);
    h.update(plaintext);
    let d = h.finalize();
    let mut r = [0u8; 8];
    r.copy_from_slice(d.get(..8).unwrap_or(&[0; 8]));
    r
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
    /// A text answering message `target` of the same conversation (§6.2).
    Reply {
        sent_at: u32,
        target: MessageRef,
        text: String,
    },
    /// The sender's reaction to message `target` (§6.2); empty removes it.
    Reaction {
        sent_at: u32,
        target: MessageRef,
        emoji: String,
    },
    /// A small file (§13.2 kinds 2 and 3, ADR-0022, ADR-0023): a name and its content, to a
    /// contact. Encoded compressed when that is smaller; at most 8 KB on the air, 64 KiB inflated.
    File {
        sent_at: u32,
        name: String,
        data: Vec<u8>,
    },
    /// An animated sticker every phone ships, named by its emoji (§6.3).
    Sticker {
        sent_at: u32,
        set: u8,
        emoji: String,
    },
    /// A GIF a provider serves, named by its id (§6.3): the receiver builds the address.
    Gif {
        sent_at: u32,
        provider: u8,
        width: u16,
        height: u16,
        id: String,
        title: String,
    },
    /// The new text of the sender's TEXT or REPLY `target` (§6.4).
    Edit {
        sent_at: u32,
        target: MessageRef,
        text: String,
    },
    /// The sender takes its message `target` back (§6.4).
    Delete { sent_at: u32, target: MessageRef },
    /// From now on, the conversation's messages disappear `seconds` after each phone stored
    /// them; 0: they stay (§6.5, ADR-0030).
    Timer { sent_at: u32, seconds: u32 },
    /// A membership notice this phone records in a group's conversation (§6, 0xE0, local:
    /// never on the air).
    Notice(Notice),
    /// What is left of a message its author deleted (§6.4, 0xE1, local): when it was written.
    Deleted { sent_at: u32 },
    /// A type this version does not interpret.
    Other { content_type: u8, body: Vec<u8> },
}

/// The local content types, 0xE0–0xEF (§6): never sent, dropped when received.
pub const LOCAL_TYPES: std::ops::RangeInclusive<u8> = 0xE0..=0xEF;
/// A membership notice (ADR-0021).
pub const NOTICE: u8 = 0xE0;
/// A deleted message's place in the history (ADR-0027).
pub const DELETED: u8 = 0xE1;

/// How long after its own timestamp a message can still be edited or deleted (§6.4), seconds.
pub const EDIT_WINDOW_S: u32 = 86_400;
/// The longest disappearing-message timer (§6.5): four weeks.
pub const TIMER_MAX_S: u32 = 28 * 86_400;

/// What changed in a group, as the conversation shows it (ADR-0021).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// One of [`notice`].
    pub kind: u8,
    /// Who did it (a node; ours when we did), and their name then.
    pub actor: [u8; 32],
    pub actor_name: String,
    /// Whom it concerns (nodes and names): who joined, left or was removed.
    pub subjects: Vec<([u8; 32], String)>,
    /// The new name, for a rename.
    pub text: String,
}

/// [`Notice::kind`] values.
pub mod notice {
    /// The actor (us) created the group.
    pub const CREATED: u8 = 1;
    /// The actor (the admin) added us.
    pub const ADDED_US: u8 = 2;
    /// The subjects joined.
    pub const JOINED: u8 = 3;
    /// The subjects left.
    pub const LEFT: u8 = 4;
    /// The actor (the admin) removed the subjects.
    pub const REMOVED: u8 = 5;
    /// The subjects are no longer in the group (no cause given).
    pub const GONE: u8 = 6;
    /// The actor (the admin) removed us.
    pub const REMOVED_US: u8 = 7;
    /// The actor (the admin) closed the group.
    pub const CLOSED: u8 = 8;
    /// The actor (the admin) renamed the group to `text`.
    pub const RENAMED: u8 = 9;
    /// We left the group.
    pub const WE_LEFT: u8 = 10;
    /// The actor set messages to disappear after `text` seconds ("0": they stay), §6.5.
    pub const TIMER: u8 = 11;
    /// The actor's radio now advertises the name `text`; `actor_name` is the name shown
    /// before. A verified contact keeps the name it was verified under (review of
    /// 2026-11-15, SR-30): the notice is how the user learns of the change.
    pub const NODE_RENAMED: u8 = 12;
}

fn put_short(v: &mut Vec<u8>, s: &str) {
    let mut end = s.len().min(255);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    v.push(end as u8);
    v.extend_from_slice(s.as_bytes().get(..end).unwrap_or(&[]));
}

fn take_short(b: &mut &[u8]) -> Option<String> {
    let (&n, rest) = b.split_first()?;
    let (s, rest) = rest.split_at_checked(usize::from(n))?;
    *b = rest;
    String::from_utf8(s.to_vec()).ok()
}

impl Notice {
    fn encode_body(&self, v: &mut Vec<u8>) {
        v.push(self.kind);
        v.extend_from_slice(&self.actor);
        put_short(v, &self.actor_name);
        v.push(self.subjects.len().min(255) as u8);
        for (node, name) in self.subjects.iter().take(255) {
            v.extend_from_slice(node);
            put_short(v, name);
        }
        put_short(v, &self.text);
    }

    fn decode_body(mut b: &[u8]) -> Option<Self> {
        let (&kind, rest) = b.split_first()?;
        b = rest;
        let (actor, rest) = b.split_first_chunk::<32>()?;
        b = rest;
        let actor_name = take_short(&mut b)?;
        let (&m, rest) = b.split_first()?;
        b = rest;
        let mut subjects = Vec::with_capacity(usize::from(m));
        for _ in 0..m {
            let (node, rest) = b.split_first_chunk::<32>()?;
            b = rest;
            subjects.push((*node, take_short(&mut b)?));
        }
        let text = take_short(&mut b)?;
        b.is_empty().then_some(Self {
            kind,
            actor: *actor,
            actor_name,
            subjects,
            text,
        })
    }
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
            Content::Reply {
                sent_at,
                target,
                text,
            } => {
                let mut v = Vec::with_capacity(13 + text.len());
                v.push(content::REPLY);
                v.extend_from_slice(&sent_at.to_le_bytes());
                v.extend_from_slice(target);
                v.extend_from_slice(text.as_bytes());
                v
            }
            Content::Reaction {
                sent_at,
                target,
                emoji,
            } => {
                let mut v = Vec::with_capacity(13 + emoji.len());
                v.push(content::REACTION);
                v.extend_from_slice(&sent_at.to_le_bytes());
                v.extend_from_slice(target);
                v.extend_from_slice(emoji.as_bytes());
                v
            }
            Content::File {
                sent_at,
                name,
                data,
            } => {
                // Compressed when the datagram gets smaller (ADR-0023): its size field costs 4.
                let deflated = (data.len() <= FILE_INFLATED_MAX)
                    .then(|| miniz_oxide::deflate::compress_to_vec(data, 9))
                    .filter(|d| d.len() + 4 < data.len());
                let mut v = Vec::with_capacity(11 + name.len() + data.len());
                v.push(content::FILE);
                v.extend_from_slice(&sent_at.to_le_bytes());
                v.push(if deflated.is_some() {
                    KIND_FILE_DEFLATE
                } else {
                    KIND_FILE
                });
                v.push(name.len().min(255) as u8);
                v.extend_from_slice(name.as_bytes());
                match deflated {
                    Some(d) => {
                        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
                        v.extend_from_slice(&d);
                    }
                    None => v.extend_from_slice(data),
                }
                v
            }
            Content::Sticker {
                sent_at,
                set,
                emoji,
            } => {
                let mut v = vec![content::STICKER];
                v.extend_from_slice(&sent_at.to_le_bytes());
                v.push(*set);
                v.extend_from_slice(emoji.as_bytes());
                v
            }
            Content::Gif {
                sent_at,
                provider,
                width,
                height,
                id,
                title,
            } => {
                let mut v = vec![content::GIF];
                v.extend_from_slice(&sent_at.to_le_bytes());
                v.push(*provider);
                v.extend_from_slice(&width.to_le_bytes());
                v.extend_from_slice(&height.to_le_bytes());
                v.push(id.len().min(255) as u8);
                v.extend_from_slice(id.as_bytes());
                v.extend_from_slice(title.as_bytes());
                v
            }
            Content::Edit {
                sent_at,
                target,
                text,
            } => {
                let mut v = Vec::with_capacity(13 + text.len());
                v.push(content::EDIT);
                v.extend_from_slice(&sent_at.to_le_bytes());
                v.extend_from_slice(target);
                v.extend_from_slice(text.as_bytes());
                v
            }
            Content::Delete { sent_at, target } => {
                let mut v = Vec::with_capacity(13);
                v.push(content::DELETE);
                v.extend_from_slice(&sent_at.to_le_bytes());
                v.extend_from_slice(target);
                v
            }
            Content::Timer { sent_at, seconds } => {
                let mut v = Vec::with_capacity(9);
                v.push(content::TIMER);
                v.extend_from_slice(&sent_at.to_le_bytes());
                v.extend_from_slice(&seconds.to_le_bytes());
                v
            }
            Content::Notice(n) => {
                let mut v = vec![NOTICE];
                n.encode_body(&mut v);
                v
            }
            Content::Deleted { sent_at } => {
                let mut v = vec![DELETED];
                v.extend_from_slice(&sent_at.to_le_bytes());
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
        } else if content_type == content::FILE
            && matches!(body.get(4), Some(&KIND_FILE | &KIND_FILE_DEFLATE))
        {
            decode_file(body)
        } else if content_type == content::ANNOUNCE {
            decode_announce(body)
        } else if content_type == content::REPLY {
            let (ts, rest) = body.split_first_chunk::<4>()?;
            let (target, text) = rest.split_first_chunk::<8>()?;
            if text.is_empty() {
                return None;
            }
            Some(Content::Reply {
                sent_at: u32::from_le_bytes(*ts),
                target: *target,
                text: String::from_utf8_lossy(text).into_owned(),
            })
        } else if content_type == content::REACTION {
            let (ts, rest) = body.split_first_chunk::<4>()?;
            let (target, emoji) = rest.split_first_chunk::<8>()?;
            if emoji.len() > REACTION_MAX {
                return None;
            }
            Some(Content::Reaction {
                sent_at: u32::from_le_bytes(*ts),
                target: *target,
                emoji: std::str::from_utf8(emoji).ok()?.to_owned(),
            })
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
        } else if content_type == content::STICKER {
            let (ts, rest) = body.split_first_chunk::<4>()?;
            let (&set, emoji) = rest.split_first()?;
            if emoji.is_empty() || emoji.len() > STICKER_MAX {
                return None;
            }
            Some(Content::Sticker {
                sent_at: u32::from_le_bytes(*ts),
                set,
                emoji: std::str::from_utf8(emoji).ok()?.to_owned(),
            })
        } else if content_type == content::GIF {
            decode_gif(body)
        } else if content_type == content::EDIT {
            let (ts, rest) = body.split_first_chunk::<4>()?;
            let (target, text) = rest.split_first_chunk::<8>()?;
            if text.is_empty() {
                return None;
            }
            Some(Content::Edit {
                sent_at: u32::from_le_bytes(*ts),
                target: *target,
                text: std::str::from_utf8(text).ok()?.to_owned(),
            })
        } else if content_type == content::DELETE {
            let (ts, rest) = body.split_first_chunk::<4>()?;
            let target: [u8; 8] = rest.try_into().ok()?;
            Some(Content::Delete {
                sent_at: u32::from_le_bytes(*ts),
                target,
            })
        } else if content_type == content::TIMER {
            let (ts, rest) = body.split_first_chunk::<4>()?;
            let seconds = u32::from_le_bytes(rest.try_into().ok()?);
            (seconds <= TIMER_MAX_S).then_some(Content::Timer {
                sent_at: u32::from_le_bytes(*ts),
                seconds,
            })
        } else if content_type == NOTICE {
            Notice::decode_body(body).map(Content::Notice)
        } else if content_type == DELETED {
            Some(Content::Deleted {
                sent_at: u32::from_le_bytes(body.try_into().ok()?),
            })
        } else {
            Some(Content::Other {
                content_type,
                body: body.to_vec(),
            })
        }
    }

    /// The sender's timestamp, for the messages that carry one.
    pub const fn sent_at(&self) -> Option<u32> {
        match self {
            Content::Text { sent_at, .. }
            | Content::Voice { sent_at, .. }
            | Content::Image { sent_at, .. }
            | Content::Reply { sent_at, .. }
            | Content::Reaction { sent_at, .. }
            | Content::File { sent_at, .. }
            | Content::Sticker { sent_at, .. }
            | Content::Gif { sent_at, .. }
            | Content::Edit { sent_at, .. }
            | Content::Delete { sent_at, .. }
            | Content::Timer { sent_at, .. }
            | Content::Deleted { sent_at } => Some(*sent_at),
            Content::Announce(a) => Some(a.sent_at),
            Content::Location(_)
            | Content::MediaRequest { .. }
            | Content::Notice(_)
            | Content::Other { .. } => None,
        }
    }

    /// A local type (0xE0–0xEF, §6): never accepted from the air.
    pub fn is_local(datagram: &[u8]) -> bool {
        datagram.first().is_some_and(|t| LOCAL_TYPES.contains(t))
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

/// A GIF id: 1..=32 ASCII letters and digits (§6.3), so that it can only ever be one path
/// segment of the provider's fixed address.
fn gif_id_ok(id: &[u8]) -> bool {
    (1..=GIF_ID_MAX).contains(&id.len()) && id.iter().all(u8::is_ascii_alphanumeric)
}

/// `ts ‖ provider ‖ width u16 ‖ height u16 ‖ id_len ‖ id ‖ title` (§6.3).
fn decode_gif(body: &[u8]) -> Option<Content> {
    let (head, rest) = body.split_first_chunk::<10>()?;
    let [t0, t1, t2, t3, provider, w0, w1, h0, h1, n] = *head;
    let (id, title) = rest.split_at_checked(usize::from(n))?;
    if !gif_id_ok(id) || title.len() > GIF_TITLE_MAX {
        return None;
    }
    Some(Content::Gif {
        sent_at: u32::from_le_bytes([t0, t1, t2, t3]),
        provider,
        width: u16::from_le_bytes([w0, w1]),
        height: u16::from_le_bytes([h0, h1]),
        id: std::str::from_utf8(id).ok()?.to_owned(),
        title: std::str::from_utf8(title).ok()?.to_owned(),
    })
}

/// The GIPHY id and title of a link to a GIF (§6.3), when `text` is only that link:
/// `https://giphy.com/gifs/[slug-]{id}`, `https://media[0-9].giphy.com/media/[v1.…/]{id}/…`,
/// `https://i.giphy.com/{id}.(gif|webp)`. The slug, if any, gives the title.
pub fn gif_link(text: &str) -> Option<(String, String)> {
    let rest = text.trim().strip_prefix("https://")?;
    if rest.contains(char::is_whitespace) {
        return None;
    }
    let rest = rest.split(['?', '#']).next()?;
    let (host, path) = rest.split_once('/')?;
    let host = host.to_ascii_lowercase();
    let mut segments = path.split('/').filter(|s| !s.is_empty());
    let (id, title) = match host.as_str() {
        "giphy.com" | "www.giphy.com" => {
            if segments.next()? != "gifs" {
                return None;
            }
            let last = segments.next()?;
            match last.rsplit_once('-') {
                Some((slug, id)) => (id, slug.replace('-', " ")),
                None => (last, String::new()),
            }
        }
        "i.giphy.com" => {
            let first = segments.next()?;
            if first == "media" {
                (media_id(&mut segments)?, String::new())
            } else {
                let (id, ext) = first.rsplit_once('.')?;
                if !matches!(ext, "gif" | "webp") {
                    return None;
                }
                (id, String::new())
            }
        }
        h if h == "media.giphy.com"
            || (h.len() == 16
                && h.starts_with("media")
                && h.as_bytes().get(5).is_some_and(u8::is_ascii_digit)
                && h.ends_with(".giphy.com")) =>
        {
            if segments.next()? != "media" {
                return None;
            }
            (media_id(&mut segments)?, String::new())
        }
        _ => return None,
    };
    if !gif_id_ok(id.as_bytes()) {
        return None;
    }
    let mut title = title.trim().to_owned();
    while title.len() > GIF_TITLE_MAX {
        title.pop();
    }
    Some((id.to_owned(), title))
}

/// The id in `/media/[v1.…/]{id}/…`.
fn media_id<'a>(segments: &mut impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let first = segments.next()?;
    if first.starts_with("v1.") {
        segments.next()
    } else {
        Some(first)
    }
}

/// `ts ‖ 2 ‖ name_len ‖ name ‖ data`, or `ts ‖ 3 ‖ name_len ‖ name ‖ size u32 ‖ deflate`
/// (§13.2): a name of 1..=64 bytes of UTF-8, 1..=8,192 bytes of data on the air.
fn decode_file(body: &[u8]) -> Option<Content> {
    let (ts, rest) = body.split_first_chunk::<4>()?;
    let (&[kind, n], rest) = rest.split_first_chunk::<2>()?;
    let n = usize::from(n);
    if !(1..=FILE_NAME_MAX).contains(&n) {
        return None;
    }
    let (name, rest) = rest.split_at_checked(n)?;
    let data = if kind == KIND_FILE_DEFLATE {
        let (size, stream) = rest.split_first_chunk::<4>()?;
        inflate(stream, usize::try_from(u32::from_le_bytes(*size)).ok()?)?
    } else {
        rest.to_vec()
    };
    if data.is_empty() || (kind == KIND_FILE && data.len() > FILE_MAX) {
        return None;
    }
    Some(Content::File {
        sent_at: u32::from_le_bytes(*ts),
        name: std::str::from_utf8(name).ok()?.to_owned(),
        data,
    })
}

/// Raw DEFLATE (RFC 1951) into exactly `size` bytes, 1..=64 KiB, from a stream of 1..=8,192
/// bytes that ends within itself (ADR-0023). Never allocates more than `size`.
fn inflate(stream: &[u8], size: usize) -> Option<Vec<u8>> {
    use miniz_oxide::inflate::TINFLStatus;
    use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};
    if !(1..=FILE_INFLATED_MAX).contains(&size) || !(1..=FILE_MAX).contains(&stream.len()) {
        return None;
    }
    let mut out = vec![0u8; size];
    let mut state = Box::<DecompressorOxide>::default();
    let (status, read, written) = decompress(
        &mut state,
        stream,
        &mut out,
        0,
        inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
    );
    (status == TINFLStatus::Done && read == stream.len() && written == size).then_some(out)
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

    fn file(data: Vec<u8>) -> Content {
        Content::File {
            sent_at: 1_793_001_000,
            name: "track.gpx".into(),
            data,
        }
    }

    #[test]
    fn files_are_compressed_when_smaller() {
        let text = b"<trkpt lat=\"45.07\" lon=\"5.52\"><ele>1200</ele></trkpt>\n".repeat(400);
        let c = file(text.clone());
        let d = c.encode();
        assert_eq!(d.get(5), Some(&KIND_FILE_DEFLATE));
        assert!(d.len() < text.len() / 4, "{} bytes", d.len());
        assert_eq!(Content::decode(&d), Some(c));
        // Already compressed: sent as it is.
        let noise: Vec<u8> = (0..3000u32)
            .flat_map(|i| Sha256::digest(i.to_le_bytes()).to_vec())
            .take(3000)
            .collect();
        let c = file(noise);
        let d = c.encode();
        assert_eq!(d.get(5), Some(&KIND_FILE));
        assert_eq!(Content::decode(&d), Some(c));
        // Too short to gain anything.
        let c = file(b"code 4521".to_vec());
        assert_eq!(c.encode().get(5), Some(&KIND_FILE));
    }

    #[test]
    fn inflating_is_bounded() {
        let c = file(vec![0; FILE_INFLATED_MAX]);
        let d = c.encode();
        assert!(d.len() < 200, "{} bytes", d.len());
        assert_eq!(Content::decode(&d), Some(c));
        // One byte more inflated than announced, or one byte less: dropped.
        let stream = miniz_oxide::deflate::compress_to_vec(&[7; 1000], 9);
        for size in [999u32, 1001] {
            let mut d = vec![content::FILE, 0, 0, 0, 0, KIND_FILE_DEFLATE, 1, b'a'];
            d.extend_from_slice(&size.to_le_bytes());
            d.extend_from_slice(&stream);
            assert_eq!(Content::decode(&d), None, "{size}");
        }
        // Trailing bytes after the end of the stream: dropped.
        let mut d = vec![content::FILE, 0, 0, 0, 0, KIND_FILE_DEFLATE, 1, b'a'];
        d.extend_from_slice(&1000u32.to_le_bytes());
        d.extend_from_slice(&stream);
        d.push(0);
        assert_eq!(Content::decode(&d), None);
    }

    #[test]
    fn notice_round_trip() {
        let n = Content::Notice(Notice {
            kind: notice::REMOVED,
            actor: [1; 32],
            actor_name: "Camille".into(),
            subjects: vec![([2; 32], "Léa".into()), ([3; 32], String::new())],
            text: String::new(),
        });
        let d = n.encode();
        assert_eq!(d[0], 0xE0);
        assert!(Content::is_local(&d));
        assert_eq!(Content::decode(&d), Some(n));
        assert_eq!(Content::decode(&d[..d.len() - 1]), None);
        assert!(!Content::is_local(&[content::TEXT, 0, 0, 0, 0, b'a']));
    }

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
        // A FILE kind this version does not know (1 image, 2 small file).
        img[5] = 9;
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

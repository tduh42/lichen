//! A message's content as the app sees it.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Message {
    Text {
        sent_at: u32,
        text: String,
    },
    /// Codec2 frames (§13.1); `mode` 0 = 700C, 1 = 1200. Play with [`decode_voice`].
    Voice {
        sent_at: u32,
        mode: u8,
        frames: u16,
        duration_ms: u32,
        data: Vec<u8>,
    },
    /// An AVIF thumbnail (§13.2): the preview, or (`full`) the full variant.
    Image {
        sent_at: u32,
        media_id: Vec<u8>,
        full: bool,
        width: u16,
        height: u16,
        /// Bytes of the full variant (0: none).
        full_len: u16,
        data: Vec<u8>,
    },
    /// Content offered over the Internet bridge (§14.1): fetch it with `LichenBridge`.
    Announce {
        sent_at: u32,
        media_id: Vec<u8>,
        /// 1 image, 2 voice, 3 file.
        kind: u8,
        hash: Vec<u8>,
        size: u32,
        providers: Vec<Provider>,
        name: String,
    },
    /// A small file over LoRa (§13.2 kind 2, ADR-0022): its name and content (up to 8 KB).
    File {
        sent_at: u32,
        name: String,
        data: Vec<u8>,
    },
    /// An animated sticker the apps ship (§6.3): `set` 1 is Noto Animated Emoji, named by
    /// its emoji. Without its animation, show the emoji.
    Sticker {
        sent_at: u32,
        set: u8,
        emoji: String,
    },
    /// A GIF its provider serves (§6.3): `provider` 1 is GIPHY; load it with
    /// `LichenBridge::gif`. `width` × `height` 0 when unknown.
    Gif {
        sent_at: u32,
        provider: u8,
        width: u16,
        height: u16,
        id: String,
        title: String,
    },
    /// A membership change in a group, recorded by this phone (ADR-0021): `kind` is one of
    /// `lc_core::content::notice` (1 created, 2 added us, 3 joined, 4 left, 5 removed, 6 no
    /// longer in the group, 7 removed us, 8 closed, 9 renamed to `text`, 10 we left), or a
    /// disappearing-message timer set to `text` seconds, "0" for off (11, §6.5), or a contact's
    /// radio now advertising the name `text`, `actor_name` being the name shown before (12).
    Notice {
        kind: u8,
        /// Who did it (a node, ours when we did), and their name then.
        actor: Vec<u8>,
        actor_name: String,
        /// Whom it concerns.
        subjects: Vec<NoticeSubject>,
        text: String,
    },
    /// A message its author took back (§6.4): its place in the history, when it was written.
    Deleted {
        sent_at: u32,
    },
    Other {
        content_type: u8,
        body: Vec<u8>,
    },
}

/// Someone a notice is about: their node and their name then.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NoticeSubject {
    pub node: Vec<u8>,
    pub name: String,
}

/// Where announced content can be fetched: transport 1 iroh, 2 Tor.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Provider {
    pub transport: u8,
    pub key: Vec<u8>,
}

/// How long a received position stays on the map, in seconds.
pub(crate) const POSITION_KEEP_S: u64 = 24 * 3600;

/// A file name as the apps show and open it: without the characters that reorder text
/// (bidirectional controls), which can make `photo` U+202E `gpj.exe` read as `photoexe.jpg` (review
/// of 2026-11-15, SR-35). The name as sent is kept for references (§6.2).
pub(crate) fn shown_name(name: &str) -> String {
    name.chars().filter(|c| !is_bidi_control(*c)).collect()
}

/// Unicode's bidirectional formatting characters.
pub(crate) const fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

impl From<Content> for Message {
    fn from(c: Content) -> Self {
        match c {
            // A reply is a text; its quote comes with the stored message (§6.2).
            Content::Text { sent_at, text } | Content::Reply { sent_at, text, .. } => {
                Self::Text { sent_at, text }
            }
            // Reactions, edits, deletes and timers are state, never shown as messages (a
            // timer shows as its notice).
            c @ (Content::Reaction { .. }
            | Content::Edit { .. }
            | Content::Delete { .. }
            | Content::Timer { .. }) => {
                let mut body = c.encode();
                let content_type = body.remove(0);
                Self::Other { content_type, body }
            }
            Content::Location(p) => Self::Other {
                content_type: lc_proto::content::LOCATION,
                body: p.encode().to_vec(),
            },
            Content::Voice {
                sent_at,
                mode,
                frames,
                data,
            } => Self::Voice {
                sent_at,
                mode,
                frames,
                duration_ms: lc_media::voice::duration_ms(frames),
                data,
            },
            Content::Image {
                sent_at,
                media_id,
                full,
                width,
                height,
                full_len,
                data,
            } => Self::Image {
                sent_at,
                media_id: media_id.to_vec(),
                full,
                width,
                height,
                full_len,
                data,
            },
            Content::Announce(a) => Self::Announce {
                sent_at: a.sent_at,
                media_id: a.media_id.to_vec(),
                kind: a.kind,
                hash: a.hash.to_vec(),
                size: a.size,
                providers: a
                    .providers
                    .into_iter()
                    .map(|(transport, key)| Provider {
                        transport,
                        key: key.to_vec(),
                    })
                    .collect(),
                name: shown_name(&a.name),
            },
            c @ Content::MediaRequest { .. } => {
                let mut v = c.encode();
                let content_type = v.remove(0);
                Self::Other {
                    content_type,
                    body: v,
                }
            }
            Content::File {
                sent_at,
                name,
                data,
            } => Self::File {
                sent_at,
                name: shown_name(&name),
                data,
            },
            Content::Sticker {
                sent_at,
                set,
                emoji,
            } => Self::Sticker {
                sent_at,
                set,
                emoji,
            },
            Content::Gif {
                sent_at,
                provider,
                width,
                height,
                id,
                title,
            } => Self::Gif {
                sent_at,
                provider,
                width,
                height,
                id,
                title,
            },
            Content::Notice(n) => Self::Notice {
                kind: n.kind,
                actor: n.actor.to_vec(),
                actor_name: n.actor_name,
                subjects: n
                    .subjects
                    .into_iter()
                    .map(|(node, name)| NoticeSubject {
                        node: node.to_vec(),
                        name,
                    })
                    .collect(),
                text: n.text,
            },
            Content::Deleted { sent_at } => Self::Deleted { sent_at },
            Content::Other { content_type, body } => Self::Other { content_type, body },
        }
    }
}

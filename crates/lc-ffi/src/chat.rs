//! Conversations: references, channels, statuses, stored messages.

use super::*;

pub(crate) fn key(v: &[u8]) -> Result<PubKey, LichenError> {
    v.try_into().map_err(|_| LichenError::InvalidKey)
}

pub(crate) fn gid(v: &[u8]) -> Result<GroupId, LichenError> {
    v.try_into().map_err(|_| LichenError::InvalidKey)
}

pub(crate) fn unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// A conversation.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ChatRef {
    Contact {
        key: Vec<u8>,
    },
    Group {
        gid: Vec<u8>,
    },
    /// Plain MeshCore direct messages with node `key` (§15.2): not lichen-encrypted.
    Mesh {
        key: Vec<u8>,
    },
    /// A MeshCore channel (§15.3), by its 8-byte id.
    Channel {
        id: Vec<u8>,
    },
    /// A MeshCore room server's posts (§15.8), by its node key: not lichen-encrypted.
    Room {
        key: Vec<u8>,
    },
}

impl ChatRef {
    pub(crate) fn to_store(&self) -> Result<Chat, LichenError> {
        Ok(match self {
            Self::Contact { key: k } => Chat::Contact(key(k)?),
            Self::Group { gid: g } => Chat::Group(gid(g)?),
            Self::Mesh { key: k } => Chat::Mesh(key(k)?),
            Self::Channel { id } => Chat::Channel(channel(id)?),
            Self::Room { key: k } => Chat::Room(key(k)?),
        })
    }

    pub(crate) fn from_store(c: &Chat) -> Self {
        match c {
            Chat::Contact(k) => Self::Contact { key: k.to_vec() },
            Chat::Group(g) => Self::Group { gid: g.to_vec() },
            Chat::Mesh(k) => Self::Mesh { key: k.to_vec() },
            Chat::Channel(c) => Self::Channel { id: c.to_vec() },
            Chat::Room(k) => Self::Room { key: k.to_vec() },
        }
    }
}

/// A private channel's 16-byte secret from 32 hex digits (spaces allowed); never all zero.
pub(crate) fn secret_from_hex(s: &str) -> Option<[u8; 16]> {
    let digits: Vec<u8> = s
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_digit(16).and_then(|d| u8::try_from(d).ok()))
        .collect::<Option<_>>()?;
    if digits.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (o, pair) in out.iter_mut().zip(digits.chunks(2)) {
        *o = pair.first()? << 4 | pair.get(1)?;
    }
    (out != [0; 16]).then_some(out)
}

pub(crate) fn plain_chat(c: PlainChat) -> Chat {
    match c {
        PlainChat::Direct(k) => Chat::Mesh(k),
        PlainChat::Channel(id) => Chat::Channel(id),
        PlainChat::Room(k) => Chat::Room(k),
    }
}

pub(crate) fn channel(b: &[u8]) -> Result<ChannelId, LichenError> {
    b.try_into().map_err(|_| LichenError::InvalidKey)
}

/// What kind of MeshCore channel (§15.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ChannelKind {
    /// MeshCore's public channel: anyone in range reads it.
    PublicChannel,
    /// `#tag`: anyone who knows the tag reads it.
    HashtagChannel,
    /// A secret shared by whoever created it.
    PrivateChannel,
}

/// A MeshCore channel of the radio.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MeshChannel {
    pub id: Vec<u8>,
    pub name: String,
    pub kind: ChannelKind,
}

impl From<&lc_core::Channel> for MeshChannel {
    fn from(c: &lc_core::Channel) -> Self {
        let kind = if c.secret == lc_core::plain::PUBLIC_SECRET {
            ChannelKind::PublicChannel
        } else if lc_core::plain::hashtag(&c.name).is_some_and(|(_, s)| s == c.secret) {
            ChannelKind::HashtagChannel
        } else {
            ChannelKind::PrivateChannel
        };
        Self {
            id: c.id().to_vec(),
            name: c.name.clone(),
            kind,
        }
    }
}

/// Why a message we sent is waiting (§6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum WaitReason {
    /// The radio is disconnected.
    Radio,
    /// Earlier messages to the same person go first.
    Earlier,
    /// Looking for a path to them.
    Path,
    /// Opening a secure session with them.
    Handshake,
    /// The airtime budget holds it until about `at_ms` (Unix milliseconds).
    Budget { at_ms: i64 },
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MessageStatus {
    Received,
    /// Accepted and waiting (§6.1). `started`: a frame of it already went out, so it may have
    /// arrived and can no longer be cancelled. It stops at `expires_at_ms` (Unix ms) unless it
    /// is being transmitted then.
    Waiting {
        reason: WaitReason,
        started: bool,
        expires_at_ms: i64,
    },
    Sending {
        sent: u32,
        total: u8,
    },
    Delivered,
    Failed {
        reason: FailReason,
    },
    /// Flooded to a group, without acknowledgement.
    Flooded,
}

/// A message from the history.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StoredMessage {
    pub id: i64,
    pub chat: ChatRef,
    pub outgoing: bool,
    pub author: Option<Vec<u8>>,
    pub message: Message,
    /// Unix milliseconds.
    pub at_ms: i64,
    pub status: MessageStatus,
    pub fragments: u8,
    pub airtime_ms: u64,
    pub snr_db: Option<f32>,
    /// Seen by the user (ours always are): the unread divider goes before the first one that
    /// is not.
    pub read: bool,
    /// It has a reference: it can be answered and reacted to (§6.2).
    pub referable: bool,
    /// The message it answers, for a reply.
    pub quote: Option<Quote>,
    /// Reactions to it, the most frequent first.
    pub reactions: Vec<ReactionCount>,
    /// A group message recovered from a member after it was lost on the air (§10.6): shown
    /// where it arrived, with "arrived late" and its sending time.
    pub late: bool,
    /// The author's name as its radio advertises it: posts on a MeshCore channel (§15.1),
    /// whose authors are known by name only.
    pub author_name: Option<String>,
    /// Its author changed its text (§6.4): `message` shows the current one.
    pub edited: bool,
    /// Ours, and still within 24 hours: it can be edited (a text or a reply) or deleted for
    /// everyone (§6.4).
    pub editable: bool,
    pub deletable: bool,
}

/// What kind of message a reply answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum QuoteKind {
    Text,
    Voice,
    Photo,
    /// A file; the excerpt is its name.
    File,
    /// A sticker; the excerpt is its emoji.
    Sticker,
    /// A GIF; the excerpt is its title.
    Gif,
    Other,
}

/// The message a reply answers (§6.2), as far as this phone knows it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Quote {
    /// The original in this conversation's history; `None`: not on this phone.
    pub message_id: Option<i64>,
    /// Ours.
    pub outgoing: bool,
    /// Its author's identity, in a group.
    pub author: Option<Vec<u8>>,
    pub kind: QuoteKind,
    /// The start of its text.
    pub excerpt: String,
}

/// One emoji's reactions to a message.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ReactionCount {
    pub emoji: String,
    pub count: u32,
    /// Ours is among them.
    pub mine: bool,
}

/// A conversation's local preferences: pinned (since when, Unix ms), muted (until when),
/// archived (out of the list until a message comes, ADR-0034).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ChatPrefs {
    pub chat: ChatRef,
    pub pinned_at_ms: Option<i64>,
    pub muted_until_ms: Option<i64>,
    pub archived: bool,
}

impl From<store::Message> for StoredMessage {
    fn from(m: store::Message) -> Self {
        Self {
            id: m.id,
            chat: ChatRef::from_store(&m.chat),
            outgoing: m.outgoing,
            author: m.author.map(|a| a.to_vec()),
            message: m.content.into(),
            at_ms: m.at_ms,
            late: m.status == Status::Late,
            status: match m.status {
                Status::Received | Status::Late => MessageStatus::Received,
                // Without the outbox's view (it is filled in by `Inner::present`).
                Status::Queued => MessageStatus::Waiting {
                    reason: WaitReason::Radio,
                    started: false,
                    expires_at_ms: 0,
                },
                Status::Sending { sent, total } => MessageStatus::Sending { sent, total },
                Status::Delivered => MessageStatus::Delivered,
                Status::Failed(r) => MessageStatus::Failed { reason: r.into() },
                Status::Flooded => MessageStatus::Flooded,
            },
            fragments: m.fragments,
            airtime_ms: m.airtime_ms,
            snr_db: m.snr4.map(snr),
            read: m.read,
            referable: m.msg_ref.is_some(),
            quote: None,
            reactions: Vec::new(),
            author_name: None,
            edited: false,
            editable: false,
            deletable: false,
        }
    }
}

/// What a conversation takes on this phone (ADR-0034).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ChatUsage {
    pub chat: ChatRef,
    pub messages: u32,
    /// Photos, files and voice messages among them.
    pub media: u32,
    /// Bytes in the database: messages and the media kept for it.
    pub bytes: u64,
    /// Content it announced (§14.1): the app adds what it downloaded of it.
    pub announced: Vec<Vec<u8>>,
}

/// A conversation with its last message.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Conversation {
    pub chat: ChatRef,
    pub last: StoredMessage,
    pub unread: u32,
    /// What lichen calls it: the group's or the channel's name; for a node (a contact, a
    /// MeshCore chat, a room), its nickname, else its contact's name, else the name the radio
    /// reported for it. Empty when nothing is known: the apps show the key (§15.8).
    pub name: String,
}

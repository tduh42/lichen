//! Persistent storage (SQLite): message history, contacts, groups, location shares and the
//! last positions received. One database per MeshCore node. Sessions and our own group sender keys are never stored (ADR-0003, protocol.md §10.2).
//!
//! The core stays IO-free: the runner (`lc-ffi`) records what the core reports and restores
//! contacts and groups at start.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use unicode_normalization::UnicodeNormalization;

use crate::content::{Content, MediaId, MessageRef};

fn media_id(c: &Content) -> Option<MediaId> {
    match c {
        Content::Image { media_id, .. } => Some(*media_id),
        _ => None,
    }
}
use crate::core::{Contact, FailReason, OwnedPath, PubKey};
use crate::group::GroupId;
use crate::location::{Precision, Share};
use crate::plain::{Channel, ChannelId};
use lc_proto::location::Position;

pub use rusqlite::Error;

/// Schema version (`PRAGMA user_version`).
/// 2: location shares and positions (tables added, nothing migrated).
/// 3: images (`messages.media_id`, added to older databases; `media` table).
/// 4: phone identities (ADR-0013): `contacts.identity`, `contacts.verified`, added likewise.
/// 5: nicknames the user gives contacts (table added, nothing migrated).
/// 6: settings and search consents (tables added).
/// 7: the outbox (ADR-0018): `messages.expires_ms`, added to older databases; status 5
/// (queued).
/// 8: pinned and muted conversations (`chat_prefs`, table added).
/// 9: replies and reactions (ADR-0019): `messages.ref`, added to older databases and filled by
/// the app; `reactions` (table added).
/// 10: MeshCore chats (ADR-0026): chat kinds 2 (a node, plain) and 3 (a channel), and
/// `channels` (table added, nothing migrated).
/// 11: edits and deletes (ADR-0027): `amendments` (table added, nothing migrated).
const VERSION: i32 = 12;

/// A conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Chat {
    Contact(PubKey),
    Group(GroupId),
    /// Plain MeshCore direct messages with a node (§15.2), apart from any lichen chat with it.
    Mesh(PubKey),
    /// A MeshCore channel (§15.3). Received posts are stored as they came, "name: text".
    Channel(ChannelId),
}

impl Chat {
    fn kind(&self) -> i64 {
        match self {
            Chat::Contact(_) => 0,
            Chat::Group(_) => 1,
            Chat::Mesh(_) => 2,
            Chat::Channel(_) => 3,
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Chat::Contact(k) | Chat::Mesh(k) => k,
            Chat::Group(g) => g,
            Chat::Channel(c) => c,
        }
    }

    fn from_row(kind: i64, bytes: &[u8]) -> Option<Self> {
        match kind {
            0 => bytes.try_into().ok().map(Chat::Contact),
            1 => bytes.try_into().ok().map(Chat::Group),
            2 => bytes.try_into().ok().map(Chat::Mesh),
            3 => bytes.try_into().ok().map(Chat::Channel),
            _ => None,
        }
    }

    /// A plain MeshCore conversation (§15): text only, not lichen-encrypted.
    pub const fn is_plain(&self) -> bool {
        matches!(self, Chat::Mesh(_) | Chat::Channel(_))
    }
}

/// Where a message stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Received,
    /// Received from a member after it was lost on the air (§10.6): shown as late. Older
    /// versions read it as received.
    Late,
    /// Accepted, nothing transmitted yet: it can still be cancelled (§6.1).
    Queued,
    /// At least one frame went out.
    Sending {
        sent: u32,
        total: u8,
    },
    Delivered,
    Failed(FailReason),
    /// Flooded to a group (no acknowledgement).
    Flooded,
}

fn fail_code(r: FailReason) -> i64 {
    match r {
        FailReason::NoPath => 1,
        FailReason::Unreachable => 2,
        FailReason::Timeout => 3,
        FailReason::Refused => 4,
        FailReason::TooLarge => 5,
        FailReason::Interrupted => 6,
        FailReason::IdentityChanged => 7,
        FailReason::Expired => 8,
    }
}

fn fail_reason(c: i64) -> FailReason {
    match c {
        1 => FailReason::NoPath,
        2 => FailReason::Unreachable,
        4 => FailReason::Refused,
        5 => FailReason::TooLarge,
        6 => FailReason::Interrupted,
        7 => FailReason::IdentityChanged,
        8 => FailReason::Expired,
        _ => FailReason::Timeout,
    }
}

impl Status {
    fn columns(&self) -> (i64, i64, i64) {
        match *self {
            Status::Received => (0, 0, 0),
            Status::Late => (6, 0, 0),
            Status::Sending { sent, total } => (1, i64::from(sent), i64::from(total)),
            Status::Delivered => (2, 0, 0),
            Status::Failed(r) => (3, fail_code(r), 0),
            Status::Flooded => (4, 0, 0),
            Status::Queued => (5, 0, 0),
        }
    }

    fn from_columns(s: i64, a: i64, b: i64) -> Self {
        match s {
            1 => Status::Sending {
                sent: u32::try_from(a).unwrap_or(0),
                total: u8::try_from(b).unwrap_or(0),
            },
            2 => Status::Delivered,
            3 => Status::Failed(fail_reason(a)),
            4 => Status::Flooded,
            5 => Status::Queued,
            6 => Status::Late,
            _ => Status::Received,
        }
    }
}

/// A stored message.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    /// Row id (0 before insertion).
    pub id: i64,
    pub chat: Chat,
    pub outgoing: bool,
    /// Author of a group message from someone else.
    pub author: Option<PubKey>,
    pub content: Content,
    /// When it was sent or received, Unix milliseconds.
    pub at_ms: i64,
    pub status: Status,
    pub fragments: u8,
    pub airtime_ms: u64,
    pub snr4: Option<i8>,
    pub read: bool,
    /// Its reference (§6.2), from its author and its plaintext as sent.
    pub msg_ref: Option<MessageRef>,
}

/// How long a received reaction, edit or delete waits for a message that is not here: its
/// target was sent at most 24 hours before it (§6.4) and arrives within the outbox's 24 hours
/// (§6.1), so after two days it never will (review of 2026-11-15, SR-15).
pub const ORPHAN_KEEP_MS: i64 = 48 * 3_600_000;

/// The SQLite store.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS contacts (
    key BLOB PRIMARY KEY,
    name TEXT NOT NULL,
    path_len INTEGER,
    path BLOB,
    identity BLOB,
    verified INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS nicknames (
    key BLOB PRIMARY KEY,
    name TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS groups (
    gid BLOB PRIMARY KEY,
    snapshot BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS timers (
    chat_kind INTEGER NOT NULL,
    chat BLOB NOT NULL,
    seconds INTEGER NOT NULL,
    ts INTEGER NOT NULL,
    pending INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (chat_kind, chat)
);
CREATE TABLE IF NOT EXISTS invitations (
    gid BLOB PRIMARY KEY,
    snapshot BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_kind INTEGER NOT NULL,
    chat BLOB NOT NULL,
    outgoing INTEGER NOT NULL,
    author BLOB,
    content BLOB NOT NULL,
    at_ms INTEGER NOT NULL,
    status INTEGER NOT NULL,
    status_a INTEGER NOT NULL,
    status_b INTEGER NOT NULL,
    fragments INTEGER NOT NULL,
    airtime_ms INTEGER NOT NULL,
    snr4 INTEGER,
    read INTEGER NOT NULL,
    media_id BLOB,
    expires_ms INTEGER,
    ref BLOB,
    vanish_ms INTEGER
);
CREATE INDEX IF NOT EXISTS messages_by_chat ON messages (chat_kind, chat, id);
CREATE TABLE IF NOT EXISTS media (
    id BLOB PRIMARY KEY,
    chat_kind INTEGER NOT NULL,
    chat BLOB NOT NULL,
    datagram BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS shares (
    chat_kind INTEGER NOT NULL,
    chat BLOB NOT NULL,
    precision INTEGER NOT NULL,
    until INTEGER NOT NULL,
    PRIMARY KEY (chat_kind, chat)
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS reactions (
    chat_kind INTEGER NOT NULL,
    chat BLOB NOT NULL,
    target BLOB NOT NULL,
    reactor BLOB NOT NULL,
    emoji TEXT NOT NULL,
    ts INTEGER NOT NULL,
    pending INTEGER NOT NULL DEFAULT 0,
    expires_ms INTEGER,
    PRIMARY KEY (chat_kind, chat, target, reactor)
);
CREATE TABLE IF NOT EXISTS chat_prefs (
    chat_kind INTEGER NOT NULL,
    chat BLOB NOT NULL,
    pinned_at INTEGER,
    muted_until INTEGER,
    PRIMARY KEY (chat_kind, chat)
);
CREATE TABLE IF NOT EXISTS amendments (
    chat_kind INTEGER NOT NULL,
    chat BLOB NOT NULL,
    target BLOB NOT NULL,
    author BLOB NOT NULL,
    deleted INTEGER NOT NULL DEFAULT 0,
    text TEXT,
    ts INTEGER NOT NULL,
    pending INTEGER NOT NULL DEFAULT 0,
    expires_ms INTEGER,
    PRIMARY KEY (chat_kind, chat, target, author)
);
CREATE TABLE IF NOT EXISTS channels (
    id BLOB PRIMARY KEY,
    name TEXT NOT NULL,
    secret BLOB NOT NULL,
    slot INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS search_consent (
    gid BLOB PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS positions (
    sender BLOB NOT NULL,
    gid BLOB NOT NULL,
    body BLOB NOT NULL,
    at INTEGER NOT NULL,
    snr4 INTEGER NOT NULL,
    PRIMARY KEY (sender, gid)
);
";

/// The last position received from a sender in a conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredPosition {
    pub from: PubKey,
    pub group: Option<GroupId>,
    pub position: Position,
    /// Unix seconds.
    pub at: u64,
    pub snr4: i8,
}

/// An edit or a delete (§6.4) as it holds for a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Amendment {
    /// Taken back by its author.
    pub deleted: bool,
    /// The latest text, when edited and not deleted.
    pub text: Option<String>,
    pub ts: u32,
}

/// One of our edits or deletes not delivered yet (§6.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingAmendment {
    pub chat: Chat,
    pub target: MessageRef,
    pub deleted: bool,
    pub text: Option<String>,
    pub ts: u32,
    pub expires_ms: Option<i64>,
}

/// Who changes a message: an empty value for us, else the sender's node (§6.4).
fn amender(author: Option<&PubKey>) -> &[u8] {
    author.map_or(&[], |a| a.as_slice())
}

/// One of our reactions not delivered yet (§6.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingReaction {
    pub chat: Chat,
    pub target: MessageRef,
    pub emoji: String,
    pub ts: u32,
    /// Given up then (Unix ms).
    pub expires_ms: Option<i64>,
}

/// A conversation's local preferences (Unix ms): when it was pinned, until when it is muted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChatPref {
    pub chat: Chat,
    pub pinned_at: Option<i64>,
    pub muted_until: Option<i64>,
}

/// Text as search compares it: lower case, accents removed (canonical decomposition, combining
/// marks dropped), so that "ecole" finds "École".
pub fn fold(s: &str) -> String {
    s.nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .flat_map(char::to_lowercase)
        .collect()
}

const COLUMNS: &str = "id, chat_kind, chat, outgoing, author, content, at_ms, status, status_a, \
                       status_b, fragments, airtime_ms, snr4, read, ref";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Option<Message>> {
    let chat_bytes: Vec<u8> = r.get(2)?;
    let content: Vec<u8> = r.get(5)?;
    let author: Option<Vec<u8>> = r.get(4)?;
    let (Some(chat), Some(content)) = (
        Chat::from_row(r.get(1)?, &chat_bytes),
        Content::decode(&content),
    ) else {
        return Ok(None);
    };
    Ok(Some(Message {
        id: r.get(0)?,
        chat,
        outgoing: r.get(3)?,
        author: author.and_then(|a| a.as_slice().try_into().ok()),
        content,
        at_ms: r.get(6)?,
        status: Status::from_columns(r.get(7)?, r.get(8)?, r.get(9)?),
        fragments: r.get(10)?,
        airtime_ms: r.get::<_, i64>(11)?.try_into().unwrap_or(0),
        snr4: r.get(12)?,
        read: r.get(13)?,
        msg_ref: r
            .get::<_, Option<Vec<u8>>>(14)?
            .and_then(|v| v.as_slice().try_into().ok()),
    }))
}

impl Store {
    /// Opens (or creates) the database at `path`.
    pub fn open(path: &Path) -> Result<Self, Error> {
        Self::init(Connection::open(path)?)
    }

    /// A database in memory (demo, tests).
    pub fn in_memory() -> Result<Self, Error> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, Error> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Deleted rows are overwritten, not left in free pages: "delete for me", a delete for
        // everyone and disappearing messages leave nothing to read back (review of
        // 2026-11-15, SR-28).
        conn.pragma_update(None, "secure_delete", "ON")?;
        let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > VERSION {
            return Err(Error::InvalidQuery);
        }
        conn.execute_batch(SCHEMA)?;
        // Databases from before version 3.
        let has_media_id = conn
            .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'media_id'")?
            .exists([])?;
        if !has_media_id {
            conn.execute_batch("ALTER TABLE messages ADD COLUMN media_id BLOB")?;
        }
        // And from before version 4.
        let has_identity = conn
            .prepare("SELECT 1 FROM pragma_table_info('contacts') WHERE name = 'identity'")?
            .exists([])?;
        if !has_identity {
            conn.execute_batch(
                "ALTER TABLE contacts ADD COLUMN identity BLOB;
                 ALTER TABLE contacts ADD COLUMN verified INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        // And from before version 7.
        let has_expiry = conn
            .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'expires_ms'")?
            .exists([])?;
        if !has_expiry {
            conn.execute_batch("ALTER TABLE messages ADD COLUMN expires_ms INTEGER")?;
        }
        // And from before version 9.
        let has_ref = conn
            .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'ref'")?
            .exists([])?;
        if !has_ref {
            conn.execute_batch("ALTER TABLE messages ADD COLUMN ref BLOB")?;
        }
        // Version 12: disappearing messages (§6.5).
        let has_vanish = conn
            .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'vanish_ms'")?
            .exists([])?;
        if !has_vanish {
            conn.execute_batch("ALTER TABLE messages ADD COLUMN vanish_ms INTEGER")?;
        }
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS messages_by_vanish ON messages (vanish_ms)
             WHERE vanish_ms IS NOT NULL",
        )?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS messages_by_ref ON messages (chat_kind, chat, ref)",
        )?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS messages_by_media ON messages (chat_kind, chat, media_id)",
        )?;
        conn.pragma_update(None, "user_version", VERSION)?;
        Ok(Self { conn })
    }

    // ----- MeshCore channels (protocol.md §15.3) ----------------------------------------

    /// Replaces the saved channel list by the companion's.
    pub fn put_channels(&self, channels: &[Channel]) -> Result<(), Error> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM channels", [])?;
        for c in channels {
            tx.execute(
                "INSERT OR REPLACE INTO channels (id, name, secret, slot) VALUES (?1, ?2, ?3, ?4)",
                params![c.id().as_slice(), c.name, c.secret.as_slice(), c.index],
            )?;
        }
        tx.commit()
    }

    /// The channels saved last, in slot order.
    pub fn channels(&self) -> Result<Vec<Channel>, Error> {
        let mut st = self
            .conn
            .prepare("SELECT name, secret, slot FROM channels ORDER BY slot")?;
        let rows = st.query_map([], |r| {
            let secret: Vec<u8> = r.get(1)?;
            Ok((r.get::<_, String>(0)?, secret, r.get::<_, u8>(2)?))
        })?;
        Ok(rows
            .filter_map(Result::ok)
            .filter_map(|(name, secret, index)| {
                Some(Channel {
                    index,
                    name,
                    secret: secret.as_slice().try_into().ok()?,
                })
            })
            .collect())
    }

    /// Channel `id`'s saved name.
    pub fn channel_name(&self, id: &ChannelId) -> Result<Option<String>, Error> {
        self.conn
            .query_row(
                "SELECT name FROM channels WHERE id = ?1",
                params![id.as_slice()],
                |r| r.get(0),
            )
            .optional()
    }

    // ----- location (protocol.md §12) ---------------------------------------------------

    /// Replaces the saved shares by `shares`.
    pub fn put_shares(&self, shares: &[Share]) -> Result<(), Error> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM shares", [])?;
        for s in shares {
            tx.execute(
                "INSERT INTO shares (chat_kind, chat, precision, until) VALUES (?1, ?2, ?3, ?4)",
                params![
                    s.chat.kind(),
                    s.chat.bytes(),
                    s.precision.code(),
                    i64::try_from(s.until).unwrap_or(i64::MAX)
                ],
            )?;
        }
        tx.commit()
    }

    /// Saved shares: (conversation, precision, end in Unix seconds).
    pub fn shares(&self) -> Result<Vec<(Chat, Precision, u64)>, Error> {
        let mut st = self
            .conn
            .prepare("SELECT chat_kind, chat, precision, until FROM shares")?;
        let rows = st.query_map([], |r| {
            let bytes: Vec<u8> = r.get(1)?;
            let until: i64 = r.get(3)?;
            Ok(Chat::from_row(r.get(0)?, &bytes)
                .zip(Precision::from_code(r.get(2)?))
                .map(|(c, p)| (c, p, u64::try_from(until).unwrap_or(0))))
        })?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    /// Keeps the newest position of `p.from` in its conversation; the end flag removes it.
    pub fn put_position(&self, p: &StoredPosition) -> Result<(), Error> {
        let gid: &[u8] = p.group.as_ref().map_or(&[], |g| g.as_slice());
        if p.position.end() {
            self.conn.execute(
                "DELETE FROM positions WHERE sender = ?1 AND gid = ?2",
                params![p.from.as_slice(), gid],
            )?;
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO positions (sender, gid, body, at, snr4) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (sender, gid) DO UPDATE SET body = ?3, at = ?4, snr4 = ?5
             WHERE excluded.at > positions.at",
            params![
                p.from.as_slice(),
                gid,
                p.position.encode().as_slice(),
                i64::try_from(p.at).unwrap_or(i64::MAX),
                p.snr4
            ],
        )?;
        Ok(())
    }

    pub fn positions(&self) -> Result<Vec<StoredPosition>, Error> {
        let mut st = self
            .conn
            .prepare("SELECT sender, gid, body, at, snr4 FROM positions")?;
        let rows = st.query_map([], |r| {
            let from: Vec<u8> = r.get(0)?;
            let gid: Vec<u8> = r.get(1)?;
            let body: Vec<u8> = r.get(2)?;
            let at: i64 = r.get(3)?;
            Ok((|| {
                Some(StoredPosition {
                    from: from.as_slice().try_into().ok()?,
                    group: if gid.is_empty() {
                        None
                    } else {
                        Some(gid.as_slice().try_into().ok()?)
                    },
                    position: Position::decode(&body)?,
                    at: u64::try_from(at).ok()?,
                    snr4: r.get(4).ok()?,
                })
            })())
        })?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    /// Forgets the received reactions, edits and deletes whose target never came, once their
    /// [`ORPHAN_KEEP_MS`] are over (§6.2, §6.4). Ours wait for their delivery instead.
    pub fn purge_orphans(&self, now_ms: i64) -> Result<usize, Error> {
        let mut n = 0;
        for table in ["amendments", "reactions"] {
            n += self.conn.execute(
                &format!(
                    "DELETE FROM {table} WHERE pending = 0 AND expires_ms IS NOT NULL
                       AND expires_ms <= ?1
                       AND NOT EXISTS (SELECT 1 FROM messages m WHERE m.chat_kind = {table}.chat_kind
                                       AND m.chat = {table}.chat AND m.ref = {table}.target)"
                ),
                params![now_ms],
            )?;
        }
        Ok(n)
    }

    /// Forgets positions older than `before` (Unix seconds).
    pub fn prune_positions(&self, before: u64) -> Result<usize, Error> {
        self.conn.execute(
            "DELETE FROM positions WHERE at < ?1",
            params![i64::try_from(before).unwrap_or(i64::MAX)],
        )
    }

    // ----- contacts and groups ---------------------------------------------------------

    /// The name the user gives contact `key` on this phone, or none (the node's own name
    /// again). Never sent: the radio's contact list and the other phones keep theirs.
    pub fn set_nickname(&self, key: &PubKey, name: Option<&str>) -> Result<(), Error> {
        match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => self.conn.execute(
                "INSERT INTO nicknames (key, name) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET name = ?2",
                params![key.as_slice(), n],
            )?,
            None => self.conn.execute(
                "DELETE FROM nicknames WHERE key = ?1",
                params![key.as_slice()],
            )?,
        };
        Ok(())
    }

    /// A small value kept across restarts (`None` removes it).
    pub fn set_setting(&self, key: &str, value: Option<&[u8]>) -> Result<(), Error> {
        match value {
            Some(v) => self.conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = ?2",
                params![key, v],
            )?,
            None => self
                .conn
                .execute("DELETE FROM settings WHERE key = ?1", params![key])?,
        };
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        let mut st = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = st.query(params![key])?;
        Ok(match rows.next()? {
            Some(r) => Some(r.get(0)?),
            None => None,
        })
    }

    /// "Let this group locate me" (protocol.md §11).
    pub fn set_search_consent(&self, gid: &GroupId, on: bool) -> Result<(), Error> {
        if on {
            self.conn.execute(
                "INSERT OR IGNORE INTO search_consent (gid) VALUES (?1)",
                params![gid.as_slice()],
            )?;
        } else {
            self.conn.execute(
                "DELETE FROM search_consent WHERE gid = ?1",
                params![gid.as_slice()],
            )?;
        }
        Ok(())
    }

    pub fn search_consents(&self) -> Result<Vec<GroupId>, Error> {
        let mut st = self.conn.prepare("SELECT gid FROM search_consent")?;
        let rows = st.query_map([], |r| {
            let gid: Vec<u8> = r.get(0)?;
            Ok(GroupId::try_from(gid.as_slice()).ok())
        })?;
        Ok(rows.filter_map(Result::ok).flatten().collect())
    }

    pub fn nicknames(&self) -> Result<HashMap<PubKey, String>, Error> {
        let mut st = self.conn.prepare("SELECT key, name FROM nicknames")?;
        let rows = st.query_map([], |r| {
            let key: Vec<u8> = r.get(0)?;
            let name: String = r.get(1)?;
            Ok(PubKey::try_from(key.as_slice()).ok().map(|k| (k, name)))
        })?;
        Ok(rows.filter_map(Result::ok).flatten().collect())
    }

    pub fn put_contact(&self, c: &Contact) -> Result<(), Error> {
        let (len, path) = match &c.out_path {
            Some(p) => (Some(i64::from(p.len_byte)), Some(p.bytes.clone())),
            None => (None, None),
        };
        self.conn.execute(
            "INSERT INTO contacts (key, name, path_len, path, identity, verified)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (key) DO UPDATE SET name = ?2, path_len = ?3, path = ?4, identity = ?5,
             verified = ?6",
            params![
                c.key.as_slice(),
                c.name,
                len,
                path,
                c.identity.as_ref().map(|i| i.as_slice()),
                c.verified
            ],
        )?;
        Ok(())
    }

    pub fn contacts(&self) -> Result<Vec<Contact>, Error> {
        let mut st = self
            .conn
            .prepare("SELECT key, name, path_len, path, identity, verified FROM contacts")?;
        let rows = st.query_map([], |r| {
            let key: Vec<u8> = r.get(0)?;
            let len: Option<i64> = r.get(2)?;
            let path: Option<Vec<u8>> = r.get(3)?;
            let identity: Option<Vec<u8>> = r.get(4)?;
            let verified: bool = r.get(5)?;
            Ok(key.as_slice().try_into().ok().map(|key| Contact {
                key,
                name: r.get(1).unwrap_or_default(),
                out_path: match (len, path) {
                    (Some(l), Some(bytes)) => u8::try_from(l)
                        .ok()
                        .map(|len_byte| OwnedPath { len_byte, bytes }),
                    _ => None,
                },
                identity: identity.and_then(|i| i.as_slice().try_into().ok()),
                verified,
                // Stored contacts are people we talked with; the companion's list corrects it.
                kind: crate::contact_kind::CHAT,
            }))
        })?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    pub fn put_group(&self, gid: &GroupId, snapshot: &[u8]) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO groups (gid, snapshot) VALUES (?1, ?2)
             ON CONFLICT (gid) DO UPDATE SET snapshot = ?2",
            params![gid.as_slice(), snapshot],
        )?;
        Ok(())
    }

    pub fn delete_group(&self, gid: &GroupId) -> Result<(), Error> {
        self.conn
            .execute("DELETE FROM groups WHERE gid = ?1", params![gid.as_slice()])?;
        Ok(())
    }

    /// A group invitation waiting for the user (`Core::invitation_snapshot`).
    pub fn put_invitation(&self, gid: &GroupId, snapshot: &[u8]) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO invitations (gid, snapshot) VALUES (?1, ?2)
             ON CONFLICT (gid) DO UPDATE SET snapshot = ?2",
            params![gid.as_slice(), snapshot],
        )?;
        Ok(())
    }

    pub fn delete_invitation(&self, gid: &GroupId) -> Result<(), Error> {
        self.conn.execute(
            "DELETE FROM invitations WHERE gid = ?1",
            params![gid.as_slice()],
        )?;
        Ok(())
    }

    pub fn invitations(&self) -> Result<Vec<Vec<u8>>, Error> {
        let mut st = self.conn.prepare("SELECT snapshot FROM invitations")?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn groups(&self) -> Result<Vec<Vec<u8>>, Error> {
        let mut st = self.conn.prepare("SELECT snapshot FROM groups")?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    // ----- messages --------------------------------------------------------------------

    /// Inserts `m` and returns it with its row id.
    pub fn insert(&self, m: &Message) -> Result<Message, Error> {
        let (s, a, b) = m.status.columns();
        self.conn.execute(
            "INSERT INTO messages (chat_kind, chat, outgoing, author, content, at_ms, status,
             status_a, status_b, fragments, airtime_ms, snr4, read, media_id, ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                m.chat.kind(),
                m.chat.bytes(),
                m.outgoing,
                m.author.as_ref().map(|a| a.as_slice()),
                m.content.encode(),
                m.at_ms,
                s,
                a,
                b,
                m.fragments,
                i64::try_from(m.airtime_ms).unwrap_or(i64::MAX),
                m.snr4,
                m.read,
                media_id(&m.content).as_ref().map(|i| i.as_slice()),
                m.msg_ref.as_ref().map(|r| r.as_slice()),
            ],
        )?;
        let mut out = m.clone();
        out.id = self.conn.last_insert_rowid();
        // The conversation's timer when it was stored decides when it goes (§6.5). Notices
        // stay: they say who changed what, the timer included.
        if matches!(m.content, Content::Notice(_)) {
            return Ok(out);
        }
        self.conn.execute(
            "UPDATE messages SET vanish_ms = at_ms + 1000 * (
                 SELECT seconds FROM timers WHERE chat_kind = ?2 AND chat = ?3)
             WHERE id = ?1 AND EXISTS (
                 SELECT 1 FROM timers WHERE chat_kind = ?2 AND chat = ?3 AND seconds > 0)",
            params![out.id, m.chat.kind(), m.chat.bytes()],
        )?;
        Ok(out)
    }

    /// The message of `chat` that holds image `media_id`.
    pub fn find_image(&self, chat: &Chat, id: &MediaId) -> Result<Option<i64>, Error> {
        self.conn
            .query_row(
                "SELECT id FROM messages WHERE chat_kind = ?1 AND chat = ?2 AND media_id = ?3
                 ORDER BY id DESC LIMIT 1",
                params![chat.kind(), chat.bytes(), id.as_slice()],
                |r| r.get(0),
            )
            .optional()
    }

    /// Replaces the content of message `id` (an image's preview by its full variant).
    pub fn set_content(&self, id: i64, content: &Content) -> Result<Option<Message>, Error> {
        self.conn.execute(
            "UPDATE messages SET content = ?2, media_id = ?3 WHERE id = ?1",
            params![
                id,
                content.encode(),
                media_id(content).as_ref().map(|i| i.as_slice())
            ],
        )?;
        self.get(id)
    }

    /// Keeps the full variant of an image we sent (its whole datagram), for whoever we sent
    /// it to.
    pub fn put_media(&self, id: &MediaId, chat: &Chat, datagram: &[u8]) -> Result<(), Error> {
        self.conn.execute(
            "INSERT OR REPLACE INTO media (id, chat_kind, chat, datagram) VALUES (?1, ?2, ?3, ?4)",
            params![id.as_slice(), chat.kind(), chat.bytes(), datagram],
        )?;
        Ok(())
    }

    /// The full variant of image `id` and the conversation it was sent to.
    pub fn media(&self, id: &MediaId) -> Result<Option<(Chat, Vec<u8>)>, Error> {
        self.conn
            .query_row(
                "SELECT chat_kind, chat, datagram FROM media WHERE id = ?1",
                params![id.as_slice()],
                |r| {
                    let bytes: Vec<u8> = r.get(1)?;
                    Ok(Chat::from_row(r.get(0)?, &bytes).map(|c| (c, r.get(2))))
                },
            )
            .optional()
            .map(|o| o.flatten().and_then(|(c, d)| d.ok().map(|d| (c, d))))
    }

    /// Updates the status of message `id` and returns it.
    pub fn set_status(&self, id: i64, status: Status) -> Result<Option<Message>, Error> {
        let (s, a, b) = status.columns();
        self.conn.execute(
            "UPDATE messages SET status = ?2, status_a = ?3, status_b = ?4 WHERE id = ?1",
            params![id, s, a, b],
        )?;
        self.get(id)
    }

    pub fn get(&self, id: i64) -> Result<Option<Message>, Error> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM messages WHERE id = ?1"),
                params![id],
                row,
            )
            .optional()
            .map(Option::flatten)
    }

    /// The `limit` messages of `chat` before `before` (row id), oldest first.
    pub fn history(
        &self,
        chat: &Chat,
        before: Option<i64>,
        limit: u32,
    ) -> Result<Vec<Message>, Error> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM messages WHERE chat_kind = ?1 AND chat = ?2 AND id < ?3
             ORDER BY id DESC LIMIT ?4"
        ))?;
        let rows = st.query_map(
            params![chat.kind(), chat.bytes(), before.unwrap_or(i64::MAX), limit],
            row,
        )?;
        let mut v: Vec<Message> = rows.filter_map(|r| r.ok().flatten()).collect();
        v.reverse();
        Ok(v)
    }

    /// The messages of `chat` from `from` (included) up to `before` (excluded), oldest first,
    /// `limit` at most: what lies between a search result and the oldest message shown.
    pub fn history_from(
        &self,
        chat: &Chat,
        from: i64,
        before: i64,
        limit: u32,
    ) -> Result<Vec<Message>, Error> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM messages WHERE chat_kind = ?1 AND chat = ?2 AND id >= ?3
             AND id < ?4 ORDER BY id LIMIT ?5"
        ))?;
        let rows = st.query_map(params![chat.kind(), chat.bytes(), from, before, limit], row)?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    /// Every conversation with a message: the last message and the number of unread ones,
    /// most recent first.
    pub fn conversations(&self) -> Result<Vec<(Message, u32)>, Error> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {COLUMNS},
                (SELECT COUNT(*) FROM messages u WHERE u.chat_kind = m.chat_kind
                    AND u.chat = m.chat AND u.read = 0 AND u.outgoing = 0)
             FROM messages m
             WHERE id IN (SELECT MAX(id) FROM messages GROUP BY chat_kind, chat)
             ORDER BY id DESC"
        ))?;
        let rows = st.query_map([], |r| Ok(row(r)?.map(|m| (m, r.get(15).unwrap_or(0)))))?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    /// Up to `limit` text messages, newest first, whose text contains `needle` regardless of
    /// case and accents ([`fold`]): a search in every conversation, on this phone only.
    pub fn search(&self, needle: &str, limit: usize) -> Result<Vec<Message>, Error> {
        let needle = fold(needle.trim());
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let mut st = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM messages WHERE substr(content, 1, 1) IN (x'01', x'08')
             ORDER BY id DESC"
        ))?;
        let rows = st.query_map([], row)?;
        let mut out = Vec::new();
        for m in rows.filter_map(|r| r.ok().flatten()) {
            // The current text of an edited message (§6.4).
            let edited = self.amendment_for(&m).ok().flatten().and_then(|a| a.text);
            if let Content::Text { text, .. } | Content::Reply { text, .. } = &m.content
                && fold(edited.as_deref().unwrap_or(text)).contains(&needle)
            {
                out.push(m);
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Pins `chat` (at `at`, Unix ms: the latest pinned comes first) or unpins it (`None`).
    pub fn set_pinned(&self, chat: &Chat, at: Option<i64>) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO chat_prefs (chat_kind, chat, pinned_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (chat_kind, chat) DO UPDATE SET pinned_at = ?3",
            params![chat.kind(), chat.bytes(), at],
        )?;
        self.prune_prefs()
    }

    /// Mutes `chat` until `until` (Unix ms), or unmutes it (`None`).
    pub fn set_muted(&self, chat: &Chat, until: Option<i64>) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO chat_prefs (chat_kind, chat, muted_until) VALUES (?1, ?2, ?3)
             ON CONFLICT (chat_kind, chat) DO UPDATE SET muted_until = ?3",
            params![chat.kind(), chat.bytes(), until],
        )?;
        self.prune_prefs()
    }

    fn prune_prefs(&self) -> Result<(), Error> {
        self.conn.execute(
            "DELETE FROM chat_prefs WHERE pinned_at IS NULL AND muted_until IS NULL",
            [],
        )?;
        Ok(())
    }

    /// Pinned and muted conversations.
    pub fn chat_prefs(&self) -> Result<Vec<ChatPref>, Error> {
        let mut st = self
            .conn
            .prepare("SELECT chat_kind, chat, pinned_at, muted_until FROM chat_prefs")?;
        let rows = st.query_map([], |r| {
            let bytes: Vec<u8> = r.get(1)?;
            Ok(Chat::from_row(r.get(0)?, &bytes).map(|chat| ChatPref {
                chat,
                pinned_at: r.get(2).ok().flatten(),
                muted_until: r.get(3).ok().flatten(),
            }))
        })?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    pub fn mark_read(&self, chat: &Chat) -> Result<(), Error> {
        self.conn.execute(
            "UPDATE messages SET read = 1 WHERE chat_kind = ?1 AND chat = ?2 AND read = 0",
            params![chat.kind(), chat.bytes()],
        )?;
        Ok(())
    }

    // ----- disappearing messages (§6.5, ADR-0030) ------------------------------------------

    /// `chat`'s timer: seconds (0: messages stay) and the timestamp of the setting.
    pub fn timer(&self, chat: &Chat) -> Result<(u32, u32), Error> {
        Ok(self
            .conn
            .query_row(
                "SELECT seconds, ts FROM timers WHERE chat_kind = ?1 AND chat = ?2",
                params![chat.kind(), chat.bytes()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or((0, 0)))
    }

    /// Sets `chat`'s timer to `seconds` if `ts` is newer than the one held (equal: the greater
    /// `seconds`); `pending`: ours, still to send. True when it changed.
    pub fn set_timer(
        &self,
        chat: &Chat,
        seconds: u32,
        ts: u32,
        pending: bool,
    ) -> Result<bool, Error> {
        let n = self.conn.execute(
            "INSERT INTO timers (chat_kind, chat, seconds, ts, pending) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (chat_kind, chat) DO UPDATE
             SET seconds = excluded.seconds, ts = excluded.ts, pending = excluded.pending
             WHERE excluded.ts > timers.ts
                OR (excluded.ts = timers.ts AND excluded.seconds > timers.seconds)",
            params![chat.kind(), chat.bytes(), seconds, ts, pending],
        )?;
        Ok(n > 0)
    }

    /// Our timers still to send: (chat, seconds, ts).
    pub fn pending_timers(&self) -> Result<Vec<(Chat, u32, u32)>, Error> {
        let mut st = self
            .conn
            .prepare("SELECT chat_kind, chat, seconds, ts FROM timers WHERE pending = 1")?;
        let rows = st.query_map([], |r| {
            let bytes: Vec<u8> = r.get(1)?;
            Ok(Chat::from_row(r.get(0)?, &bytes)
                .map(|c| (c, r.get(2).unwrap_or(0), r.get(3).unwrap_or(0))))
        })?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    /// Our timer of `ts` reached the conversation.
    pub fn timer_sent(&self, chat: &Chat, ts: u32) -> Result<(), Error> {
        self.conn.execute(
            "UPDATE timers SET pending = 0 WHERE chat_kind = ?1 AND chat = ?2 AND ts = ?3",
            params![chat.kind(), chat.bytes(), ts],
        )?;
        Ok(())
    }

    /// The messages whose time is up at `now_ms` (Unix ms), removed with their reactions,
    /// edits and media, as `delete_for_me` does. Returned, for the app.
    pub fn vanish(&self, now_ms: i64) -> Result<Vec<Message>, Error> {
        let ids: Vec<i64> = {
            let mut st = self.conn.prepare(
                "SELECT id FROM messages WHERE vanish_ms IS NOT NULL AND vanish_ms <= ?1",
            )?;
            let rows = st.query_map(params![now_ms], |r| r.get(0))?;
            rows.filter_map(Result::ok).collect()
        };
        let mut gone = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(m) = self.delete_for_me(id)? {
                gone.push(m);
            }
        }
        Ok(gone)
    }

    /// When the next message disappears, Unix ms.
    pub fn next_vanish(&self) -> Result<Option<i64>, Error> {
        self.conn.query_row(
            "SELECT MIN(vanish_ms) FROM messages WHERE vanish_ms IS NOT NULL",
            [],
            |r| r.get(0),
        )
    }

    /// The content hashes announced in `chat` (§14.1): what to stop offering, or to remove
    /// from downloads, when the conversation is deleted.
    pub fn announced_hashes(&self, chat: &Chat) -> Result<Vec<[u8; 32]>, Error> {
        let mut st = self
            .conn
            .prepare("SELECT content FROM messages WHERE chat_kind = ?1 AND chat = ?2")?;
        let rows = st.query_map(params![chat.kind(), chat.bytes()], |r| {
            r.get::<_, Vec<u8>>(0)
        })?;
        Ok(rows
            .filter_map(Result::ok)
            .filter_map(|c| match Content::decode(&c) {
                Some(Content::Announce(a)) => Some(a.hash),
                _ => None,
            })
            .collect())
    }

    /// Deletes a conversation: its messages, the media we kept for it and its timer. Location
    /// shares and positions are separate (the map).
    pub fn delete_chat(&self, chat: &Chat) -> Result<usize, Error> {
        let n = self.conn.execute(
            "DELETE FROM messages WHERE chat_kind = ?1 AND chat = ?2",
            params![chat.kind(), chat.bytes()],
        )?;
        self.conn.execute(
            "DELETE FROM media WHERE chat_kind = ?1 AND chat = ?2",
            params![chat.kind(), chat.bytes()],
        )?;
        for table in ["reactions", "amendments", "timers"] {
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE chat_kind = ?1 AND chat = ?2"),
                params![chat.kind(), chat.bytes()],
            )?;
        }
        Ok(n)
    }

    // ----- edits and deletes (protocol.md §6.4) ----------------------------------------

    /// Records an edit (`text`) or a delete (`None`) of `target` in `chat` by `author` (`None`:
    /// us), unless it changes nothing (§6.4: a delete wins; among edits the greatest `ts`, then
    /// the greater text). `pending`: ours, not delivered yet. Returns whether it changed.
    #[allow(clippy::too_many_arguments)]
    pub fn amend(
        &self,
        chat: &Chat,
        target: &MessageRef,
        author: Option<&PubKey>,
        text: Option<&str>,
        ts: u32,
        pending: bool,
        expires_ms: Option<i64>,
    ) -> Result<bool, Error> {
        let n = self.conn.execute(
            "INSERT INTO amendments
                 (chat_kind, chat, target, author, deleted, text, ts, pending, expires_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (chat_kind, chat, target, author) DO UPDATE
             SET deleted = excluded.deleted, text = excluded.text, ts = excluded.ts,
                 pending = excluded.pending, expires_ms = excluded.expires_ms
             WHERE amendments.deleted = 0
               AND (excluded.deleted = 1 OR excluded.ts > amendments.ts
                    OR (excluded.ts = amendments.ts AND excluded.text > amendments.text))",
            params![
                chat.kind(),
                chat.bytes(),
                target.as_slice(),
                amender(author),
                text.is_none(),
                text,
                ts,
                pending,
                expires_ms
            ],
        )?;
        Ok(n > 0)
    }

    /// The edit or delete that holds for message `m` (§6.4): by its author, within 24 hours
    /// of its timestamp, an edit only for a TEXT or REPLY.
    pub fn amendment_for(&self, m: &Message) -> Result<Option<Amendment>, Error> {
        let (Some(target), Some(sent_at)) = (m.msg_ref, m.content.sent_at()) else {
            return Ok(None);
        };
        let author: Option<PubKey> = match (m.outgoing, &m.chat) {
            (true, _) => None,
            (false, Chat::Contact(k)) => Some(*k),
            (false, Chat::Group(_)) => match m.author {
                Some(a) => Some(a),
                None => return Ok(None),
            },
            (false, Chat::Mesh(_) | Chat::Channel(_)) => return Ok(None),
        };
        let found = self
            .conn
            .query_row(
                "SELECT deleted, text, ts FROM amendments
                 WHERE chat_kind = ?1 AND chat = ?2 AND target = ?3 AND author = ?4",
                params![
                    m.chat.kind(),
                    m.chat.bytes(),
                    target.as_slice(),
                    amender(author.as_ref())
                ],
                |r| {
                    Ok(Amendment {
                        deleted: r.get(0)?,
                        text: r.get(1)?,
                        ts: r.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(found.filter(|a| {
            let within = a.ts.saturating_sub(sent_at) <= crate::content::EDIT_WINDOW_S;
            let editable = matches!(m.content, Content::Text { .. } | Content::Reply { .. });
            within && (a.deleted || editable)
        }))
    }

    /// Our edits and deletes not delivered yet (§6.4).
    pub fn pending_amendments(&self) -> Result<Vec<PendingAmendment>, Error> {
        let mut st = self.conn.prepare(
            "SELECT chat_kind, chat, target, deleted, text, ts, expires_ms FROM amendments
             WHERE author = x'' AND pending = 1",
        )?;
        let rows = st.query_map([], |r| {
            let chat: Vec<u8> = r.get(1)?;
            let target: Vec<u8> = r.get(2)?;
            Ok(Chat::from_row(r.get(0)?, &chat)
                .zip(<[u8; 8]>::try_from(target.as_slice()).ok())
                .map(|(chat, target)| PendingAmendment {
                    chat,
                    target,
                    deleted: r.get(3).unwrap_or(false),
                    text: r.get(4).ok().flatten(),
                    ts: r.get(5).unwrap_or(0),
                    expires_ms: r.get(6).ok().flatten(),
                }))
        })?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    /// Our change of `target` with timestamp `ts` was delivered (or given up).
    pub fn amendment_done(&self, chat: &Chat, target: &MessageRef, ts: u32) -> Result<(), Error> {
        self.conn.execute(
            "UPDATE amendments SET pending = 0
             WHERE chat_kind = ?1 AND chat = ?2 AND target = ?3 AND author = x'' AND ts = ?4",
            params![chat.kind(), chat.bytes(), target.as_slice(), ts],
        )?;
        Ok(())
    }

    /// Message `id` was taken back by its author (§6.4): its place stays, its content, media,
    /// reactions and edits go. Returns the row as it is now.
    pub fn tombstone(&self, id: i64) -> Result<Option<Message>, Error> {
        let Some(m) = self.get(id)? else {
            return Ok(None);
        };
        if let Some(r) = m.msg_ref {
            self.conn.execute(
                "DELETE FROM reactions WHERE chat_kind = ?1 AND chat = ?2 AND target = ?3",
                params![m.chat.kind(), m.chat.bytes(), r.as_slice()],
            )?;
            self.conn.execute(
                "UPDATE amendments SET text = NULL
                 WHERE chat_kind = ?1 AND chat = ?2 AND target = ?3",
                params![m.chat.kind(), m.chat.bytes(), r.as_slice()],
            )?;
        }
        if let Some(media) = media_id(&m.content) {
            self.conn
                .execute("DELETE FROM media WHERE id = ?1", params![media.as_slice()])?;
        }
        let sent_at = m.content.sent_at().unwrap_or(0);
        self.set_content(id, &Content::Deleted { sent_at })
    }

    /// Deletes message `id` from this phone only (§6.4), with its media, reactions and
    /// changes.
    pub fn delete_for_me(&self, id: i64) -> Result<Option<Message>, Error> {
        let Some(m) = self.get(id)? else {
            return Ok(None);
        };
        if let Some(r) = m.msg_ref {
            for table in ["reactions", "amendments"] {
                self.conn.execute(
                    &format!(
                        "DELETE FROM {table} WHERE chat_kind = ?1 AND chat = ?2 AND target = ?3"
                    ),
                    params![m.chat.kind(), m.chat.bytes(), r.as_slice()],
                )?;
            }
        }
        if let Some(media) = media_id(&m.content) {
            self.conn
                .execute("DELETE FROM media WHERE id = ?1", params![media.as_slice()])?;
        }
        self.delete_message(id)?;
        Ok(Some(m))
    }

    // ----- replies and reactions (protocol.md §6.2) ------------------------------------

    /// The message of `chat` with reference `r`.
    pub fn message_by_ref(&self, chat: &Chat, r: &MessageRef) -> Result<Option<Message>, Error> {
        self.conn
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM messages WHERE chat_kind = ?1 AND chat = ?2 AND ref = ?3
                     ORDER BY id LIMIT 1"
                ),
                params![chat.kind(), chat.bytes(), r.as_slice()],
                row,
            )
            .optional()
            .map(Option::flatten)
    }

    /// Up to `limit` messages without a reference (from before version 9), to fill in.
    pub fn messages_without_ref(&self, limit: u32) -> Result<Vec<Message>, Error> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM messages WHERE ref IS NULL ORDER BY id LIMIT ?1"
        ))?;
        let rows = st.query_map(params![limit], row)?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    /// Sets message `id`'s reference; an empty value marks one that cannot have any (it is
    /// not filled in again).
    pub fn set_ref(&self, id: i64, r: Option<&MessageRef>) -> Result<(), Error> {
        self.conn.execute(
            "UPDATE messages SET ref = ?2 WHERE id = ?1",
            params![id, r.map_or(&[][..], |r| r.as_slice())],
        )?;
        Ok(())
    }

    /// Records `reactor`'s reaction to `target` in `chat` unless a later one holds (§6.2:
    /// greatest `ts`, then greatest bytes). `pending`: ours, not delivered yet, given up at
    /// `expires_ms`. Returns whether it changed anything.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_reaction(
        &self,
        chat: &Chat,
        target: &MessageRef,
        reactor: &PubKey,
        emoji: &str,
        ts: u32,
        pending: bool,
        expires_ms: Option<i64>,
    ) -> Result<bool, Error> {
        let n = self.conn.execute(
            "INSERT INTO reactions (chat_kind, chat, target, reactor, emoji, ts, pending, expires_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (chat_kind, chat, target, reactor) DO UPDATE
             SET emoji = excluded.emoji, ts = excluded.ts, pending = excluded.pending,
                 expires_ms = excluded.expires_ms
             WHERE excluded.ts > reactions.ts
                OR (excluded.ts = reactions.ts AND excluded.emoji > reactions.emoji)",
            params![
                chat.kind(),
                chat.bytes(),
                target.as_slice(),
                reactor.as_slice(),
                emoji,
                ts,
                pending,
                expires_ms
            ],
        )?;
        Ok(n > 0)
    }

    /// The reactions to `target` in `chat` (removed ones left out): reactor and emoji.
    pub fn reactions_for(
        &self,
        chat: &Chat,
        target: &MessageRef,
    ) -> Result<Vec<(PubKey, String)>, Error> {
        let mut st = self.conn.prepare(
            "SELECT reactor, emoji FROM reactions
             WHERE chat_kind = ?1 AND chat = ?2 AND target = ?3 AND emoji <> '' ORDER BY ts",
        )?;
        let rows = st.query_map(params![chat.kind(), chat.bytes(), target.as_slice()], |r| {
            let k: Vec<u8> = r.get(0)?;
            Ok(<[u8; 32]>::try_from(k.as_slice())
                .ok()
                .map(|k| (k, r.get::<_, String>(1).unwrap_or_default())))
        })?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    /// `reactor`'s reaction to `target` in `chat`: emoji and `ts`.
    pub fn reaction(
        &self,
        chat: &Chat,
        target: &MessageRef,
        reactor: &PubKey,
    ) -> Result<Option<(String, u32)>, Error> {
        self.conn
            .query_row(
                "SELECT emoji, ts FROM reactions
                 WHERE chat_kind = ?1 AND chat = ?2 AND target = ?3 AND reactor = ?4",
                params![
                    chat.kind(),
                    chat.bytes(),
                    target.as_slice(),
                    reactor.as_slice()
                ],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
    }

    /// `reactor`'s reactions not delivered yet (§6.2).
    pub fn pending_reactions(&self, reactor: &PubKey) -> Result<Vec<PendingReaction>, Error> {
        let mut st = self.conn.prepare(
            "SELECT chat_kind, chat, target, emoji, ts, expires_ms FROM reactions
             WHERE reactor = ?1 AND pending = 1",
        )?;
        let rows = st.query_map(params![reactor.as_slice()], |r| {
            let chat: Vec<u8> = r.get(1)?;
            let target: Vec<u8> = r.get(2)?;
            Ok(Chat::from_row(r.get(0)?, &chat)
                .zip(<[u8; 8]>::try_from(target.as_slice()).ok())
                .map(|(chat, target)| PendingReaction {
                    chat,
                    target,
                    emoji: r.get(3).unwrap_or_default(),
                    ts: r.get(4).unwrap_or(0),
                    expires_ms: r.get(5).ok().flatten(),
                }))
        })?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    /// `reactor`'s reaction to `target` with timestamp `ts` was delivered (or given up): no
    /// longer pending. A newer change of the same reaction stays pending (recheck R03).
    pub fn reaction_done(
        &self,
        chat: &Chat,
        target: &MessageRef,
        reactor: &PubKey,
        ts: u32,
    ) -> Result<(), Error> {
        self.conn.execute(
            "UPDATE reactions SET pending = 0
             WHERE chat_kind = ?1 AND chat = ?2 AND target = ?3 AND reactor = ?4 AND ts = ?5",
            params![
                chat.kind(),
                chat.bytes(),
                target.as_slice(),
                reactor.as_slice(),
                ts
            ],
        )?;
        Ok(())
    }

    /// Messages unfinished when the app stopped (§6.1): texts, replies, stickers and GIFs are
    /// returned with their expiry (Unix ms), to be sent again; the rest (media) fail as
    /// `Interrupted`.
    pub fn resume_unfinished(&self) -> Result<Vec<(Message, Option<i64>)>, Error> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS}, expires_ms FROM messages WHERE outgoing = 1 AND status IN (1, 5)
             ORDER BY id"
        ))?;
        let rows: Vec<(Option<Message>, i64, Option<i64>)> = stmt
            .query_map([], |r| Ok((row(r)?, r.get(0)?, r.get(15)?)))?
            .collect::<Result<_, _>>()?;
        let mut texts = Vec::new();
        for (m, id, expires) in rows {
            match m {
                Some(m)
                    if matches!(
                        m.content,
                        Content::Text { .. }
                            | Content::Reply { .. }
                            | Content::Sticker { .. }
                            | Content::Gif { .. }
                    ) =>
                {
                    texts.push((m, expires));
                }
                _ => {
                    self.set_status(id, Status::Failed(FailReason::Interrupted))?;
                }
            }
        }
        Ok(texts)
    }

    /// Deletes one message (a cancelled one, §6.1).
    pub fn delete_message(&self, id: i64) -> Result<(), Error> {
        self.conn
            .execute("DELETE FROM messages WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// When unfinished message `id` stops waiting (Unix ms; `None`: never), §6.1.
    pub fn set_expiry(&self, id: i64, expires_ms: Option<i64>) -> Result<(), Error> {
        self.conn.execute(
            "UPDATE messages SET expires_ms = ?2 WHERE id = ?1",
            params![id, expires_ms],
        )?;
        Ok(())
    }

    /// Whether `content` was already received from `author` (groups; `None` in a direct
    /// conversation) in `chat`: a resend, dropped by the receiver (§6.1).
    pub fn has_duplicate(
        &self,
        chat: &Chat,
        author: Option<&PubKey>,
        content: &Content,
    ) -> Result<bool, Error> {
        self.conn
            .prepare(
                "SELECT 1 FROM messages WHERE chat_kind = ?1 AND chat = ?2 AND outgoing = 0
                 AND author IS ?3 AND content = ?4",
            )?
            .exists(params![
                chat.kind(),
                chat.bytes(),
                author.map(|a| a.as_slice()),
                content.encode()
            ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(chat: Chat, outgoing: bool, t: &str, at_ms: i64) -> Message {
        Message {
            id: 0,
            chat,
            outgoing,
            author: None,
            content: Content::Text {
                sent_at: 1,
                text: t.into(),
            },
            at_ms,
            status: if outgoing {
                Status::Sending { sent: 0, total: 1 }
            } else {
                Status::Received
            },
            fragments: 1,
            airtime_ms: 400,
            snr4: Some(24),
            read: outgoing,
            msg_ref: None,
        }
    }

    /// Disappearing messages (§6.5): the timer in force when a message is stored decides
    /// when it goes; the newest setting holds.
    #[test]
    fn timers_and_vanishing() {
        let s = Store::in_memory().unwrap();
        let chat = Chat::Contact([4; 32]);
        let before = s.insert(&text(chat, false, "stays", 1_000)).unwrap();
        assert!(s.set_timer(&chat, 300, 10, true).unwrap());
        assert!(!s.set_timer(&chat, 60, 9, false).unwrap(), "older setting");
        assert_eq!(s.timer(&chat).unwrap(), (300, 10));
        assert_eq!(s.pending_timers().unwrap(), vec![(chat, 300, 10)]);
        s.timer_sent(&chat, 10).unwrap();
        assert!(s.pending_timers().unwrap().is_empty());
        let goes = s.insert(&text(chat, true, "goes", 2_000)).unwrap();
        assert_eq!(s.next_vanish().unwrap(), Some(2_000 + 300_000));
        assert!(s.vanish(2_000 + 299_999).unwrap().is_empty());
        let gone = s.vanish(2_000 + 300_000).unwrap();
        assert_eq!(gone.iter().map(|m| m.id).collect::<Vec<_>>(), vec![goes.id]);
        assert!(s.get(goes.id).unwrap().is_none());
        assert!(
            s.get(before.id).unwrap().is_some(),
            "stored before the timer"
        );
        assert_eq!(s.next_vanish().unwrap(), None);
        // Notices stay.
        let notice = Message {
            content: Content::Notice(crate::content::Notice {
                kind: crate::content::notice::TIMER,
                actor: [4; 32],
                actor_name: String::new(),
                subjects: Vec::new(),
                text: "300".into(),
            }),
            ..text(chat, false, "", 2_500)
        };
        s.insert(&notice).unwrap();
        assert_eq!(s.next_vanish().unwrap(), None);
        // Turned off: new messages stay.
        assert!(s.set_timer(&chat, 0, 11, false).unwrap());
        s.insert(&text(chat, false, "stays too", 3_000)).unwrap();
        assert_eq!(s.next_vanish().unwrap(), None);
    }

    /// Review of 2026-11-15, SR-15: a reaction or an edit whose target never comes is not kept
    /// forever; one whose target is here stays.
    #[test]
    fn orphans_are_purged() {
        let s = Store::in_memory().unwrap();
        let chat = Chat::Contact([4; 32]);
        let here = s
            .insert(&Message {
                msg_ref: Some([1; 8]),
                ..text(chat, false, "here", 1_000)
            })
            .unwrap();
        let bob = [4u8; 32];
        let keep = Some(1_000 + ORPHAN_KEEP_MS);
        for target in [[1u8; 8], [2; 8]] {
            s.upsert_reaction(&chat, &target, &bob, "👍", 5, false, keep)
                .unwrap();
            s.amend(&chat, &target, Some(&bob), Some("edited"), 5, false, keep)
                .unwrap();
        }
        // Ours, still on its way: never purged here.
        s.amend(&chat, &[3; 8], None, None, 6, true, Some(0))
            .unwrap();
        assert_eq!(s.purge_orphans(1_000 + ORPHAN_KEEP_MS - 1).unwrap(), 0);
        assert_eq!(s.purge_orphans(1_000 + ORPHAN_KEEP_MS).unwrap(), 2);
        assert!(s.amendment_for(&here).unwrap().is_some());
        assert_eq!(s.reactions_for(&chat, &[1; 8]).unwrap().len(), 1);
        assert_eq!(s.pending_amendments().unwrap().len(), 1);
    }

    /// Review of 2026-11-15, SR-28: a deleted message leaves nothing to read back in the file.
    #[test]
    fn deleted_text_is_overwritten() {
        let dir = std::env::temp_dir().join(format!("lichen-store-sd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("db.sqlite");
        let secret = "the key is under the third stone";
        {
            let s = Store::open(&path).unwrap();
            let m = s
                .insert(&text(Chat::Contact([3; 32]), false, secret, 1))
                .unwrap();
            s.delete_message(m.id).unwrap();
            // Everything into the main file, as at a checkpoint.
            s.conn
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .unwrap();
        }
        let mut bytes = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            bytes.extend(std::fs::read(entry.unwrap().path()).unwrap());
        }
        assert!(
            !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
            "the deleted text is still in the files"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn images_and_migration() {
        let dir = std::env::temp_dir().join(format!("lichen-store-v2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.sqlite");
        // A version 2 database: no media_id column.
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                &SCHEMA
                    .replace(",\n    media_id BLOB", "")
                    .replace(",\n    expires_ms INTEGER", "")
                    .replace(",\n    ref BLOB", ""),
            )
            .unwrap();
            c.pragma_update(None, "user_version", 2).unwrap();
            c.execute(
                "INSERT INTO messages (chat_kind, chat, outgoing, content, at_ms, status, status_a,
                 status_b, fragments, airtime_ms, read) VALUES (0, ?1, 0, ?2, 1, 0, 0, 0, 0, 0, 1)",
                params![
                    [1u8; 32].as_slice(),
                    Content::Text {
                        sent_at: 1,
                        text: "old".into()
                    }
                    .encode()
                ],
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        let a = Chat::Contact([1; 32]);
        assert_eq!(s.history(&a, None, 10).unwrap().len(), 1);
        // The outbox column was added too (version 7), and the reference (version 9).
        assert!(s.resume_unfinished().unwrap().is_empty());
        assert_eq!(s.messages_without_ref(10).unwrap().len(), 1);
        let image = |full: bool, len: usize| Content::Image {
            sent_at: 2,
            media_id: [7; 8],
            full,
            width: 160,
            height: 120,
            full_len: 900,
            data: vec![1; len],
        };
        let mut m = text(a, false, "", 3);
        m.content = image(false, 400);
        let row = s.insert(&m).unwrap();
        assert_eq!(s.find_image(&a, &[7; 8]).unwrap(), Some(row.id));
        assert_eq!(
            s.find_image(&Chat::Contact([2; 32]), &[7; 8]).unwrap(),
            None
        );
        let up = s.set_content(row.id, &image(true, 900)).unwrap().unwrap();
        assert_eq!(up.content, image(true, 900));
        s.put_media(&[7; 8], &a, b"datagram").unwrap();
        assert_eq!(s.media(&[7; 8]).unwrap(), Some((a, b"datagram".to_vec())));
        assert_eq!(s.media(&[8; 8]).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shares_and_positions() {
        let s = Store::in_memory().unwrap();
        let (a, g) = (Chat::Contact([1; 32]), Chat::Group([3; 16]));
        s.put_shares(&[
            Share::new(a, Precision::Exact, 100),
            Share::new(g, Precision::Km1, 200),
        ])
        .unwrap();
        let mut got = s.shares().unwrap();
        got.sort_by_key(|x| x.2);
        assert_eq!(
            got,
            vec![(a, Precision::Exact, 100), (g, Precision::Km1, 200)]
        );
        s.put_shares(&[Share::new(g, Precision::M100, 300)])
            .unwrap();
        assert_eq!(s.shares().unwrap(), vec![(g, Precision::M100, 300)]);

        let pos = |lat: i32, flags: u8| Position {
            lat,
            lon: 2,
            alt: 3,
            t16: 4,
            heading: 5,
            speed: 6,
            prec: 7,
            flags,
        };
        let p = |at: u64, group: Option<GroupId>, lat: i32, flags: u8| StoredPosition {
            from: [1; 32],
            group,
            position: pos(lat, flags),
            at,
            snr4: -3,
        };
        s.put_position(&p(10, None, 1, 0)).unwrap();
        s.put_position(&p(20, None, 2, 0)).unwrap();
        // Older than what we hold: ignored.
        s.put_position(&p(15, None, 3, 0)).unwrap();
        s.put_position(&p(12, Some([3; 16]), 4, 0)).unwrap();
        let mut all = s.positions().unwrap();
        all.sort_by_key(|x| x.at);
        assert_eq!(all, vec![p(12, Some([3; 16]), 4, 0), p(20, None, 2, 0)]);
        // The end flag removes the marker of that conversation only.
        s.put_position(&p(25, None, 2, lc_proto::location::FLAG_END))
            .unwrap();
        assert_eq!(s.positions().unwrap(), vec![p(12, Some([3; 16]), 4, 0)]);
        assert_eq!(s.prune_positions(13).unwrap(), 1);
        assert!(s.positions().unwrap().is_empty());
    }

    #[test]
    fn messages_history_conversations() {
        let s = Store::in_memory().unwrap();
        let (a, b, g) = (
            Chat::Contact([1; 32]),
            Chat::Contact([2; 32]),
            Chat::Group([3; 16]),
        );
        let m1 = s.insert(&text(a, false, "hi", 10)).unwrap();
        let m2 = s.insert(&text(a, true, "hello", 20)).unwrap();
        s.insert(&text(b, false, "yo", 30)).unwrap();
        let mut gm = text(g, false, "group", 40);
        gm.author = Some([2; 32]);
        s.insert(&gm).unwrap();
        // A resend is recognised by its plaintext, sender and conversation (§6.1).
        let c = |t: &str| Content::Text {
            sent_at: 1,
            text: t.into(),
        };
        assert!(s.has_duplicate(&b, None, &c("yo")).unwrap());
        assert!(!s.has_duplicate(&a, None, &c("yo")).unwrap());
        assert!(s.has_duplicate(&g, Some(&[2; 32]), &c("group")).unwrap());
        assert!(!s.has_duplicate(&g, Some(&[4; 32]), &c("group")).unwrap());
        assert!(!s.has_duplicate(&g, None, &c("group")).unwrap());
        // Our own messages are not received ones.
        assert!(!s.has_duplicate(&a, None, &c("hello")).unwrap());
        assert!(m2.id > m1.id);
        let h = s.history(&a, None, 10).unwrap();
        assert_eq!(
            h.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![m1.id, m2.id]
        );
        assert_eq!(s.history(&a, Some(m2.id), 10).unwrap().len(), 1);
        let conv = s.conversations().unwrap();
        assert_eq!(conv.len(), 3);
        assert_eq!(conv[0].0.chat, g);
        assert_eq!(conv[0].0.author, Some([2; 32]));
        assert_eq!(conv.iter().find(|(m, _)| m.chat == a).unwrap().1, 1);
        s.mark_read(&a).unwrap();
        assert_eq!(
            s.conversations()
                .unwrap()
                .iter()
                .find(|(m, _)| m.chat == a)
                .unwrap()
                .1,
            0
        );
        let up = s.set_status(m2.id, Status::Delivered).unwrap().unwrap();
        assert_eq!(up.status, Status::Delivered);
        // Unfinished at a restart (§6.1): texts and stickers come back with their expiry,
        // media fail.
        let m3 = s.insert(&text(b, true, "pending", 50)).unwrap();
        s.set_expiry(m3.id, Some(9_000)).unwrap();
        let mut queued = text(b, true, "queued", 51);
        queued.status = Status::Queued;
        let m4 = s.insert(&queued).unwrap();
        let mut voice = text(b, true, "", 52);
        voice.content = Content::Voice {
            sent_at: 1,
            mode: 2,
            frames: 1,
            data: vec![0; 8],
        };
        let m5 = s.insert(&voice).unwrap();
        let mut sticker = queued.clone();
        sticker.content = Content::Sticker {
            sent_at: 53,
            set: crate::content::STICKER_SET_NOTO,
            emoji: "🥳".into(),
        };
        let m6 = s.insert(&sticker).unwrap();
        let resumed = s.resume_unfinished().unwrap();
        assert_eq!(
            resumed
                .iter()
                .map(|(m, e)| (m.id, m.status, *e))
                .collect::<Vec<_>>(),
            vec![
                (m3.id, Status::Sending { sent: 0, total: 1 }, Some(9_000)),
                (m4.id, Status::Queued, None),
                (m6.id, Status::Queued, None)
            ]
        );
        s.delete_message(m6.id).unwrap();
        assert_eq!(
            s.get(m5.id).unwrap().unwrap().status,
            Status::Failed(FailReason::Interrupted)
        );
        s.delete_message(m3.id).unwrap();
        s.delete_message(m4.id).unwrap();
        s.delete_message(m5.id).unwrap();
        // Deleting a conversation leaves the others alone.
        assert_eq!(s.delete_chat(&a).unwrap(), 2);
        assert!(s.history(&a, None, 10).unwrap().is_empty());
        let left: Vec<Chat> = s
            .conversations()
            .unwrap()
            .iter()
            .map(|(m, _)| m.chat)
            .collect();
        assert_eq!(left.len(), 2);
        assert!(!left.contains(&a));
        assert_eq!(s.delete_chat(&a).unwrap(), 0);
    }

    #[test]
    fn pins_mutes_and_search() {
        let dir = std::env::temp_dir().join(format!("lichen-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.sqlite");
        let (a, g) = (Chat::Contact([1; 32]), Chat::Group([3; 16]));
        {
            let s = Store::open(&path).unwrap();
            s.set_pinned(&a, Some(100)).unwrap();
            s.set_muted(&g, Some(i64::MAX)).unwrap();
            s.set_muted(&a, Some(500)).unwrap();
            s.set_muted(&a, None).unwrap();
            for (i, t) in ["Rendez-vous à l'École du col", "on monte", "ECOLE fermée"]
                .iter()
                .enumerate()
            {
                s.insert(&text(a, i % 2 == 0, t, 10 + i as i64)).unwrap();
            }
            let mut voice = text(g, false, "", 20);
            voice.content = Content::Voice {
                sent_at: 1,
                mode: 2,
                frames: 1,
                data: b"ecole".to_vec(),
            };
            s.insert(&voice).unwrap();
        }
        // Kept across a restart; an entry with nothing left is gone.
        let s = Store::open(&path).unwrap();
        let mut prefs = s.chat_prefs().unwrap();
        prefs.sort_by_key(|p| p.chat.kind());
        let pref = |chat, pinned_at, muted_until| ChatPref {
            chat,
            pinned_at,
            muted_until,
        };
        assert_eq!(
            prefs,
            vec![pref(a, Some(100), None), pref(g, None, Some(i64::MAX))]
        );
        s.set_pinned(&a, None).unwrap();
        assert_eq!(s.chat_prefs().unwrap().len(), 1);
        // Case and accents do not matter; newest first; texts only; the limit holds.
        let texts = |v: Vec<Message>| -> Vec<String> {
            v.into_iter()
                .filter_map(|m| match m.content {
                    Content::Text { text, .. } => Some(text),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            texts(s.search("ecole", 10).unwrap()),
            vec!["ECOLE fermée", "Rendez-vous à l'École du col"]
        );
        assert_eq!(texts(s.search(" École ", 1).unwrap()), vec!["ECOLE fermée"]);
        assert_eq!(texts(s.search("FERMEE", 10).unwrap()), vec!["ECOLE fermée"]);
        assert!(s.search("", 10).unwrap().is_empty());
        assert!(s.search("refuge", 10).unwrap().is_empty());
        assert_eq!(fold("Ça VA, Noël ?"), "ca va, noel ?");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn references_and_reactions() {
        let s = Store::in_memory().unwrap();
        let (a, g) = (Chat::Contact([1; 32]), Chat::Group([3; 16]));
        let mut m = text(a, false, "hi", 1);
        m.msg_ref = Some([7; 8]);
        let row = s.insert(&m).unwrap();
        assert_eq!(
            s.message_by_ref(&a, &[7; 8]).unwrap().map(|m| m.id),
            Some(row.id)
        );
        assert_eq!(s.message_by_ref(&g, &[7; 8]).unwrap(), None);
        let old = s.insert(&text(a, true, "old", 2)).unwrap();
        assert_eq!(s.messages_without_ref(10).unwrap().len(), 1);
        s.set_ref(old.id, None).unwrap();
        assert!(s.messages_without_ref(10).unwrap().is_empty());
        assert_eq!(s.get(old.id).unwrap().unwrap().msg_ref, None);

        // The latest by ts holds, then the greater bytes; repeats change nothing (§6.2).
        let (t, bob, me) = ([7; 8], [2; 32], [9; 32]);
        assert!(
            s.upsert_reaction(&a, &t, &bob, "👍", 10, false, None)
                .unwrap()
        );
        assert!(
            !s.upsert_reaction(&a, &t, &bob, "👍", 10, false, None)
                .unwrap()
        );
        assert!(
            !s.upsert_reaction(&a, &t, &bob, "❤️", 9, false, None)
                .unwrap()
        );
        assert!(
            s.upsert_reaction(&a, &t, &bob, "😂", 11, false, None)
                .unwrap()
        );
        assert!(
            s.upsert_reaction(&a, &t, &me, "❤️", 5, true, Some(99))
                .unwrap()
        );
        assert_eq!(
            s.reactions_for(&a, &t).unwrap(),
            vec![(me, "❤️".to_string()), (bob, "😂".to_string())]
        );
        assert_eq!(
            s.reaction(&a, &t, &me).unwrap(),
            Some(("❤️".to_string(), 5))
        );
        assert_eq!(s.pending_reactions(&me).unwrap().len(), 1);
        // Changed before the first was delivered: the old delivery leaves the new one pending.
        assert!(
            s.upsert_reaction(&a, &t, &me, "👍", 6, true, Some(99))
                .unwrap()
        );
        s.reaction_done(&a, &t, &me, 5).unwrap();
        assert_eq!(s.pending_reactions(&me).unwrap().len(), 1);
        assert!(
            s.upsert_reaction(&a, &t, &me, "❤️", 7, true, Some(99))
                .unwrap()
        );
        s.reaction_done(&a, &t, &me, 7).unwrap();
        assert!(s.pending_reactions(&me).unwrap().is_empty());
        // Removed: empty, left out.
        assert!(
            s.upsert_reaction(&a, &t, &bob, "", 12, false, None)
                .unwrap()
        );
        assert_eq!(
            s.reactions_for(&a, &t).unwrap(),
            vec![(me, "❤️".to_string())]
        );
        // A reaction may come before its target: it is there when the target is.
        assert!(
            s.upsert_reaction(&a, &[8; 8], &bob, "🙏", 1, false, None)
                .unwrap()
        );
        let mut later = text(a, false, "later", 3);
        later.msg_ref = Some([8; 8]);
        s.insert(&later).unwrap();
        assert_eq!(s.reactions_for(&a, &[8; 8]).unwrap().len(), 1);
        // Deleting the conversation deletes its reactions.
        s.delete_chat(&a).unwrap();
        assert!(s.reactions_for(&a, &t).unwrap().is_empty());
    }

    #[test]
    fn contacts_and_groups_persist_on_disk() {
        let dir = std::env::temp_dir().join(format!("lichen-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("node.sqlite");
        let _ = std::fs::remove_file(&path);
        {
            let s = Store::open(&path).unwrap();
            s.put_contact(&Contact {
                key: [5; 32],
                name: "Camille".into(),
                out_path: Some(OwnedPath {
                    len_byte: 1,
                    bytes: vec![0xAB],
                }),
                identity: Some([9; 32]),
                verified: true,
                kind: 1,
            })
            .unwrap();
            s.put_contact(&Contact {
                key: [6; 32],
                name: "Julien".into(),
                out_path: None,
                identity: None,
                verified: false,
                kind: 1,
            })
            .unwrap();
            s.put_group(&[7; 16], b"snapshot").unwrap();
            s.put_group(&[8; 16], b"gone").unwrap();
            s.delete_group(&[8; 16]).unwrap();
        }
        let s = Store::open(&path).unwrap();
        let mut c = s.contacts().unwrap();
        c.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].out_path.as_ref().unwrap().bytes, vec![0xAB]);
        assert_eq!((c[0].identity, c[0].verified), (Some([9; 32]), true));
        assert_eq!(c[1].out_path, None);
        assert_eq!((c[1].identity, c[1].verified), (None, false));
        assert_eq!(s.groups().unwrap(), vec![b"snapshot".to_vec()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_and_search_consent() {
        let s = Store::in_memory().unwrap();
        assert_eq!(s.setting("tx_power_restore").unwrap(), None);
        s.set_setting("tx_power_restore", Some(&[22])).unwrap();
        s.set_setting("tx_power_restore", Some(&[20])).unwrap();
        assert_eq!(s.setting("tx_power_restore").unwrap(), Some(vec![20]));
        s.set_setting("tx_power_restore", None).unwrap();
        assert_eq!(s.setting("tx_power_restore").unwrap(), None);
        let (a, b) = ([1u8; 16], [2u8; 16]);
        s.set_search_consent(&a, true).unwrap();
        s.set_search_consent(&a, true).unwrap();
        s.set_search_consent(&b, true).unwrap();
        s.set_search_consent(&b, false).unwrap();
        assert_eq!(s.search_consents().unwrap(), vec![a]);
    }

    #[test]
    fn nicknames_set_and_cleared() {
        let s = Store::in_memory().unwrap();
        let k = [7u8; 32];
        s.set_nickname(&k, Some("  Tanguy ")).unwrap();
        assert_eq!(
            s.nicknames().unwrap().get(&k).map(String::as_str),
            Some("Tanguy")
        );
        s.set_nickname(&k, Some("Nina")).unwrap();
        assert_eq!(
            s.nicknames().unwrap().get(&k).map(String::as_str),
            Some("Nina")
        );
        s.set_nickname(&k, Some("  ")).unwrap();
        assert!(s.nicknames().unwrap().is_empty());
    }

    /// Edits and deletes (§6.4): the latest edit holds, a delete wins, only the author's
    /// count, within 24 hours; deleting for oneself removes the row.
    #[test]
    fn edits_and_deletes() {
        let s = Store::in_memory().unwrap();
        let bob = [7; 32];
        let chat = Chat::Contact(bob);
        let text = Content::Text {
            sent_at: 1_000,
            text: "first".into(),
        };
        let m = s
            .insert(&Message {
                id: 0,
                chat,
                outgoing: false,
                author: None,
                content: text,
                at_ms: 0,
                status: Status::Received,
                fragments: 1,
                airtime_ms: 0,
                snr4: None,
                read: true,
                msg_ref: Some([1; 8]),
            })
            .unwrap();
        let r = [1; 8];
        // Someone else's edit counts for nothing; Bob's does, the latest wins.
        assert!(
            s.amend(
                &chat,
                &r,
                Some(&[9; 32]),
                Some("forged"),
                1_010,
                false,
                None
            )
            .unwrap()
        );
        assert_eq!(s.amendment_for(&m).unwrap(), None);
        assert!(
            s.amend(&chat, &r, Some(&bob), Some("second"), 1_020, false, None)
                .unwrap()
        );
        assert!(
            !s.amend(&chat, &r, Some(&bob), Some("older"), 1_015, false, None)
                .unwrap()
        );
        let a = s.amendment_for(&m).unwrap().unwrap();
        assert_eq!((a.deleted, a.text.as_deref()), (false, Some("second")));
        assert_eq!(s.search("second", 10).unwrap().len(), 1);
        assert!(s.search("first", 10).unwrap().is_empty());
        // A delete wins over any later edit.
        assert!(
            s.amend(&chat, &r, Some(&bob), None, 1_030, false, None)
                .unwrap()
        );
        assert!(
            !s.amend(&chat, &r, Some(&bob), Some("third"), 1_040, false, None)
                .unwrap()
        );
        assert!(s.amendment_for(&m).unwrap().unwrap().deleted);
        let gone = s.tombstone(m.id).unwrap().unwrap();
        assert_eq!(gone.content, Content::Deleted { sent_at: 1_000 });
        assert_eq!(gone.msg_ref, Some(r));
        // Too late: more than 24 hours after the message.
        let late = s
            .insert(&Message {
                msg_ref: Some([2; 8]),
                ..m.clone()
            })
            .unwrap();
        s.amend(
            &chat,
            &[2; 8],
            Some(&bob),
            Some("much later"),
            1_000 + 86_401,
            false,
            None,
        )
        .unwrap();
        assert_eq!(s.amendment_for(&late).unwrap(), None);
        // Ours wait to be delivered.
        let mine = s
            .insert(&Message {
                outgoing: true,
                msg_ref: Some([3; 8]),
                ..m.clone()
            })
            .unwrap();
        s.amend(&chat, &[3; 8], None, Some("fixed"), 1_050, true, Some(5))
            .unwrap();
        assert_eq!(
            s.amendment_for(&mine).unwrap().unwrap().text.as_deref(),
            Some("fixed")
        );
        assert_eq!(s.pending_amendments().unwrap().len(), 1);
        s.amendment_done(&chat, &[3; 8], 1_050).unwrap();
        assert!(s.pending_amendments().unwrap().is_empty());
        // Deleted for me: gone with its changes.
        s.delete_for_me(mine.id).unwrap();
        assert!(s.get(mine.id).unwrap().is_none());
        assert_eq!(s.amendment_for(&mine).unwrap(), None);
    }
}

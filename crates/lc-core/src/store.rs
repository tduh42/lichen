//! Persistent storage (SQLite): message history, contacts, groups, location shares and the
//! last positions received. One database per MeshCore node. Sessions and our own group sender keys are never stored (ADR-0003, protocol.md §10.2).
//!
//! The core stays IO-free: the runner (`lc-ffi`) records what the core reports and restores
//! contacts and groups at start.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use unicode_normalization::UnicodeNormalization;

use crate::content::{Content, MediaId};

fn media_id(c: &Content) -> Option<MediaId> {
    match c {
        Content::Image { media_id, .. } => Some(*media_id),
        _ => None,
    }
}
use crate::core::{Contact, FailReason, OwnedPath, PubKey};
use crate::group::GroupId;
use crate::location::{Precision, Share};
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
const VERSION: i32 = 8;

/// A conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Chat {
    Contact(PubKey),
    Group(GroupId),
}

impl Chat {
    fn kind(&self) -> i64 {
        match self {
            Chat::Contact(_) => 0,
            Chat::Group(_) => 1,
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Chat::Contact(k) => k,
            Chat::Group(g) => g,
        }
    }

    fn from_row(kind: i64, bytes: &[u8]) -> Option<Self> {
        match kind {
            0 => bytes.try_into().ok().map(Chat::Contact),
            1 => bytes.try_into().ok().map(Chat::Group),
            _ => None,
        }
    }
}

/// Where a message stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Received,
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
}

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
    expires_ms INTEGER
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
CREATE TABLE IF NOT EXISTS chat_prefs (
    chat_kind INTEGER NOT NULL,
    chat BLOB NOT NULL,
    pinned_at INTEGER,
    muted_until INTEGER,
    PRIMARY KEY (chat_kind, chat)
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
                       status_b, fragments, airtime_ms, snr4, read";

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
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS messages_by_media ON messages (chat_kind, chat, media_id)",
        )?;
        conn.pragma_update(None, "user_version", VERSION)?;
        Ok(Self { conn })
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
             status_a, status_b, fragments, airtime_ms, snr4, read, media_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
            ],
        )?;
        let mut out = m.clone();
        out.id = self.conn.last_insert_rowid();
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
        let rows = st.query_map([], |r| Ok(row(r)?.map(|m| (m, r.get(14).unwrap_or(0)))))?;
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
            "SELECT {COLUMNS} FROM messages WHERE substr(content, 1, 1) = x'01' ORDER BY id DESC"
        ))?;
        let rows = st.query_map([], row)?;
        let mut out = Vec::new();
        for m in rows.filter_map(|r| r.ok().flatten()) {
            if let Content::Text { text, .. } = &m.content
                && fold(text).contains(&needle)
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

    /// Deletes a conversation: its messages and the media we kept for it. Location shares
    /// and positions are separate (the map).
    pub fn delete_chat(&self, chat: &Chat) -> Result<usize, Error> {
        let n = self.conn.execute(
            "DELETE FROM messages WHERE chat_kind = ?1 AND chat = ?2",
            params![chat.kind(), chat.bytes()],
        )?;
        self.conn.execute(
            "DELETE FROM media WHERE chat_kind = ?1 AND chat = ?2",
            params![chat.kind(), chat.bytes()],
        )?;
        Ok(n)
    }

    /// Messages unfinished when the app stopped (§6.1): the texts are returned with their
    /// expiry (Unix ms), to be sent again; the rest (media) fail as `Interrupted`.
    pub fn resume_unfinished(&self) -> Result<Vec<(Message, Option<i64>)>, Error> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS}, expires_ms FROM messages WHERE outgoing = 1 AND status IN (1, 5)
             ORDER BY id"
        ))?;
        let rows: Vec<(Option<Message>, i64, Option<i64>)> = stmt
            .query_map([], |r| Ok((row(r)?, r.get(0)?, r.get(14)?)))?
            .collect::<Result<_, _>>()?;
        let mut texts = Vec::new();
        for (m, id, expires) in rows {
            match m {
                Some(m) if matches!(m.content, Content::Text { .. }) => texts.push((m, expires)),
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
        }
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
                    .replace(",\n    expires_ms INTEGER", ""),
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
        // The outbox column was added too (version 7).
        assert!(s.resume_unfinished().unwrap().is_empty());
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
        // Unfinished at a restart (§6.1): texts come back with their expiry, media fail.
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
        let resumed = s.resume_unfinished().unwrap();
        assert_eq!(
            resumed
                .iter()
                .map(|(m, e)| (m.id, m.status, *e))
                .collect::<Vec<_>>(),
            vec![
                (m3.id, Status::Sending { sent: 0, total: 1 }, Some(9_000)),
                (m4.id, Status::Queued, None)
            ]
        );
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
}

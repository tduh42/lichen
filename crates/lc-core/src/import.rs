//! History from the official MeshCore app (ADR-0033): the database it exports ("export app
//! database", a SQLite file in the app's own format, as of its schema 29), read once into
//! lichen's MeshCore chats (§15). Only plain text: direct messages and channel posts. Room
//! posts and repeater command lines are left out.
//!
//! The file comes from outside, so it is read defensively: a copy without any `-wal` beside
//! it, opened read-only and immutable (no journal or WAL is ever read), in SQLite's defensive
//! mode with untrusted schema functions off, and only named columns of known tables are read.

use std::path::Path;

use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags};

use crate::content::Content;
use crate::core::{FailReason, PubKey};
use crate::plain::channel_id;
use crate::store::{Chat, Message, Status};

/// What the export holds that lichen keeps.
#[derive(Debug, Default)]
pub struct Imported {
    /// Direct messages and channel posts, oldest first.
    pub messages: Vec<Message>,
    /// The people messages were exchanged with: their node, the name their radio advertised,
    /// and the name the user gave them in the MeshCore app.
    pub people: Vec<(PubKey, String, Option<String>)>,
    /// Rows left out: other kinds of text, or malformed.
    pub skipped: usize,
}

/// Why nothing was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// Not a database, or not the MeshCore app's.
    NotMeshCore,
}

/// The longest text kept from one row (a MeshCore text is at most 160 bytes; a channel post
/// carries its sender's name too).
const TEXT_MAX: usize = 1_000;

/// Reads the MeshCore app's export at `path`, a file nobody else writes (the caller's copy).
pub fn meshcore_app(path: &Path) -> Result<Imported, ImportError> {
    let uri = format!("file:{}?immutable=1", path.display());
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let db = Connection::open_with_flags(uri, flags).map_err(|_| ImportError::NotMeshCore)?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .and_then(|_| db.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false))
        .and_then(|_| db.pragma_update(None, "cell_size_check", true))
        .map_err(|_| ImportError::NotMeshCore)?;
    let mut out = Imported::default();
    direct(&db, &mut out)?;
    channels(&db, &mut out)?;
    people(&db, &mut out)?;
    out.messages.sort_by_key(|m| m.at_ms);
    Ok(out)
}

/// A node key written as 64 hex digits.
fn key(hex_text: &str) -> Option<PubKey> {
    let (pairs, rest) = hex_text.as_bytes().as_chunks::<2>();
    if !rest.is_empty() || pairs.len() != 32 {
        return None;
    }
    let nibble = |c: u8| (c as char).to_digit(16).and_then(|d| u8::try_from(d).ok());
    let mut k = [0u8; 32];
    for (byte, &[hi, lo]) in k.iter_mut().zip(pairs) {
        *byte = nibble(hi)? << 4 | nibble(lo)?;
    }
    Some(k)
}

fn text(t: String) -> Option<String> {
    (!t.is_empty() && t.len() <= TEXT_MAX).then_some(t)
}

fn direct(db: &Connection, out: &mut Imported) -> Result<(), ImportError> {
    let mut st = db
        .prepare(
            "SELECT status, \"to\", \"from\", txt_type, sender_timestamp, text, timestamp, snr \
             FROM contact_messages ORDER BY id",
        )
        .map_err(|_| ImportError::NotMeshCore)?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, Option<f64>>(7)?,
            ))
        })
        .map_err(|_| ImportError::NotMeshCore)?;
    for row in rows {
        let Ok((status, to, from, kind, sent_at, body, at_ms, snr)) = row else {
            out.skipped += 1;
            continue;
        };
        let received = status == "received";
        let peer = key(if received { &from } else { &to });
        let (Some(peer), 0, Some(body), Ok(sent_at)) =
            (peer, kind, text(body), u32::try_from(sent_at))
        else {
            out.skipped += 1;
            continue;
        };
        out.messages.push(Message {
            id: 0,
            chat: Chat::Mesh(peer),
            outgoing: !received,
            author: received.then_some(peer),
            content: Content::Text {
                sent_at,
                text: body,
            },
            at_ms,
            status: match status.as_str() {
                "received" => Status::Received,
                "delivered" => Status::Delivered,
                "failed" => Status::Failed(FailReason::Timeout),
                _ => Status::Failed(FailReason::Interrupted),
            },
            fragments: 0,
            airtime_ms: 0,
            snr4: snr.map(|s| (s * 4.0).round().clamp(-128.0, 127.0) as i8),
            read: true,
            msg_ref: None,
        });
    }
    Ok(())
}

fn channels(db: &Connection, out: &mut Imported) -> Result<(), ImportError> {
    let mut st = db
        .prepare(
            "SELECT channel_secret, \"from\", txt_type, sender_timestamp, text, timestamp, snr \
             FROM channel_messages ORDER BY id",
        )
        .map_err(|_| ImportError::NotMeshCore)?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, Option<Vec<u8>>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<f64>>(6)?,
            ))
        })
        .map_err(|_| ImportError::NotMeshCore)?;
    for row in rows {
        let Ok((secret, from, kind, sent_at, body, at_ms, snr)) = row else {
            out.skipped += 1;
            continue;
        };
        let secret: Option<[u8; 16]> = secret.and_then(|s| s.try_into().ok());
        let (Some(secret), 0, Some(body), Ok(sent_at)) =
            (secret, kind, text(body), u32::try_from(sent_at))
        else {
            out.skipped += 1;
            continue;
        };
        // Ours carry our node in `from`; a received post has its sender's name in its text,
        // as lichen keeps it (§15.3).
        let outgoing = from.is_some();
        out.messages.push(Message {
            id: 0,
            chat: Chat::Channel(channel_id(&secret)),
            outgoing,
            author: None,
            content: Content::Text {
                sent_at,
                text: body,
            },
            at_ms,
            status: if outgoing {
                Status::Flooded
            } else {
                Status::Received
            },
            fragments: 0,
            airtime_ms: 0,
            snr4: snr.map(|s| (s * 4.0).round().clamp(-128.0, 127.0) as i8),
            read: true,
            msg_ref: None,
        });
    }
    Ok(())
}

/// Names of the people with a direct message, from the app's contacts.
fn people(db: &Connection, out: &mut Imported) -> Result<(), ImportError> {
    let peers: Vec<PubKey> = {
        let mut v: Vec<PubKey> = out
            .messages
            .iter()
            .filter_map(|m| match m.chat {
                Chat::Mesh(k) => Some(k),
                _ => None,
            })
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let mut st = db
        .prepare("SELECT adv_name, custom_name FROM contacts WHERE public_key = ?1")
        .map_err(|_| ImportError::NotMeshCore)?;
    for k in peers {
        let found = st
            .query_row([k.as_slice()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })
            .ok();
        if let Some((name, custom)) = found {
            let custom = custom.filter(|c| !c.trim().is_empty() && c.len() <= 64);
            out.people
                .push((k, name.chars().take(32).collect(), custom));
        }
    }
    Ok(())
}

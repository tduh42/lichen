//! The store encrypted at rest (ADR-0036): SQLCipher, under a random 32-byte key the app keeps
//! in the Keychain or behind the Android Keystore, given as a raw key (no passphrase to stretch).
//!
//! SQLCipher is built in with the `sqlcipher` feature only. Without it, a key is refused rather
//! than ignored (plain SQLite takes `PRAGMA key` and does nothing): no build stores in clear while
//! the app believes otherwise.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

/// Whether SQLCipher is built in.
pub const AVAILABLE: bool = cfg!(feature = "sqlcipher");

/// The first bytes of a plain SQLite database; an encrypted one starts with its random salt.
const PLAIN_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// Whether the file at `path` is a plain SQLite database. False when there is none yet.
pub fn is_plain(path: &Path) -> io::Result<bool> {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let mut head = [0u8; 16];
    let mut n = 0;
    while n < head.len() {
        match f.read(&mut head[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n == head.len() && &head == PLAIN_HEADER)
}

/// An IO failure as a store error.
pub(crate) fn io_error(e: io::Error) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
        Some(e.to_string()),
    )
}

fn key_literal(key: &[u8; 32]) -> String {
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    format!("x'{hex}'")
}

fn not_sqlcipher() -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
        Some("SQLCipher is not built in".into()),
    )
}

/// Fails, before any file is touched, when SQLCipher is not built in.
pub(crate) fn required() -> Result<(), rusqlite::Error> {
    if AVAILABLE {
        Ok(())
    } else {
        Err(not_sqlcipher())
    }
}

/// Keys `conn` and checks that SQLCipher is the SQLite linked and that the key opens the
/// database (another key fails here, not at the first query).
pub(crate) fn apply_key(conn: &Connection, key: &[u8; 32]) -> Result<(), rusqlite::Error> {
    required()?;
    conn.execute_batch(&format!("PRAGMA key = \"{}\";", key_literal(key)))?;
    let version: Option<String> = conn
        .query_row("PRAGMA cipher_version", [], |r| r.get(0))
        .optional()?;
    if version.is_none() {
        return Err(not_sqlcipher());
    }
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn path_str(path: &Path) -> Result<&str, rusqlite::Error> {
    path.to_str().ok_or_else(|| {
        io_error(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not UTF-8",
        ))
    })
}

/// Encrypts the plain database at `path` with `key`, in place (ADR-0036): an encrypted copy is
/// written beside it (`sqlcipher_export`, its schema version carried over), synced, then renamed
/// over the plain file, whose journal files go. Interrupted, it leaves the plain database as it
/// was, and the next start does it again.
pub fn encrypt_plain(path: &Path, key: &[u8; 32]) -> Result<(), rusqlite::Error> {
    required()?;
    let tmp = sibling(path, "-encrypting");
    for p in [tmp.clone(), sibling(&tmp, "-journal")] {
        let _ = fs::remove_file(p);
    }
    {
        let plain = Connection::open(path)?;
        // What the journal still holds goes into the file first.
        plain.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        let version: i64 = plain.pragma_query_value(None, "user_version", |r| r.get(0))?;
        plain.execute(
            "ATTACH DATABASE ?1 AS encrypted KEY ?2",
            params![path_str(&tmp)?, key_literal(key)],
        )?;
        plain.query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))?;
        plain.execute_batch(&format!(
            "PRAGMA encrypted.user_version = {version}; DETACH DATABASE encrypted;"
        ))?;
    }
    File::open(&tmp)
        .and_then(|f| f.sync_all())
        .map_err(io_error)?;
    for p in ["-wal", "-shm", "-journal"] {
        let _ = fs::remove_file(sibling(path, p));
    }
    fs::rename(&tmp, path).map_err(io_error)
}

/// Copies the encrypted database at `path` into a plain database at `copy`: what a backup
/// carries, which it encrypts on its own (ADR-0037). A consistent snapshot, on a connection of
/// its own.
pub fn export_plain(path: &Path, key: &[u8; 32], copy: &Path) -> Result<(), rusqlite::Error> {
    required()?;
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    apply_key(&conn, key)?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    conn.execute(
        "ATTACH DATABASE ?1 AS plain KEY ''",
        params![path_str(copy)?],
    )?;
    conn.query_row("SELECT sqlcipher_export('plain')", [], |_| Ok(()))?;
    conn.execute_batch(&format!(
        "PRAGMA plain.user_version = {version}; DETACH DATABASE plain;"
    ))
}

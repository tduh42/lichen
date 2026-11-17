//! The encrypted backup file (`docs/spec/backup.md`, ADR-0037): the store, the media fetched
//! over the bridge and the app's settings, sealed in 64 KiB chunks under a key derived from the
//! identity seed, so that the 24 words open it.
//!
//! Streamed both ways: a backup of any size never sits in memory, and a restore installs
//! nothing before the whole file has verified.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use lc_proto::{Key, TAG_LEN, aead, kdf};
use rusqlite::Connection;
use zeroize::Zeroize;

use crate::store::Store;

/// The file's first bytes.
pub const MAGIC: &[u8; 13] = b"lichen/backup";
/// The format this code writes and reads.
pub const VERSION: u8 = 1;
/// Magic, version, salt, check.
pub const HEADER_LEN: usize = MAGIC.len() + 1 + 32 + 16;
/// Plaintext bytes in every chunk but the last.
pub const CHUNK: usize = 65_536;
/// A sealed chunk, but the last.
const SEALED: usize = CHUNK + TAG_LEN;
/// The largest SETTINGS entry.
pub const SETTINGS_MAX: u64 = 65_536;

const KEY_INFO: &[u8] = b"lichen/1 backup key";
const CHECK_INFO: &[u8] = b"lichen/1 backup check";

/// Entry kinds (backup.md §3).
pub mod kind {
    pub const END: u8 = 0x00;
    pub const DATABASE: u8 = 0x01;
    pub const FILE: u8 = 0x02;
    pub const SETTINGS: u8 = 0x03;
}

/// Why a backup could not be made or read.
#[derive(Debug)]
pub enum BackupError {
    /// Not a lichen backup.
    NotABackup,
    /// A backup made by a newer lichen (another format version, or a newer database).
    Newer,
    /// The words are not the ones of this backup.
    WrongWords,
    /// Cut short, changed or reordered: a chunk fails its tag, or the contents do not parse.
    Damaged,
    Io(io::Error),
    Database(rusqlite::Error),
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for BackupError {}

/// A chunk that fails its tag, inside an `io::Error` from [`Opener`].
#[derive(Debug)]
struct DamagedChunk;

impl std::fmt::Display for DamagedChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("damaged backup chunk")
    }
}

impl std::error::Error for DamagedChunk {}

fn damaged() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, DamagedChunk)
}

impl From<io::Error> for BackupError {
    fn from(e: io::Error) -> Self {
        if e.get_ref().is_some_and(|r| r.is::<DamagedChunk>())
            || e.kind() == io::ErrorKind::UnexpectedEof
        {
            Self::Damaged
        } else {
            Self::Io(e)
        }
    }
}

impl From<rusqlite::Error> for BackupError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Database(e)
    }
}

/// The file key and the check of `seed` under `salt` (backup.md §1).
fn derive(seed: &[u8; 32], salt: &[u8; 32]) -> (Key, [u8; 16]) {
    let key = kdf::hkdf(salt, seed, &[KEY_INFO]);
    let mut c = kdf::hkdf(salt, seed, &[CHECK_INFO]);
    let mut check = [0u8; 16];
    check.copy_from_slice(&c[..16]);
    c.zeroize();
    (key, check)
}

fn nonce(index: u64, last: bool) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&index.to_le_bytes());
    n[8] = u8::from(last);
    n
}

/// Writes a backup: the header at once, then the contents written into it, sealed chunk by
/// chunk. [`Sealer::finish`] seals the last chunk; a sealer dropped without it leaves a file
/// that every reader refuses.
pub struct Sealer<W: Write> {
    out: W,
    key: Key,
    header: [u8; HEADER_LEN],
    buf: Vec<u8>,
    index: u64,
}

impl<W: Write> std::fmt::Debug for Sealer<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sealer")
    }
}

impl<W: Write> Drop for Sealer<W> {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl<W: Write> Sealer<W> {
    /// A backup for `seed` under `salt`: 32 random bytes, new for every backup.
    pub fn new(mut out: W, seed: &[u8; 32], salt: [u8; 32]) -> io::Result<Self> {
        let (key, check) = derive(seed, &salt);
        let mut header = [0u8; HEADER_LEN];
        header[..MAGIC.len()].copy_from_slice(MAGIC);
        header[MAGIC.len()] = VERSION;
        header[MAGIC.len() + 1..MAGIC.len() + 33].copy_from_slice(&salt);
        header[MAGIC.len() + 33..].copy_from_slice(&check);
        out.write_all(&header)?;
        Ok(Self {
            out,
            key,
            header,
            buf: Vec::with_capacity(CHUNK),
            index: 0,
        })
    }

    fn seal(&mut self, last: bool) -> io::Result<()> {
        let tag = aead::seal(
            &self.key,
            &nonce(self.index, last),
            &self.header,
            &mut self.buf,
        );
        self.out.write_all(&self.buf)?;
        self.out.write_all(&tag)?;
        self.buf.clear();
        self.index += 1;
        Ok(())
    }

    /// Writes one entry's head: its kind, name and size. Its `size` bytes follow.
    pub fn entry(&mut self, kind: u8, name: &str, size: u64) -> io::Result<()> {
        let len = u8::try_from(name.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name too long"))?;
        self.write_all(&[kind, len])?;
        self.write_all(name.as_bytes())?;
        self.write_all(&size.to_le_bytes())
    }

    /// Ends the contents (END), seals the last chunk and flushes.
    pub fn finish(mut self) -> io::Result<()> {
        self.entry(kind::END, "", 0)?;
        self.seal(true)?;
        self.out.flush()
    }
}

impl<W: Write> Write for Sealer<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        // A full chunk is sealed only once more comes: the last chunk may be full.
        if self.buf.len() == CHUNK {
            self.seal(false)?;
        }
        let n = (CHUNK - self.buf.len()).min(data.len());
        self.buf.extend_from_slice(&data[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// Reads a backup's contents, chunk by chunk, each one only once its tag has verified. Reading
/// returns 0 only after the last chunk verified; a file cut short, reordered or extended fails
/// with an error ([`BackupError::Damaged`] once converted).
pub struct Opener<R: Read> {
    src: R,
    key: Key,
    header: [u8; HEADER_LEN],
    index: u64,
    plain: Vec<u8>,
    pos: usize,
    /// The first byte of the next chunk, read to learn that the current one is not the last.
    peeked: Option<u8>,
    done: bool,
}

impl<R: Read> std::fmt::Debug for Opener<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Opener")
    }
}

impl<R: Read> Drop for Opener<R> {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl<R: Read> Opener<R> {
    /// Reads the header and checks the magic, the version and the words' `seed`.
    pub fn new(mut src: R, seed: &[u8; 32]) -> Result<Self, BackupError> {
        let mut header = [0u8; HEADER_LEN];
        src.read_exact(&mut header).map_err(|e| {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                BackupError::NotABackup
            } else {
                BackupError::Io(e)
            }
        })?;
        let (magic, rest) = header.split_at(MAGIC.len());
        if magic != MAGIC {
            return Err(BackupError::NotABackup);
        }
        let (&version, rest) = rest.split_first().ok_or(BackupError::NotABackup)?;
        match version {
            VERSION => {}
            0 => return Err(BackupError::NotABackup),
            _ => return Err(BackupError::Newer),
        }
        let (salt, check) = rest.split_at(32);
        let salt: [u8; 32] = salt.try_into().map_err(|_| BackupError::NotABackup)?;
        let (key, expected) = derive(seed, &salt);
        // Not a secret, but compared in constant time all the same.
        if expected
            .iter()
            .zip(check)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            != 0
        {
            return Err(BackupError::WrongWords);
        }
        Ok(Self {
            src,
            key,
            header,
            index: 0,
            plain: Vec::new(),
            pos: 0,
            peeked: None,
            done: false,
        })
    }

    fn next_chunk(&mut self) -> io::Result<()> {
        let mut sealed = Vec::with_capacity(SEALED);
        sealed.extend(self.peeked.take());
        let want = (SEALED - sealed.len()) as u64;
        (&mut self.src).take(want).read_to_end(&mut sealed)?;
        let last = sealed.len() < SEALED || {
            let mut one = [0u8; 1];
            let n = loop {
                match self.src.read(&mut one) {
                    Ok(n) => break n,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e),
                }
            };
            if n == 1 {
                self.peeked = Some(one[0]);
            }
            n == 0
        };
        let Some(split) = sealed.len().checked_sub(TAG_LEN) else {
            return Err(damaged());
        };
        let (body, tag) = sealed.split_at_mut(split);
        let tag: [u8; TAG_LEN] = (&*tag).try_into().map_err(|_| damaged())?;
        aead::open(
            &self.key,
            &nonce(self.index, last),
            &self.header,
            body,
            &tag,
        )
        .map_err(|_| damaged())?;
        sealed.truncate(split);
        self.plain = sealed;
        self.pos = 0;
        self.index += 1;
        self.done = last;
        Ok(())
    }
}

impl<R: Read> Read for Opener<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.pos == self.plain.len() {
            if self.done {
                return Ok(0);
            }
            self.next_chunk()?;
        }
        let n = out.len().min(self.plain.len() - self.pos);
        out[..n].copy_from_slice(&self.plain[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// One entry's head: kind, name, size.
pub fn read_entry<R: Read>(r: &mut R) -> Result<(u8, String, u64), BackupError> {
    let mut head = [0u8; 2];
    r.read_exact(&mut head)?;
    let [kind, len] = head;
    let mut name = vec![0u8; usize::from(len)];
    r.read_exact(&mut name)?;
    let name = String::from_utf8(name).map_err(|_| BackupError::Damaged)?;
    let mut size = [0u8; 8];
    r.read_exact(&mut size)?;
    Ok((kind, name, u64::from_le_bytes(size)))
}

/// A FILE entry's name (backup.md §3): never a path.
pub fn valid_file_name(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// What a backup is made of.
#[derive(Debug)]
pub struct Sources<'a> {
    /// The live store.
    pub database: &'a Path,
    /// Its key, when it is encrypted (ADR-0036).
    pub key: Option<&'a [u8; 32]>,
    /// The app's `downloads` folder, if it has one.
    pub downloads: Option<&'a Path>,
    /// The app's settings (backup.md §4), empty for none.
    pub settings: &'a [u8],
    /// Store settings that belong to this phone or its radio, left out.
    pub drop_settings: &'a [&'a str],
    /// A folder of the app's own (as protected as the store) for the copy of the store.
    pub work: &'a Path,
}

/// What a backup holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    pub bytes: u64,
    pub messages: u64,
    pub files: u64,
}

/// `path` and its journal files (`-wal`, `-shm`, `-journal`).
fn with_journals(path: &Path) -> [PathBuf; 4] {
    let side = |suffix: &str| {
        let mut s = path.as_os_str().to_owned();
        s.push(suffix);
        PathBuf::from(s)
    };
    [
        path.to_owned(),
        side("-wal"),
        side("-shm"),
        side("-journal"),
    ]
}

fn remove_quietly(path: &Path) {
    let _ = fs::remove_file(path);
}

/// Copies the store (encrypted with `key`, if any) into a plain database at `copy` and makes it
/// what a backup keeps; returns its messages. The copy is taken on a connection of its own, a
/// consistent snapshot (`VACUUM INTO`, or `sqlcipher_export` from an encrypted store): the app's
/// connection goes on meanwhile.
pub fn snapshot(
    database: &Path,
    key: Option<&[u8; 32]>,
    copy: &Path,
    drop_settings: &[&str],
) -> Result<u64, BackupError> {
    for p in with_journals(copy) {
        remove_quietly(&p);
    }
    let into = copy
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not UTF-8"))?;
    match key {
        Some(key) => crate::cipher::export_plain(database, key, copy)?,
        None => {
            let conn = Connection::open(database)?;
            conn.busy_timeout(std::time::Duration::from_secs(10))?;
            conn.execute("VACUUM INTO ?1", [into])?;
        }
    }
    let store = Store::open(copy)?;
    Ok(store.prepare_backup(drop_settings)?)
}

/// Writes a backup of `src` to `out`, for `seed` under `salt` (32 random bytes). `out` is
/// removed when anything fails; the copy of the store always is.
pub fn export(
    src: &Sources<'_>,
    out: &Path,
    seed: &[u8; 32],
    salt: [u8; 32],
) -> Result<Summary, BackupError> {
    if src.settings.len() as u64 > SETTINGS_MAX {
        return Err(BackupError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "settings too large",
        )));
    }
    fs::create_dir_all(src.work)?;
    let copy = src.work.join("backup.sqlite");
    let made = snapshot(src.database, src.key, &copy, src.drop_settings)
        .and_then(|messages| write(src, &copy, out, seed, salt, messages));
    for p in with_journals(&copy) {
        remove_quietly(&p);
    }
    if made.is_err() {
        remove_quietly(out);
    }
    made
}

fn write(
    src: &Sources<'_>,
    copy: &Path,
    out: &Path,
    seed: &[u8; 32],
    salt: [u8; 32],
    messages: u64,
) -> Result<Summary, BackupError> {
    let file = File::create(out)?;
    let mut w = Sealer::new(BufWriter::new(&file), seed, salt)?;
    put_file(&mut w, kind::DATABASE, "", copy)?;
    let mut files = 0;
    if let Some(dir) = src.downloads {
        let mut names: Vec<String> = match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| valid_file_name(n))
                // Still downloading (`<file>.part`, lc-bridge): not a medium yet.
                .filter(|n| !n.ends_with(".part"))
                .collect(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        names.sort();
        for name in names {
            // A file removed meanwhile is simply not in the backup.
            match put_file(&mut w, kind::FILE, &name, &dir.join(&name)) {
                Ok(()) => files += 1,
                Err(BackupError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
    }
    if !src.settings.is_empty() {
        w.entry(kind::SETTINGS, "", src.settings.len() as u64)?;
        w.write_all(src.settings)?;
    }
    w.finish()?;
    file.sync_all()?;
    Ok(Summary {
        bytes: fs::metadata(out)?.len(),
        messages,
        files,
    })
}

/// One entry with the bytes of file `path`, opened before anything is written.
fn put_file<W: Write>(
    w: &mut Sealer<W>,
    kind: u8,
    name: &str,
    path: &Path,
) -> Result<(), BackupError> {
    let f = File::open(path)?;
    let size = f.metadata()?.len();
    w.entry(kind, name, size)?;
    let copied = io::copy(&mut BufReader::new(f).take(size), w)?;
    if copied != size {
        // Shrunk while being read: the entry would lie about its size.
        return Err(BackupError::Io(io::Error::other("file changed while read")));
    }
    Ok(())
}

/// A backup read and verified into a staging folder, not installed yet.
#[derive(Debug)]
pub struct Unpacked {
    staging: PathBuf,
    /// The FILE entries' names.
    pub files: Vec<String>,
    /// The SETTINGS entry (backup.md §4), empty when there was none.
    pub settings: Vec<u8>,
    /// Messages in the database.
    pub messages: u64,
}

impl Unpacked {
    fn database(&self) -> PathBuf {
        self.staging.join("database.sqlite")
    }

    fn downloads(&self) -> PathBuf {
        self.staging.join("downloads")
    }

    /// Installs it (backup.md §5): the database replaces `database` (its journal files are
    /// removed first), the files join `downloads`. The staging folder is removed.
    pub fn install(self, database: &Path, downloads: &Path) -> Result<(), BackupError> {
        for p in with_journals(database).iter().skip(1) {
            remove_quietly(p);
        }
        fs::rename(self.database(), database)?;
        fs::create_dir_all(downloads)?;
        for name in &self.files {
            fs::rename(self.downloads().join(name), downloads.join(name))?;
        }
        let _ = fs::remove_dir_all(&self.staging);
        Ok(())
    }

    /// Leaves it: the staging folder is removed.
    pub fn discard(self) {
        let _ = fs::remove_dir_all(&self.staging);
    }
}

/// Reads backup `path` with the `seed` of the typed words into `staging` (a folder of the
/// app's own, on the same volume as the store, emptied first), and checks that its database
/// is a lichen store this version reads. Nothing outside `staging` is touched; on failure it
/// is removed.
pub fn unpack(path: &Path, seed: &[u8; 32], staging: &Path) -> Result<Unpacked, BackupError> {
    let _ = fs::remove_dir_all(staging);
    let read = (|| {
        let src = File::open(path).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                BackupError::NotABackup
            } else {
                BackupError::Io(e)
            }
        })?;
        fs::create_dir_all(staging.join("downloads"))?;
        let mut r = Opener::new(BufReader::new(src), seed)?;
        let mut out = Unpacked {
            staging: staging.to_owned(),
            files: Vec::new(),
            settings: Vec::new(),
            messages: 0,
        };
        let (mut database, mut settings) = (false, false);
        loop {
            let (k, name, size) = read_entry(&mut r)?;
            let target = match k {
                kind::END if name.is_empty() && size == 0 => break,
                kind::DATABASE if name.is_empty() && !database => {
                    database = true;
                    out.database()
                }
                kind::FILE if valid_file_name(&name) && !out.files.contains(&name) => {
                    let p = out.downloads().join(&name);
                    out.files.push(name);
                    p
                }
                kind::SETTINGS if name.is_empty() && !settings && size <= SETTINGS_MAX => {
                    settings = true;
                    (&mut r).take(size).read_to_end(&mut out.settings)?;
                    if out.settings.len() as u64 != size {
                        return Err(BackupError::Damaged);
                    }
                    continue;
                }
                _ => return Err(BackupError::Damaged),
            };
            let mut f = BufWriter::new(File::create(&target)?);
            let copied = io::copy(&mut (&mut r).take(size), &mut f)?;
            f.flush()?;
            if copied != size {
                return Err(BackupError::Damaged);
            }
        }
        // Nothing after END, and the last chunk verified.
        if r.read(&mut [0u8; 1])? != 0 || !database {
            return Err(BackupError::Damaged);
        }
        // A store this version reads (an older one is brought up to date as at any start).
        out.messages = Store::open(&out.database())
            .map_err(|e| match e {
                rusqlite::Error::InvalidQuery => BackupError::Newer,
                e => BackupError::Database(e),
            })?
            .message_count()?;
        Ok(out)
    })();
    if read.is_err() {
        let _ = fs::remove_dir_all(staging);
    }
    read
}

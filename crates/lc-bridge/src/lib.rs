//! lichen's Internet bridge (protocol.md §14, ADR-0015): offers blobs and fetches them with
//! the `lichen/blob/1` protocol, over iroh (peer to peer) or Tor (onion services), verified
//! against their BLAKE3 hash.
//!
//! Async (tokio), unlike the IO-free core: the app runs it next to the core.

mod tor;
pub mod web;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use iroh::endpoint::{Connection, presets};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub use tor::{ONION_PORT, OnionState, onion_address};

/// ALPN of the blob protocol (§14.2).
pub const ALPN: &[u8] = b"lichen/blob/1";
/// Largest blob served or accepted.
pub const MAX_SIZE: u64 = 64 << 20;
/// How long an offered blob stays available.
pub const OFFER_DAYS: u64 = 7;

/// Transports in announcements (§14.1).
pub mod transport {
    pub const IROH: u8 = 1;
    pub const TOR: u8 = 2;
}

pub type Hash = [u8; 32];

/// BLAKE3 of `bytes`.
pub fn hash(bytes: &[u8]) -> Hash {
    *blake3::hash(bytes).as_bytes()
}

/// The bridge key of a phone: `HKDF(seed, label)` (§14.3), `"lichen/1 iroh"` or
/// `"lichen/1 onion"`.
pub fn derive_key(identity_seed: &[u8; 32], label: &str) -> [u8; 32] {
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, identity_seed);
    let mut out = [0u8; 32];
    // 32 bytes is always a valid HKDF-SHA256 output length.
    let _ = hk.expand(label.as_bytes(), &mut out);
    out
}

/// Where the endpoint finds its peers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    /// n0's relays and address lookup: reachable from anywhere.
    Default,
    /// n0's relays only, no direct connection: peers never see our IP address (the relay
    /// does).
    RelayOnly,
    /// No relay, no lookup: local addresses only (tests).
    Local,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchError {
    /// No connection to the provider (offline, unreachable, or not running).
    Unreachable(String),
    /// The provider does not offer this hash (any more).
    NotFound,
    /// Larger than announced or than [`MAX_SIZE`].
    TooLarge,
    /// The content does not match its hash: dropped.
    Mismatch,
    Protocol(String),
    Timeout,
    /// The download could not be written where asked (disk full, no such folder).
    Storage(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for FetchError {}

/// Most bytes of offers kept in memory when there is no directory (tests, the demo network);
/// the oldest are forgotten first.
const MEM_MAX: usize = 128 << 20;
/// Requests served at once, all transports together: one more is closed at once (review of
/// 2026-11-15, SR-33).
const SERVING_MAX: usize = 8;
/// A request (33 bytes) must arrive within this.
const REQUEST_LIMIT: Duration = Duration::from_secs(30);
/// A whole answer must go within this.
pub(crate) const SERVE_LIMIT: Duration = Duration::from_secs(600);
/// Bytes read at a time from a provider, and from an offer's file when serving it.
const CHUNK: usize = 64 << 10;

/// An offer: its content and when it was (last) offered.
#[derive(Debug)]
struct Offered {
    bytes: Arc<Vec<u8>>,
    at: SystemTime,
}

/// Blobs we offer: on disk when a directory is given (the apps: they survive restarts and are
/// served from their file, never held in memory, second read of 2026-11-17, N17), in memory
/// otherwise. Both expire after [`OFFER_DAYS`] (§14.2, recheck R11); offering the same content
/// again renews it.
#[derive(Debug)]
struct Offers {
    dir: Option<PathBuf>,
    mem: Mutex<HashMap<Hash, Offered>>,
    /// Requests being served ([`SERVING_MAX`]).
    serving: tokio::sync::Semaphore,
    /// Names the files being written apart.
    writes: std::sync::atomic::AtomicU64,
}

impl Default for Offers {
    fn default() -> Self {
        Self {
            dir: None,
            mem: Mutex::default(),
            serving: tokio::sync::Semaphore::new(SERVING_MAX),
            writes: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

/// Offered at `at` and not expired yet (a clock set back counts as fresh).
fn fresh(at: SystemTime) -> bool {
    SystemTime::now()
        .duration_since(at)
        .map_or(true, |age| age < Duration::from_secs(OFFER_DAYS * 86_400))
}

/// What an offer is served from.
#[derive(Debug)]
enum Source {
    Mem(Arc<Vec<u8>>),
    /// Its file, opened, and its length.
    File(tokio::fs::File, u64),
}

/// The name of an offer's file: its hash in hex (anything else in the folder is being written).
fn is_offer_name(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|n| n.len() == 64 && n.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Offers {
    fn path(&self, h: &Hash) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(hex::encode(h)))
    }

    fn put(&self, bytes: Vec<u8>) -> Result<Hash, FetchError> {
        let h = hash(&bytes);
        if let Some(p) = self.path(&h) {
            // Written aside, then renamed over: a request being served keeps reading the file
            // it opened, whole.
            let n = self
                .writes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut tmp = p.as_os_str().to_owned();
            tmp.push(format!(".{}.{n}.tmp", std::process::id()));
            let tmp = PathBuf::from(tmp);
            let written = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &p));
            if let Err(e) = written {
                let _ = std::fs::remove_file(&tmp);
                return Err(storage(e));
            }
            return Ok(h);
        }
        if let Ok(mut m) = self.mem.lock() {
            m.insert(
                h,
                Offered {
                    bytes: Arc::new(bytes),
                    at: SystemTime::now(),
                },
            );
            m.retain(|_, o| fresh(o.at));
            // Bounded: the oldest go first.
            while m.len() > 1 && m.values().map(|o| o.bytes.len()).sum::<usize>() > MEM_MAX {
                let Some(oldest) = m.iter().min_by_key(|(_, o)| o.at).map(|(k, _)| *k) else {
                    break;
                };
                m.remove(&oldest);
            }
        }
        Ok(h)
    }

    /// Whether any offer is still served. Expired ones on disk are removed on the way: nothing
    /// is served, or kept, past its week (§14.2).
    fn any_fresh(&self) -> bool {
        let mem = self.mem.lock().is_ok_and(|mut m| {
            m.retain(|_, o| fresh(o.at));
            !m.is_empty()
        });
        let mut disk = false;
        if let Some(d) = &self.dir
            && let Ok(entries) = std::fs::read_dir(d)
        {
            for e in entries.flatten() {
                if e.metadata().and_then(|m| m.modified()).is_ok_and(fresh) {
                    disk |= is_offer_name(&e.file_name());
                } else {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        mem || disk
    }

    /// No longer offered: gone from memory and disk.
    fn remove(&self, h: &Hash) {
        if let Ok(mut m) = self.mem.lock() {
            m.remove(h);
        }
        if let Some(p) = self.path(h) {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Offer `h`, to be served: its content in memory, or its file opened (an expired one is
    /// removed). The file is not read here: it is checked against `h` while it is sent.
    async fn open(&self, h: &Hash) -> Option<Source> {
        let Some(p) = self.path(h) else {
            let mut m = self.mem.lock().ok()?;
            return match m.get(h) {
                Some(o) if fresh(o.at) => Some(Source::Mem(o.bytes.clone())),
                Some(_) => {
                    m.remove(h);
                    None
                }
                None => None,
            };
        };
        let file = tokio::fs::File::open(&p).await.ok()?;
        let meta = file.metadata().await.ok()?;
        if !meta.modified().is_ok_and(fresh) {
            let _ = tokio::fs::remove_file(&p).await;
            return None;
        }
        (meta.len() <= MAX_SIZE).then_some(Source::File(file, meta.len()))
    }
}

/// Sends the `len` bytes of an offer's file, hashed on the way (§14.2, N17): the last chunk
/// goes only once the whole hashes to `h`. False when it does not (changed or cut on disk):
/// the requester then gets less than announced, never other content whole.
async fn send_file<W: AsyncWrite + Unpin>(
    file: &mut tokio::fs::File,
    len: u64,
    h: &Hash,
    w: &mut W,
) -> std::io::Result<bool> {
    let mut hasher = blake3::Hasher::new();
    let (mut held, mut next) = (vec![0u8; CHUNK], vec![0u8; CHUNK]);
    let mut held_len = 0;
    let mut left = len;
    while left > 0 {
        let want = usize::try_from(left).unwrap_or(CHUNK).min(CHUNK);
        let chunk = next
            .get_mut(..want)
            .ok_or(std::io::ErrorKind::InvalidInput)?;
        if file.read_exact(chunk).await.is_err() {
            return Ok(false);
        }
        hasher.update(chunk);
        if let Some(previous) = held.get(..held_len).filter(|p| !p.is_empty()) {
            w.write_all(previous).await?;
        }
        std::mem::swap(&mut held, &mut next);
        held_len = want;
        left -= want as u64;
    }
    if *hasher.finalize().as_bytes() != *h {
        return Ok(false);
    }
    if let Some(last) = held.get(..held_len) {
        w.write_all(last).await?;
    }
    Ok(true)
}

#[cfg(test)]
mod offers {
    use super::*;

    /// Review of 2026-11-15, SR-33: a provider that announces 64 MiB and sends ten bytes is
    /// refused without the 64 MiB being set aside, and what arrives is hashed on the way.
    #[tokio::test]
    async fn a_provider_cannot_make_us_reserve_what_it_does_not_send() {
        let (mut ours, mut theirs) = tokio::io::duplex(1 << 16);
        let mut head = vec![0u8];
        head.extend_from_slice(&MAX_SIZE.to_le_bytes());
        head.extend_from_slice(b"ten bytes!");
        theirs.write_all(&head).await.unwrap();
        drop(theirs);
        let r = read_response(&mut ours, &[7; 32], MAX_SIZE).await;
        assert!(matches!(r, Err(FetchError::Protocol(_))), "{r:?}");
        // Honest bytes in several chunks: accepted.
        let blob: Vec<u8> = (0..(3 * CHUNK + 5)).map(|i| (i % 251) as u8).collect();
        let (mut ours, mut theirs) = tokio::io::duplex(1 << 16);
        let mut head = vec![0u8];
        head.extend_from_slice(&(blob.len() as u64).to_le_bytes());
        let send = blob.clone();
        let writer = tokio::spawn(async move {
            theirs.write_all(&head).await.unwrap();
            theirs.write_all(&send).await.unwrap();
        });
        let got = read_response(&mut ours, &hash(&blob), MAX_SIZE)
            .await
            .unwrap();
        writer.await.unwrap();
        assert_eq!(got, blob);
    }

    /// SR-33: eight requests at once at most; one more is closed at once, and a request that
    /// never arrives is given up.
    #[tokio::test(start_paused = true)]
    async fn serving_is_limited() {
        let offers = Arc::new(Offers::default());
        let mut silent = Vec::new();
        let mut tasks = Vec::new();
        for _ in 0..SERVING_MAX {
            let (a, b) = tokio::io::duplex(64);
            silent.push(a);
            let o = offers.clone();
            tasks.push(tokio::spawn(async move {
                let (mut r, mut w) = tokio::io::split(b);
                serve(&o, &mut r, &mut w).await
            }));
        }
        tokio::task::yield_now().await;
        let (_x, b) = tokio::io::duplex(64);
        let (mut r, mut w) = tokio::io::split(b);
        let one_more = serve(&offers, &mut r, &mut w).await;
        assert_eq!(
            one_more.unwrap_err().kind(),
            std::io::ErrorKind::QuotaExceeded
        );
        // The silent ones are given up after the request limit.
        tokio::time::advance(REQUEST_LIMIT + Duration::from_secs(1)).await;
        for t in tasks {
            assert_eq!(
                t.await.unwrap().unwrap_err().kind(),
                std::io::ErrorKind::TimedOut
            );
        }
        assert_eq!(offers.serving.available_permits(), SERVING_MAX);
    }

    /// An offer expires after 7 days in memory too (recheck R11); offered again, it is back.
    #[tokio::test]
    async fn offers_expire_in_memory() {
        let o = Offers::default();
        let h = o.put(b"photo".to_vec()).unwrap();
        assert!(o.open(&h).await.is_some());
        if let Ok(mut m) = o.mem.lock()
            && let Some(x) = m.get_mut(&h)
        {
            x.at = SystemTime::now() - Duration::from_secs(OFFER_DAYS * 86_400 + 1);
        }
        assert!(o.open(&h).await.is_none());
        assert_eq!(o.put(b"photo".to_vec()), Ok(h));
        assert!(o.open(&h).await.is_some());
    }

    /// Asks `offers` for `h` over a pipe; the answer as the requester reads it.
    async fn ask(offers: &Arc<Offers>, h: &Hash, max: u64) -> Result<Vec<u8>, FetchError> {
        let (mut ours, theirs) = tokio::io::duplex(1 << 16);
        let o = offers.clone();
        let server = tokio::spawn(async move {
            let (mut r, mut w) = tokio::io::split(theirs);
            serve(&o, &mut r, &mut w).await
        });
        write_request(&mut ours, h).await?;
        let got = read_response(&mut ours, h, max).await;
        let _ = server.await;
        got
    }

    /// Second read of 2026-11-17, N17: an offer on disk is served from its file a chunk at a
    /// time, checked against its hash on the way. A file that changed on disk is cut short
    /// before its last chunk, never sent whole, and is no longer offered.
    #[tokio::test]
    async fn a_changed_file_is_cut_short() {
        let dir = std::env::temp_dir().join(format!("lichen-served-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let offers = Arc::new(Offers {
            dir: Some(dir.clone()),
            ..Offers::default()
        });
        let blob: Vec<u8> = (0..(3 * CHUNK + 5)).map(|i| (i % 249) as u8).collect();
        let h = offers.put(blob.clone()).unwrap();
        assert!(offers.mem.lock().unwrap().is_empty(), "on disk only");
        assert_eq!(ask(&offers, &h, MAX_SIZE).await, Ok(blob.clone()));
        // One byte changed in place.
        {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::File::options()
                .write(true)
                .open(offers.path(&h).unwrap())
                .unwrap();
            f.seek(SeekFrom::Start(CHUNK as u64 + 1)).unwrap();
            f.write_all(&[blob[CHUNK + 1] ^ 0xFF]).unwrap();
        }
        let r = ask(&offers, &h, MAX_SIZE).await;
        assert!(matches!(r, Err(FetchError::Protocol(_))), "{r:?}");
        assert!(!offers.path(&h).unwrap().exists());
        assert_eq!(ask(&offers, &h, MAX_SIZE).await, Err(FetchError::NotFound));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Whether anything is still offered decides whether iroh runs (SR-29); expired offers on
    /// disk are removed when asked.
    #[test]
    fn live_offers_and_the_sweep() {
        let dir = std::env::temp_dir().join(format!("lichen-offers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let o = Offers {
            dir: Some(dir.clone()),
            ..Offers::default()
        };
        assert!(!o.any_fresh());
        let h = o.put(b"track".to_vec()).unwrap();
        assert!(o.any_fresh());
        // A week later: its file is removed.
        let old = SystemTime::now() - Duration::from_secs(OFFER_DAYS * 86_400 + 60);
        std::fs::File::options()
            .write(true)
            .open(o.path(&h).unwrap())
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(!o.any_fresh());
        assert!(!o.path(&h).unwrap().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The provider side of `lichen/blob/1` (§14.2), on any byte stream: reads one request,
/// answers it. At most [`SERVING_MAX`] at once; the request within [`REQUEST_LIMIT`], the
/// whole answer within [`SERVE_LIMIT`].
async fn serve<R, W>(offers: &Offers, r: &mut R, w: &mut W) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let Ok(_serving) = offers.serving.try_acquire() else {
        return Err(std::io::ErrorKind::QuotaExceeded.into());
    };
    tokio::time::timeout(SERVE_LIMIT, answer(offers, r, w))
        .await
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?
}

async fn answer<R, W>(offers: &Offers, r: &mut R, w: &mut W) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut req = [0u8; 33];
    tokio::time::timeout(REQUEST_LIMIT, r.read_exact(&mut req))
        .await
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))??;
    let (op, h) = req.split_at(1);
    let (found, h) = match (op, <Hash>::try_from(h)) {
        ([1], Ok(h)) => (offers.open(&h).await, h),
        _ => (None, [0; 32]),
    };
    let head = |len: u64| {
        let mut head = [0u8; 9];
        head[1..].copy_from_slice(&len.to_le_bytes());
        head
    };
    match found {
        Some(Source::Mem(bytes)) => {
            w.write_all(&head(bytes.len() as u64)).await?;
            w.write_all(&bytes).await?;
        }
        // From the file as it is read, a chunk at a time: eight requests for a 64 MiB offer
        // no longer hold 512 MiB (N17).
        Some(Source::File(mut file, len)) => {
            w.write_all(&head(len)).await?;
            if !send_file(&mut file, len, &h, w).await? {
                // Not what we offered any more: no longer offered.
                offers.remove(&h);
                return Err(std::io::ErrorKind::InvalidData.into());
            }
        }
        None => w.write_all(&[1]).await?,
    }
    w.flush().await
}

/// The requester side: asks for `hash`.
async fn write_request<W: AsyncWrite + Unpin>(w: &mut W, hash: &Hash) -> Result<(), FetchError> {
    let mut req = [0u8; 33];
    req[0] = 1;
    req[1..].copy_from_slice(hash);
    w.write_all(&req).await.map_err(protocol)?;
    w.flush().await.map_err(protocol)
}

/// The requester side: reads at most `max` bytes and keeps them only if they hash to `hash`.
#[cfg(test)]
async fn read_response<R: AsyncRead + Unpin>(
    r: &mut R,
    hash: &Hash,
    max: u64,
) -> Result<Vec<u8>, FetchError> {
    let mut bytes = Vec::new();
    read_response_into(r, &mut bytes, hash, max).await?;
    Ok(bytes)
}

/// Reads an answer into `w` as it comes, hashed on the way; returns its size. What `w` holds
/// is only the content once this returns `Ok`: the hash is checked at the end.
async fn read_response_into<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    r: &mut R,
    w: &mut W,
    hash: &Hash,
    max: u64,
) -> Result<u64, FetchError> {
    let mut status = [0u8; 1];
    r.read_exact(&mut status).await.map_err(protocol)?;
    if status[0] != 0 {
        return Err(FetchError::NotFound);
    }
    let mut len = [0u8; 8];
    r.read_exact(&mut len).await.map_err(protocol)?;
    let size = u64::from_le_bytes(len);
    if size > max.min(MAX_SIZE) {
        return Err(FetchError::TooLarge);
    }
    // Read as it comes, hashed on the way, and handed to `w`: nothing is set aside for what a
    // provider only claims it will send, and a download goes to a file, not memory (SR-33).
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; CHUNK];
    let mut left = size;
    while left > 0 {
        let want = usize::try_from(left).unwrap_or(CHUNK).min(CHUNK);
        let chunk = buf.get_mut(..want).ok_or_else(|| protocol("buffer"))?;
        let n = r.read(chunk).await.map_err(protocol)?;
        if n == 0 {
            return Err(protocol("cut short"));
        }
        let got = chunk.get(..n).ok_or_else(|| protocol("buffer"))?;
        hasher.update(got);
        w.write_all(got).await.map_err(storage)?;
        left -= n as u64;
    }
    if *hasher.finalize().as_bytes() != *hash {
        return Err(FetchError::Mismatch);
    }
    Ok(size)
}

fn protocol(e: impl std::fmt::Display) -> FetchError {
    FetchError::Protocol(e.to_string())
}

fn storage(e: impl std::fmt::Display) -> FetchError {
    FetchError::Storage(e.to_string())
}

/// A download's `<path>.part` file: removed unless it became the download, including when the
/// fetch is dropped half-way (a cancelled download).
struct Part {
    path: PathBuf,
    kept: bool,
}

impl Part {
    async fn create(path: &Path) -> Result<(Self, tokio::fs::File), FetchError> {
        let mut name = path.as_os_str().to_owned();
        name.push(".part");
        let part = Self {
            path: PathBuf::from(name),
            kept: false,
        };
        let file = tokio::fs::File::create(&part.path).await.map_err(storage)?;
        Ok((part, file))
    }

    /// The content is complete and matches: it becomes `path`.
    async fn keep(mut self, mut file: tokio::fs::File, path: &Path) -> Result<(), FetchError> {
        file.flush().await.map_err(storage)?;
        file.sync_all().await.map_err(storage)?;
        drop(file);
        tokio::fs::rename(&self.path, path).await.map_err(storage)?;
        self.kept = true;
        Ok(())
    }
}

impl Drop for Part {
    fn drop(&mut self) {
        if !self.kept {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// `lichen/blob/1` on iroh.
#[derive(Debug, Clone)]
struct Provider {
    offers: Arc<Offers>,
}

impl ProtocolHandler for Provider {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let (mut send, mut recv) = connection.accept_bi().await?;
        if serve(&self.offers, &mut recv, &mut send).await.is_ok() {
            let _ = send.finish();
            connection.closed().await;
        } else {
            // Cut short, not finished: the requester sees an answer that ended early.
            let _ = send.reset(0u32.into());
        }
        Ok(())
    }
}

/// This phone's bridge: the blobs it offers, its iroh endpoint and its Tor client, each
/// started on demand (a "Tor only" phone never starts iroh).
pub struct Bridge {
    offers: Arc<Offers>,
    iroh: Mutex<Option<Router>>,
    tor: tokio::sync::Mutex<Option<tor::Tor>>,
}

impl std::fmt::Debug for Bridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bridge")
            .field("iroh", &self.iroh_id().map(hex::encode))
            .finish_non_exhaustive()
    }
}

impl Bridge {
    /// A bridge with no transport yet; offered blobs are kept in `dir` when given.
    pub fn new(dir: Option<PathBuf>) -> Result<Self, String> {
        if let Some(d) = &dir {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        Ok(Self {
            offers: Arc::new(Offers {
                dir,
                ..Offers::default()
            }),
            iroh: Mutex::new(None),
            tor: tokio::sync::Mutex::new(None),
        })
    }

    /// A bridge with its iroh endpoint started (key `secret`, from [`derive_key`]).
    pub async fn start(
        secret: [u8; 32],
        dir: Option<PathBuf>,
        network: Network,
    ) -> Result<Self, String> {
        let b = Self::new(dir)?;
        b.start_iroh(secret, network).await?;
        Ok(b)
    }

    fn router(&self) -> Option<Router> {
        self.iroh.lock().ok().and_then(|r| r.clone())
    }

    /// Starts the iroh endpoint (once); returns its endpoint id.
    pub async fn start_iroh(&self, secret: [u8; 32], network: Network) -> Result<[u8; 32], String> {
        if let Some(id) = self.iroh_id() {
            return Ok(id);
        }
        let endpoint = match network {
            Network::Default => Endpoint::builder(presets::N0),
            Network::RelayOnly => Endpoint::builder(presets::N0).clear_ip_transports(),
            Network::Local => Endpoint::builder(presets::Minimal),
        }
        .secret_key(SecretKey::from_bytes(&secret))
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .map_err(|e| e.to_string())?;
        let router = Router::builder(endpoint)
            .accept(
                ALPN,
                Provider {
                    offers: self.offers.clone(),
                },
            )
            .spawn();
        let id = *router.endpoint().id().as_bytes();
        if let Ok(mut r) = self.iroh.lock() {
            *r = Some(router);
        }
        Ok(id)
    }

    /// Stops the iroh endpoint ("Tor only").
    pub async fn stop_iroh(&self) {
        let router = self.iroh.lock().ok().and_then(|mut r| r.take());
        if let Some(r) = router {
            let _ = r.shutdown().await;
        }
    }

    /// Whether we still offer something: the apps keep iroh running only then, or while the
    /// user sends or fetches (review of 2026-11-15, SR-29).
    pub fn has_offers(&self) -> bool {
        self.offers.any_fresh()
    }

    /// Our endpoint id: the `key` of the iroh transport in announcements.
    pub fn iroh_id(&self) -> Option<[u8; 32]> {
        self.router().map(|r| *r.endpoint().id().as_bytes())
    }

    /// Our iroh addresses (direct and relay), for tests on a local network.
    pub fn addr(&self) -> Option<EndpointAddr> {
        self.router().map(|r| r.endpoint().addr())
    }

    /// Waits until the iroh endpoint is reachable (a relay connected); at most `limit`.
    pub async fn online(&self, limit: Duration) -> bool {
        let Some(r) = self.router() else {
            return false;
        };
        tokio::time::timeout(limit, r.endpoint().online())
            .await
            .is_ok()
    }

    /// Connects to Tor (once), keeping its state and directory cache in the given folders;
    /// gives up after `limit`.
    pub async fn start_tor(
        &self,
        state: &Path,
        cache: &Path,
        limit: Duration,
    ) -> Result<(), String> {
        let mut t = self.tor.lock().await;
        if t.is_none() {
            let started = tokio::time::timeout(limit, tor::Tor::start(state, cache))
                .await
                .map_err(|_| "Tor did not connect in time".to_string())??;
            *t = Some(started);
        }
        Ok(())
    }

    /// Publishes our onion service (once), keyed by `secret` (from [`derive_key`] with
    /// `"lichen/1 onion"`); returns its identity, the `key` of the Tor transport.
    pub async fn publish_onion(&self, secret: [u8; 32]) -> Result<[u8; 32], String> {
        let mut t = self.tor.lock().await;
        let tor = t.as_mut().ok_or("Tor is not started")?;
        tor.publish(secret, self.offers.clone())
    }

    /// Our onion service identity, once published.
    pub async fn onion_id(&self) -> Option<[u8; 32]> {
        self.tor.lock().await.as_ref().and_then(tor::Tor::onion_id)
    }

    pub async fn onion_state(&self) -> OnionState {
        match self.tor.lock().await.as_ref() {
            Some(t) => t.state(),
            None => OnionState::Off,
        }
    }

    /// Waits until our onion service is reachable; at most `limit`.
    pub async fn onion_reachable(&self, limit: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + limit;
        loop {
            match self.onion_state().await {
                OnionState::Reachable => return true,
                OnionState::Off | OnionState::Broken => return false,
                OnionState::Starting => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Stops Tor: the onion service and the client.
    pub async fn stop_tor(&self) {
        self.tor.lock().await.take();
    }

    /// Stops offering `hash`: its message was deleted (§6.4, review of 2026-11-15, SR-28).
    pub fn revoke(&self, hash: &Hash) {
        self.offers.remove(hash);
    }

    /// Offers `bytes`: returns their hash and size for the announcement. With a directory,
    /// they are written there and served from that file; a write that fails is a storage
    /// error.
    pub fn offer(&self, bytes: Vec<u8>) -> Result<(Hash, u64), FetchError> {
        let size = bytes.len() as u64;
        if size > MAX_SIZE {
            return Err(FetchError::TooLarge);
        }
        Ok((self.offers.put(bytes)?, size))
    }

    /// Fetches `hash` from a provider of an announcement (`transport`, `key`), at most `max`
    /// bytes, into memory (tests, the demo network).
    pub async fn fetch(
        &self,
        transport: u8,
        key: [u8; 32],
        hash: Hash,
        max: u64,
    ) -> Result<Vec<u8>, FetchError> {
        let mut bytes = Vec::new();
        self.fetch_into(transport, key, hash, max, &mut bytes)
            .await?;
        Ok(bytes)
    }

    /// Fetches `hash` like [`Bridge::fetch`] into the file `path`, never holding it in memory
    /// (review of 2026-11-15, SR-33). It is written to `<path>.part` and renamed to `path` only
    /// once its hash matches; on any failure, or if this future is dropped, nothing is left.
    pub async fn fetch_to_file(
        &self,
        transport: u8,
        key: [u8; 32],
        hash: Hash,
        max: u64,
        path: &Path,
    ) -> Result<u64, FetchError> {
        let (part, mut file) = Part::create(path).await?;
        let size = self
            .fetch_into(transport, key, hash, max, &mut file)
            .await?;
        part.keep(file, path).await?;
        Ok(size)
    }

    /// [`Bridge::fetch_to_file`] over iroh from a full address (tests, or an address learned
    /// otherwise).
    pub async fn fetch_from_to_file(
        &self,
        addr: EndpointAddr,
        hash: Hash,
        max: u64,
        path: &Path,
    ) -> Result<u64, FetchError> {
        let (part, mut file) = Part::create(path).await?;
        let size = self.fetch_from_into(addr, hash, max, &mut file).await?;
        part.keep(file, path).await?;
        Ok(size)
    }

    /// Fetches `hash` into `w`; what `w` holds is the content only if this returns `Ok`.
    async fn fetch_into<W: AsyncWrite + Unpin + Send>(
        &self,
        transport: u8,
        key: [u8; 32],
        hash: Hash,
        max: u64,
        w: &mut W,
    ) -> Result<u64, FetchError> {
        match transport {
            transport::IROH => {
                let id = EndpointId::from_bytes(&key).map_err(protocol)?;
                self.fetch_from_into(EndpointAddr::from(id), hash, max, w)
                    .await
            }
            transport::TOR => {
                let client = self
                    .tor
                    .lock()
                    .await
                    .as_ref()
                    .map(tor::Tor::client)
                    .ok_or_else(|| FetchError::Unreachable("Tor is not started".into()))?;
                let work = tor::fetch(&client, key, hash, max, w);
                tokio::time::timeout(tor::FETCH_LIMIT, work)
                    .await
                    .map_err(|_| FetchError::Timeout)?
            }
            _ => Err(FetchError::Protocol("unknown transport".into())),
        }
    }

    /// GIPHY GIF `id` (§6.3, ADR-0025): its animated WebP, else the GIF itself, through Tor
    /// when `via_tor` (Tor must be started), directly otherwise.
    pub async fn gif(&self, id: &str, via_tor: bool) -> Result<Vec<u8>, FetchError> {
        let paths = web::giphy_paths(id).ok_or(FetchError::NotFound)?;
        let client = if via_tor {
            Some(
                self.tor
                    .lock()
                    .await
                    .as_ref()
                    .map(tor::Tor::client)
                    .ok_or_else(|| FetchError::Unreachable("Tor is not started".into()))?,
            )
        } else {
            None
        };
        let limit = Duration::from_secs(if via_tor { 90 } else { 30 });
        for path in paths {
            let work = async {
                match &client {
                    Some(c) => {
                        let s = c
                            .connect((web::GIPHY_HOST, 443))
                            .await
                            .map_err(|e| FetchError::Unreachable(e.to_string()))?;
                        web::get(s, web::GIPHY_HOST, &path).await
                    }
                    None => {
                        let s = tokio::net::TcpStream::connect((web::GIPHY_HOST, 443))
                            .await
                            .map_err(|e| FetchError::Unreachable(e.to_string()))?;
                        web::get(s, web::GIPHY_HOST, &path).await
                    }
                }
            };
            match tokio::time::timeout(limit, work).await {
                Ok(Err(FetchError::NotFound)) => {}
                Ok(result) => return result,
                Err(_) => return Err(FetchError::Timeout),
            }
        }
        Err(FetchError::NotFound)
    }

    /// Fetches over iroh from a full address (tests, or an address learned otherwise).
    pub async fn fetch_from(
        &self,
        addr: EndpointAddr,
        hash: Hash,
        max: u64,
    ) -> Result<Vec<u8>, FetchError> {
        let mut bytes = Vec::new();
        self.fetch_from_into(addr, hash, max, &mut bytes).await?;
        Ok(bytes)
    }

    async fn fetch_from_into<W: AsyncWrite + Unpin + Send>(
        &self,
        addr: EndpointAddr,
        hash: Hash,
        max: u64,
        w: &mut W,
    ) -> Result<u64, FetchError> {
        let router = self
            .router()
            .ok_or_else(|| FetchError::Unreachable("iroh is not started".into()))?;
        let work = async {
            let conn = router
                .endpoint()
                .connect(addr, ALPN)
                .await
                .map_err(|e| FetchError::Unreachable(e.to_string()))?;
            let (mut send, mut recv) = conn
                .open_bi()
                .await
                .map_err(|e| FetchError::Unreachable(e.to_string()))?;
            write_request(&mut send, &hash).await?;
            let _ = send.finish();
            let size = read_response_into(&mut recv, w, &hash, max).await;
            conn.close(0u32.into(), b"done");
            size
        };
        tokio::time::timeout(Duration::from_secs(120), work)
            .await
            .map_err(|_| FetchError::Timeout)?
    }

    pub async fn shutdown(self) {
        self.stop_tor().await;
        self.stop_iroh().await;
    }
}

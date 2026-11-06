//! lichen's Internet bridge (protocol.md §14, ADR-0015): offers blobs and fetches them over
//! iroh with the `lichen/blob/1` protocol, verified against their BLAKE3 hash.
//!
//! Async (tokio), unlike the IO-free core: the app runs it next to the core.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use iroh::endpoint::{Connection, presets};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};

/// ALPN of the blob protocol (§14.2).
pub const ALPN: &[u8] = b"lichen/blob/1";
/// Largest blob served or accepted.
pub const MAX_SIZE: u64 = 64 << 20;
/// How long an offered blob stays available.
pub const OFFER_DAYS: u64 = 7;

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
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for FetchError {}

/// Blobs we offer: in memory, and on disk when a directory is given (they survive restarts).
#[derive(Debug, Default)]
struct Offers {
    dir: Option<PathBuf>,
    mem: Mutex<HashMap<Hash, Arc<Vec<u8>>>>,
}

impl Offers {
    fn path(&self, h: &Hash) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(hex::encode(h)))
    }

    fn put(&self, bytes: Vec<u8>) -> Hash {
        let h = hash(&bytes);
        if let Some(p) = self.path(&h) {
            let _ = std::fs::write(p, &bytes);
        }
        if let Ok(mut m) = self.mem.lock() {
            m.insert(h, Arc::new(bytes));
        }
        h
    }

    fn get(&self, h: &Hash) -> Option<Arc<Vec<u8>>> {
        if let Some(b) = self.mem.lock().ok().and_then(|m| m.get(h).cloned()) {
            return Some(b);
        }
        let p = self.path(h)?;
        let fresh = std::fs::metadata(&p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < Duration::from_secs(OFFER_DAYS * 86_400));
        if !fresh {
            let _ = std::fs::remove_file(&p);
            return None;
        }
        let bytes = std::fs::read(&p).ok()?;
        (hash(&bytes) == *h).then(|| Arc::new(bytes))
    }
}

/// The provider side of `lichen/blob/1`.
#[derive(Debug, Clone)]
struct Provider {
    offers: Arc<Offers>,
}

impl ProtocolHandler for Provider {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let (mut send, mut recv) = connection.accept_bi().await?;
        let mut req = [0u8; 33];
        if recv.read_exact(&mut req).await.is_err() {
            return Ok(());
        }
        let (op, h) = req.split_at(1);
        let found = match (op, <Hash>::try_from(h)) {
            ([1], Ok(h)) => self.offers.get(&h),
            _ => None,
        };
        let r = match found {
            Some(bytes) => {
                let mut head = [0u8; 9];
                head[1..].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
                match send.write_all(&head).await {
                    Ok(()) => send.write_all(&bytes).await,
                    Err(e) => Err(e),
                }
            }
            None => send.write_all(&[1]).await,
        };
        if r.is_ok() {
            let _ = send.finish();
            connection.closed().await;
        }
        Ok(())
    }
}

/// An iroh endpoint that offers and fetches blobs.
#[derive(Debug)]
pub struct Bridge {
    router: Router,
    offers: Arc<Offers>,
}

impl Bridge {
    /// Starts the endpoint with key `secret` (from [`derive_key`]); offered blobs are kept in
    /// `dir` when given.
    pub async fn start(
        secret: [u8; 32],
        dir: Option<PathBuf>,
        network: Network,
    ) -> Result<Self, String> {
        if let Some(d) = &dir {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let key = SecretKey::from_bytes(&secret);
        let endpoint = match network {
            Network::Default => Endpoint::builder(presets::N0),
            Network::Local => Endpoint::builder(presets::Minimal),
        }
        .secret_key(key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .map_err(|e| e.to_string())?;
        let offers = Arc::new(Offers {
            dir,
            mem: Mutex::default(),
        });
        let router = Router::builder(endpoint)
            .accept(
                ALPN,
                Provider {
                    offers: offers.clone(),
                },
            )
            .spawn();
        Ok(Self { router, offers })
    }

    /// Our endpoint id: the `key` of the iroh transport in announcements.
    pub fn id(&self) -> [u8; 32] {
        *self.router.endpoint().id().as_bytes()
    }

    /// Our addresses (direct and relay), for tests on a local network.
    pub fn addr(&self) -> EndpointAddr {
        self.router.endpoint().addr()
    }

    /// Waits until the endpoint is reachable (a relay connected); at most `limit`.
    pub async fn online(&self, limit: Duration) -> bool {
        tokio::time::timeout(limit, self.router.endpoint().online())
            .await
            .is_ok()
    }

    /// Offers `bytes`: returns their hash and size for the announcement.
    pub fn offer(&self, bytes: Vec<u8>) -> Result<(Hash, u64), FetchError> {
        let size = bytes.len() as u64;
        if size > MAX_SIZE {
            return Err(FetchError::TooLarge);
        }
        Ok((self.offers.put(bytes), size))
    }

    /// Fetches `hash` from provider `id` (by its endpoint id), at most `max` bytes.
    pub async fn fetch(&self, id: [u8; 32], hash: Hash, max: u64) -> Result<Vec<u8>, FetchError> {
        let id = EndpointId::from_bytes(&id).map_err(|e| FetchError::Protocol(e.to_string()))?;
        self.fetch_from(EndpointAddr::from(id), hash, max).await
    }

    /// Fetches from a full address (tests, or an address learned otherwise).
    pub async fn fetch_from(
        &self,
        addr: EndpointAddr,
        hash: Hash,
        max: u64,
    ) -> Result<Vec<u8>, FetchError> {
        let max = max.min(MAX_SIZE);
        let work = async {
            let conn = self
                .router
                .endpoint()
                .connect(addr, ALPN)
                .await
                .map_err(|e| FetchError::Unreachable(e.to_string()))?;
            let (mut send, mut recv) = conn
                .open_bi()
                .await
                .map_err(|e| FetchError::Unreachable(e.to_string()))?;
            let mut req = [0u8; 33];
            req[0] = 1;
            req[1..].copy_from_slice(&hash);
            send.write_all(&req)
                .await
                .map_err(|e| FetchError::Protocol(e.to_string()))?;
            let _ = send.finish();
            let mut status = [0u8; 1];
            recv.read_exact(&mut status)
                .await
                .map_err(|e| FetchError::Protocol(e.to_string()))?;
            if status[0] != 0 {
                return Err(FetchError::NotFound);
            }
            let mut len = [0u8; 8];
            recv.read_exact(&mut len)
                .await
                .map_err(|e| FetchError::Protocol(e.to_string()))?;
            let size = u64::from_le_bytes(len);
            if size > max {
                return Err(FetchError::TooLarge);
            }
            let mut bytes = vec![0u8; size as usize];
            recv.read_exact(&mut bytes)
                .await
                .map_err(|e| FetchError::Protocol(e.to_string()))?;
            conn.close(0u32.into(), b"done");
            if crate::hash(&bytes) != hash {
                return Err(FetchError::Mismatch);
            }
            Ok(bytes)
        };
        tokio::time::timeout(Duration::from_secs(120), work)
            .await
            .map_err(|_| FetchError::Timeout)?
    }

    pub async fn shutdown(self) {
        let _ = self.router.shutdown().await;
    }
}

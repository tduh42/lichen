//! The Internet bridge for the apps (protocol.md §14, ADR-0015): an iroh endpoint that offers
//! content and fetches it, verified by BLAKE3. Async: Swift sees `async` methods.

use std::sync::Arc;
use std::time::Duration;

use lc_bridge::{Bridge, FetchError, Network, derive_key};

#[derive(Debug, PartialEq, Eq, uniffi::Error)]
pub enum BridgeError {
    /// The provider could not be reached (offline, or its app is not running).
    Unreachable {
        reason: String,
    },
    /// The provider no longer offers this content.
    NotFound,
    /// Larger than announced, or than 64 MiB.
    TooLarge,
    /// The content did not match its hash: dropped.
    Mismatch,
    Timeout,
    Failed {
        reason: String,
    },
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for BridgeError {}

impl From<FetchError> for BridgeError {
    fn from(e: FetchError) -> Self {
        match e {
            FetchError::Unreachable(reason) => Self::Unreachable { reason },
            FetchError::NotFound => Self::NotFound,
            FetchError::TooLarge => Self::TooLarge,
            FetchError::Mismatch => Self::Mismatch,
            FetchError::Timeout => Self::Timeout,
            FetchError::Protocol(reason) => Self::Failed { reason },
        }
    }
}

/// Content offered: what goes in the announcement.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Offered {
    pub hash: Vec<u8>,
    pub size: u64,
}

/// The iroh endpoint of this phone.
#[derive(Debug, uniffi::Object)]
pub struct LichenBridge {
    bridge: Bridge,
}

#[uniffi::export(async_runtime = "tokio")]
impl LichenBridge {
    /// Starts the endpoint: its key derives from the identity seed (§14.3); offered content is
    /// kept in `dir` (7 days).
    #[uniffi::constructor]
    pub async fn start(
        identity_seed: Vec<u8>,
        dir: Option<String>,
    ) -> Result<Arc<Self>, BridgeError> {
        let seed: [u8; 32] =
            identity_seed
                .as_slice()
                .try_into()
                .map_err(|_| BridgeError::Failed {
                    reason: "identity seed of 32 bytes".into(),
                })?;
        let bridge = Bridge::start(
            derive_key(&seed, "lichen/1 iroh"),
            dir.map(Into::into),
            Network::Default,
        )
        .await
        .map_err(|reason| BridgeError::Failed { reason })?;
        Ok(Arc::new(Self { bridge }))
    }

    /// Our iroh endpoint id: the key of the iroh transport in announcements.
    pub fn endpoint_id(&self) -> Vec<u8> {
        self.bridge.id().to_vec()
    }

    /// True once a relay answers (within `limit_s` seconds): the phone is reachable.
    pub async fn online(&self, limit_s: u32) -> bool {
        self.bridge
            .online(Duration::from_secs(u64::from(limit_s)))
            .await
    }

    /// Offers content; returns its hash and size for the announcement.
    pub fn offer(&self, bytes: Vec<u8>) -> Result<Offered, BridgeError> {
        let (hash, size) = self.bridge.offer(bytes)?;
        Ok(Offered {
            hash: hash.to_vec(),
            size,
        })
    }

    /// Fetches announced content from its provider, verified against its hash.
    pub async fn fetch(
        &self,
        provider: Vec<u8>,
        hash: Vec<u8>,
        size: u64,
    ) -> Result<Vec<u8>, BridgeError> {
        let (Ok(provider), Ok(hash)) = (<[u8; 32]>::try_from(provider), <[u8; 32]>::try_from(hash))
        else {
            return Err(BridgeError::Failed {
                reason: "keys and hashes are 32 bytes".into(),
            });
        };
        Ok(self.bridge.fetch(provider, hash, size).await?)
    }
}

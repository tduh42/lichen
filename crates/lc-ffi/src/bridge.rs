//! The Internet bridge for the apps (protocol.md §14, ADR-0015): offers content and fetches
//! it over iroh or Tor, verified by BLAKE3. Async: Swift sees `async` methods.

use std::sync::Arc;
use std::time::Duration;

use lc_bridge::{Bridge, FetchError, Network, OnionState, derive_key};

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

/// Our onion service (Tor).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum OnionStatus {
    /// Not published.
    Off,
    /// Publishing, or recovering.
    Starting,
    /// Reachable by others.
    Reachable,
    /// Failed; restarting Tor may help.
    Broken,
}

impl From<OnionState> for OnionStatus {
    fn from(s: OnionState) -> Self {
        match s {
            OnionState::Off => Self::Off,
            OnionState::Starting => Self::Starting,
            OnionState::Reachable => Self::Reachable,
            OnionState::Broken => Self::Broken,
        }
    }
}

/// The `.onion` address of an onion service identity (an announcement's Tor key).
#[uniffi::export]
pub fn onion_address(key: Vec<u8>) -> Option<String> {
    lc_bridge::onion_address(&<[u8; 32]>::try_from(key).ok()?)
}

fn bytes32(v: Vec<u8>) -> Result<[u8; 32], BridgeError> {
    <[u8; 32]>::try_from(v).map_err(|_| BridgeError::Failed {
        reason: "keys and hashes are 32 bytes".into(),
    })
}

/// This phone's bridge: its iroh endpoint and its Tor client, each started on demand.
#[derive(Debug, uniffi::Object)]
pub struct LichenBridge {
    bridge: Bridge,
    iroh_key: [u8; 32],
    onion_key: [u8; 32],
}

#[uniffi::export(async_runtime = "tokio")]
impl LichenBridge {
    /// A bridge for the identity of `identity_seed` (its keys derive from it, §14.3), with
    /// no transport yet; offered content is kept in `dir` (7 days).
    #[uniffi::constructor]
    pub fn new(identity_seed: Vec<u8>, dir: Option<String>) -> Result<Arc<Self>, BridgeError> {
        let seed = bytes32(identity_seed)?;
        let bridge =
            Bridge::new(dir.map(Into::into)).map_err(|reason| BridgeError::Failed { reason })?;
        Ok(Arc::new(Self {
            bridge,
            iroh_key: derive_key(&seed, "lichen/1 iroh"),
            onion_key: derive_key(&seed, "lichen/1 onion"),
        }))
    }

    /// Starts the iroh endpoint; returns its endpoint id, the key of the iroh transport.
    /// With `relay_only`, every connection goes through a relay: peers never see our IP.
    pub async fn start_iroh(&self, relay_only: bool) -> Result<Vec<u8>, BridgeError> {
        let network = if relay_only {
            Network::RelayOnly
        } else {
            Network::Default
        };
        let id = self
            .bridge
            .start_iroh(self.iroh_key, network)
            .await
            .map_err(|reason| BridgeError::Failed { reason })?;
        Ok(id.to_vec())
    }

    /// Stops the iroh endpoint (iroh off, or before switching relay only).
    pub async fn stop_iroh(&self) {
        self.bridge.stop_iroh().await;
    }

    /// Our iroh endpoint id, while iroh runs.
    pub fn endpoint_id(&self) -> Option<Vec<u8>> {
        self.bridge.iroh_id().map(|id| id.to_vec())
    }

    /// True once a relay answers (within `limit_s` seconds): reachable over iroh.
    pub async fn online(&self, limit_s: u32) -> bool {
        self.bridge
            .online(Duration::from_secs(u64::from(limit_s)))
            .await
    }

    /// Connects to Tor (within `limit_s` seconds), keeping its state and directory cache in
    /// the given folders; with `publish`, also publishes our onion service and returns its
    /// identity, the key of the Tor transport.
    pub async fn start_tor(
        &self,
        state_dir: String,
        cache_dir: String,
        publish: bool,
        limit_s: u32,
    ) -> Result<Option<Vec<u8>>, BridgeError> {
        self.bridge
            .start_tor(
                state_dir.as_ref(),
                cache_dir.as_ref(),
                Duration::from_secs(u64::from(limit_s)),
            )
            .await
            .map_err(|reason| BridgeError::Unreachable { reason })?;
        if !publish {
            return Ok(None);
        }
        let id = self
            .bridge
            .publish_onion(self.onion_key)
            .await
            .map_err(|reason| BridgeError::Failed { reason })?;
        Ok(Some(id.to_vec()))
    }

    /// Stops Tor: our onion service and the client.
    pub async fn stop_tor(&self) {
        self.bridge.stop_tor().await;
    }

    /// Our onion service identity, once published.
    pub async fn onion_id(&self) -> Option<Vec<u8>> {
        self.bridge.onion_id().await.map(|id| id.to_vec())
    }

    pub async fn onion_status(&self) -> OnionStatus {
        self.bridge.onion_state().await.into()
    }

    /// True once our onion service is reachable (within `limit_s` seconds).
    pub async fn onion_reachable(&self, limit_s: u32) -> bool {
        self.bridge
            .onion_reachable(Duration::from_secs(u64::from(limit_s)))
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

    /// GIPHY GIF `id` (§6.3): its animated WebP, else the GIF itself, through Tor when
    /// `via_tor` (Tor must be started), directly otherwise. The address is built from the id,
    /// host and path fixed by the spec.
    pub async fn gif(&self, id: String, via_tor: bool) -> Result<Vec<u8>, BridgeError> {
        Ok(self.bridge.gif(&id, via_tor).await?)
    }

    /// Fetches announced content from one of its providers (`transport` 1 iroh, 2 Tor),
    /// verified against its hash.
    pub async fn fetch(
        &self,
        transport: u8,
        provider: Vec<u8>,
        hash: Vec<u8>,
        size: u64,
    ) -> Result<Vec<u8>, BridgeError> {
        let (provider, hash) = (bytes32(provider)?, bytes32(hash)?);
        Ok(self.bridge.fetch(transport, provider, hash, size).await?)
    }
}

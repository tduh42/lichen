//! The Tor transport (§14, ADR-0015): an Arti client fetches from onion services, and our own
//! onion service, keyed from the identity seed, serves `lichen/blob/1` on [`ONION_PORT`].
//!
//! The onion key lives in Arti's in-memory keystore only: it is derived again from the seed
//! (kept in the Keychain) at every start, never written by Arti.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use arti_client::config::onion_service::OnionServiceConfigBuilder;
use arti_client::config::{BoolOrAuto, TorClientConfigBuilder};
use arti_client::{StreamPrefs, TorClient};
use futures::StreamExt;
use safelog::DisplayRedacted;
use tor_cell::relaycell::msg::{Connected, End, EndReason};
use tor_config::ExplicitOrAuto;
use tor_hscrypto::pk::{HsId, HsIdKey, HsIdKeypair};
use tor_hsservice::status::State;
use tor_hsservice::{HsNickname, RunningOnionService, handle_rend_requests};
use tor_keymgr::config::ArtiKeystoreKind;
use tor_llcrypto::pk::ed25519;
use tor_proto::stream::IncomingStreamRequest;
use tor_rtcompat::PreferredRuntime;

use crate::{FetchError, Hash, Offers, read_response, serve, write_request};

/// The onion service's virtual port for `lichen/blob/1`.
pub const ONION_PORT: u16 = 1;
/// Longest fetch over Tor: building circuits to an onion service takes tens of seconds.
pub(crate) const FETCH_LIMIT: Duration = Duration::from_secs(180);
/// Longest time to serve one request.
const SERVE_LIMIT: Duration = Duration::from_secs(600);

/// Our onion service, as far as we know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnionState {
    /// Not published (Tor off, or connected for fetching only).
    Off,
    /// Publishing, or recovering from a failure.
    Starting,
    /// Reachable by others.
    Reachable,
    /// Failed for good; restarting Tor may help.
    Broken,
}

/// The `.onion` address of an onion service identity (rend-spec-v3 §6).
pub fn onion_address(key: &[u8; 32]) -> Option<String> {
    let public = ed25519::PublicKey::from_bytes(key).ok()?;
    let id: HsId = HsIdKey::from(public).id();
    Some(id.display_unredacted().to_string())
}

pub(crate) struct Tor {
    client: Arc<TorClient<PreferredRuntime>>,
    onion: Option<Onion>,
}

struct Onion {
    service: Arc<RunningOnionService>,
    id: [u8; 32],
    accept: tokio::task::JoinHandle<()>,
}

impl Drop for Onion {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

impl Tor {
    /// Bootstraps a Tor client (directory and guards in `state` and `cache`).
    pub(crate) async fn start(state: &Path, cache: &Path) -> Result<Self, String> {
        let mut b = TorClientConfigBuilder::from_directories(state, cache);
        b.storage()
            .keystore()
            .primary()
            .kind(ExplicitOrAuto::Explicit(ArtiKeystoreKind::Ephemeral));
        // The folders are inside the app's sandbox (iOS) or a test's temporary folder:
        // Arti's Unix permission checks have nothing to add there.
        b.storage().permissions().dangerously_trust_everyone();
        let config = b.build().map_err(|e| e.to_string())?;
        let client = TorClient::create_bootstrapped(config)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            onion: None,
        })
    }

    pub(crate) fn client(&self) -> Arc<TorClient<PreferredRuntime>> {
        self.client.clone()
    }

    pub(crate) fn onion_id(&self) -> Option<[u8; 32]> {
        self.onion.as_ref().map(|o| o.id)
    }

    pub(crate) fn state(&self) -> OnionState {
        let Some(o) = &self.onion else {
            return OnionState::Off;
        };
        // A service just launched reports Shutdown until its parts start: we hold a launched
        // service, so Shutdown means starting.
        match o.service.status().state() {
            State::Running | State::DegradedReachable => OnionState::Reachable,
            State::Broken => OnionState::Broken,
            _ => OnionState::Starting,
        }
    }

    /// Publishes the onion service of key `secret` (once) and serves `offers` on it.
    pub(crate) fn publish(
        &mut self,
        secret: [u8; 32],
        offers: Arc<Offers>,
    ) -> Result<[u8; 32], String> {
        if let Some(o) = &self.onion {
            return Ok(o.id);
        }
        let expanded = ed25519::ExpandedKeypair::from(&ed25519::Keypair::from_bytes(&secret));
        let id = expanded.public().to_bytes();
        let nickname = HsNickname::new("lichen".into()).map_err(|e| e.to_string())?;
        let config = OnionServiceConfigBuilder::default()
            .nickname(nickname)
            .build()
            .map_err(|e| e.to_string())?;
        let (service, requests) = self
            .client
            .launch_onion_service_with_hsid(config, HsIdKeypair::from(expanded))
            .map_err(|e| e.to_string())?
            .ok_or("onion service disabled")?;
        let accept = tokio::spawn(async move {
            let mut streams = std::pin::pin!(handle_rend_requests(requests));
            while let Some(req) = streams.next().await {
                let offers = offers.clone();
                tokio::spawn(async move {
                    let ours = matches!(
                        req.request(),
                        IncomingStreamRequest::Begin(b) if b.port() == ONION_PORT
                    );
                    if !ours {
                        let _ = req.reject(End::new_with_reason(EndReason::DONE)).await;
                        return;
                    }
                    if let Ok(stream) = req.accept(Connected::new_empty()).await {
                        let (mut r, mut w) = tokio::io::split(stream);
                        let _ =
                            tokio::time::timeout(SERVE_LIMIT, serve(&offers, &mut r, &mut w)).await;
                    }
                });
            }
        });
        self.onion = Some(Onion {
            service,
            id,
            accept,
        });
        Ok(id)
    }
}

/// Fetches `hash` from the onion service of identity `key`.
pub(crate) async fn fetch(
    client: &TorClient<PreferredRuntime>,
    key: [u8; 32],
    hash: Hash,
    max: u64,
) -> Result<Vec<u8>, FetchError> {
    let host = onion_address(&key).ok_or_else(|| FetchError::Protocol("bad onion key".into()))?;
    let mut prefs = StreamPrefs::new();
    prefs.connect_to_onion_services(BoolOrAuto::Explicit(true));
    let stream = client
        .connect_with_prefs((host.as_str(), ONION_PORT), &prefs)
        .await
        .map_err(|e| FetchError::Unreachable(e.to_string()))?;
    let (mut r, mut w) = tokio::io::split(stream);
    write_request(&mut w, &hash).await?;
    read_response(&mut r, &hash, max).await
}

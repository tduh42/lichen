//! GIFs by id from their provider (protocol.md §6.3, ADR-0025): one HTTPS GET to a host and a
//! path the spec fixes, directly or through Tor. Nothing else on the web is reachable from
//! here: the address is built from the provider and an id of letters and digits, never taken
//! from a message.

use std::sync::Arc;

use http_body_util::{BodyExt, Empty, Limited};
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::TlsConnector;

use crate::FetchError;

/// Largest GIF accepted.
pub const GIF_MAX: usize = 8 << 20;
/// GIPHY's media host (§6.3).
pub const GIPHY_HOST: &str = "media.giphy.com";

/// The paths of GIPHY GIF `id` (§6.3): animated WebP 200 pixels wide, then the GIF itself.
/// None unless `id` is 1..=32 letters and digits.
pub fn giphy_paths(id: &str) -> Option<[String; 2]> {
    ((1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| {
        [
            format!("/media/{id}/200w.webp"),
            format!("/media/{id}/200w.gif"),
        ]
    })
}

fn tls() -> Result<TlsConnector, FetchError> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| FetchError::Protocol(e.to_string()))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(TlsConnector::from(Arc::new(config)))
}

/// GET `https://{host}{path}` over `stream` (a TCP connection or a Tor stream to `host`:443):
/// the body of a 200 answer, at most [`GIF_MAX`] bytes.
pub(crate) async fn get<S>(stream: S, host: &str, path: &str) -> Result<Vec<u8>, FetchError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let name =
        ServerName::try_from(host.to_owned()).map_err(|e| FetchError::Protocol(e.to_string()))?;
    let tls = tls()?
        .connect(name, stream)
        .await
        .map_err(|e| FetchError::Unreachable(e.to_string()))?;
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
        .await
        .map_err(|e| FetchError::Protocol(e.to_string()))?;
    tokio::spawn(conn);
    let request = hyper::Request::get(path)
        .header(hyper::header::HOST, host)
        // No User-Agent: a fixed "lichen" told GIPHY which app asked, over Tor too (review of
        // 2026-11-15, SR-36). HTTP/1.1 does not require one.
        .body(Empty::<Bytes>::new())
        .map_err(|e| FetchError::Protocol(e.to_string()))?;
    let response = sender
        .send_request(request)
        .await
        .map_err(|e| FetchError::Unreachable(e.to_string()))?;
    if response.status() != hyper::StatusCode::OK {
        return Err(FetchError::NotFound);
    }
    let body = Limited::new(response.into_body(), GIF_MAX)
        .collect()
        .await
        .map_err(|_| FetchError::TooLarge)?;
    Ok(body.to_bytes().to_vec())
}

//! Two bridges on this machine, without relays (§14.2).

use std::time::Duration;

use lc_bridge::{Bridge, FetchError, Network, derive_key, hash};

async fn pair() -> (Bridge, Bridge) {
    let a = Bridge::start(derive_key(&[1; 32], "lichen/1 iroh"), None, Network::Local)
        .await
        .unwrap();
    let b = Bridge::start(derive_key(&[2; 32], "lichen/1 iroh"), None, Network::Local)
        .await
        .unwrap();
    (a, b)
}

#[tokio::test]
async fn offer_and_fetch() {
    let (a, b) = pair().await;
    let photo: Vec<u8> = (0..1_800_000u32).map(|i| (i * 31 % 251) as u8).collect();
    let (h, size) = a.offer(photo.clone()).unwrap();
    assert_eq!((h, size), (hash(&photo), 1_800_000));
    let got = b.fetch_from(a.addr(), h, size).await.unwrap();
    assert_eq!(got, photo);

    // An unknown hash, a size larger than announced.
    assert_eq!(
        b.fetch_from(a.addr(), [7; 32], size).await,
        Err(FetchError::NotFound)
    );
    assert_eq!(
        b.fetch_from(a.addr(), h, size - 1).await,
        Err(FetchError::TooLarge)
    );
    a.shutdown().await;
    b.shutdown().await;
}

/// A dishonest provider that answers any hash with other bytes: the fetch is refused.
#[derive(Debug, Clone)]
struct Liar;

impl iroh::protocol::ProtocolHandler for Liar {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        let (mut send, mut recv) = connection.accept_bi().await?;
        let mut req = [0u8; 33];
        let _ = recv.read_exact(&mut req).await;
        let evil = b"not the photo you asked for";
        let mut head = [0u8; 9];
        head[1..].copy_from_slice(&(evil.len() as u64).to_le_bytes());
        let _ = send.write_all(&head).await;
        let _ = send.write_all(evil).await;
        let _ = send.finish();
        connection.closed().await;
        Ok(())
    }
}

#[tokio::test]
async fn substitution_is_caught() {
    let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
        .alpns(vec![lc_bridge::ALPN.to_vec()])
        .bind()
        .await
        .unwrap();
    let liar = iroh::protocol::Router::builder(endpoint)
        .accept(lc_bridge::ALPN, Liar)
        .spawn();
    let b = Bridge::start(derive_key(&[5; 32], "lichen/1 iroh"), None, Network::Local)
        .await
        .unwrap();
    let wanted = hash(b"the real photo");
    assert_eq!(
        b.fetch_from(liar.endpoint().addr(), wanted, 1_000).await,
        Err(FetchError::Mismatch)
    );
    let _ = liar.shutdown().await;
    b.shutdown().await;
}

#[tokio::test]
async fn offers_survive_a_restart() {
    let dir = std::env::temp_dir().join(format!("lc-bridge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let key = derive_key(&[3; 32], "lichen/1 iroh");
    let a = Bridge::start(key, Some(dir.clone()), Network::Local)
        .await
        .unwrap();
    let id = a.id();
    let (h, size) = a.offer(vec![5; 1000]).unwrap();
    a.shutdown().await;
    let a = Bridge::start(key, Some(dir.clone()), Network::Local)
        .await
        .unwrap();
    assert_eq!(a.id(), id, "the same key, the same endpoint id");
    let b = Bridge::start(derive_key(&[4; 32], "lichen/1 iroh"), None, Network::Local)
        .await
        .unwrap();
    assert_eq!(
        b.fetch_from(a.addr(), h, size).await.unwrap(),
        vec![5; 1000]
    );
    a.shutdown().await;
    b.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
    let _ = Duration::ZERO;
}

#[test]
fn keys_are_derived_apart() {
    let seed = [9; 32];
    assert_ne!(
        derive_key(&seed, "lichen/1 iroh"),
        derive_key(&seed, "lichen/1 onion")
    );
    assert_eq!(
        derive_key(&seed, "lichen/1 iroh"),
        derive_key(&seed, "lichen/1 iroh")
    );
}

/// Over n0's relays and address lookup, by endpoint id only, as phones do. Needs the Internet:
/// `cargo test -p lc-bridge -- --ignored`.
#[tokio::test]
#[ignore = "needs the Internet"]
async fn over_n0_by_endpoint_id() {
    let a = Bridge::start(
        derive_key(&[6; 32], "lichen/1 iroh"),
        None,
        Network::Default,
    )
    .await
    .unwrap();
    let b = Bridge::start(
        derive_key(&[7; 32], "lichen/1 iroh"),
        None,
        Network::Default,
    )
    .await
    .unwrap();
    assert!(a.online(Duration::from_secs(30)).await, "a reached a relay");
    assert!(b.online(Duration::from_secs(30)).await, "b reached a relay");
    let photo = vec![42u8; 300_000];
    let (h, size) = a.offer(photo.clone()).unwrap();
    let t = std::time::Instant::now();
    let got = b.fetch(a.id(), h, size).await.unwrap();
    eprintln!("300 kB over n0 by endpoint id in {:?}", t.elapsed());
    assert_eq!(got, photo);
    a.shutdown().await;
    b.shutdown().await;
}

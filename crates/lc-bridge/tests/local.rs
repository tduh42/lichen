//! Two bridges on this machine, without relays (§14.2); over n0 and over Tor when asked.

use std::time::Duration;

use lc_bridge::{Bridge, FetchError, Network, derive_key, hash, onion_address, transport};

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
    let got = b.fetch_from(a.addr().unwrap(), h, size).await.unwrap();
    assert_eq!(got, photo);

    // An unknown hash, a size larger than announced.
    assert_eq!(
        b.fetch_from(a.addr().unwrap(), [7; 32], size).await,
        Err(FetchError::NotFound)
    );
    assert_eq!(
        b.fetch_from(a.addr().unwrap(), h, size - 1).await,
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
    let id = a.iroh_id();
    let (h, size) = a.offer(vec![5; 1000]).unwrap();
    a.shutdown().await;
    let a = Bridge::start(key, Some(dir.clone()), Network::Local)
        .await
        .unwrap();
    assert_eq!(a.iroh_id(), id, "the same key, the same endpoint id");
    let b = Bridge::start(derive_key(&[4; 32], "lichen/1 iroh"), None, Network::Local)
        .await
        .unwrap();
    assert_eq!(
        b.fetch_from(a.addr().unwrap(), h, size).await.unwrap(),
        vec![5; 1000]
    );
    a.shutdown().await;
    b.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn transports_must_be_started() {
    let b = Bridge::new(None).unwrap();
    assert_eq!(b.iroh_id(), None);
    assert!(!b.online(Duration::from_millis(10)).await);
    assert!(matches!(
        b.fetch(transport::TOR, [1; 32], [2; 32], 10).await,
        Err(FetchError::Unreachable(_))
    ));
    assert!(
        b.fetch(transport::IROH, derive_key(&[1; 32], "x"), [2; 32], 10)
            .await
            .is_err()
    );
    assert!(matches!(
        b.fetch(9, [1; 32], [2; 32], 10).await,
        Err(FetchError::Protocol(_))
    ));
    assert_eq!(b.onion_id().await, None);
    assert!(b.publish_onion([3; 32]).await.is_err(), "Tor first");
}

#[test]
fn onion_addresses() {
    // A v3 address: 56 base32 characters, the last one carrying version 3.
    let key = ed25519_public(&derive_key(&[9; 32], "lichen/1 onion"));
    let address = onion_address(&key).unwrap();
    assert_eq!(address.len(), 56 + ".onion".len());
    assert!(address.ends_with("d.onion"), "{address}");
    assert!(
        address
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'.')
    );
}

/// The Ed25519 public key of a secret, as the onion service derives it.
fn ed25519_public(secret: &[u8; 32]) -> [u8; 32] {
    tor_llcrypto::pk::ed25519::Keypair::from_bytes(secret)
        .verifying_key()
        .to_bytes()
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
    let got = b
        .fetch(transport::IROH, a.iroh_id().unwrap(), h, size)
        .await
        .unwrap();
    eprintln!("300 kB over n0 by endpoint id in {:?}", t.elapsed());
    assert_eq!(got, photo);
    a.shutdown().await;
    b.shutdown().await;
}

/// Over Tor: one bridge publishes its onion service, the other fetches from it. Needs the
/// Internet and a few minutes: `cargo test -p lc-bridge --release -- --ignored tor --nocapture`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the Internet and the Tor network"]
async fn over_tor_onion_service() {
    let root = std::env::temp_dir().join(format!("lc-bridge-tor-{}", std::process::id()));
    let a = Bridge::new(None).unwrap();
    let b = Bridge::new(None).unwrap();
    let t = std::time::Instant::now();
    let limit = Duration::from_secs(180);
    let dirs = ["a/state", "a/cache", "b/state", "b/cache"].map(|d| root.join(d));
    let (ra, rb) = tokio::join!(
        a.start_tor(&dirs[0], &dirs[1], limit),
        b.start_tor(&dirs[2], &dirs[3], limit),
    );
    ra.unwrap();
    rb.unwrap();
    eprintln!("both connected to Tor in {:?}", t.elapsed());

    let secret = derive_key(&[8; 32], "lichen/1 onion");
    let id = a.publish_onion(secret).await.unwrap();
    assert_eq!(
        id,
        ed25519_public(&secret),
        "the onion key derives from the seed"
    );
    assert_eq!(a.onion_id().await, Some(id));
    let t = std::time::Instant::now();
    assert!(
        a.onion_reachable(Duration::from_secs(300)).await,
        "published"
    );
    eprintln!(
        "{} reachable in {:?}",
        onion_address(&id).unwrap(),
        t.elapsed()
    );

    let photo: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 253) as u8).collect();
    let (h, size) = a.offer(photo.clone()).unwrap();
    let t = std::time::Instant::now();
    let got = b.fetch(transport::TOR, id, h, size).await.unwrap();
    eprintln!("300 kB over Tor in {:?}", t.elapsed());
    assert_eq!(got, photo);
    let t = std::time::Instant::now();
    assert_eq!(
        b.fetch(transport::TOR, id, [7; 32], size).await,
        Err(FetchError::NotFound)
    );
    eprintln!("a second request (unknown hash) in {:?}", t.elapsed());
    a.shutdown().await;
    b.shutdown().await;
    let _ = std::fs::remove_dir_all(&root);
}

/// Relay only: both ends without IP transports, everything through n0's relays.
#[tokio::test]
#[ignore = "needs the Internet"]
async fn over_n0_relay_only() {
    let a = Bridge::start(
        derive_key(&[10; 32], "lichen/1 iroh"),
        None,
        Network::RelayOnly,
    )
    .await
    .unwrap();
    let b = Bridge::start(
        derive_key(&[11; 32], "lichen/1 iroh"),
        None,
        Network::RelayOnly,
    )
    .await
    .unwrap();
    assert!(a.online(Duration::from_secs(30)).await, "a reached a relay");
    assert!(b.online(Duration::from_secs(30)).await, "b reached a relay");
    let ips = a.addr().unwrap().ip_addrs().count();
    assert_eq!(ips, 0, "no direct address published");
    let photo = vec![9u8; 300_000];
    let (h, size) = a.offer(photo.clone()).unwrap();
    let t = std::time::Instant::now();
    let got = b
        .fetch(transport::IROH, a.iroh_id().unwrap(), h, size)
        .await
        .unwrap();
    eprintln!("300 kB over n0, relay only, in {:?}", t.elapsed());
    assert_eq!(got, photo);
    a.shutdown().await;
    b.shutdown().await;
}

/// A GIPHY GIF by id (§6.3), directly then through Tor; an id that does not exist is not
/// found, and anything but letters and digits is refused before any connection. Needs the
/// Internet: `cargo test -p lc-bridge --release -- --ignored gif --nocapture`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the Internet and the Tor network"]
async fn gif_by_id() {
    let b = Bridge::new(None).unwrap();
    let t = std::time::Instant::now();
    let webp = b.gif("3o7abKhOpu0NwenH3O", false).await.unwrap();
    eprintln!("direct: {} bytes in {:?}", webp.len(), t.elapsed());
    assert_eq!(webp.get(..4), Some(&b"RIFF"[..]));
    assert_eq!(webp.get(8..12), Some(&b"WEBP"[..]));
    assert_eq!(
        b.gif("zzzznotagif000", false).await,
        Err(FetchError::NotFound)
    );
    assert_eq!(b.gif("../../x", false).await, Err(FetchError::NotFound));
    let root = std::env::temp_dir().join(format!("lc-bridge-gif-{}", std::process::id()));
    b.start_tor(
        &root.join("state"),
        &root.join("cache"),
        Duration::from_secs(180),
    )
    .await
    .unwrap();
    let t = std::time::Instant::now();
    let over_tor = b.gif("3o7abKhOpu0NwenH3O", true).await.unwrap();
    eprintln!("through Tor: {} bytes in {:?}", over_tor.len(), t.elapsed());
    assert_eq!(over_tor, webp);
    b.shutdown().await;
}

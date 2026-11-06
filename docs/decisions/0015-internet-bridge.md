# ADR-0015 — Internet bridge: announcements over LoRa, content over iroh (then Tor), our own blob protocol

- Status: accepted
- Date: 2026-11-06

## Context

The design moves media to the Internet whenever it is available: an encrypted announcement of about 70 bytes on LoRa pins the provider's key and the content's BLAKE3 hash; the recipient fetches over iroh (direct QUIC, NAT traversal, relay fallback) or Tor, verified against the hash. It names `iroh-blobs` for the transfer and asks whether Arti can host onion services.

## Decision

- **iroh 1.4** for connectivity (n0's relays and address lookup; a key per phone derived from the identity seed, distinct from the lichen identity).
- **Our own blob protocol** (`lichen/blob/1`, §14.2) instead of `iroh-blobs`: version 0.104 says it is not production quality yet (0.35 is, but only with iroh 0.35). Request a hash, receive length and bytes, verify BLAKE3: a few lines, the same over iroh and over a Tor stream. Streaming verification (bao) is not needed for media of a few megabytes, held in memory and checked before use.
- **ANNOUNCE** (§14.1) carries one key per transport, the hash, the size, a name and the `media_id` that links it to a LoRa preview.
- **Groups**: an announcement is an ordinary group message, so groups get media without flooding them.
- **Tor**: Arti 0.48 hosts onion services as a stable feature (`onion-service-service`); it comes in a second step (L7b), with the same blob protocol.
- The bridge lives in its own crate (`lc-bridge`, async, tokio), outside the IO-free core: the app runs it next to the core.

## Consequences

- Full-resolution photos and files reach contacts and groups as soon as both phones have Internet access; LoRa only carries the announcement (one frame).
- Synchronous mode only: the sender's app must be running when the recipient fetches. The asynchronous mode of the design (content left on storage with a key in the announcement) is later.
- The iroh relays see connections (encrypted) and endpoint ids, not contents nor lichen identities; a relay-only or Tor-only setting addresses users who do not want to expose their IP (relay-only with iroh, Tor in L7b).
- The app grows by the size of iroh (QUIC, TLS) and links SystemConfiguration (DNS and proxy settings).
- iOS build: libc 0.2.191 limits the routing-socket API (`RTAX_*`, `RTM_*`, `in6_ifreq`) to macOS, which breaks iroh's `netwatch` 0.19 and `netdev` 0.46 for iOS. `lc-bridge` holds libc at `=0.2.189` (iOS target dependency, for the resolver) until they follow; `netdev` 0.47 already has.

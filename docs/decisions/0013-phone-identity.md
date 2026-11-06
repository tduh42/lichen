# ADR-0013 — The identity is a key generated on the phone; the node key only routes

- Status: accepted (Tanguy, 2026-11-06); supersedes ADR-0004
- Date: 2026-11-06

## Context

Until now a lichen identity was the MeshCore node's key, exported from the companion (`EXPORT_PRIVATE_KEY`, ADR-0004). Tanguy's design (`docs/design/`, Identity and Add-a-contact screens) generates the key on the phone instead, with a 24-word recovery phrase, a key fingerprint, QR codes and safety numbers. He chose it after weighing the costs.

## Decision

- **Identity**: a 32-byte seed drawn on the phone, kept in the Keychain or Keystore, expanded into an Ed25519 key pair the MeshCore way (the same arithmetic as before: X25519 from the Ed25519 key for ECDH, Ed25519 for group signatures). The seed never leaves the phone; its backup is the 24-word BIP-39 phrase of the seed.
- **The node key only routes**: frame headers keep the node hashes (`dst`, `src`), MeshCore paths and contacts are unchanged, and the companion is never asked for its private key.
- **A contact** is a node key (where to send) plus, once known, an identity key (who) and a verified flag.
- **First contact** (the peer's identity is unknown): a three-message **introduction**, INTRO → INTRO_ACK → INTRO_FIN, on the Noise XX pattern: each side's identity travels encrypted under ephemeral keys (a listener cannot link an identity across radios) and each side proves it holds its identity key (`es`, `se`). The learned identity is **trusted on first use**, shown "to verify" until the users compare safety numbers or scan a QR code.
- **Known identities** keep the two-message HELLO / HELLO_ACK of §4, with the static secret computed between identity keys.
- **Verification**: a 24-hex fingerprint, a 30-digit safety number common to a pair, and a QR card (identity, node key, name) scanned in person.
- **Path discovery** without a known identity uses an unauthenticated probe (keys from public node keys only); with one, the probe is authenticated as before.

## Consequences

- A lost, stolen or borrowed radio exposes nothing; changing radio keeps the identity, the contacts and their verification.
- No dependency on companion firmwares that refuse to export their key.
- First contact costs three more frames (once per pair of identities, then stored).
- An identity that changes (reinstalled app, or an attack) is reported to the app and drops the contact back to "to verify".
- Losing the phone without the 24 words loses the identity; using lichen from two devices would need multi-device support (later).
- Sessions and groups made before this change are not compatible (only demo and test data existed).
- Rejected: identities in clear in the first message (links a person across radios); keeping the node key (the radio would hold the end-to-end key).

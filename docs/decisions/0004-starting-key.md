# ADR-0004 — Starting key: the node's key, exported once

- Status: superseded by ADR-0013 (2026-11-06): the identity is now a key generated on the phone
- Date: 2026-11-06

## Context

The original design starts from the static X25519 secret that MeshCore derives between two contacts. The companion never hands it to the app. It does hand over its private key through `EXPORT_PRIVATE_KEY` (23), as 64 "expanded" orlp bytes.

## Decision

- On first pairing, the app exports the node's private key, keeps it in the Keychain (iOS) or Keystore (Android), and computes the secret with each contact itself, exactly like MeshCore (A:§2). Contacts' public keys come from `GET_CONTACTS`.
- If the export is refused (`DISABLED` or an error), the app creates its own Ed25519 identity and exchanges it in a first message. This fallback will be specified if we meet it in the field.

## Consequences

- The node's key also lives on the phone. A compromised phone also compromises the user's MeshCore identity, which was already the case for their messages.
- No configuration: existing MeshCore contacts become lichen contacts.
- To check on first connection, on the M5 and the Heltec.

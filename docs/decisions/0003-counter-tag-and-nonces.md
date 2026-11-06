# ADR-0003 — Counter `tag` per direction, nonce from the header, sessions not persisted

- Status: accepted
- Date: 2026-11-06

## Context

The original design uses a random 16-bit `tag` and a nonce `src ‖ tag ‖ index ‖ attempt ‖ session counter`, without transmitting the counter. Two problems:
- the receiver cannot track a counter that is not transmitted when datagrams are lost;
- with a random 16-bit `tag`, two datagrams of the same session have even odds of sharing a `tag` after about 300 sends. That is nonce reuse, which enables forgery with Poly1305.

## Decision

- One session key **per direction** (HKDF), with forward secrecy: ephemeral X25519 at every handshake.
- `tag` = strictly increasing counter per session and direction. Renewal is mandatory before `0xFF00`.
- Nonce = `header[2..7] ‖ 0×7` (`tag`, `kind|index`, `total`, `attempt`). `attempt` is in the nonce, not only in the AAD.
- Replay window of 64 `tag`s at the receiver.
- Sessions are **not** persisted: after a restart, new handshake. A counter already used can never be resumed.
- A proptest enumerates the nonces emitted by lossy transfers and checks that none repeats.

## Consequences

- One handshake (2 frames, about 1 s of airtime) after every app restart.
- The `tag` reveals the number of datagrams in the session, metadata already visible from the transmission rhythm.
- Rejected: random 32-bit `tag` (longer header, still probabilistic); counter sent separately (extra bytes in every fragment).

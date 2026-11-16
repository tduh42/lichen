# ADR-0009 — Groups: sender keys, signed flooded messages

- Status: accepted
- Date: 2026-11-06

## Context

The original design gives each member a sender key so that "no member can impersonate another". Sender keys are symmetric: every member holds every other member's key, so any member can produce a message that decrypts under someone else's key. Sender authentication within the group needs something the others do not hold. Group messages are flooded, read through `LOG_RX_DATA`, and must fit in a frame that a stock node rejects.

## Decision

- Every group message is **signed** (Ed25519, 64 bytes) with the sender's MeshCore identity, inside the ciphertext, over the group id, key epoch, counter and content. Proposed on 2026-11-06, no objection.
- Sender keys per member and per epoch, distributed over pairwise sessions; new keys when a member leaves; current and previous key kept.
- Flooded envelope on channel hash `0x7C` (shared with the probe, ADR-0008) with a type byte, a pad byte when needed so stock nodes reject it, at most 159 bytes so it fits `LOG_RX_DATA` after 8 hops, 1 or 2 fragments per message.
- Our own sender key and counter are never stored: a restart draws a new key, so no counter repeats. The others' keys are stored with the group (ADR-0010).
- The creator is the admin and the only one who changes membership.

## Consequences

- A TEXT carries 70 bytes in one frame, 209 in two. Each frame costs about 0.8 to 1 s of airtime per repeater at SF8 / 62.5 kHz.
- Creating a group of `n` costs `n(n−1)` pairwise datagrams (sender keys), plus path discovery and handshakes between members who had no session.
- Rejected: one shared group key (any member can impersonate any other, and the departed member keeps reading until everyone rotates); per-recipient MAC vectors (smaller for 2–5 members, but they grow with the group and reveal the member count); MLS (too heavy for LoRa).

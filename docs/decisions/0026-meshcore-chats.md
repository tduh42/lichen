# ADR-0026 — Plain text with MeshCore users, in chats of their own

- Status: accepted
- Date: 2026-11-14

## Context

lichen runs on stock MeshCore radios, among people who use the stock MeshCore apps: a test Heltec lists 163 nodes in range. Until now lichen could not talk to any of them. It never sent `SYNC_NEXT_MESSAGE`, so plain direct messages and channel posts that the companion decrypted for us piled up in its offline queue until the oldest were dropped. `docs/spec/design.md` listed compatibility with official clients as a non-goal for lichen's own features, which stays true. lichen users should be able to chat with MeshCore users.

## Decision

- **Plain MeshCore text, driven through the companion** (protocol §15): direct messages to any node the radio knows and posts on its channels (Public, hashtag channels, private channels by secret). The companion encrypts, decrypts, routes and acknowledges; lichen sends the commands stock clients send, with the same retries (three attempts, the last one flooded after `RESET_PATH`).
- **Separate conversations, never mixed.** A MeshCore chat is its own kind of conversation, even with someone who also uses lichen. It carries text only. A lichen chat never falls back to plain text, so a user's choice of end-to-end encryption is never silently downgraded.
- **Honest labels.** The chat says it is not lichen-encrypted and what that means: MeshCore's direct messages are AES-128 between the two radios, with a 2-byte MAC, no forward secrecy, and readable by the companion; a channel is readable by everyone who holds its secret, Public by anyone in range.
- **Channels live on the radio**, as for stock clients, in its slots. lichen identifies a channel by a hash of its secret, so history follows the channel whatever slot it is in, and keeps a copy of the list so history shows while the radio is away. A channel joined from lichen starts muted: Public in a city is busy.
- **Our name** on the mesh is the node's advertised name, which lichen can now set.
- **Airtime** of plain messages counts in the hourly budget like lichen's, and its estimate is shown before sending.
- **Out of scope for now**: room servers (signed posts, logins), repeater administration, sharing private channels by QR.

## Consequences

- The companion's queue is drained at every connection, so plain messages also reach a phone that was away, unlike lichen's pushes (ADR-0006).
- The store gains two chat kinds (version 10) and a channel table. `lc-meshcore` gains six commands and three responses, checked against the vectors copied from oxymesh, which were cross-checked with meshcore.js.
- A plain message is as private as MeshCore makes it, and the app says so. lichen adds nothing to it and takes nothing away.
- Rejected: merging a person's plain and lichen messages into one chat (the encryption of each bubble would have to be told apart, and a reply could leak to the weaker channel); reading channels from `LOG_RX_DATA` ourselves (it would need MeshCore's channel cipher in lichen and duplicate what the companion does).

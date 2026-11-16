# ADR-0040 — A digest after each burst; recovery copies kept across restarts

- Status: accepted
- Date: 2026-11-16

## Context

Members recover lost group messages from each other (§10.6, ADR-0024), but only once they see a gap: a later message from the same author. ADR-0024 left two holes. "The last message of a burst is not recovered until its author sends another message": the end of a conversation, often the post that matters ("we turn back at the col"), is the one that stays lost. And "envelopes are not kept across a restart": a phone that restarts cannot answer for what it heard or sent before.

## Decision

- **GROUP_DIGEST (0x19), a group message from the core.** `key_epoch (1) ‖ counter u32`: the author's last message. Two minutes after a member's last message, if it sent no other one, it floods one digest, signed and copied as any group message (§10.3), with a counter of the core's own range: it opens no gap itself.
- **A digest reveals what a later message would.** A receiver takes the message counters it lacks under that key, after the highest it received (or from the key's next counter) up to the digest's counter included, inside the replay window, as gaps, and asks for them as usual (§10.6). Only the author can sign a digest: no member can make the others ask for messages that do not exist.
- **One per burst.** Every message resets the two minutes: someone writing every minute sends one digest when they stop.
- **Recovery copies are stored with the group.** The kept envelopes (64 per group, an hour) go into the group's stored snapshot with the Unix time they were kept and the step they were sent at (ADR-0039), and come back after a restart for what is left of their hour. They are ciphertext under sender keys the members hold anyway; our own key still never is stored, and our own messages kept that way are only useful to the others.

## Consequences

- A lost last post comes back about three minutes after it was sent instead of never (or at its author's next message): the measurements in `docs/measurements.md` (2026-11-16).
- A digest is a 70-byte signed plaintext, one fragment of 97 bytes: 2 × 0.67 s of the author's airtime per burst at SF8 / 62.5 kHz / CR 4/5 with two copies, plus each repeater's relays, about what a short text costs. In the measurement below, where each member posts every five minutes, every post ends a burst: the worst case, one digest per post.
- The snapshot grows by up to 64 messages of one or two payloads (about 20 KB at the most) and is written with every message received, as the replay windows already were.
- Rejected: an unsigned digest (64 of its 70 bytes saved, but every member holds the author's sender key and could make all the others send requests for nothing); receipts from every member (the storm ADR-0006 avoided); periodic digests while nothing happens (a constant cost for silent groups); keeping envelopes in memory only (ADR-0024's choice, which a restart defeats).

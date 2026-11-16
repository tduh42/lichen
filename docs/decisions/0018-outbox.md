# ADR-0018 — Outbox: message intents that wait for the radio and survive a restart

- Status: accepted
- Date: 2026-11-09

## Context

Off-grid chat is the product (the product brief of 2026-11-08, not published). Until now a message could only be sent while the radio was connected; a radio that dropped mid-send kept the core transmitting into nothing until the message failed; and a restart marked every unfinished message failed (`Interrupted`, ADR-0010) for the user to resend as a new message. Group texts were recorded as sent while their frames still waited in memory. Sessions, sender keys and nonces must never be persisted (ADR-0003), so the ciphertext of a pending message cannot be kept either.

## Decision

- **A message is an intent**: conversation, plaintext (the sender's timestamp fixed when the user sends), expiry. Unfinished TEXT intents are kept in the history database and submitted again at start; they are encrypted afresh every time (protocol.md §6.1).
- **The core pauses while the companion is disconnected** (`Core::disconnect`): no frame, no timer, no retry counter. In-flight transfers resume on reconnection; another node drops sessions and paths but keeps the queue.
- **Queued and sending are distinct states**: a message is cancellable until its first frame leaves, never after (it may have arrived). A group message is "sent to the group" when the first copy of its fragments has left, not when it is queued.
- **Expiry after 24 hours**, shown on the waiting message: long enough for a radio left off overnight, short enough that a stale text does not appear the next week.
- **At least once, deduplicated by the receiver**: resends use the stored plaintext, and a receiver that keeps a history drops a TEXT or VOICE equal to one it stored from the same sender and conversation. Nothing changes on the air.
- **Retry is by hand and keeps the message**: the same row and plaintext, a new expiry.
- **Texts only**: voice notes and images still need the radio and are still refused when over budget (§13.4); unfinished media at a restart stay `Interrupted`.

## Consequences

- A message can arrive hours after it was written; receivers show the sender's time when it is well before the reception.
- Two genuine identical texts sent within the same second are merged on the receiving side (already true within an hour, §6).
- The receiver's duplicate check is a lookup on the conversation's history for every TEXT or VOICE received.
- Rejected: persisting ciphertext or sessions (ADR-0003); message identifiers on the air for exactly-once delivery (they come with replies and reactions, which need message references anyway); retrying automatically when the peer is unreachable (shared airtime spent for nothing); queuing media offline (§13.4).

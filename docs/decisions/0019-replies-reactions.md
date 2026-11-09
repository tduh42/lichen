# ADR-0019 — Replies and reactions: 8-byte message references computed on both sides

- Status: accepted
- Date: 2026-11-09

## Context

Off-grid chat is the product (`docs/messaging-product-brief-2026-11-08.md` §2): people want to answer one message in a busy group, or acknowledge it with a 👍. Both need a way to name a message that every phone agrees on, survives resends and restarts, and binds the message's author; local row ids differ per phone. Every byte costs shared airtime, and a group message already carries a 64-byte signature and goes out twice (§10.3).

## Decision

- **A message reference is computed, not sent with the message**: `SHA-256("lichen/1 ref" ‖ author identity ‖ plaintext)[0..8]`. Sender and receivers know both inputs, so no message grows; a resend (§6.1) keeps its reference; the author is bound into it.
- **REPLY (0x08)** is a text plus the reference, without an excerpt; the receiver shows the original from its history, or "a message not on this phone".
- **REACTION (0x09)** is a timestamp, a reference and an emoji (empty removes it). The latest by timestamp holds per sender and target, so repeats and reordering converge; a reaction may arrive before its target and waits for it. It is state, not a message of the history.
- Both join the outbox (§6.1): replies as texts, the sender's own reactions until delivered or sent.
- The apps offer six common reactions and show the cost before sending, a group one included.

## Consequences

- A reference is 8 bytes on every reply or reaction; 64 bits keep collisions negligible within a conversation.
- A phone that lacks the original shows the reply without its context; carrying an excerpt would cost up to dozens of bytes on every reply.
- In a group, a reaction costs about one flood of a short text (signature and two copies): measured in `docs/measurements.md`.
- Older clients show "unsupported content"; the apps are pre-release, so there is no version negotiation.
- Rejected: local row ids (they differ per phone); an id carried by every message (bytes on every message, for the few that get answers); reactions as ordinary visible messages (noise, and no change or removal); counting reactions at the original's sender and broadcasting totals (extra traffic, a single point of truth that may be offline).

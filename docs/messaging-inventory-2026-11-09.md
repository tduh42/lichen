# Messaging: what exists, what the brief asks — 2026-11-09

The product brief (`docs/messaging-product-brief-2026-11-08.md`) makes off-grid chat the main product. This is where both apps stand against its seven proposals, after the audit fixes (M01–M15, up to f08c2ac). "Both" means iOS and Android alike.

| # | Proposal | Exists | Missing |
|---|---|---|---|
| 1 | Dependable outbox | Drafts per chat, across restarts (M07). Statuses sending / delivered / failed with a reason; failed texts and voice notes can be sent again. | Messages queued in the core are **lost at a restart**: the store marks them failed ("the app stopped"). No "waiting for the radio" state: a message typed offline is refused or fails. No cancel, no expiry. |
| 2 | Replies and reactions | — | Everything, and it is protocol: a stable message reference (sender + per-sender id), reply and reaction content types, their cost on air, deduplication. |
| 3 | Chat essentials | History in pages (M06), new-message pill and reading position (M07), day separators, message times, swipe to delete, contact nicknames. | Copy / select text; an unread divider; search in messages; pin and mute per chat. |
| 4 | Complete groups | Create, invite, leave, close, remove with key rotation (M08, ADR-0017); verification badges; group cost before sending (M15). | An invitation by QR in person; group pictures (later, never flooded). |
| 5 | Voice notes | Record (bound to its chat, M02), cost so far, three quality modes, playback. | Preview before sending, discard after stopping, seeking; the quality choice explained as "voice quality vs time on air". |
| 6 | Photos | LoRa preview, the full thumbnail on request, the original over iroh or Tor, verified by hash. | One message that improves (preview → original) is mostly there; retry / cancel of an original's download and a clear "offered / waiting / unavailable" state are partial. |
| 7 | Contact exchange | QR code and scan, safety numbers, identity changes handled, nicknames. | Polish only (the brief: improve the current flows, no second system). |

## Proposed order

1. **A dependable outbox** (protocol-free in the main): messages accepted while the radio is away, persisted as intents (never sessions, keys or nonces), sent when it comes back, with a visible waiting reason, cancel before transmission and an expiry. Needs a spec section for the restart rules (what is resent, duplicates on the receiving side).
2. **Chat essentials** (app-only): copy / select, the unread divider, pin and mute, search in the local history.
3. **Replies and reactions** (protocol): spec and vectors first (message references, content types, airtime), then both apps.
4. Voice and photo polish, a group invitation by QR.

Each starts with its own plan; the protocol ones with the spec and an ADR.

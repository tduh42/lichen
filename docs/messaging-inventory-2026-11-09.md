# Messaging: what exists, what the brief asks — 2026-11-09

The product brief (`docs/messaging-product-brief-2026-11-08.md`) makes off-grid chat the main product. This is where both apps stand against its seven proposals, after the audit fixes (M01–M15, up to f08c2ac). "Both" means iOS and Android alike.

| # | Proposal | Exists | Missing |
|---|---|---|---|
| 1 | Dependable outbox | **Done** (§6.1, ADR-0018): texts are accepted without the radio and survive a restart; the core pauses while the radio is away; waiting reasons (radio, earlier messages, path, session, airtime), cancel before the first frame, 24 h expiry, retry as the same message, receiver deduplication; group texts count as sent once they left. Drafts per chat (M07). | Voice notes and photos still need the radio (by design, §13.4). |
| 2 | Replies and reactions | **Done** (§6.2, ADR-0019): 8-byte message references computed on both sides; REPLY with a quote of the original (tap to go to it, "a message not on this phone" otherwise); REACTION with six choices, last-writer-wins, before-target arrival, kept until delivered across restarts; cost shown in the picker (measured: a reaction costs about a short text). | Reactions on photos from before replies existed (no reference). |
| 3 | Chat essentials | **Done**: copy and select text (iOS: links, addresses and phone numbers tappable); an unread divider the chat opens on; search in every conversation, case and accents aside, opening the chat on the message; pin and mute per chat (1 h, 8 h, until turned back on). With history in pages (M06), the new-message pill (M07), day separators, message times, swipe to delete, contact nicknames. **SOS is never muted**: muting a chat silences its ordinary messages only; an SOS from a member of a muted group still raises the alert and its time-sensitive notification (critical alerts need an Apple entitlement lichen does not have). | "Show on the map" for coordinates in a text (needs a map focus API). |
| 4 | Complete groups | Create, invite, leave, close, remove with key rotation (M08, ADR-0017); verification badges; group cost before sending (M15). **Invitation by QR code done** (§10.5, ADR-0020): a 10-minute code with no secret in it (the admin's card and a token), any number of people, revoked when closed; the joiner adds the admin verified and sends GROUP_JOIN; the admin's app admits a valid token and shows who joined. A group can start with the admin alone. Measured: two joiners admitted after 72 s on the fake network, 21 s next to the admin in the demo. **Reliable and readable groups done** (ADR-0021): the admin relays sender keys on additions (no session between strangers; two joiners hold every key after 130 s instead of 489 s, median of 10); membership lines in the conversation ("Léa joined", "Camille removed Bob", "You renamed the group"), with the cause of a departure in GROUP_STATE; renaming. A crossed session renewal no longer costs about 2 minutes (§7.1, §7.3). | Group pictures (later, never flooded). Rotating keys on additions (open question, ADR-0017). **Lost group messages are recovered** (§10.6, ADR-0024, see below). |
| 5 | Voice notes | **Done**: stopping a recording opens a review (listen to what will be sent, Codec2 included; switch quality with its cost; discard or send); a progress bar, elapsed / total and seeking on the waveform. | — |
| 6 | Photos | **Done**: tapping a photo opens it full screen (zoom, pan, share the best version on the phone); one clear state per photo (preview, full requested, original offered / downloading with cancel / failed with retry / unavailable without iroh or Tor). | — |
| 7 | Contact exchange | QR code and scan, safety numbers, identity changes handled, nicknames. | Polish only (the brief: improve the current flows, no second system). |

## Proposed order

1. ~~**A dependable outbox**~~ (done: §6.1, ADR-0018) (protocol-free in the main): messages accepted while the radio is away, persisted as intents (never sessions, keys or nonces), sent when it comes back, with a visible waiting reason, cancel before transmission and an expiry. Needs a spec section for the restart rules (what is resent, duplicates on the receiving side).
2. ~~**Chat essentials**~~ (done) (app-only): copy / select, the unread divider, pin and mute, search in the local history.
3. ~~**Replies and reactions**~~ (done: §6.2, ADR-0019) (protocol): spec and vectors first (message references, content types, airtime), then both apps.
4. ~~**Voice and photo polish, a group invitation by QR**~~ (done: §10.5, ADR-0020), with a full-screen photo viewer.

Each starts with its own plan; the protocol ones with the spec and an ADR.

## Finding (2026-11-09): group messages lost to collisions — addressed by ADR-0024

Group messages are flooded twice and never acknowledged (§10.3). When a unicast transfer crosses the same repeater (a photo, a voice note), it keeps that repeater busy about half the time, and nodes behind it cannot hear each other: both copies of a fragment can collide there, and a two-fragment message then has roughly even odds of being lost. In the demo app, Julien's first post ("Leaving the col…") was lost that way while Camille's photo went to the app; the members only notice the silence. A receiver sees a gap in a member's counters once a later message (or position) from that member arrives: asking the sender, or the admin, for the missing counter would recover it. That needs a spec section and an ADR (request format, who answers, how long messages are kept, airtime), and is the next candidate for the messaging work.

**Done** (§10.6, ADR-0024): members keep the last hour of message envelopes in memory; a member that sees a gap among a member's message counters asks the author or the admin 20–40 s later, and the original signed envelopes come back through the normal receiver, marked "arrived late". Positions and search packets moved to their own counter range, so a gap is always a lost message. A member asked for a message it lost too hands it on once it recovers it. A member's first messages are recoverable too (gaps count from the key's next counter). On the fake network with 10 % loss per link, delivery goes from 96.0 % to 100 % for 80 % more frames; at 20 %, from 87.5 % to 93.1 % for eleven times the frames (`docs/measurements.md`). Still lost: the last message of a burst until its author sends again.

## Since then (2026-11-15)

Both apps, with the spec and an ADR for each protocol change:

| Feature | Where |
|---|---|
| Stickers (animated emoji every phone ships) and GIFs by GIPHY id, one packet each | §6.3, ADR-0025 |
| Plain chats with people on the stock MeshCore apps, and MeshCore channels such as Public, marked as not lichen-encrypted | §15, ADR-0026 |
| Edit and delete for everyone within 24 hours, delete for me, forward | §6.4, ADR-0027 |
| Disappearing messages: off, 5 min, 1 h, 8 h, 1 day, 1 week, shared with the conversation, MeshCore chats local only | §6.5, ADR-0030 |
| Group invitations that wait for an answer unless the admin is verified or the code was scanned; identities pinned at first use; names pinned at verification; notifications labelled "· MeshCore" | review of 2026-11-15 |

In progress: an optional app lock (Face ID / fingerprint, hidden notifications, SOS excepted).

Still missing, in the roadmap's order (M7, "Everyday messaging"): tappable links and "Show on the map" for coordinates; reply and mark as read from a notification; sharing into lichen from other apps; a media gallery per chat and storage per chat; block and archive; voice notes in groups; a pending reply kept with the draft.

# ADR-0030 — Disappearing messages

- Status: accepted
- Date: 2026-11-15

## Context

Disappearing messages were among the messenger conveniences Tanguy chose (2026-11-14). On a phone carried into the mountains, or lost, a conversation that cleans itself up is worth having. The question is whether a message carries its own lifetime, or whether a conversation has a setting both sides share.

## Decision

- **A setting of the conversation**, carried by its own datagram: **TIMER (0x0E)**, `ts u32 ‖ seconds u32` (protocol §6.5). It follows REACTION's state rules (ADR-0019): greatest `ts` wins, equal `ts` the greater `seconds`, repeats change nothing. Either side of a direct conversation, any member of a group, may change it; each change is noted in the history.
- **Counted from storage, on each phone.** A message goes `seconds` after this phone stored it, sent or received. No clock is compared between phones; a message that arrives late (off-grid, recovered, §10.6) still gets its full time to be read.
- **No per-message field.** TEXT, REPLY and the others keep their bytes and their airtime; the eight bytes of TIMER travel once per change.
- **Removal is a delete for oneself** (§6.4): the message, its media, its reactions and edits, with SQLite `secure_delete` on (SR-28). The phone sweeps when it saves its state and wakes for the next one due. Notices stay.
- **Our setting is kept until it goes**: stored as pending, submitted again at each start until it is delivered or sent to the group. In a group, TIMER is recoverable like an edit.
- **Choices**: off, 5 minutes, 1 hour, 8 hours, 1 day, 1 week; the protocol accepts up to 28 days.
- **MeshCore chats** keep a timer on this phone only.
- **Honesty.** Where the timer is set, the apps say that screenshots, copies and modified apps can still keep messages.

## Consequences

- One content type, store version 12 (a `timers` table, `messages.vanish_ms` set at insert), one event (`MessageRemoved`).
- An older client shows TIMER as unsupported content and keeps its messages.
- The two sides can disagree for a while: a phone that has not received the change keeps the old setting, and messages stored meanwhile follow it. They converge as soon as the TIMER arrives.
- Rejected:
  - a lifetime in every message (bytes on every message, and a sender's clock deciding what a receiver keeps);
  - counting from the sender's timestamp (a message that arrives late could vanish unread);
  - letting only a group's admin set it (members already share the group's other conveniences; a member who objects can leave);
  - a "read" start for the countdown (lichen sends no read receipts).

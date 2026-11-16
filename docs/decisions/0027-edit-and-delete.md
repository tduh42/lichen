# ADR-0027 — Editing and deleting a sent message, and forwarding

- Status: accepted
- Date: 2026-11-14

## Context

The conveniences of current messengers were asked for. Two of them change something already sent: correcting a message, and taking it back. lichen already names messages by a reference both sides compute (§6.2, ADR-0019), and reactions show how to change a message's appearance without touching the message: a small datagram naming its target, last writer wins, kept when it arrives before its target. A third one, forwarding, only reuses what the user already has.

## Decision

- **EDIT (0x0C)** carries the new text of a TEXT or REPLY; **DELETE (0x0D)** takes back a message of any type that has a reference (protocol §6.4). Both follow REACTION: a timestamp and the target's reference; the greatest timestamp wins among edits; a delete wins over every edit; either is kept until its target arrives.
- **Only the author**, checked twice: the author's identity is inside the reference, and the receiver compares the target's stored author with the sender before applying anything.
- **Within 24 hours** of the target's own timestamp. The window is the outbox's (§6.1): long enough for a phone that was off-grid, short enough that old conversations stay what they were.
- **The reference does not change.** Replies and reactions stay attached to an edited message, and a quote shows its current text. A deleted message keeps its place as "message deleted"; its content, media, reactions and edits leave the phone.
- **Delete for me** sends nothing and removes one message locally.
- **Honesty.** Before a delete for everyone, the apps say that it only reaches the phones that receive it, and that whoever read the message has read it.
- **Forwarding** sends a new message of the user's own with the same content, through the usual send paths and their cost sheets. No "forwarded" marker travels: like copying and pasting, the user is its author. A marker would need a new content field for every type and would add nothing that the forwarder could not write themselves.

## Consequences

- Two content types, outbox persistence like reactions, two store tables (store version 11), and group recovery keeps them.
- An older client shows them as unsupported content.
- Rejected: replacing the stored content on edit (a resent original would no longer be recognised as a duplicate, §6.1); editing media (a new photo is a new message); unlimited windows (an old conversation could be rewritten); deleting by row instead of reference (rows are local).

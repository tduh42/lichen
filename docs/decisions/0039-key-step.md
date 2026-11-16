# ADR-0039 — Sender keys step forward at every new group state

- Status: accepted
- Date: 2026-11-16

## Context

A state that only adds members kept every sender key (§10.2, ADR-0017): newcomers received the current keys, and could open anything recorded under them before they joined. The replay floor (§10.2, `next_counter`) keeps a newcomer's app from showing those messages, but the key opens them: someone who joins a group, or whose phone is later taken, reads what the group said before, if a recording of the air exists. Rotating every key on each addition closes this, at `n(n−1)` pairwise datagrams per addition, the cost ADR-0021 removed.

## Decision

- **A key belongs to a state epoch, its step.** `K(e) = HKDF-SHA256(salt = gid, ikm = K(e − 1), info = "lichen/1 key step" ‖ e u32)`. A member that takes a state with a newer epoch replaces every sender key it holds, its own included, with its derivation at that epoch. The key epoch byte, the counters and both replay windows (messages and the core's own traffic, §10.3) carry on unchanged: the derived key continues both counter ranges. Nothing travels: no airtime.
- **Keys travel with their step.** SENDER_KEY and each SENDER_KEYS entry say the step of the key they carry (4 bytes). A member sends its key at its own step; the admin relays and bundles keys at its own epoch, stepping them forward first, so a newcomer admitted by state `E` holds keys at `E` or later. HKDF is one-way: nothing sent at an earlier step opens under them.
- **Every new epoch steps, not only additions.** States waiting for a member merge (§10.2): a member that missed some cannot tell what each one changed, but it knows how many epochs it skipped. A rename or a handover (ADR-0038) steps the keys too; a departure still rotates them.
- **Members take a state at different times.** The envelope says nothing of the step. A receiver tries the key at its own step, then the next 8 (a sender that took the newer state first), then the steps since the one it held before its last change (frames sent before the step, arriving late or recovered, §10.6). Envelopes are signed: whatever key opens one, the signature decides.
- **Recovery by step.** A member hands a kept message only to members added at or before the step it was sent at (§10.6), in place of the time it was heard.

## Consequences

- A newcomer cannot open anything sent before the state that admitted it, whatever it recorded or is handed: the tests check it with the keys themselves, not only what the app shows.
- Receiving costs up to 17 key tries for an envelope that opens under no step (another member's under a shared sender byte, garbage), and one HKDF per step tried; nothing measurable on a phone.
- A member that answers GROUP_SYNC gives the keys at its current step: an envelope held from before the last state stays unreadable for a member that missed the keys across it.
- A key more than 64 steps away from the receiver's epoch is dropped (a bounded derivation).
- The format of SENDER_KEY and SENDER_KEYS changes: every phone of a group must run this version (no one else runs lichen yet).
- Rejected: rotating every key on additions (`n(n−1)` datagrams per addition); a step number in the envelope header (a byte per frame, and members disagree on it while a state travels); stepping on additions only (needs the history of every state).

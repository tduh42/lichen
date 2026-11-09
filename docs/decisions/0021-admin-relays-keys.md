# ADR-0021 — The admin relays sender keys on additions; membership notices in the conversation

- Status: accepted
- Date: 2026-11-09

## Context

Since ADR-0017, an addition made every member send its sender key to each newcomer, and each newcomer send its key to every member. People who join a group are often strangers to its members: each such pair needed its own path probe, introduction and session through the repeaters before a key could pass. On the fake network, two people joining a group of three by invitation (ADR-0020) were admitted after 72 s, but the group held every key only after 489 s (`docs/measurements.md`). Until then, newcomers could not read the others, and envelopes waiting for a key are kept 10 minutes and 16 at most (§10.3): a busy group could lose messages.

Separately, nothing in a conversation said who joined, left or was removed, and a group could not be renamed (the brief's "understandable membership changes"). GROUP_STATE did not say why someone was gone.

## Decision

- **The admin relays keys on additions** (§10.2). A joining member sends its first key to the admin only; on a state that only adds, the others send nothing. The admin sends each newcomer one SENDER_KEYS (0x14) with every current key it holds, its own included, and forwards a member's key to every other member when it is that member's first, or to the members it admitted in the last 10 minutes otherwise (a key in flight during an admission). Members and newcomers then need a session with the admin only.
- **Trust**: the admin already holds every member's key and decides who is a member, so relaying gives it nothing new. It cannot forge messages with a relayed key: every group message is signed with its sender's identity (§10.3). A wrong key from a faulty admin makes one member unreadable, which the admin could cause anyway by removing them. A receiver takes SENDER_KEYS only from the admin and never lets a relayed key replace a newer one.
- **What stays direct**: rotation after a departure, and the new key after a restart, go from each member to every other one. They must not depend on the admin being reachable (it may have gone home while the others still hike).
- **GROUP_STATE gets an optional `cause` byte** (§10.1) when members are gone: left, or removed by the admin. The admin can **rename** the group with a new epoch.
- **Notices are local** (§6, types 0xE0–0xEF): each phone records "Léa joined", "Camille removed Bob", "Camille renamed the group" in the conversation itself, from the states it receives and the changes it makes. They never travel, and the core drops any datagram or group message of that range from the air, so nobody can plant a notice on another phone.

## Consequences

- An addition costs the admin one bundle per newcomer (69 bytes per key) and one small forward per member, over sessions it already has; no session between strangers. The admin's airtime grows with the group, within its hourly budget for a group of 32.
- A member that restarts soon after a join sends its new key to the newcomer directly, which still needs a session between them. Relaying restart keys through the admin is a possible follow-up, at the cost of depending on the admin.
- The relay rules, the cause byte and the notices are new: a phone with the earlier rules in a mixed group would still send keys directly, which is wasted airtime, not a failure.
- Rejected: members asking the admin for a missing key when an envelope arrives (a request type and a round trip in the middle of a conversation); relaying every key, rotations included (a group would stop rotating when the admin is away); inferring "left" or "removed" without a cause byte (the members cannot tell them apart).

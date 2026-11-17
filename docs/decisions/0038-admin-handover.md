# ADR-0038 — Handing a group over to another admin

- Status: accepted
- Date: 2026-11-16

## Context

Only the admin changes a group (§10.1): it adds and removes members, renames the group, relays keys to newcomers (ADR-0021) and answers members that missed them (§10.7). ADR-0017 let the admin leave only by closing the group, and noted that "a group that should live on after its admin needs an admin transfer". On a hike the admin is whoever created the group, not whoever stays longest: when that phone goes home, the others keep a group nobody can manage any more, or lose it.

States travel pairwise and a member accepts one only from the admin it holds (the session's identity). A transfer sent the same way would reach the members the admin can reach before it leaves; a member away at that moment would then refuse every state of the new admin, and the old one, gone, could not tell it.

## Decision

- **A handover is a signed state.** GROUP_HANDOVER (0x18) carries a GROUP_STATE whose `admin` is the successor, signed with Ed25519 by the admin that hands over (`"lichen/1 handover" ‖ SHA-256(state)`: a state of 32 members is 2.4 KB). Its epoch is new; the members are the same ("Make admin": the former admin stays a member), or the same without the former admin (it leaves, `cause` 1).
- **Only the current admin can name the next one.** A member accepts a handover whose epoch is above the one it holds, that names another admin, and that verifies under the admin it holds; then it applies the state as any state (key step, rotation if the former admin left, notices) and takes the successor as admin. Who delivers it does not matter: the signature is the authority, epochs make a replay useless.
- **The new admin can hand it on.** Every member keeps the last 4 handovers it accepted, stored with the group. GROUP_HANDOVER holds 1 to 4 of them, oldest first, so a chain stays in order. A member that gets a state from a listed member naming itself admin with a newer epoch keeps it aside and asks that member with GROUP_SYNC (§10.7, once in 10 minutes); the admin answers a GROUP_SYNC whose epoch is older than a handover it keeps with the handovers first, then the state and the keys. A former admin answers GROUP_SYNC with the handovers alone. What was kept aside (the state, keys relayed by the new admin) is used once the handover arrives, so the order the three datagrams arrive in does not matter.
- **An admin who leaves hands over or closes the group.** The apps ask: Group info › Leave lists the members to hand over to, and "Close the group for everyone". Group info › a member › "Make admin" hands over without leaving. The former admin's open invitations stop at once.
- **Notices** (local, ADR-0021): kind 40, the former admin as actor and the successor as subject: "Camille made Julien admin", "You made Julien admin", "Camille made you admin". A former admin that leaves with the handover also gets "Camille left" on the others' phones.

## Consequences

- A group survives its creator. A handover costs what a state costs, plus 68 bytes, to each member, retried like a state (every hour for a day).
- A member that misses more than four handovers in a row cannot follow; it joins again with an invitation (§10.5). A GROUP_LEAVE that reaches the former admin after the handover is not acted on: the leaver is gone on its own side, and stays listed until the new admin removes it.
- A handover is a new state epoch: every key steps forward (ADR-0039); when the former admin leaves with it, the members rotate their keys as after any departure.
- Rejected: a handover as a plain state over the admin's session (members who miss it could never accept the new admin once the old one is gone); members electing an admin when the admin is silent (no single authority, groups would split on a partition); several admins at once (two writers of the member list, conflicting epochs).

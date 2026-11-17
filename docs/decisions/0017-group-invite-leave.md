# ADR-0017 — Inviting into a group, and leaving it

- Status: accepted (audit M08); amended by ADR-0038 (the admin hands over before leaving) and ADR-0039 (keys step on additions)
- Date: 2026-11-08

## Context

Groups (§10, ADR-0009) had a creator-admin who could create and remove, and nothing else: a member could not leave, the admin could not add anyone after the creation, and both apps showed disabled Invite and Leave buttons. Only the admin may send GROUP_STATE, and a member must not be able to rewrite the state.

## Decision

- **Invitation is an admin addition**: a new `epoch` listing the newcomers, sent to everyone, newcomers included. Newcomers must be contacts whose identity is known (scanned or introduced, ADR-0013). The keys follow the existing "only adds" rule (§10.2): members send their current key to the newcomers, who could read messages recorded under those keys before they joined (ADR-0014's trade-off, unchanged).
- **A member leaves with a request**: GROUP_LEAVE (type 0x12, pairwise, so the sender's identity is the session's) to the admin, and forgets the group at once (state and every sender key, its own included). The admin removes it as an ordinary removal: new `epoch`, every remaining member rotates its key.
- **The admin leaves by closing the group**: a last GROUP_STATE listing the admin alone, which every member already treats as "removed" (they forget the group); no admin transfer for now.
- The conversation already received stays on each phone; location shares with the group stop with it.

## Consequences

- A leave is a request: the admin must be reachable. The request is an internal message, retried with backoff (1 to 16 minutes, 5 tries); past that the leaver is still gone on its own side, but the others keep the old keys until the admin removes it by hand.
- Closing a group ends it for everyone; a group that should live on after its admin needs an admin transfer (not designed yet).
- Rejected: a member broadcasting its own departure to every member (no single authority on the member list, races with the admin's states).

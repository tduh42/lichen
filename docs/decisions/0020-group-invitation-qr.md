# ADR-0020 — Group invitation by QR code: a short-lived token, no secret in the code

- Status: accepted
- Date: 2026-11-09

## Context

An admin could only invite people already in their contacts (ADR-0017): at a trailhead, someone new needed two steps (exchange contact codes, then be invited). The brief (`docs/messaging-product-brief-2026-11-08.md` §4) asks for an in-person invitation by QR code, warning that a code or link must not carry group secrets, and that authentication, expiry and revocation, membership authorization and key rotation must be specified. A QR code can be photographed by anyone around.

## Decision

- **The invitation carries no secret**: the admin's public identity and node (as a contact card, §4.7), names to show, an expiry, and a random 16-byte **token** that only the admin's app maps to the group (protocol.md §10.5). No group key, no group id.
- **Short-lived and revocable**: 10 minutes, any number of joiners while it is shown; closing it or restarting the app revokes it. Tokens are never stored.
- **Joining is an authenticated request**: the joiner adds the admin as a verified contact (scanned in person) and sends GROUP_JOIN (0x13) with the token over a pairwise session. The admin's app admits a valid token automatically and adds the joiner as any invitation does (new epoch, keys exchanged pairwise, ADR-0017).
- **Authorization is possession of the token**, which only people who saw the admin's screen have; the admin sees each newcomer appear and can remove them.

## Consequences

- Someone who photographs the screen within 10 minutes can join; the admin sees who joined and removes anyone unexpected (new epoch, keys rotate). A shorter expiry or a manual approval would trade this for friction at the trailhead.
- The admin must be reachable when the joiner asks (the request is retried with backoff, like GROUP_LEAVE).
- The admin's node may never have heard the joiner's: the joiner sends a zero-hop advert first, and its path probes are anonymous (the admin cannot check a probe keyed with an identity it does not know yet). This showed a wider gap, now closed in §4.5: a contact card scanned one way, with no known path, never found one, because every probe was keyed. Only the first probe is keyed now; the next ones are anonymous.
- Newcomers read what was recorded under the current keys (the existing "only adds" rule, ADR-0017); rotating on additions remains an open question.
- Rejected: putting the group key or id in the code (anyone with a photo of it would read the group); single-use codes (one screen per person at a trailhead); asking the admin to approve each request on screen (slower, and the admin already chose to show the code).

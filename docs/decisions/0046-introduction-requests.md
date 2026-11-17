# ADR-0046 — An introduction we did not start is a request

- Status: accepted
- Date: 2026-11-17

## Context

An introduction (§4.6) names its sender's node by one hash byte, and nothing in it proves that node: the node key only routes (ADR-0013). Since SR-01, the first identity seen for a contact is pinned and a different one waits for the user. That left one gap (second read of 2026-11-17, N3): for a contact whose identity is not known yet (a radio heard in an advert, not met), the first introduction pinned whatever identity it carried. Anyone in radio range could introduce themselves under that node's hash byte before its owner ever wrote: their identity became the contact's, their messages showed in its conversation under its name, and when the real person wrote later, that showed as an "identity change" against the squatter.

## Decision

- **A request**: an identity learned for a contact that has none, from an introduction **we did not start** (the peer's INTRO, then its INTRO_FIN), is not pinned. It is held as a request from "someone using <node>'s radio", and the app is told (`Requested`).
- **An introduction we started only because a HELLO we could not open asked for it** (§4.6, the peer scanned our card) counts as theirs, unless the user wrote to that contact meanwhile: otherwise a HELLO forged under a node's byte, then an INTRO_ACK, would pin a squatter the same way.
- **The session still opens**, so what the requester sends arrives. Their messages are kept apart from the contact's conversation (`RequestMessage`), and nothing of theirs is used: no position, no group state or keys, no media request; nothing is sent to them. The apps show each request in Messages, with its messages as text lines (a photo or a voice message is named, not decoded, and nothing is fetched), and three choices:
  - **Accept**: the identity becomes the contact's, unverified, and the session goes on as the contact's; what they sent moves into the conversation, which opens.
  - **Block**: that identity only, with M7's block (ADR-0034): its introductions from any node are dropped. The node is not blocked: it is someone else's radio. Listed in Settings › Blocked as "Someone using <node>'s radio".
  - **Delete**: forgotten, with its messages; a later introduction is a new request.
- **Two identities for one node**: while the contact has no identity, a second identity introducing itself under the same node is a second request, not an identity change; the user chooses. One node holds one session at a time: the latest requester's. An earlier requester's HELLO is still answered (we hold its identity), and its session then takes over. Once the contact has an identity, the other requests on its node stay listed but hold no session: their HELLOs are not answered, and their introductions end without one (and without an identity-change warning: they already wait).
- **Not requests**: an introduction we started because the user wrote (we pin who answers, as before), a contact card (§4.7), an invitation scanned in person (§10.5). If the identity they pin was a request, the request is accepted with it.
- **Limits**: 4 requests per node and 32 in all; beyond, an introduction from yet another identity ends without a session or an event until the user deletes or blocks some. Refused rather than evicting an older one: eviction would let a flood of requests push out the real person's. The phone keeps a request's last 50 messages.
- **Kept across restarts**: the store holds each request (identity, node, first seen) and its messages as received (`requests`, `request_messages`, store version 15); requests are restored into the core at start, so a requester's HELLO is answered after a restart.
- **The demo's people are friends**: the demo network turns requests off (`set_request_consent(false)`), like its groups; a debug launch argument keeps them on to show the screen.

## Consequences

- A squatter in radio range gets a request card next to the real person's, not their name: the user compares what each wrote, or the safety numbers, before accepting one.
- Accepting a request is trust on first use again, by the user's choice; it stays "to verify" (§4.7).
- An introduction the user starts is still answered by whoever is in range with the right hash byte: writing to a radio met only by its advert pins whoever answers first. Scanning a card in person is what proves a person.
- A stranger's first message waits behind a tap, as in other messengers' message requests.
- What a request sends is acknowledged at the session layer even when it is not used: a group invitation, a position or a media request sent before the user accepts is dropped, and the sender is not told. The sender has to send it again once accepted (an admin adds the member again).
- Rejected:
  - keeping the request's messages in the core (lost at each restart, and the core stays IO-free);
  - one session per (node, identity) at once (every peer structure is keyed by node; two requesters on one node only alternate);
  - showing a request's photos and fetching its announcements before acceptance (a stranger would learn our address, and decoding is attack surface).

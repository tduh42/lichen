# ADR-0024 — Members recover lost group messages from each other

- Status: accepted
- Date: 2026-11-09

## Context

Group messages are flooded twice and never acknowledged (§10.3, ADR-0006). On the fake network and in the demo app, a unicast transfer crossing a shared repeater (a photo, a voice note) keeps it busy about half the time; nodes behind it cannot hear each other, both copies of a group fragment can collide there, and the message is lost for everyone behind it. Julien's first post in the demo was lost that way, and the members only notice the silence. The 2026-11-09 recheck asked for bounded recovery before calling groups reliable, without an acknowledgement storm, and noted that counter gaps also come from positions, which are not worth recovering.

## Decision

- **Every member keeps the recent envelopes it heard or sent**, as they were on the air (§10.6): 64 messages per group, 1 hour, in memory only; message types only, not positions or search packets.
- **Messages and the core's own traffic use separate counter ranges** under the same sender key (§10.3): messages from 0, positions and search packets from 2^31, each with its own replay window. A gap among message counters is then always a lost message; without this, every lost position (frequent while sharing, and during an SOS search) would trigger requests for nothing. Nonces stay unique: the ranges do not overlap, and a key is never reused across restarts (ADR-0003).
- **A receiver that sees a gap in a member's message counters asks for it**, 20–40 s later, over a pairwise session: the author if a session is open, otherwise the admin (which has a session with everyone, ADR-0021). One retry with the other, then it gives up.
- **Any member may answer with the original payloads**, but only with what it kept after the asker joined: recovery never hands a newcomer the history from before its admission.
- **The original payloads are self-authenticating.** They are encrypted under the author's sender key and signed by the author: the requester runs them through the normal group receiver, replay window and signature check included. A relay can neither forge nor replay one, and learns nothing it did not already hold.
- **Recovered messages are marked late**, shown where they arrive with their sending time: the history keeps its arrival order.

## Consequences

- A lost one-fragment message costs the requester's path about three fragments (request and answer) instead of nothing; nothing is flooded again. Members who lost the same message each ask, spread over 20 s: in a group of 32 behind one busy repeater, that is up to 31 small exchanges with the admin, within its hourly budget, and only after a real loss.
- The last message of a burst is not recovered until its author sends anything else (a position share is enough). Receipts or periodic digests would close that gap at a constant airtime cost, rejected for now.
- Envelopes are not kept across a restart: a member that restarted cannot answer for what came before.
- Older versions ignore GROUP_MISSING (an unknown internal type) and do not ask. The counter ranges are not compatible with them: an older receiver takes a position's counter (from 2^31) as the newest message counter and then rejects the member's messages as replays. Every phone of a group must run this version; at this stage that is Tanguy's own phones and the demo.
- Rejected: flooding the lost message again (every member and repeater pays for one member's loss); acknowledgements from every member (the storm ADR-0006 avoided); keeping envelopes on disk (they would outlive the sender key's lifetime on the phone, ADR-0010).

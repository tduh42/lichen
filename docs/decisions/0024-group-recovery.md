# ADR-0024 — Members recover lost group messages from each other

- Status: accepted; amended by ADR-0040 (a digest after each burst; kept envelopes stored with the group)
- Date: 2026-11-09

## Context

Group messages are flooded twice and never acknowledged (§10.3, ADR-0006). On the fake network and in the demo app, a unicast transfer crossing a shared repeater (a photo, a voice note) keeps it busy about half the time; nodes behind it cannot hear each other, both copies of a group fragment can collide there, and the message is lost for everyone behind it. Julien's first post in the demo was lost that way, and the members only notice the silence. The 2026-11-09 recheck asked for bounded recovery before calling groups reliable, without an acknowledgement storm, and noted that counter gaps also come from positions, which are not worth recovering.

## Decision

- **Every member keeps the recent envelopes it heard or sent**, as they were on the air (§10.6): 64 messages per group, 1 hour, in memory only; message types only, not positions or search packets.
- **Messages and the core's own traffic use separate counter ranges** under the same sender key (§10.3): messages from 0, positions and search packets from 2^31, each with its own replay window. A gap among message counters is then always a lost message; without this, every lost position (frequent while sharing, and during an SOS search) would trigger requests for nothing. Nonces stay unique: the ranges do not overlap, and a key is never reused across restarts (ADR-0003).
- **A receiver that sees a gap in a member's message counters asks for it**, 20–40 s later, over a pairwise session: the author if a session is open, otherwise the admin (which has a session with everyone, ADR-0021). One retry with the other, then it gives up.
- **Any member may answer with the original payloads**, but only with what it kept after the asker joined: recovery never hands a newcomer the history from before its admission.
- **A member's first messages are recoverable too.** Gaps before the first message received under a key count from the key's next counter: the owner sends it with the counter of its next message, and the admin relays it with the next counter it expects (one past the highest it received), so what follows was sent after the receiver held the key. The first post after a group is created is the one most exposed (key exchanges and the first transfers share the repeater); in the demo, Julien's first post was lost in two app runs out of three, and could not come back.
- **A member asked for a message it lost too hands it on once it recovers it** (within 2 minutes). Losses are correlated: a busy repeater drops a message for every member behind it, the admin included, and the admin then fetches it once from the author for everyone. Without this, each member waited 2 minutes for nothing, then opened its own session with the author: in the demo, Julien's lost post came back 3 minutes after his next message revealed it, instead of 70 seconds. An empty answer ("I have none") was tried first: every member then went to the author at once, and at 20 % loss per link it delivered 90.4 % instead of 95.7 % for 60 % more frames.
- **The original payloads are self-authenticating.** They are encrypted under the author's sender key and signed by the author: the requester runs them through the normal group receiver, replay window and signature check included. A relay can neither forge nor replay one, and learns nothing it did not already hold.
- **Recovered messages are marked late**, shown where they arrive with their sending time: the history keeps its arrival order.

## Consequences

- A lost one-fragment message costs the requester's path about three fragments (request and answer) instead of nothing; nothing is flooded again. Members who lost the same message each ask, spread over 20 s: in a group of 32 behind one busy repeater, that is up to 31 small exchanges with the admin, within its hourly budget, and only after a real loss.
- The last message of a burst is not recovered until its author sends another message (positions use their own counters and do not reveal it). Receipts or periodic digests would close that gap at a constant airtime cost, rejected for now.
- Envelopes are not kept across a restart: a member that restarted cannot answer for what came before.
- Older versions ignore GROUP_MISSING (an unknown internal type) and do not ask. The counter ranges are not compatible with them: an older receiver takes a position's counter (from 2^31) as the newest message counter and then rejects the member's messages as replays. Every phone of a group must run this version; at this stage that is the test phones and the demo.
- Rejected: flooding the lost message again (every member and repeater pays for one member's loss); acknowledgements from every member (the storm ADR-0006 avoided); keeping envelopes on disk (they would outlive the sender key's lifetime on the phone, ADR-0010).

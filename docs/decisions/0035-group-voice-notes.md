# ADR-0035 — Voice notes in groups

- Status: accepted
- Date: 2026-11-16

## Context

Voice notes go to contacts only (§13): media are never flooded, and a group message holds two fragments, 193 bytes of text. Yet in a group on the move ("we reach the col in twenty minutes"), a few seconds of voice is what people want, hands busy, gloves on. A 15-second note in 1600 is 3,000 bytes, 24 fragments: flooded twice by every member's radio and every repeater, it would hold a small mesh for a minute. A short note in Codec2 700C is a few hundred bytes.

## Decision

- **A VOICE may be flooded to a group** (§10.3) in **at most 5 fragments**: `content_type ‖ ts ‖ mode ‖ frames ‖ data ‖ signature` in 640 bytes, so `data` ≤ 568 bytes: 6.48 s in 700C, 3.76 s in 1200, 2.84 s in 1600, 1.88 s in 2400. The apps record at most **6 seconds in 700C** for a group (525 bytes, 5 fragments); the review offers the other modes only when they fit.
- **Why 5, from airtime.** On the EU narrow preset (SF8, 62.5 kHz) a full 156-byte envelope is 1.00 s at CR 4/5 and 1.49 s at CR 4/8 (§9). Every fragment is flooded twice (§10.3): a 6-second note costs its sender 9.6 s (14.3 s), about 3 % (4 %) of the 360 s it may transmit in an hour, and every repeater that hears it spends as much again. That is the price of about three of the longest texts. Seven fragments, the most the `frag` byte can carry, would make it 13 s (20 s) for 9 s of 700C; two members exchanging such notes would keep a one-repeater mesh busy a third of the time.
- **Wire**: `total` in the `frag` byte takes 3 bits (bit 2 was reserved, zero): 1..=7 on the air. A message of more than 2 fragments must be a VOICE, a VOICE at most 5; anything else is dropped once its signature is checked. Older versions drop such envelopes (bit 2 set, or `total` above 2): they see nothing, never a wrong message.
- **Reassembly** generalises SR-17: the fragments of a message are kept side by side, `total` + 4 at most, and the combination (one per index) whose signature verifies is the message; at most 32 combinations are tried, so a member injecting fragments under another's key costs a bounded number of signature checks.
- **Not recoverable** (§10.6): handing on 5 fragments pairwise to each member who missed them would cost more than the floods themselves. A lost note is lost, as a group voice call would be.
- **Budget**: like every medium, refused when its floods do not fit in the hour's airtime left (§13.4), never queued; its cost (fragments, our floods' airtime, relays on top) is shown before sending.

## Consequences

- The constraint "never flood media" now reads "never flood media but a group voice note within these 5 fragments" (CLAUDE.md to update).
- New vectors: envelopes with `total` 5 (`frag` bit 2 set), `testdata/lichen/frames.toml`.
- Store, references (§6.2), replies, reactions, forwarding and deletion treat a group voice note like any group message; disappearing timers apply.
- Rejected:
  - voice over the Internet bridge only (off-grid groups are the point);
  - a longer cap with fewer copies (a single flood is lost to one collision at the first hop, §10.3);
  - recording in 1600 for groups (2.8 s is too short to say anything);
  - letting texts use more than 2 fragments too (long texts already go fine in 193 bytes, and every extra fragment is flooded by the whole mesh).

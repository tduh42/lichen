# ADR-0014 — 8-byte AEAD tags: a ChaCha20-Poly1305 variant, with its bounds

- Status: superseded by ADR-0028 (2026-11-15: full 16-byte tags; the bound below does not hold for a frame the attacker heard, and the rate limit is not enforced off the radio path)
- Date: 2026-11-06

## Context

Every lichen frame carries ChaCha20-Poly1305 with the tag cut to its first 8 bytes (§5). RFC 8439 requires the full 16-byte tag and forbids truncation, so lichen's AEAD is a **variant**, not RFC 8439. The choice was made for airtime: a frame is at most 172 bytes, a fragment carries at most 157 bytes of data, and every frame pays the tag (DATA, ACK, handshake, group envelopes, probes).

The first version of this ADR bounded an attacker by the EU duty cycle and claimed key lifetimes the code did not all enforce. The review's second recheck pointed out that an attacker need not obey the duty cycle, that the 24 h / 4 MB session renewal was not implemented, and that group sender keys do not change when a member is added. This version rests on physical limits and on lifetimes the code enforces.

## Decision

Keep 8-byte tags, call the construction what it is (ChaCha20-Poly1305 with a truncated tag), and hold to these bounds.

### Forgery per attempt

Truncating Poly1305 to 64 bits leaves a forgery probability of about `2^-64` per attempt and per candidate key, plus Poly1305's `⌈ℓ/16⌉ · 2^-106` term, negligible for frames under 200 bytes. This review did not prove the truncated bound independently: it is the usual figure for a truncated universal-hash MAC.

### Attempts are limited by the receiver, not by the attacker's manners

- **One demodulator per receiver.** A forgery attempt is a frame the victim's radio demodulates. A LoRa receiver demodulates one frame at a time, whatever the number of transmitters: frames that overlap collide.
- **The shortest frame that reaches the tag check is 17 bytes**: MeshCore header and path length, lichen header, tag. At SF8 / 62.5 kHz with MeshCore's 32-symbol preamble, it takes 283 ms on air at CR 4/5 and 345 ms at CR 4/8, the same as 18 bytes (`lc-meshcore` airtime). That is at most about 12,700 frames an hour at one receiver, with the channel saturated and no duty cycle respected at all.
- **Candidate keys per frame.** A frame is tried against the keys its header hashes select: the sessions (current and pending) of the contacts sharing a node-hash byte, or the sender keys (current and previous epoch) of the group members sharing a sender byte, in the groups sharing a channel hash. Even 256 candidates per frame, far more than real collisions give, keep a year of saturated transmission at about `12,700 · 8,766 · 256 ≈ 2^34.7` tries against one receiver. That is a success probability near `2^-29` per victim and per year.

### Key lifetimes, as enforced

- **Sessions.** A new handshake before the send `tag` reaches `0xFF00`, after 24 h, or after 4 MB of datagrams (§4.4, `SESSION_MAX_AGE_MS`, `SESSION_MAX_BYTES`; configurable). Sessions are never stored (ADR-0003, ADR-0010), so an app restart renews them too.
- **Group sender keys.** A member draws a new key when another member leaves or is removed, and at every start of its app (its own key is never stored, §10). An **added** member receives the current keys: it could decrypt group messages under those keys that it recorded before joining. That is a confidentiality question, not a tag question, and is left open.
- **Handshake, introduction and discovery keys.** These are not literally single-use: retries send several frames under one key. Every frame has its own nonce: `attempt` and `index` are in the nonce, and PATH_REPLY binds fresh responder randomness `r` (§4.5). Each key is used a bounded number of times, set by the retry limits.

### Other rules

- **Failures cost nothing** to the receiver beyond dropping the frame. Comparison is in constant time, so no oracle reveals a partial tag match.
- **Transport assumption.** These bounds rest on LoRa's rate. If lichen frames are ever carried at network speed, they need full-strength authentication. The Internet bridge does not carry lichen frames: it moves content over iroh's QUIC / TLS or Tor, checked by BLAKE3 against the hash in the announcement (§14).

## Consequences

- The spec and the code say "ChaCha20-Poly1305 with an 8-byte tag", never "RFC 8439" alone.
- Moving to 16-byte tags would cost 8 bytes per frame and change every size limit: about 5 % of a full fragment, more for ACKs and positions; fragments, group payloads and padding, introduction frames and positions all move. It remains possible later as a protocol version. It is not worth it under the bounds above.
- Rejected: full tags now, because of the airtime on every frame for a margin the receiver's physics already provides.

# ADR-0014 — 8-byte AEAD tags: a ChaCha20-Poly1305 variant, with its bounds

- Status: accepted (after the review of 2026-11-06, finding PR-05)
- Date: 2026-11-06

## Context

Every lichen frame carries ChaCha20-Poly1305 with the tag cut to its first 8 bytes (§5). RFC 8439 requires the full 16-byte tag and forbids truncation, so lichen's AEAD is a **variant**, not RFC 8439. The choice was made for airtime: a frame is at most 172 bytes, a fragment carries at most 157 bytes of data, and every frame pays the tag (DATA, ACK, handshake, group envelopes, probes).

## Decision

Keep 8-byte tags, call the construction what it is (ChaCha20-Poly1305 with a truncated tag), and hold to these bounds:

- **Forgery per attempt.** Truncating Poly1305 to 64 bits leaves a forgery probability of about `2^-64` per attempt (plus Poly1305's `⌈ℓ/16⌉ · 2^-106` term, negligible for frames under 200 bytes).
- **Attempts are rate-limited by the radio.** An attacker must transmit each try: at SF8 / 62.5 kHz a frame takes 0.3 to 1.4 s, and the EU sub-band allows 360 s an hour, about 1,000 to 10,000 tries an hour at most from one transmitter. Even a thousand transmitters for a year (`~10^11` tries) stay below `2^-27` of success.
- **Keys are short-lived.** A session key is renewed before its `tag` counter wraps, after 24 h or 4 MB (§4.4); handshake, probe and introduction keys are single-use; group sender keys change at every membership change.
- **Failures cost nothing to the receiver** beyond dropping the frame; no oracle reveals partial tag matches (constant-time comparison).
- **Transport assumption.** These bounds rest on LoRa's rate. If a future transport (the Internet bridge, L7) carries lichen frames, it uses its own full-strength authentication (QUIC / TLS in iroh, Tor's) rather than relying on 8-byte tags at network speed.

## Consequences

- The spec and the code say "ChaCha20-Poly1305 with an 8-byte tag", never "RFC 8439" alone.
- Moving to 16-byte tags would cost 8 bytes per frame (5 % of a full fragment, more for ACKs and positions) and change every size limit (fragments, group payloads and padding, introduction frames, positions): possible later as a protocol version, not worth it under the bounds above.
- Rejected: full tags now (airtime on every frame, for a margin the radio already enforces).

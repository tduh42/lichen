# ADR-0028 — Full 16-byte tags: ChaCha20-Poly1305 as RFC 8439 defines it

- Status: accepted (supersedes ADR-0014)
- Date: 2026-11-15

## Context

Every lichen frame carried ChaCha20-Poly1305 with the tag cut to its first 8 bytes (ADR-0014), to save airtime. RFC 8439 §4 says "Tag truncation MUST NOT be done", so the construction was a variant, assembled by hand from the two primitives because the AEAD crates only verify full tags. ADR-0014 defended it with two claims: a forgery succeeds with about `2^-64` per attempt, and attempts are limited by the rate at which a LoRa receiver demodulates frames.

A review on 2026-11-15 checked both.

- **A frame the attacker never saw a valid tag for** (a blind forgery): the tag is uniform, so a guess succeeds with exactly `2^-64`, assuming ChaCha20 is a pseudo-random function. That part of ADR-0014 holds.
- **A frame the attacker heard and the receiver has not accepted** (lost, or jammed): the only proven bound for a Poly1305 tag cut to 64 bits is `⌈ℓ/16⌉ · 2^-39`, about `2^-35` for a full frame, against about `2^-98` with the whole tag.
- **A concrete forgery at `2^-52`.** Appending one zero byte to a ciphertext whose length is not a multiple of 16 only changes the length block of the MAC input, by `2^64`. The 8-byte tag then moves by `5 · (r ≫ 66)` (or that plus 5), where `r` is the one-time Poly1305 key. Poly1305 clamps `r`, so this value has 52 unknown bits, not 64. The relation held on 200,000 random keys. The gain is small (one uncontrolled byte appended to a short message, or a dropped datagram), but ADR-0014's figure was wrong by a factor of 4,096, and nothing proves that no better modification exists.
- **The rate limit is not enforced.** No code counts failed tags. The limit is physical, and only on the radio path: the companion link over TCP (clear and unauthenticated) or USB, or a hostile radio, delivers frames at wire speed. ADR-0014 said itself that frames carried at network speed need full-strength authentication.

Each frame has its own one-time Poly1305 key, so one forgery does not help the next, and truncation never touched the key or confidentiality. The exposure was limited. The argument for accepting it was not sound.

## Decision

- **RFC 8439 as it is.** The AEAD of every frame is ChaCha20-Poly1305 with its full 16-byte tag: DATA, ACK, ABORT, HELLO, HELLO_ACK, PATH_REPLY, the introduction's sealed parts, the PROBE's MAC, group envelopes. Group envelopes too, although their Ed25519 signature is what authenticates a message: one construction everywhere.
- **The standard implementation.** `lc-proto` uses the `chacha20poly1305` crate (no_std, in place, the tag apart, no allocation); the hand assembly is gone from the library and stays in its tests as an independent reference. The tag is compared in constant time and nothing is decrypted before it verifies.
- **A packed ACK** (§7.2): the `COMPLETE` flag and one bit per fragment in `⌈(total + 1) / 8⌉` bytes, instead of a fixed 9 bytes. Up to 7 fragments the ACK frame is 24 bytes, exactly what it was. An ACK is sent twice, so a fixed body would have made every short message pay the 8 bytes three times.
- **Sizes that follow** (what we send was trimmed again by ADR-0031, to cross a Bluetooth link whole: fragment 146, datagram 9,344, group fragment 128, group text 59 / 187, preview 417):

  | | Before | Now |
  |---|---|---|
  | Fragment (up to 2 path bytes) | 157 | 149 |
  | Largest datagram | 10,048 | 9,536 |
  | ACK frame | 24 | 24 to 32 |
  | ABORT frame | 16 | 24 |
  | HELLO / HELLO_ACK | 65 + path / 64 | 73 + path / 72 |
  | INTRO_ACK / INTRO_FIN | 104 / 56 | 120 / 72 |
  | PROBE payload (version 1 → 2) | 20 | 28 |
  | Group fragment plaintext | 139 | 131 |
  | Group text, one fragment / at most | 70 / 209 | 62 / 193 |
  | Photo preview | 450 | 426 (still 3 fragments) |

- **No migration.** The change is incompatible with earlier builds, and no one else holds one. Nothing encrypted is stored: sessions and our own sender key never are (ADR-0003, ADR-0010), and the outbox keeps plaintext (ADR-0018). Phones simply handshake again. Explicit protocol versions, with labels and a rule against downgrade, come with the handshake change of the same breaking batch (review of 2026-11-15, SR-06).

## Consequences

- **Forgery:** at most about `2^-98` per attempt for any frame, with a proof, whatever the path the frame came by. The radio's rate is no longer part of the argument.
- **Airtime.** At SF8 a frame grows by one code block per 4 bytes, so 8 bytes are two blocks on every frame, whatever its size: 65.5 ms at CR 4/8 (the French preset), 41 ms at CR 4/5. Computed from §9's formula, 62.5 kHz, CR 4/8, no repeater, whole exchange with its two ACK copies (with repeaters the percentages hold):

  | Exchange | Fragments | Before | Now | |
  |---|---|---|---|---|
  | Text, 20 characters | 1 → 1 | 1,363 ms | 1,428 ms | +4.8 % |
  | Reaction, position | 1 → 1 | 1,297 ms | 1,363 ms | +5.1 % |
  | Text, 144 characters | 1 → 1 | 2,379 ms | 2,444 ms | +2.8 % |
  | Text, 150 characters | 1 → 2 | 2,444 ms | 2,920 ms | +19.5 % |
  | Text, 300 characters | 2 → 3 | 4,002 ms | 4,543 ms | +13.5 % |
  | Voice, 10 s at 700C | 6 → 6 | 10.10 s | 10.49 s | +3.9 % |
  | Photo preview | 3 → 3 | 5.69 s | 5.69 s | 0 % (24 bytes less image) |
  | File, 8 KB | 53 → 56 | 86.0 s | 90.8 s | +5.6 % |
  | Handshake | | 1,509 ms | 1,640 ms | +8.7 % |
  | Introduction | | 2,412 ms | 2,674 ms | +10.9 % |
  | Group text, 20 characters, per copy and transmitter | 1 → 1 | 1,132 ms | 1,197 ms | +5.8 % |
  | Group text, 70 characters, per copy and transmitter | 1 → 2 | 1,558 ms | 2,099 ms | +34.8 % |

  The cost is the constant per frame, plus one more fragment for what sat just under a boundary. Without the packed ACK a 20-character text would cost +14.4 %.
- **Timers** (§7.4) count on the longest ACK (32 bytes), so waits are a little longer than a short datagram needs.
- **Measurements** on the fake network are rerun with these sizes and recorded in `docs/measurements.md`.
- Rejected: keeping 8 bytes in group envelopes, where the signature authenticates (two tag lengths, and the hand-assembled construction stays in the library for a few percent of flooded airtime); a failure counter to enforce a rate limit (it bounds attempts, not the per-attempt probability, and adds state an attacker can drive); a second AEAD with a standard short tag (none exists for ChaCha20-Poly1305).

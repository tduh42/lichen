# ADR-0029 — Protocol version 2: a handshake bound to its transcript, fresh HELLOs, longer safety numbers

- Status: accepted
- Date: 2026-11-15

## Context

The review of 2026-11-15 found four weaknesses in how sessions start. ADR-0028 (full tags) had already made the protocol incompatible with earlier builds, and nobody else holds one, so they are fixed together, with no migration.

- **SR-06, HELLO authenticated by the static secret alone.** HELLO and HELLO_ACK were sealed under keys derived from `S`, the static Diffie-Hellman secret of the two identities. Whoever learned one phone's seed could compute `S` with any of its contacts, and so answer that phone's HELLO as any of them (key-compromise impersonation), and read past HELLO bodies. Data still had forward secrecy (`ee`). Nothing bound the ephemerals, the flags or the return path into the session keys.
- **SR-03, HELLO without freshness.** A recorded HELLO stayed valid forever: replayed, it drew an answer and, in the core, changed state. Part of that is fixed in the core (answers limited, SR-04); a stale HELLO still had to be recognisable.
- **SR-05, a 30-digit safety number for two identities.** One SHA-512 over both identities, cut to 30 digits (99.7 bits): a man in the middle replacing both identities needs two numbers that collide, about `2^50` key generations per side.
- **No protocol version.** ADR-0019 said "no version negotiation": a newer phone's content would show as "unsupported" with nothing telling the user why.

## Decision

**HELLO / HELLO_ACK (§4.1–4.3).**

- Each ephemeral key travels **in clear**, after the salt (it was inside the ciphertext): the frames keep their sizes.
- HELLO is sealed under `HKDF(salt_I, S ‖ es, "lichen/2 hello" ‖ id_I ‖ id_R ‖ e_I)`, with `es = X25519(e_I, u(id_R))`.
- HELLO_ACK is sealed under `HKDF(salt_R, S ‖ es ‖ ee ‖ se, "lichen/2 hello-ack" ‖ h1)`. Here `se = X25519(x_I, e_R)`, and `h1` hashes the identities, both salts, both ephemerals, I's version and the return path.
- The session keys add R's version to the hash (`h2`) and use the same four terms.

What this gives:
- Answering as R needs R's static key or I's ephemeral: a stolen seed of I does not do it.
- Nothing either side said can be changed on the way.
- The session keeps forward secrecy.

The shape is close to Noise KK, with the static secret kept as a pre-shared input. It is not a Noise implementation: the frames, salts and labels are lichen's.

**Freshness (§4.4).** A HELLO's salt starts with I's Unix time (`t_I`, 4 bytes; 12 random bytes follow). R does not answer a HELLO more than 10 minutes away from its own clock, unless either clock is unknown. Its genuine resends fall within the 10 minutes R remembers the handshake and get the same answer.

**Introduction (§4.6).**
- Every key of the introduction includes `h`, which hashes INTRO's salt, ephemeral and return path with R's salt and ephemeral. A return path changed on the way gives other keys, and the introduction fails, instead of leading R's answers astray (part of SR-02).
- The sealed `flags` bytes of INTRO_ACK and INTRO_FIN carry the versions.

**Versions.**
- `VERSION = 2`, carried in HELLO, HELLO_ACK, INTRO_ACK and INTRO_FIN.
- A peer that shows a newer version is reported to the app once, which shows "Update lichen".
- There is no older version to fall back to: version 1 frames (8-byte tags, `lichen/1` handshake labels) do not open, so a downgrade cannot happen silently.
- When a version 3 exists, a phone will keep the highest version each contact has shown and refuse a lower one.

**Safety number (§4.7).**
- 60 digits: 30 per identity (`SHA-512("lichen/2 safety" ‖ id)`, six groups of five digits), the lower identity's first.
- A man in the middle must then match each half with a key of his own, about `2^100` tries each.
- Both phones still show the same twelve groups.

Labels elsewhere (probes, references, group signatures, the bridge keys) keep their `lichen/1` names: they did not change.

## Consequences

- **Airtime:** none. HELLO and HELLO_ACK are as long as before (the ephemeral moved out of the ciphertext); a handshake costs four X25519 operations instead of two on each side.
- **Clocks:** a phone whose clock is more than 10 minutes wrong cannot start sessions with phones whose clocks are right, and vice versa. Phones keep accurate time; a phone that does not know the time sends `t_I = 0` and is answered.
- **Safety numbers** are twice as long to read aloud.
- **Tests:** a stolen seed does not answer as the contact, the transcript and the return path are bound, small-order ephemerals are refused, and every changed byte is rejected. The two fuzz targets cover the new openers.
- **Rejected:**
  - a counter or a challenge for freshness (an extra round trip, or state that a restart loses);
  - signatures in the handshake (64 bytes per message on LoRa);
  - a real Noise implementation (its framing does not fit the 7-byte header and the attempt-in-nonce rule of ADR-0003);
  - a short safety number with a slow hash (it adds the user's waiting, not the attacker's).

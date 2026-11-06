# lichen v1 — application protocol over MeshCore (normative spec)

Source of truth for `lc-meshcore`, `lc-proto` and `lc-core`. Tanguy's design document is `design.md`: it keeps the goals, the rationale and the parts not detailed yet (media, Internet bridge, location, search). This document fixes the bytes and the corrections decided in ADRs 0002 to 0007.

Integers are **little-endian**. `‖` = concatenation. MeshCore facts come from `oxymesh/docs/spec/{air,companion}.md` (MeshCore `companion-v1.18.0`, commit 81dbae5); `A:` refers to `air.md`, `C:` to `companion.md`.

## 1. What we use from MeshCore

| Use | Send | Receive |
|---|---|---|
| Unicast (everything but groups) | `SEND_RAW_DATA` (25): `path_len ‖ path ‖ payload`, payload ≥ 4 bytes, direct route (C:§2) | push `RAW_DATA` (0x84): `snr×4 i8 ‖ rssi i8 ‖ 0xFF ‖ payload` (C:§4) |
| Groups (L3) | `SEND_RAW_PACKET` (65): raw `GRP_DATA` frame, flood | push `LOG_RX_DATA` (0x88): `snr×4 ‖ rssi ‖ raw frame` |
| Paths | contacts' `out_path` (`GET_CONTACTS`), otherwise lichen's own discovery (§4.5): a flooded probe via `SEND_RAW_PACKET`, read through `LOG_RX_DATA` | PATH_REPLY in `RAW_DATA` |
| Flood scope | `GET_DEFAULT_FLOOD_SCOPE` (64): region name and key, used for the transport codes of our floods | — |
| Identity | `EXPORT_PRIVATE_KEY` (23) → 64 "expanded" orlp bytes; public key in `SELF_INFO` | — |
| Radio | `SELF_INFO`: frequency kHz, BW Hz, SF, CR (5..8) | — |

Facts that constrain the protocol:

1. **Companion frames are ≤ 176 bytes** in both directions (C:§0). Hence `SEND_RAW_DATA`: payload ≤ `174 − path_bytes`; `RAW_DATA`: payload ≤ 172; `LOG_RX_DATA`: raw frame ≤ 173 (ADR-0002).
2. On air: payload ≤ 184 bytes, path ≤ 64 bytes (A:§1).
3. `RAW_CUSTOM` (type 0xF): forwarded in direct mode only, by the node whose hash heads the path; delivered to **every** node in range once the path is empty; never flooded (A:§3, §4.2). No addressing, no crypto: everything is in our header.
4. **Duplicate filter**: ring of 160 hashes `SHA256(type ‖ payload)[0..8]` per node, checked before anything else (A:§1.4, §4.4). The path is not covered. Every retransmission must change at least one payload byte.
5. Pushes 0x84 and 0x88 are only emitted while the app is connected; otherwise they are **lost** (C:§4, ADR-0006). `LOG_RX_DATA` is emitted for every frame received, before the duplicate filter.
8. MeshCore's own path discovery (`SEND_PATH_DISCOVERY_REQ`) is a flooded telemetry request: a companion only answers it when its owner allows base telemetry, which is off by default. lichen therefore does its own (§4.5, ADR-0008).
9. A stock node only tries to decrypt a `GRP_DATA` payload whose ciphertext (after its 2-byte MAC) is a non-empty multiple of 16 bytes. lichen's flooded payloads never have that length, so stock nodes reject them before any MAC check, whatever channels they follow.
6. The companion does not report when a frame has left: `SEND_RAW_DATA` answers OK as soon as it is queued. Pacing is our job (§7.3).
7. Hash of an identity = `pub[0]` (A:§3).

## 2. Identity and static secret (ADR-0013)

- A user's lichen **identity** is a key generated on the phone: a random 32-byte `seed`, kept in the Keychain or Keystore, expanded the MeshCore way into `prv` (64 bytes, A:§2) and `id` (32 bytes, Ed25519). The seed never leaves the phone; its backup is the 24-word BIP-39 phrase (English list) of the seed.
- The MeshCore **node key** `node` only routes: frame headers carry node hashes (§3.1), and the companion's private key is never used.
- A **contact** is a node key plus, once known, an identity key and a verified flag (§4.7).
- **Static secret** `S` (32 bytes) with a contact of identity `id_P`: MeshCore's ECDH (A:§2) between identities, `S = X25519(clamp(prv[0..32]), u(id_P))`, `u` being the Montgomery coordinate of the Ed25519 point `id_P`. An `id_P` that does not decompress, or an all-zero `S`, is rejected.
- The static secret **never** encrypts data directly: it only feeds the handshake (§4).

## 3. lichen frame

A lichen frame is the payload of a `RAW_CUSTOM` packet.

### 3.1 Header (7 bytes, always in clear, always in the AAD)

| Byte | Field | Content |
|---|---|---|
| 0 | `dst` | `node[0]` of the recipient |
| 1 | `src` | `node[0]` of the sender |
| 2-3 | `tag` | u16: datagram number within the session and direction (§5), or handshake identifier |
| 4 | `kind ‖ index` | bits 7-6: `kind`; bits 5-0: `index` |
| 5 | `total` | 1..64 |
| 6 | `attempt` | round number (data), sequence number (ACK, ABORT) or try number (handshake) |

| `kind` | Name | `index` | `total` | `attempt` | Body after the header |
|---|---|---|---|---|---|
| 0 | DATA | 0..total−1 | 1..64 | round (0, 1, …) | `ciphertext (1..157) ‖ AEAD tag (8)` |
| 1 | ACK | 0 | `total` of the acknowledged datagram | ACK sequence for this `tag` | `ciphertext (9) ‖ tag (8)` |
| 2 | ABORT | 0: "I give up my datagram"; 1: "I refuse yours" | `total` of the datagram | sequence | `ciphertext (1) ‖ tag (8)` |
| 3 | CONTROL | 0: HELLO; 1: HELLO_ACK; 2: PATH_REPLY; 3: INTRO; 4: INTRO_ACK; 5: INTRO_FIN; others reserved, ignored | 1 | try | §4 |

Decoding (otherwise silently dropped): length ≥ 7 + 1 + 8; `total` ∈ 1..64; DATA: `index < total`; ACK: `index = 0` and length = 24; ABORT: `index ∈ {0, 1}` and length = 16; CONTROL: `total = 1`, `index ≤ 5`. `dst` must be our hash.

### 3.2 Useful size (ADR-0002)

```
payload_max(path_bytes) = min(172, 174 − path_bytes)
chunk(path_bytes)       = payload_max − 15            (7 header + 8 tag)
```

A datagram is at most `64 × chunk` bytes (10,048 bytes up to 2 path bytes). All its fragments are `chunk` bytes long except the last (1..`chunk`). The chunk is fixed when the first fragment is sent; if the path grows during the transfer so that a fragment no longer fits, the datagram is abandoned and resent under a new `tag`.

## 4. Handshake

Every session starts with HELLO → HELLO_ACK. Both messages are encrypted under **single-use** keys derived from the static secret and a random salt; session keys add forward secrecy (ephemeral X25519 on both sides, always).

Notation: `I` = initiator, `R` = responder; `HKDF(salt, ikm, info, L)` = HKDF-SHA256 (RFC 5869); `info` strings start with the ASCII `"lichen/1 "`. `pub_I`, `pub_R` are the **identity** keys `id_I`, `id_R` (§2). This handshake needs both identities known; a first contact starts with the introduction (§4.6), which ends with a session too.

### 4.1 HELLO (I → R)

Header: `kind = 3`, `index = 0`, `total = 1`, `tag = h` (random u16, handshake identifier), `attempt = n` (0, then +1 on each resend).

```
body  = salt_I (16, in clear) ‖ ciphertext ‖ tag (8)
plain = e_I (32, ephemeral X25519 public) ‖ flags (1) ‖ return_path_len (1) ‖ return_path (0..64)
K_h   = HKDF(salt_I, S, "lichen/1 hello" ‖ pub_I ‖ pub_R, 32)
AAD   = header (7) ‖ salt_I
```

- `return_path`: path from R to I in MeshCore encoding (`path_len`: bits 0-5 hop count, bits 6-7 hash size − 1), i.e. the I → R path **reversed**. `0xFF` = unknown (no path bytes). R uses it to answer, since `RAW_DATA` does not tell it where the frame came from.
- `flags`: all reserved, sent as 0, ignored.
- Resends keep `h`, `salt_I` and `e_I` and increment `attempt`: the payload changes, the duplicate filter lets it through, and the nonce changes.

### 4.2 HELLO_ACK (R → I)

Header: `kind = 3`, `index = 1`, `total = 1`, `tag = h` (copied from the HELLO), `attempt` = that of the HELLO being answered.

```
body   = salt_R (16, in clear) ‖ ciphertext ‖ tag (8)
plain  = e_R (32) ‖ flags (1)
K_ha   = HKDF(salt_R, S, "lichen/1 hello-ack" ‖ pub_R ‖ pub_I ‖ salt_I, 32)
AAD    = header (7) ‖ salt_R
```

R keeps `(h, salt_I) → (salt_R, e_R)` while the handshake is recent (10 min): a resent HELLO gets the same answer, with the new `attempt`.

### 4.3 Session keys

```
ikm      = S ‖ X25519(e_priv, e_pub_peer)              (an all-zero result is rejected)
salt     = salt_I ‖ salt_R
K_{I→R}  = HKDF(salt, ikm, "lichen/1 data" ‖ pub_I ‖ pub_R, 32)
K_{R→I}  = HKDF(salt, ikm, "lichen/1 data" ‖ pub_R ‖ pub_I, 32)
```

Each side encrypts with its own direction's key and decrypts with the other. `tag` counters start at 0 in each direction.

### 4.4 Rules

- **Retries**: up to 8 HELLOs, each after a random wait uniform in `[W, 2W)` (`W` from §7.4). Fixed timers keep two nodes that cannot hear each other, and started together, colliding at the repeater between them on every try.
- **Crossing**: if I receives a HELLO from R while its own is waiting for an answer, the handshake of the smaller public key (lexicographic order) wins. The losing side drops its own and answers the other.
- **Confirmation**: I adopts the session on HELLO_ACK. R keeps it "pending" and adopts it only on the first valid message from I under the new key. Until then it keeps the previous session if there is one. A replayed HELLO therefore cannot break a running session.
- **No persistence** (ADR-0003): sessions live in memory. After a restart, the first send runs a new handshake.
- **Renewal**: new handshake before the send `tag` reaches `0xFF00`, after 24 h or after 4 MB sent (configurable). The old receive key stays valid for 10 min.

### 4.5 Path discovery (ADR-0008)

When neither the companion nor the session knows a path to the peer, lichen floods a **PROBE** and the peer answers with a **PATH_REPLY** in direct mode.

**PROBE**, a raw on-air frame handed to the companion with `SEND_RAW_PACKET` (priority 1):

```
frame   = header ‖ [tc0 ‖ tc1] ‖ path_len (0) ‖ payload
header  = FLOOD | GRP_DATA << 2 = 0x19, or TRANSPORT_FLOOD | GRP_DATA << 2 = 0x18 with the transport
          codes of the companion's default flood scope (tc0 = code(region key, 6, payload), tc1 = 0)
payload = 0x7C (channel hash, ADR-0008) ‖ 0x01 (probe version) ‖ dst (1) ‖ src (1) ‖ id (8, random) ‖ mac (8)
K_p     = HKDF(id, S, "lichen/1 probe" ‖ id_src ‖ id_dst, 32)
mac     = ChaCha20-Poly1305 tag (8) under K_p, nonce 0, empty plaintext, AAD = payload[0..12]
```

`dst` and `src` are node hashes. When the prober does not know the target's identity yet, the probe is **anonymous**: `K_p = HKDF(id, 0×32, "lichen/1 probe-anon" ‖ node_src ‖ node_dst, 32)`, which anyone can compute; the answer to an anonymous probe is anonymous too (`K_pr` below with `0×32`, `"lichen/1 path-reply-anon"` and the node keys). It is not authenticated: a forged answer can only send the following handshake down a wrong path, which then fails and probes again.

The payload is 20 bytes: after the 2-byte "MAC" a stock node would read, 17 bytes remain, not a multiple of 16 (§1 fact 9). Repeaters forward it as any flood, appending their hash to the path.

**On receipt** (a `LOG_RX_DATA` whose frame decodes as `GRP_DATA`, flood route, 20-byte payload starting `0x7C 0x01`, `dst` = our hash): try the contacts whose `pub[0] = src`; the one whose `K_p` verifies `mac` sent it. The frame's path is the sender-to-us path; its reverse is our path back. The first copy of a given `id` wins (usually the shortest); later copies are ignored. We store the reversed path as our path to the sender and answer.

**PATH_REPLY**, a lichen frame in `RAW_CUSTOM` along the reversed path:

```
header = kind 3, index 2, total 1, tag = id[0..2], attempt 0
body   = id (8, in clear) ‖ r (8, random, in clear) ‖ ciphertext ‖ tag (8)
plain  = path_len (1) ‖ path (the sender-to-us path, as received)
K_pr   = HKDF(id ‖ r, S, "lichen/1 path-reply" ‖ id_us ‖ id_sender, 32)
AAD    = header (7) ‖ id ‖ r
```

`r` is drawn for every reply, so a probe answered twice (a copy by another path, or a replay after the cache of answered probes forgot it) never reuses a key: the path differs between answers, and a repeated key and nonce would leak it and allow forged replies (review of 2026-11-06, PR-01). The prober matches `id` with its pending probe, stores the path and goes on (handshake, data). Without an answer, it probes again with a new `id` after a random wait uniform in `[20 s, 40 s)`, 3 probes at most, then reports `NoPath`. Because `id` makes every key single-use, nonces never repeat.

### 4.6 Introduction (first contact, ADR-0013)

When I does not know R's identity, it introduces itself in three messages on the Noise XX pattern: identities travel encrypted under ephemeral keys, and each side proves it holds its identity key. Notation: `e_X` ephemeral X25519 keys, `x_X = clamp(prv_X[0..32])` the identity's X25519 secret, `u(id)` as in §2; every DH that gives zero is rejected.

```
salt   = salt_I ‖ salt_R
ee     = X25519(e_I, e_R)
es     = X25519(e_I, u(id_R)) = X25519(x_R, e_I)
se     = X25519(x_I, e_R)     = X25519(e_R, u(id_I))
K_1    = HKDF(salt, ee,           "lichen/1 intro r"  ‖ e_I ‖ e_R, 32)
K_2    = HKDF(salt, ee ‖ es,      "lichen/1 intro r2" ‖ e_I ‖ e_R ‖ id_R, 32)
K_3    = HKDF(salt, ee ‖ es,      "lichen/1 intro i"  ‖ e_I ‖ e_R ‖ id_R, 32)
K_4    = HKDF(salt, ee ‖ es ‖ se, "lichen/1 intro i2" ‖ e_I ‖ e_R ‖ id_R ‖ id_I, 32)
K_{I→R} = HKDF(salt, ee ‖ es ‖ se, "lichen/1 data" ‖ id_I ‖ id_R, 32)
K_{R→I} = HKDF(salt, ee ‖ es ‖ se, "lichen/1 data" ‖ id_R ‖ id_I, 32)
```

| Message | Header | Body |
|---|---|---|
| INTRO (I → R) | `index 3`, `tag = h` (random), `attempt` = try | `salt_I (16) ‖ e_I (32) ‖ return_path_len (1) ‖ return_path (0..64)`, in clear |
| INTRO_ACK (R → I) | `index 4`, `tag = h`, `attempt` copied | `salt_R (16) ‖ e_R (32) ‖ seal(K_1, id_R) (32 + 8) ‖ seal(K_2, flags (1)) (1 + 8)` |
| INTRO_FIN (I → R) | `index 5`, `tag = h`, `attempt` = try | `seal(K_3, id_I) (32 + 8) ‖ seal(K_4, flags (1)) (1 + 8)` |

- `seal` is the AEAD of §5 with the frame's header nonce, and as AAD the header followed by everything before the sealed part (for INTRO_ACK, `salt_R ‖ e_R`, then also the first sealed part for the second); each key is used once.
- INTRO carries nothing secret: the return path is public routing information. Anyone can send one; R answers each `(src, h, salt_I)` with the same `(salt_R, e_R)` for 10 minutes, so a resend gets the same answer.
- I checks INTRO_ACK's second tag, which only the holder of `id_R` could produce, then sends INTRO_FIN and holds the session (`K_{I→R}`, `K_{R→I}`). R checks INTRO_FIN's second tag and holds the session too, pending until I's first message (§4.4): the introduction replaces HELLO / HELLO_ACK for this first session.
- I resends INTRO until INTRO_ACK (as HELLO, §4.4). Until something from R arrives under the new session, I resends INTRO_FIN (re-sealed with the next `attempt`) whenever its data waits for an acknowledgement. A session set up by HELLO / HELLO_ACK in the meantime cancels a pending introduction to the same peer.
- **Trust**: an identity learned this way is new to the receiver and **to verify** (§4.7). A different identity for a contact whose identity is **unverified** replaces it, and the app is told (a reinstalled app, or an attack). For a **verified** contact, a different identity is **not adopted**: the introduction ends without a session, the app is told, and nothing goes to the new identity until the user accepts it; it then starts unverified (review of 2026-11-06, PR-03).
- A peer whose identity is known but who answers no HELLO (8 tries) is introduced again on the next attempt: it may have a new identity.

### 4.7 Verification

- **Fingerprint** of an identity: the first 12 bytes of `SHA-256("lichen/1 fingerprint" ‖ id)`, shown as 6 groups of 4 hexadecimal digits.
- **Safety number** of two identities `a < b` (lexicographic): `h = SHA-512("lichen/1 safety" ‖ a ‖ b)`; group `i` (0..5) is the big-endian integer of `h[5i..5i+5]` modulo 100,000, shown with 5 digits. Both phones show the same 30 digits.
- **Contact card** (QR code): `lichen:1:` followed by the unpadded base64url of `id (32) ‖ node (32) ‖ name (UTF-8, ≤ 32 bytes)`. Scanning it in person adds the contact **verified**; comparing safety numbers aloud verifies it too.

## 5. Nonces and AAD (ADR-0003)

```
nonce (12 bytes) = header[2..7] ‖ 00 00 00 00 00 00 00
                 = tag_lo ‖ tag_hi ‖ (kind << 6 | index) ‖ total ‖ attempt ‖ 0×7
AAD              = header (7)                 (DATA, ACK, ABORT)
```

AEAD: ChaCha20-Poly1305 with the tag truncated to its **first 8 bytes**, compared in constant time. This is a variant of RFC 8439, which requires the full tag; its bounds (forgery about `2^-64` per attempt, attempts rate-limited by the radio, short-lived keys) are in ADR-0014.

Uniqueness under a session key (one direction):
- DATA: a `tag` serves a single datagram (strictly increasing counter); within a datagram, each fragment is sent at most once per round, and the round is in `attempt`.
- ACK: only about **the peer's** datagrams, with its own sequence per `tag`.
- ABORT: `index` separates "my datagram" from "yours".
- `kind` separates the families, and `total` is constant for a `tag`.

Under a handshake key, the key is new for each salt and `attempt` changes on each resend.

## 6. Datagrams

A datagram's plaintext is `content_type (1) ‖ body`.

| Type | Name | Body | Milestone |
|---|---|---|---|
| 0x01 | TEXT | `ts u32 (sender's Unix seconds) ‖ UTF-8 text (≥ 1 byte)` | L2 |
| 0x02 | FILE | §13.2 (images) | L6 |
| 0x03 | LOCATION | §12 (14 bytes) | L5 |
| 0x04 | ANNOUNCE | §14.1 | L7 |
| 0x05 | VOICE | §13.1 | L6 |
| 0x06 | MEDIA_REQUEST | §13.3 | L6 |
| 0x10 | SENDER_KEY | §10.2 | L3 |
| 0x11 | GROUP_STATE | §10.1 | L3 |

An unknown type is handed to the app as "unsupported", with no error on the network.

Delivery is at least once: a datagram that was delivered but whose ACKs were all lost may be sent again after a new handshake (§7.1, ADR-0006). The receiver drops a datagram byte-identical to one received from the same peer within the last hour. TEXT carries the sender's timestamp, so two genuine identical messages are told apart unless they are sent within the same second.

## 7. Fragmentation (ACK-on-Error, after RFC 8724)

### 7.1 Sender

1. The datagram is cut into `total` chunks (§3.2). Each fragment is encrypted separately (nonce §5).
2. **Round 0**: all fragments, in order, at the pace of §7.3.
3. After the last fragment of a round, wait for an ACK for `W` (§7.4).
4. ACK without `COMPLETE`: new round (`attempt + 1`) with only the fragments missing from the bitmap.
5. No ACK within `W`: new round that resends only the **last** fragment, to solicit an ACK.
6. ACK with `COMPLETE`: datagram **delivered**.
7. Give up after 4 consecutive ACKs that confirm no new fragment, after 8 consecutive rounds without any ACK (peer unreachable), or beyond `R_max` = 32 rounds (which bounds the airtime of a datagram): ABORT (`index 0`), datagram **failed**.
8. A single datagram in flight per direction and session; others wait in `lc-core`'s queue.

### 7.2 Receiver

1. A DATA fragment whose AEAD tag is valid, and only then:
   - `tag` already completed: send a `COMPLETE` ACK again (next sequence);
   - `tag` older than the window (§8): ignored;
   - new `tag`: allocate the reassembly buffer whatever the `index`, thanks to `total`. A reassembly in progress with a smaller `tag` is dropped: the sender has moved on.
2. Store the fragment in its slot (64 slots of 157 bytes). A duplicate is not stored, but it shows the sender is alive (it resets the give-up counter of step 4), and a repeated fragment that closes the round (step 3) triggers an ACK: it is the sender asking for an ACK it missed (§7.1 step 5).
3. **ACK** (`bitmap u64 ‖ flags`, bit `i` = fragment `i` received; flag bit 0 = `COMPLETE`):
   - everything received: the datagram goes to the app, the `tag` is marked completed, `COMPLETE` ACK;
   - the fragment that **closes the round** arrives while some are missing: ACK with the bitmap. The round is closed by any fragment whose index is at least `round_last`: `total − 1` at first, then the highest index reported missing in our last ACK (the sender resends missing fragments in increasing order). This plays the role of SCHC's "All-1" fragment without spending a header bit;
   - otherwise, when the reassembly timer fires (§7.4): ACK with the bitmap.
4. After an incomplete ACK, if nothing new arrives before the timer, resend the ACK (next sequence), at most 4 times. Then give up: ABORT `index 1`, reason `TIMEOUT`.
5. On delivery, every fragment but the last must have the same length, and the last must not be longer. Otherwise: give up, reason `MALFORMED`.

ABORT, 1-byte plaintext: 1 `TIMEOUT`, 2 `TOO_LARGE`, 3 `MALFORMED`, 4 `CANCELLED`, others reserved.

### 7.3 Send pacing

The companion does not say when a frame has left, and repeaters are half duplex. If the sender chains its fragments, the next repeater is still transmitting the previous fragment and misses the new one. Hand-offs to the companion are therefore spaced:

```
Δ = t_frame × k + 100 ms,   k = 1 (0 repeaters), 3 (1 repeater), 7 (2 repeaters or more)
```

`t_frame` = airtime of the whole frame (`2 + path_bytes + payload`) with the preset read from `SELF_INFO` (§9). Every gap gets a fresh random jitter `U[0, t_frame)`, and the ACK wait `W` (§7.4) a jitter `U[0, W/2)`: two senders that cannot hear each other (both sides of a conversation behind one repeater) would otherwise keep the same rhythm and collide at the repeater on every round.

**One radio, several sessions.** A node sends one data frame at a time across all its sessions, in turn (round robin). After any frame of airtime `t` on a path of `h` repeaters, the next data frame waits `t × k(h) + 100 ms` (`k` as above; `k = 10` after a flood). ACKs and ABORTs do not wait for this slot. With 2 repeaters or more, the worst case is the second repeater still forwarding fragment `i` (MeshCore repeaters wait up to `1.5 × t` before forwarding) when fragment `i + 1` reaches the first one, which cannot hear it: about `6 t`. Measured on the fake network, `k = 5` loses 12 % of receptions to collisions, `k = 7` less than 1 %, and is not slower (`docs/measurements.md`).

**ACKs are sent twice.** The copy goes `7 × t_ack + 200 ms` after the first, with the next sequence number (so a new nonce and new bytes for the duplicate filter); it carries the current bitmap. When an ACK starts a new round, the sender waits `9 × t_ack + 200 ms` before its first fragment, so that the copy does not collide with it at a repeater that both hear but that cannot hear each other. The sender ignores a copy that confirms nothing new while it is already resending.

### 7.4 Timers

```
W   (ACK wait, sender)          = (hops + 1) × (2 × t_frame + 2 × t_ack) + 2 s
G   (reassembly, receiver)      = last_received + (missing_after_highest + 2) × g + 3 s
```

`hops` = number of repeaters; `t_ack` = airtime of an ACK frame (24-byte payload). `g` = measured gap between the last two fragments received, clamped to 1..60 s; 10 s while only one fragment has been received.

## 8. Replay protection

Per session and receive direction:
- `H` = highest `tag` seen (with at least one valid fragment); window of 64 `tag`s below `H` with a bitmap of completed datagrams.
- Accepted: `tag > H`, or `H − 64 < tag ≤ H` and not completed. A `tag ≤ H − 64` is ignored.
- The sender starts at `tag = 0` and never goes back; it renews the session before `0xFF00` (§4.4).

## 9. Airtime and budget

- LoRa airtime (Semtech AN1200.13), explicit header, CRC on, LDRO when a symbol lasts 16 ms or more. MeshCore preamble: 32 symbols when SF ≤ 8, otherwise 16 (A:§6).
- Example, 869.618 MHz / 62.5 kHz / SF8, 255-byte frame: 1.51 s at CR 4/5; 2.31 s at CR 4/8 (French preset). The original design counted 1.4 s with a short preamble.
- The budget follows the `config/eu868.toml` rule that contains the frequency (g3: 10 %, i.e. 360 s per sliding hour). The host counts its own transmissions and only refuses or defers costly sends (ADR-0007).

## 10. Groups (ADR-0009)

A group is a set of members who each hold the **sender key** of every other member. A message is encrypted once with the sender's key, **signed** with the sender's identity, and flooded. Group state and sender keys travel over the pairwise sessions (§4–§8), which are authenticated and forward-secret.

### 10.1 Group state (pairwise datagram, type 0x11 GROUP_STATE)

```
gid (16, random) ‖ epoch u32 ‖ admin id (32) ‖ name_len (1) ‖ name (UTF-8, ≤ 32) ‖ n (1, 2..32)
‖ n × (id (32) ‖ node (32) ‖ name_len (1) ‖ name (≤ 32))
```

Members are identities (§2); `node` is where to reach them (MeshCore routing).

- The creator is the **admin**. Only the admin sends GROUP_STATE. A receiver accepts it when it comes from the group's admin with a higher `epoch` than the one it holds, or, for an unknown `gid`, from any contact that lists itself as admin and lists the receiver (joining is reported to the app).
- Members who are not contacts yet are added as contacts from the list (their pairwise traffic needs a path, found by §4.5).
- A receiver that is no longer listed forgets the group (reported to the app).

### 10.2 Sender keys (pairwise datagram, type 0x10 SENDER_KEY)

```
gid (16) ‖ key_epoch (1) ‖ key (32) ‖ next_counter u32
```

- Every member draws a random sender key per group and sends it to every other member. A new GROUP_STATE whose member list **lost** someone makes every remaining member draw a new key (`key_epoch + 1`) and send it to the remaining members only: the departed member cannot read what follows.
- Our own sender key and its counter are never stored: after a restart, the app draws a new key (`key_epoch + 1`) and sends it to the members once the node is connected, so a counter is never reused under a key (as for sessions, ADR-0003). The other members' keys and replay windows are stored with the group (ADR-0010): they do not resend them.
- A receiver keeps the current and the previous key of each member (messages in flight during a rekey), and drops keys of members who left.

### 10.3 Group message (flooded envelope)

Sent with `SEND_RAW_PACKET` as a `GRP_DATA` flood (transport codes of the default flood scope when set), read through `LOG_RX_DATA`.

```
payload = 0x7C ‖ type (1) ‖ gid_hash (2) ‖ sender (1) ‖ key_epoch (1) ‖ counter u32 ‖ frag (1)
          ‖ ciphertext ‖ tag (8) ‖ [0x00 if type = 0x03]
type    = 0x02, or 0x03 when a trailing pad byte is present
gid_hash = SHA256(gid)[0..2];  sender = id[0] of the sender's identity
frag    = index << 4 | copy << 3 | total, total ∈ {1, 2}, index < total, copy ∈ {0, 1}, bit 2 = 0
nonce   = counter (4) ‖ key_epoch ‖ frag ‖ 0×6;  AAD = payload[0..11];  key = sender key
```

- **Length rule** (§1 fact 9): a stock node reads a 2-byte MAC after the channel hash, so the rest of the payload must not be a non-empty multiple of 16. When `len(payload) − 3` would be one, the sender appends the pad byte and uses type 0x03 (the type byte is in the AAD).
- **Size**: `len(payload) ≤ 159`, so that the frame still fits `LOG_RX_DATA` (173 bytes) after 8 hops with transport codes (`171 − 8 − 4`). A fragment carries at most 139 bytes of plaintext.
- **Plaintext of a message** (cut into 1 or 2 fragments, all but the last full): `content_type ‖ body ‖ signature (64)`, with `signature = Ed25519(sender identity, "lichen/1 group" ‖ gid ‖ key_epoch ‖ counter ‖ content_type ‖ body)`. A TEXT therefore holds 70 bytes in one fragment, 209 in two.
- **Counter**: one per message, strictly increasing under a sender key; every fragment of a message has the same counter, and `frag` separates their nonces.
- **Two copies**: every message is flooded twice (`copy` 0 then 1, each frame spaced by `10 × t + U[0, 10 × t)`, `t` the airtime of a full envelope). The copy bit changes the nonce and the bytes, so repeaters' duplicate filters forward both; receivers keep the first and drop the second with the replay window. A flood lost at its first hop to a collision with other traffic is otherwise lost for every member.
- **Receiver**: find the group by `gid_hash`, the members by `sender`, try their current and previous keys; check the replay window (highest counter and a 64-bit bitmap per key); reassemble; verify the signature with the member's identity key; drop anything that fails. Copies of the same flood (several paths) are dropped by the replay window.
- **Early messages**: an envelope from a listed member whose key (for that `key_epoch`) has not arrived yet is kept, 16 at most, for 10 minutes, and tried again when a new sender key arrives. Sender keys travel pairwise over several hops and often arrive after a member's first post.
- **No acknowledgement** in groups: a member who is not connected misses the message (ADR-0006).

## 11. Parts taken as is from `design.md`

Search mode, regulatory constraints and threat model: `design.md` is authoritative until their milestone. Each will be detailed here before being coded.

## 12. Location (ADR-0011)

### 12.1 Format (datagram type 0x03, 14 bytes)

| Bytes | Field | Encoding |
|---|---|---|
| 0-2 | `lat` | i24, `round(lat° × 2^23 / 90)`, clamped to the i24 range: ≈ 1.19 m |
| 3-5 | `lon` | i24, `round(lon° × 2^23 / 180)`, clamped: ≈ 2.39 m at the equator |
| 6-7 | `alt` | i16, metres; −32768 = unknown |
| 8-9 | `t16` | u16, the sender's Unix seconds mod 65,536 |
| 10-11 | `hs` | u16: heading in degrees in bits 0-8 (0..359, 511 = unknown), speed in bits 9-15 in units of 0.5 m/s (0..126, 127 = unknown) |
| 12 | `prec` | radius of uncertainty, `2^(prec/16)` m: 0 = 1 m, 160 = 1 km, 255 ≈ 61 km |
| 13 | `flags` | bit 0 SOS, bit 1 end of sharing; others 0 |

Integers little-endian. The receiver rebuilds the full time as the unique instant within `[now − 18 h, now + 10 min]` whose low 16 bits are `t16`. It drops a position whose time is not newer than the last one it holds from the same sender in the same conversation (direct, or that group), on top of the session or group replay windows. The time is the one the sender vouches for the position: the measurement, or the heartbeat.

One direct position costs about 30 bytes on air (7 header + 1 type + 14 + 8 tag).

### 12.2 Sharing

A **share** is a target (a contact or a group), a precision and an end time.

- **Precision** is chosen per share: exact, 100 m, 1 km or 10 km. A coarse position is snapped to the centre of a fixed grid cell, the same cell for everyone and every time: never random noise, which averaging would remove. The grid is in on-air units: rows `h = round(size / 1.19435 m)` latitude units high, row `r = ⌊lat / h⌋`, centre `r·h + ⌊h/2⌋`; in that row, columns `w = round(size / (2.3887 m × cos φ))` longitude units wide (φ the latitude of the row centre, `w` at least 1), centre `c·w + ⌊w/2⌋`. Only the sender computes it. Coarse positions carry no heading, speed or altitude, and their radius is at least half the cell's diagonal.
- **When to send.** Sender and receiver extrapolate the last position sent along its heading at its speed, for 15 minutes at most. An exact share sends when the actual position is more than 50 m from the extrapolation; a coarse share when the cell changes. Nothing else is sent while stationary, except a heartbeat every 15 minutes (contact) or 30 minutes (group). Positions to the same target are at least 30 s apart (contact) or 5 minutes (group). A direct position is not resent when it fails: the share sends a fresh one at the next update, and a position still waiting in the queue is replaced by the newer one.
- **End.** At the end time, or when the user stops, one last position goes out with flag "end of sharing", and the receiver removes the marker.
- **SOS.** While on, the exact position goes to every share and every group every 2 minutes, with the SOS flag, whatever their precision. When it is turned off, the shares go back to their precision at once, and groups without a share get the end flag.
- Never in MeshCore adverts (`design.md`).

## 13. Media over LoRa (ADR-0012)

Small media only, direct sessions only (never flooded: in a group they wait for the Internet bridge, L7). Integers little-endian.

### 13.1 VOICE (0x05)

`ts u32 ‖ mode u8 ‖ frames u16 ‖ data`

- `mode`: 0 = Codec2 700C (28 bits per 40 ms frame, what lichen sends), 1 = Codec2 1200 (48 bits per 40 ms frame, still decoded). Audio is 8 kHz mono.
- `data`: the frames' bits back to back, most significant bit first, `⌈frames × bits / 8⌉` bytes; anything else is rejected.
- At most 375 frames (15 s): 1,313 bytes in 700C, 2,250 in 1200. Ten seconds is 875 bytes (6 fragments on one hop) in 700C, 1,500 (10 fragments) in 1200.

### 13.2 FILE (0x02): images

`ts u32 ‖ kind u8 ‖ media_id [8] ‖ variant u8 ‖ width u16 ‖ height u16 ‖ full_len u16 ‖ data`

- `kind` 1: an AVIF image (other kinds: later).
- `media_id`: random, chosen by the sender; the same for both variants.
- `variant` 0: the **preview**, at most 450 bytes of AVIF (3 fragments on one hop); `variant` 1: the **full** thumbnail, at most 4,096 bytes. Both fit in 160 × 120 pixels.
- `full_len`: size of the full variant, so the receiver can show its cost before asking; 0 when there is none.
- The sender sends the preview only. The full variant goes out when the receiver asks for it (§13.3), and the receiver replaces the preview with it.

### 13.3 MEDIA_REQUEST (0x06)

`media_id [8] ‖ variant u8`: "send me this variant". The sender answers with the FILE of that variant if it still has it, and ignores the request otherwise. Not shown as a message.

### 13.4 Airtime

Before sending a medium, the core estimates the airtime of its first round on the current path. When it does not fit in what is left of the sliding hour (§9), the send is refused with the time at which it would fit; nothing is queued. Text and positions are never refused this way (they are deferred frame by frame, §9).

## 14. Internet bridge (ADR-0015)

LoRa carries an authenticated **announcement**; the content travels over the Internet, verified against the hash the announcement pins. A malicious relay can neither impersonate the provider nor substitute the content.

### 14.1 ANNOUNCE (0x04)

`ts u32 ‖ media_id [8] ‖ kind u8 ‖ hash [32] ‖ size u32 ‖ n (1..2) ‖ n × (transport u8 ‖ key [32]) ‖ name_len u8 ‖ name (UTF-8, ≤ 32)`

- `kind`: 1 image, 2 voice, 3 file.
- `hash`: BLAKE3 of the content; `size`: its length in bytes (at most 64 MiB).
- `transport` 1: **iroh**, `key` the provider's iroh endpoint id (Ed25519); 2: **Tor**, `key` the provider's onion service identity (Ed25519, the v3 address derives from it). Unknown transports are skipped.
- `media_id` links the announcement to a LoRa preview of the same image (§13.2), when one was sent.
- Sent like any datagram (direct) or as a group message (§10.3, two fragments at most): **media reach groups this way**, never flooded themselves.

One announcement over iroh with a 20-byte name is 105 bytes: one fragment on any path.

### 14.2 Blob protocol

Over a bidirectional stream (iroh ALPN `lichen/blob/1`; the same bytes over a Tor stream to the onion service):

```
request  = 0x01 ‖ hash [32]
response = 0x00 ‖ size u64 ‖ content     (found)
         | 0x01                         (unknown hash)
```

- The requester reads at most the announced `size` (and never more than 64 MiB), computes BLAKE3 and drops the content unless it matches `hash`.
- The provider serves whoever asks for a hash it offers: the hash is the capability, and it only travels inside encrypted announcements. iroh authenticates the provider by its endpoint id (TLS 1.3, end to end even through a relay).
- The provider offers a blob while the app runs (synchronous mode), for 7 days after the announcement.

### 14.3 Keys

The iroh key is `HKDF(seed, "lichen/1 iroh")` (§2): stable for a phone, so connections reuse what the network learned, but distinct from the lichen identity. The Tor key is derived the same way with `"lichen/1 onion"`.


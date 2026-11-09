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

`dst` and `src` are node hashes. When the prober does not know the target's identity yet, the probe is **anonymous**: `K_p = HKDF(id, 0×32, "lichen/1 probe-anon" ‖ node_src ‖ node_dst, 32)`, which anyone can compute; the answer to an anonymous probe is anonymous too (`K_pr` below with `0×32`, `"lichen/1 path-reply-anon"` and the node keys). It is not authenticated: a forged answer can only send the following handshake down a wrong path, which then fails and probes again. The target may not know the prober's identity even when the prober knows the target's (a contact card or an invitation scanned one way, a reinstalled app), and could not check a keyed probe: **only the first probe is keyed, the next ones are anonymous**; a joiner's probes to the admin whose invitation it scanned are anonymous from the first (§10.5), and probes are keyed again once a session with that peer is made.

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

`r` is drawn for every reply, so a probe answered twice (a copy by another path, or a replay after the cache of answered probes forgot it) gets a fresh key each time: the path differs between answers, and a repeated key and nonce would leak it and allow forged replies (review of 2026-11-06, PR-01). Since `r` is 64 random bits, freshness is probabilistic: `q` replies to one probe `id` share a key with probability about `q(q−1) / 2^65`, below `2^-33` for `q ≤ 2^16`. A node answers an `id` once while it remembers it (64 entries, in memory), so `q` counts replays of a recorded probe, each of which costs the attacker airtime. The prober matches `id` with its pending probe, stores the path and goes on (handshake, data). Without an answer, it probes again with a new `id` after a random wait uniform in `[20 s, 40 s)`, 3 probes at most, then reports `NoPath`.

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
- **Which node sent INTRO.** Its header names I by one hash byte. R takes I among its contacts with that byte that are people (MeshCore types chat or unknown: repeaters, rooms and sensors never run lichen), preferring one whose known path equals the return path, else the first in key order. Nothing in INTRO proves the node, and nothing needs to: the node key only routes (ADR-0013), and the answer goes back by the return path. A wrong pick attaches the identity to the wrong node, which only misleads later path lookups. Observed in Paris: the phone's node shared its byte with a repeater.
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
| 0x02 | FILE | §13.2 (images; small files, ADR-0022, compressed, ADR-0023) | L6 |
| 0x03 | LOCATION | §12 (14 bytes) | L5 |
| 0x04 | ANNOUNCE | §14.1 | L7 |
| 0x05 | VOICE | §13.1 | L6 |
| 0x06 | MEDIA_REQUEST | §13.3 | L6 |
| 0x07 | SEARCH | §11 (groups only) | L8 |
| 0x08 | REPLY | `ts u32 ‖ ref (8) ‖ UTF-8 text (≥ 1 byte)`, §6.2 | ADR-0019 |
| 0x09 | REACTION | `ts u32 ‖ ref (8) ‖ emoji (UTF-8, 0..=32 bytes)`, §6.2 | ADR-0019 |
| 0x10 | SENDER_KEY | §10.2 | L3 |
| 0x11 | GROUP_STATE | §10.1 | L3 |
| 0x12 | GROUP_LEAVE | §10.4 | L3 (ADR-0017) |
| 0x13 | GROUP_JOIN | §10.5 | ADR-0020 |
| 0x14 | SENDER_KEYS | §10.2 (from the admin) | ADR-0021 |
| 0x15 | GROUP_MISSING | §10.6 | ADR-0024 |
| 0x16 | GROUP_RESEND | §10.6 | ADR-0024 |
| 0xE0–0xEF | (local) | never on the air: notices the phone records for itself (a member joined, the group was renamed) | ADR-0021 |

An unknown type is handed to the app as "unsupported", with no error on the network. A received datagram or group message whose type is in the local range 0xE0–0xEF is dropped: nobody can make another phone record a notice.

Delivery is at least once: a datagram that was delivered but whose ACKs were all lost may be sent again after a new handshake (§7.1, ADR-0006), or after a restart (§6.1). The receiver drops a datagram byte-identical to one received from the same peer within the last hour. TEXT carries the sender's timestamp, so two genuine identical messages are told apart unless they are sent within the same second.

### 6.1 Outbox and restart (ADR-0018)

What the user sends is a **message intent**: the conversation, the datagram's plaintext (`content_type ‖ body`, the sender's timestamp fixed when the user sends) and an **expiry**, 24 hours later. An app that keeps a history persists the intents of its unfinished TEXT and REPLY messages, direct or group, and of its own REACTIONs (§6.2), and nothing else: no session, sender key, counter, nonce or ciphertext (ADR-0003, ADR-0010). After a restart a message is encrypted afresh, under whatever session or sender key exists then.

1. **States.** *Queued*: accepted, nothing transmitted; the user may cancel it, and it disappears. *Sending*: at least one frame of it went out; it can no longer be cancelled, since it may already have arrived. Then one of: *delivered* (direct: the `COMPLETE` ACK, §7.1), *sent to the group* (the first copy of every fragment has left, §10.3; nobody acknowledges), or *failed* with a reason.
2. **Radio away.** A TEXT is accepted while the companion is disconnected. Nothing is transmitted and no timer or retry counter advances until it is back (no frame can leave, so none is counted lost); a transfer in progress resumes where it stopped. When the companion that comes back is another node, sessions and paths, which belonged to the old node, are dropped; the messages stay queued.
3. **Restart.** At start, unfinished TEXT, REPLY and REACTION intents (queued or sending) are submitted again with their stored plaintext. Unfinished media fail ("interrupted"): media are never queued for later (§13.4).
4. **Expiry.** An unfinished message that is not being transmitted when its expiry passes stops and fails ("expired"): never sent if it was queued; if it was sending, its transfer is abandoned (ABORT `CANCELLED`, §7.2) and it may have arrived. A message being transmitted at that moment finishes normally.
5. **Retry.** A failed message sent again by the user is the same message: the same stored plaintext, a new expiry. No failure is retried automatically: an unreachable peer would cost shared airtime for nothing.
6. **Duplicates.** A receiver that keeps a history drops a TEXT, REPLY or VOICE whose plaintext equals one it already stored from the same sender in the same conversation (direct, or group and author), however long ago. With the sender's timestamp in the plaintext, this covers a resend after a restart on either side and a group message sent again under a new sender key. It changes nothing on the air.

### 6.2 Message references, replies and reactions (ADR-0019)

**Reference.** A message is referred to by `ref = SHA-256("lichen/1 ref" ‖ author ‖ plaintext)[0..8]`, where `author` is its author's identity key (§2, 32 bytes) and `plaintext` its datagram plaintext as sent, `content_type ‖ body`: without the signature in a group (§10.3), the preview for an image (§13.2, which the full variant later replaces). Sender and receivers compute it, nothing carries it; a resend (§6.1) has the same reference. References are compared within one conversation (a contact, or a group); one that matches no stored message refers to "a message not on this phone". Vectors: `testdata/lichen/content.toml`.

**REPLY (0x08)**: `ts u32 ‖ ref (8) ‖ UTF-8 text (≥ 1 byte)`, a text answering the message `ref` of the same conversation. It carries no excerpt: the receiver shows the original when it has it. Otherwise it follows TEXT: outbox (§6.1), duplicates, one group fragment up to 62 bytes of text.

**REACTION (0x09)**: `ts u32 ‖ ref (8) ‖ emoji`, the emoji in UTF-8, 0 to 32 bytes; empty removes the sender's reaction. Anything else (not UTF-8, longer) is dropped. Not a message of the history:

1. **State.** Per conversation, target `ref` and sender identity, the reaction with the greatest `ts` holds; equal `ts`, the greater bytes. Repeats and any order of arrival converge. A sender gives each change to the same target a greater `ts` than its previous one, within the same second too.
2. **Out of order.** A reaction whose target is not stored yet is kept and applies when the target arrives.
3. **Outbox.** The sender's own reaction is applied at once, kept until delivered or sent to the group (§6.1), submitted again after a restart, and given up after 24 hours.

**Authorship.** A reply or reaction is authenticated as its sender's by its session or signature (§4, §10.3). The target's author is part of `ref`: a reference to a message names its author, and only that author's message matches it.

**Older clients** show REPLY and REACTION as unsupported content (§6); nothing else changes on the air.

## 7. Fragmentation (ACK-on-Error, after RFC 8724)

### 7.1 Sender

1. The datagram is cut into `total` chunks (§3.2). Each fragment is encrypted separately (nonce §5).
2. **Round 0**: all fragments, in order, at the pace of §7.3.
3. After the last fragment of a round, wait for an ACK for `W` (§7.4).
4. ACK without `COMPLETE`: new round (`attempt + 1`) with only the fragments missing from the bitmap.
5. No ACK within `W`: new round that resends only the **last** fragment, to solicit an ACK.
6. ACK with `COMPLETE`: datagram **delivered**.
7. Give up after 4 consecutive ACKs that confirm no new fragment, after 8 consecutive rounds without any ACK (peer unreachable), or beyond `R_max` = 32 rounds (which bounds the airtime of a datagram): ABORT (`index 0`), datagram **failed**.
   - **The peer may have restarted** (ADR-0006): after 3 rounds without any ACK for a datagram the receiver never acknowledged, the sender also starts a new handshake (§4.4, §4.6), but **keeps the session**. Anything authenticated that still arrives under it ends the handshake: the frames were lost, not the session (a busy repeater). A HELLO or INTRO from the peer, answered, replaces the session: the peer did restart, and the datagram goes again under the new one. If the handshake gives up (8 tries), the session is dropped too. A datagram whose rounds run out while the handshake is under way waits for the new session instead of failing.
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

**Room for the answer.** After a frame the peer answers at once (the last fragment of a round, HELLO, INTRO), the next data frame, to any peer, also waits `t_ack × k(h) + 200 ms`: the answer comes back through the repeaters, and a frame of ours to another peer behind the same repeater would collide with it there. Measured on the fake network, this brings a group of 5 created around one repeater from 298 s to 237 s (median of 10) before every member holds every key (`docs/measurements.md`).

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
gid (16, random) ‖ epoch u32 ‖ admin id (32) ‖ name_len (1) ‖ name (UTF-8, ≤ 32) ‖ n (1, 1..32: 1 only when the admin closes the group)
‖ n × (id (32) ‖ node (32) ‖ name_len (1) ‖ name (≤ 32)) ‖ [cause (1)]
```

`cause`, present when the list lost members compared with the previous state: 1 = they left (GROUP_LEAVE, §10.4), 2 = the admin removed them. Absent or another value: "no longer in the group". The phones show it in the conversation ("Bob left", "Camille removed Bob").

Members are identities (§2); `node` is where to reach them (MeshCore routing).

- The creator is the **admin**. Only the admin sends GROUP_STATE. A receiver accepts it when it comes from the group's admin with a higher `epoch` than the one it holds, or, for an unknown `gid`, from any contact that lists itself as admin and lists the receiver (joining is reported to the app).
- Members who are not contacts yet are added as contacts from the list (their pairwise traffic needs a path, found by §4.5).
- A receiver that is no longer listed forgets the group (reported to the app).
- The admin adds members (an invitation) with a new `epoch` listing them, sent to every member, newcomers included; the newcomers must be contacts whose identity is known. The keys follow the "only adds" rule of §10.2 (ADR-0017).
- The admin leaves by closing the group: a last GROUP_STATE (new `epoch`) that lists the admin alone, so every member forgets the group; then the admin forgets it too.
- The admin renames the group with a new `epoch` carrying the new name, sent to every member; nobody is gone, so no key changes. A group may start with the admin alone (to invite in person, §10.5); it sends no state until it has members.

### 10.2 Sender keys (pairwise datagram, type 0x10 SENDER_KEY)

```
gid (16) ‖ key_epoch (1) ‖ key (32) ‖ next_counter u32
```

- Every member draws a random sender key per group. A new GROUP_STATE whose member list **lost** someone makes every remaining member draw a new key (`key_epoch + 1`) and send it to the remaining members only: the departed member cannot read what follows. A GROUP_STATE that only **adds** members keeps the keys: newcomers receive the current keys and could therefore read messages under them recorded before they joined (ADR-0014).
- **The admin relays keys on additions** (ADR-0021), so that newcomers and members need no session with each other, only with the admin:
  - A member that joins (a state for an unknown `gid` that lists it) sends its first key to the **admin only**. On a state that only adds members, the others send nothing.
  - On admitting members, the admin sends each newcomer, after the state, one **SENDER_KEYS** with every member's current key it holds, its own included.
  - On a SENDER_KEY from member X, the admin forwards X's key in a SENDER_KEYS: to every other member when it held no key of X (X joined, or the group was just created); otherwise to the members it admitted in the last 10 minutes (a key in flight during an admission). The admin remembers its admissions in memory only.
  - A key rotation after a departure, and the new key after a restart, still go from the member to every other member directly: they do not depend on the admin being reachable.

```
SENDER_KEYS (0x14) = gid (16) ‖ n (1, 1..32) ‖ n × (owner id (32) ‖ key_epoch (1) ‖ key (32) ‖ next_counter u32)
```

- A receiver accepts SENDER_KEYS only from the group's admin (the session's identity, §4.6), ignores an entry for itself, and keeps an entry for an owner not listed yet as a key that came before its state. A relayed key never replaces a newer one: an entry with the `key_epoch` it holds is ignored, and it is adopted only when the receiver holds no key of that owner or the entry is newer (`(e − current) mod 256` in 1..127). A SENDER_KEY from its owner is always adopted.
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
- **Counter**: one per message, strictly increasing under a sender key; every fragment of a message has the same counter, and `frag` separates their nonces. Messages (what the user sends: texts, replies, reactions, announcements) take counters from 0 to 2^31 − 1; what the core sends by itself (positions, search packets) takes counters from 2^31, each range increasing on its own (ADR-0024). A receiver keeps one replay window per range, so a gap among message counters is a lost message (§10.6), never a stale position. The `next_counter` of SENDER_KEY counts messages.
- **Two copies**: every message is flooded twice (`copy` 0 then 1, each frame spaced by `10 × t + U[0, 10 × t)`, `t` the airtime of a full envelope). The copy bit changes the nonce and the bytes, so repeaters' duplicate filters forward both; receivers keep the first and drop the second with the replay window. A flood lost at its first hop to a collision with other traffic is otherwise lost for every member.
- **Receiver**: find the group by `gid_hash`, the members by `sender`, try their current and previous keys; check the replay window (highest counter and a 64-bit bitmap per key and counter range); reassemble; verify the signature with the member's identity key; drop anything that fails. Copies of the same flood (several paths) are dropped by the replay window.
- **Early messages**: an envelope from a listed member whose key (for that `key_epoch`) has not arrived yet is kept, 16 at most, for 10 minutes, and tried again when a new sender key arrives. Sender keys travel pairwise over several hops and often arrive after a member's first post.
- **No acknowledgement** in groups: a member who is not connected misses the message (ADR-0006). For the sender, a message is "sent to the group" once the first copy of every fragment has left (§6.1); until then it waits like any queued message.


### 10.4 Leaving (pairwise datagram, type 0x12 GROUP_LEAVE, ADR-0017)

```
gid (16) ‖ epoch u32
```

- A member who is not the admin leaves by sending GROUP_LEAVE to the admin (`epoch` the state it holds), and forgets the group at once: its state, the members' sender keys and its own (reported to the app). The conversation already received stays on the phone.
- The admin, on a GROUP_LEAVE from a listed member other than itself (the sender's identity is the session's, §4.6), removes it as an admin removal does (§10.1: new `epoch`, every remaining member rotates its key, §10.2). A GROUP_LEAVE for a group the receiver does not administer, or from someone not listed, is ignored.
- Until the admin has removed the member, the others keep sending under keys the leaver held: it deleted them, and a leaver that kept them could read only what is sent before the rotation (the same exposure as a removal waiting for the admin).
### 10.5 Invitation by QR code (pairwise datagram, type 0x13 GROUP_JOIN, ADR-0020)

An admin can let people join in person, without knowing them first: their app shows an **invitation** that people scan.

- **Invitation**: `lichen:1:g:` followed by the unpadded base64url of `admin identity (32) ‖ admin node (32) ‖ token (16) ‖ expires u32 (Unix seconds) ‖ n (1) ‖ admin name (n ≤ 32 bytes of UTF-8) ‖ m (1) ‖ group name (m ≤ 32 bytes of UTF-8)`. It carries no key and no group id: only the admin's public identity and an unguessable token (random, 16 bytes) that the admin's app maps to the group.
- **Validity**: 10 minutes from its creation, for any number of joiners until the group is full. Closing the invitation (or the app stopping) revokes the token at once. The joiner's app refuses an invitation more than 2 minutes past `expires` by its own clock (phone clocks differ); the admin's app decides by its own timer.
- **Joining**: the joiner's app adds the admin as a contact, **verified** (scanned in person, as a contact card, §4.7), and sends **GROUP_JOIN** `token (16)` to the admin over a pairwise session (introduced if needed, §4.6). The request is an internal message, sent again with a backoff like GROUP_LEAVE. The admin's node may never have heard the joiner's: the joiner's app first sends a **zero-hop advert** (they stand next to each other), which the admin's companion adds as a contact, so that the admin can answer the probe and the handshake; the joiner's probes are anonymous (§4.5), since the admin does not know its identity yet.
- **Admission**: the admin's app accepts a GROUP_JOIN whose token is known, not expired or revoked, for a group it still administers and that is not full; it then adds the sender as an invitation does (§10.1, ADR-0017: new epoch, state to everyone, sender keys). From a member already listed (who lost the group, e.g. a phone restored from its phrase), it sends the state and its sender key again to that member alone. Anything else is dropped without an answer. The joiner's identity is the one of the authenticated session; holding the token is the authorization.
- **Keys**: as for any addition, newcomers receive the members' current sender keys (ADR-0017): what was recorded under them before is readable to them.

Vectors: `testdata/lichen/content.toml` (`[[invite]]`, `[[group_join]]`; `[[group_state]]` and `[[sender_keys]]` for §10.1–10.2).

### 10.6 Recovering lost group messages (pairwise datagrams 0x15 GROUP_MISSING, 0x16 GROUP_RESEND, ADR-0024)

Group messages are flooded twice and never acknowledged (§10.3): when both copies collide at a busy repeater, the message is lost for every member behind it. A member that notices a gap asks for it over a pairwise session. An envelope is encrypted under its author's sender key and **signed** by the author, so any member who holds it can hand it on: the receiver authenticates it exactly as if heard on the air.

**Keeping.** Every member keeps in memory, never on disk, the envelope payloads (§10.3, as heard: any copy) of the last 64 group messages of each group, for 1 hour, its own included: one payload per fragment. Only TEXT, REPLY, REACTION and ANNOUNCE are kept (and the later message types that say so); positions and search packets go stale, are not worth resending, and do not use message counters anyway.

**Gaps.** When a member's message (a counter below 2^31, §10.3) is accepted under a key, the message counters it skipped are missing: from the highest message counter received under that key plus one up to the accepted counter minus one, and only those still inside the 64-counter replay window. Nothing is missing before the first message received under a key: what came before may predate our membership. A receiver remembers at most 16 missing counters per group and forgets one when it arrives. A loss at the end of a burst is noticed only when that member sends another message.

```
GROUP_MISSING (0x15) = gid (16) ‖ author id (32) ‖ key_epoch (1) ‖ n (1, 1..8) ‖ n × counter u32
GROUP_RESEND  (0x16) = gid (16) ‖ n (1, 1..16) ‖ n × (len (1, 1..159) ‖ envelope payload)
```

**Asking.** 20 seconds after a gap is seen, plus `U[0, 20 s)` (the second copy and slower flood paths arrive meanwhile, and members who lost the same message do not ask at once), the receiver sends GROUP_MISSING for the counters still missing under one author and key: to the author when a session with it is open, otherwise to the group's admin, which has a session with every member (§10.2); to the author when the receiver is the admin. If no GROUP_RESEND arrives within 2 minutes, or one from another member than the author lacks some of the counters, it asks the other one once; then it gives up. GROUP_MISSING is an internal datagram, deferred by the airtime budget like text (§9).

**Answering.** A member accepts GROUP_MISSING only from a listed member of that group (the session's identity, §4.6), for a group it belongs to, and answers with GROUP_RESEND holding the kept payloads of those counters (both fragments of a two-fragment message), 16 at most, and only those it kept after the asker joined (since the state that added it, as far as it saw): nobody gets back what was sent before it was a member. It sends nothing when it holds none. Counters left out are not available from it.

**Receiving.** GROUP_RESEND is accepted only from a listed member of a group the receiver belongs to. Each payload goes through §10.3's receiver unchanged (key, replay window, reassembly, signature), so a relayed payload can be neither forged nor replayed; only, it was not heard on the air, so it is not a search measurement (§11). A message recovered this way is reported as **late**: the phones show it where it arrives, with "arrived late" and its sending time.

**Airtime.** A request is one fragment; an answer for a one-fragment message (any text up to 70 bytes) is two fragments on the requester's path, for a two-fragment one three. Nothing is flooded again.

## 11. Search (ADR-0016)

Group members (the **searchers**) locate another member (the **target**) from the target's group floods heard **directly**: each searcher's phone records their SNR and RSSI with its own GPS position, shares them in the group, and fuses everyone's measurements and bearings in a particle filter (local computation, not on the wire). Phones and stock radios only; regulatory constraints and the threat model stay in `design.md`.

**Consent.** Locating a member needs their prior opt-in (`design.md`): a member answers PING and POWER only while in SOS, or with "let this group locate me" on for that group (per group, off by default, kept by the member's own app). A searcher can measure whatever the target floods anyway (its SOS beacons), but cannot make it transmit without consent.

### 11.1 SEARCH (datagram type 0x07, group envelopes only)

`kind (1) ‖ body` (the sizes below count `kind`). `target` is the first 4 bytes of the target's identity key (§2). Integers little-endian. A body that is not exactly one of these lengths, an unknown `kind`, a bearing ≥ 360° or of an unknown class, or a POWER of 0 or more than 30 minutes is dropped.

| kind | Name | Body | Bytes |
|---|---|---|---|
| 0 | PING | `target (4) ‖ id u16` | 7 |
| 1 | BEACON | `id u16 ‖ tx_dbm i8 ‖ battery u8 ‖ flags u8 ‖ [LOCATION (14)]` | 6 or 20 |
| 2 | MEASURE | `target (4) ‖ t16 u16 ‖ lat i24 ‖ lon i24 ‖ prec u8 ‖ rssi i8 ‖ snr4 i8 ‖ noise i8 ‖ flags u8 ‖ [bearing u16]` | 18 or 20 |
| 3 | POWER | `target (4) ‖ dbm i8 ‖ minutes u8` | 7 |

- **PING**: asks `target` to transmit now. `id` is the searcher's random ping number.
- **BEACON** (sent by the target): `id` is the ping answered (0 when unsolicited); `tx_dbm` the radio's TX power (`SELF_INFO`, or the one set by POWER), which ranging needs; `battery` 0..100 % (255 unknown); `flags` bit 0: a LOCATION (§12.1) follows, with its SOS flag when the target is in SOS; bit 1: the target is in SOS (with or without a position), so that an SOS without GPS is still an SOS for the group. A BEACON with a position also counts as a position from the target (§12), with the usual checks.
- **MEASURE** (sent by a searcher): what it heard of `target`. `t16` the time of reception (§12.1 encoding); `lat`, `lon`, `prec` the searcher's position then (§12.1 encodings; no MEASURE without a fix); `rssi` dBm (−128: none, a bearing alone); `snr4` SNR × 4; `noise` the radio's noise floor in dBm (−128 unknown); `flags` bit 0 RSSI estimated (noise + SNR), bit 1 saturated (SNR ≥ 10 dB with an estimated RSSI: the true RSSI is higher), bit 2 a bearing follows; `bearing` degrees from true north in bits 0-8, its class in bits 9-11 (0 Yagi ±15°, 1 body shadowing ±40°, 2 other ±60°).
- **POWER**: asks `target` to set its TX power to `dbm` for `minutes` (1..30), at close range where receivers saturate. The target clamps it to its radio's range, sets it with `SET_RADIO_TX_POWER`, answers with a BEACON carrying the new `tx_dbm`, and restores the previous power after `minutes`, when it leaves SOS, or when consent is turned off.

Every SEARCH message is one group message (§10.3): signed, flooded, two copies.

### 11.2 Measurements

- A measurement is a target's group envelope (any type: LOCATION, BEACON, TEXT…) heard in a `LOG_RX_DATA` push with **0 hops**: a relayed copy measures the last repeater, not the target. Every direct copy counts, both copies of a message included, even those the replay window then drops: the envelope's clear header (`gid_hash`, `sender`, `counter`) ties a copy to the message once one copy is decrypted.
- RSSI: the pushed value; when the radio reports 0 (stock MeshCore on a Heltec V4), the noise floor plus the SNR, flagged estimated. The noise floor comes from `GET_STATS` type 1 (`noise i16`), asked every 30 s while a search runs.
- A searcher shares a MEASURE for the best direct reception (highest SNR) of each target message, at most one per 60 s per target unless it moved more than 50 m since its last one; a bearing is shared when added, at most one per 10 s, in a MEASURE of its own (`rssi` −128).

### 11.3 Pings, beacons and airtime

- A searcher sends a PING only if no BEACON or measurement of the target is less than 60 s old (its own or shared), and at most one per 30 s.
- The target answers pending PINGs with one BEACON, at most one per 20 s. In SOS without a GPS fix, it sends a BEACON (no position, SOS bit set) every 120 s to its groups, in place of the SOS position it cannot send; when it leaves SOS still without a fix, one last BEACON with the SOS bit clear ends it for those groups (an end position needs a fix).
- Cost: a SEARCH message is one envelope of about 100 bytes, about 1 s on air at SF8 / 62.5 kHz / CR 4/8, twice (copies). On the fake network a searcher pinging when allowed and sharing its measures spends about 1.3 s a minute (2.6 s with a bearing a minute), a fifth to two fifths of a 10 % duty cycle; search floods are charged to the budget like other group messages.

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

- `mode`, with its bits per 40 ms frame: 0 = Codec2 700C (28), 1 = Codec2 1200 (48), 2 = Codec2 1600 (64), 3 = Codec2 2400 (96: two 20 ms Codec2 frames of 48 bits). `frames` counts 40 ms frames in every mode. Audio is 8 kHz mono. The sender picks the mode (the apps: 1600 by default, 700C to save airtime, 2400 for clarity); a receiver decodes all four.
- `data`: the frames' bits back to back, most significant bit first, `⌈frames × bits / 8⌉` bytes; anything else is rejected.
- At most 375 frames (15 s): 1,313 bytes in 700C, 2,250 in 1200, 3,000 in 1600, 4,500 in 2400. Ten seconds on one hop: 875 bytes (6 fragments) in 700C, 2,000 (13) in 1600, 3,000 (20) in 2400.
- Before encoding, the sender removes DC and rumble (high-pass at about 100 Hz) and brings the speech to about -20 dBFS (at most +24 dB, peaks softly limited); in 700C the encoder's equaliser is on. Codec2 is sensitive to level: a quiet recording comes out muffled.

### 13.2 FILE (0x02): images and small files

`ts u32 ‖ kind u8 ‖ media_id [8] ‖ variant u8 ‖ width u16 ‖ height u16 ‖ full_len u16 ‖ data`

- `kind` 1: an AVIF image; `kind` 2: a small file, `kind` 3: a compressed small file (below). Other kinds are shown as unsupported.
- `media_id`: random, chosen by the sender; the same for both variants.
- `variant` 0: the **preview**, at most 450 bytes of AVIF (3 fragments on one hop); `variant` 1: the **full** thumbnail, at most 4,096 bytes. Both fit in 160 × 120 pixels.
- `full_len`: size of the full variant, so the receiver can show its cost before asking; 0 when there is none.
- The sender sends the preview only. The full variant goes out when the receiver asks for it (§13.3), and the receiver replaces the preview with it.

**A small file** (`kind` 2, ADR-0022): any type, up to 8 KB, in one datagram, to a contact only (never flooded).

`ts u32 ‖ kind u8 (2) ‖ name_len u8 ‖ name (UTF-8, 1..=64 bytes) ‖ data (1..=8,192 bytes)`

- The name says what it is (its extension); the receiver opens it with whatever handles that type, and never runs it.
- The name is a label, never a path: whoever writes a copy (to open or share it) drops separators, control characters and leading dots and spaces, writes it into a new folder of its own, and never removes or overwrites anything at a path derived from it. A name such as `..` must not reach the file system.
- It is a medium: refused when its first round does not fit in the hour (§13.4). Its cost is shown before sending: 8 KB is about 53 fragments, measured in `docs/measurements.md`.
- Anything larger goes over the Internet bridge (§14.1, `kind` 3).

**A compressed small file** (`kind` 3, ADR-0023): the same file, its content compressed.

`ts u32 ‖ kind u8 (3) ‖ name_len u8 ‖ name (UTF-8, 1..=64 bytes) ‖ size u32 ‖ deflate (1..=8,192 bytes)`

- `deflate` is raw DEFLATE (RFC 1951, no zlib or gzip wrapper) of the content; `size` is the content's length once inflated, 1..=65,536 bytes.
- The sender compresses when that makes the datagram smaller, and otherwise sends `kind` 2: text, tracks (GPX, KML), CSV and JSON shrink several times, while PDFs, images and archives are already compressed. A file goes over LoRa when its datagram (kind 2 or 3) carries at most 8,192 bytes of data.
- The receiver inflates into at most `size` bytes and drops the datagram when the stream is invalid, does not end within the data, or yields more or fewer than `size` bytes: a few KB can never expand into more than 64 KiB.
- The file is then the inflated content: shown, saved and opened like `kind` 2. Its reference (§6.2) is computed on the datagram as sent.

Vectors: `testdata/lichen/content.toml` (`[[file]]`, `[[bad_file]]`).

### 13.3 MEDIA_REQUEST (0x06)

`media_id [8] ‖ variant u8`: "send me this variant". The sender answers with the FILE of that variant if it still has it, and ignores the request otherwise. Not shown as a message.

### 13.4 Airtime

Before sending a medium, the core estimates the airtime of its first round on the current path. When it does not fit in what is left of the sliding hour (§9), the send is refused with the time at which it would fit; nothing is queued. Text and positions are never refused this way (they are deferred frame by frame, §9).

## 14. Internet bridge (ADR-0015)

LoRa carries an authenticated **announcement**; the content travels over the Internet, verified against the hash the announcement pins. A malicious relay can neither impersonate the provider nor substitute the content.

### 14.1 ANNOUNCE (0x04)

`ts u32 ‖ media_id [8] ‖ kind u8 ‖ hash [32] ‖ size u32 ‖ n (1..2) ‖ n × (transport u8 ‖ key [32]) ‖ name_len u8 ‖ name (UTF-8, ≤ 32)`

- `kind`: 1 image, 2 voice, 3 file (any type, ADR-0022). A name longer than 32 bytes is cut before its extension, which is kept: the receiver recognises the type by it.
- `hash`: BLAKE3 of the content; `size`: its length in bytes (at most 64 MiB).
- `transport` 1: **iroh**, `key` the provider's iroh endpoint id (Ed25519); 2: **Tor**, `key` the provider's onion service identity (Ed25519, the v3 address derives from it). Unknown transports are skipped.
- `media_id` links the announcement to a LoRa preview of the same image (§13.2), when one was sent.
- Sent like any datagram (direct) or as a group message (§10.3, two fragments at most): **media reach groups this way**, never flooded themselves.

One announcement over iroh with a 20-byte name is 105 bytes: one fragment on any path. With both iroh and Tor, 138 bytes.

### 14.2 Blob protocol

Over a bidirectional stream (iroh ALPN `lichen/blob/1`; the same bytes over a Tor stream to the provider's onion service, virtual port **1**):

```
request  = 0x01 ‖ hash [32]
response = 0x00 ‖ size u64 ‖ content     (found)
         | 0x01                         (unknown hash)
```

- The requester reads at most the announced `size` (and never more than 64 MiB), computes BLAKE3 and drops the content unless it matches `hash`.
- The provider serves whoever asks for a hash it offers: the hash is the capability, and it only travels inside encrypted announcements. iroh authenticates the provider by its endpoint id (TLS 1.3, end to end even through a relay).
- The provider offers a blob while the app runs (synchronous mode), for 7 days after the announcement.
- An onion service accepts only `BEGIN` to port 1 and answers anything else with `END` (reason `DONE`), as other onion services do.

### 14.3 Keys

The iroh key is `HKDF(seed, "lichen/1 iroh")` (§2): stable for a phone, so connections reuse what the network learned, but distinct from the lichen identity. The Tor key is derived the same way with `"lichen/1 onion"`: those 32 bytes are the Ed25519 secret of the onion service, whose public key is the Tor `key` of announcements (the `.onion` address follows from it, rend-spec-v3 §6). Neither key is stored: both are derived again from the seed at every start.

### 14.4 Choosing a transport

- **Sender**: an announcement lists iroh (when on and reachable) and Tor (when our onion service is reachable). Sending "via Tor" lists Tor only: the recipient never learns the sender's IP address.
- **Recipient**: tries iroh first (when on), then Tor, moving on only when the provider is unreachable or too slow; any other failure (unknown hash, size, hash mismatch) ends the fetch. A recipient without Tor connects to it on demand, without publishing an onion service.
- **Relay only**: iroh without direct paths; contacts see the relay, never our IP address (the relay does).


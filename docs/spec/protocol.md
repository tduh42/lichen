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

## 2. Identity and static secret

- A user's lichen identity is their MeshCore node identity: `prv` (64 bytes, exported once, kept in the Keychain or Keystore) and `pub` (32 bytes, Ed25519). ADR-0004.
- **Static secret** `S` (32 bytes) with a contact of key `pub_P`: MeshCore's ECDH (A:§2), `S = X25519(clamp(prv[0..32]), u(pub_P))`, `u` being the Montgomery coordinate of the Ed25519 point `pub_P`. A `pub_P` that does not decompress, or an all-zero `S`, is rejected.
- The static secret **never** encrypts data directly: it only feeds the handshake (§4).

## 3. lichen frame

A lichen frame is the payload of a `RAW_CUSTOM` packet.

### 3.1 Header (7 bytes, always in clear, always in the AAD)

| Byte | Field | Content |
|---|---|---|
| 0 | `dst` | `pub[0]` of the recipient |
| 1 | `src` | `pub[0]` of the sender |
| 2-3 | `tag` | u16: datagram number within the session and direction (§5), or handshake identifier |
| 4 | `kind ‖ index` | bits 7-6: `kind`; bits 5-0: `index` |
| 5 | `total` | 1..64 |
| 6 | `attempt` | round number (data), sequence number (ACK, ABORT) or try number (handshake) |

| `kind` | Name | `index` | `total` | `attempt` | Body after the header |
|---|---|---|---|---|---|
| 0 | DATA | 0..total−1 | 1..64 | round (0, 1, …) | `ciphertext (1..157) ‖ AEAD tag (8)` |
| 1 | ACK | 0 | `total` of the acknowledged datagram | ACK sequence for this `tag` | `ciphertext (9) ‖ tag (8)` |
| 2 | ABORT | 0: "I give up my datagram"; 1: "I refuse yours" | `total` of the datagram | sequence | `ciphertext (1) ‖ tag (8)` |
| 3 | CONTROL | 0: HELLO; 1: HELLO_ACK; 2: PATH_REPLY; others reserved, ignored | 1 | try | §4 |

Decoding (otherwise silently dropped): length ≥ 7 + 1 + 8; `total` ∈ 1..64; DATA: `index < total`; ACK: `index = 0` and length = 24; ABORT: `index ∈ {0, 1}` and length = 16; CONTROL: `total = 1`, `index ∈ {0, 1, 2}`. `dst` must be our hash.

### 3.2 Useful size (ADR-0002)

```
payload_max(path_bytes) = min(172, 174 − path_bytes)
chunk(path_bytes)       = payload_max − 15            (7 header + 8 tag)
```

A datagram is at most `64 × chunk` bytes (10,048 bytes up to 2 path bytes). All its fragments are `chunk` bytes long except the last (1..`chunk`). The chunk is fixed when the first fragment is sent; if the path grows during the transfer so that a fragment no longer fits, the datagram is abandoned and resent under a new `tag`.

## 4. Handshake

Every session starts with HELLO → HELLO_ACK. Both messages are encrypted under **single-use** keys derived from the static secret and a random salt; session keys add forward secrecy (ephemeral X25519 on both sides, always).

Notation: `I` = initiator, `R` = responder; `HKDF(salt, ikm, info, L)` = HKDF-SHA256 (RFC 5869); `info` strings start with the ASCII `"lichen/1 "`.

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
K_p     = HKDF(id, S, "lichen/1 probe" ‖ pub_src ‖ pub_dst, 32)
mac     = ChaCha20-Poly1305 tag (8) under K_p, nonce 0, empty plaintext, AAD = payload[0..12]
```

The payload is 20 bytes: after the 2-byte "MAC" a stock node would read, 17 bytes remain, not a multiple of 16 (§1 fact 9). Repeaters forward it as any flood, appending their hash to the path.

**On receipt** (a `LOG_RX_DATA` whose frame decodes as `GRP_DATA`, flood route, 20-byte payload starting `0x7C 0x01`, `dst` = our hash): try the contacts whose `pub[0] = src`; the one whose `K_p` verifies `mac` sent it. The frame's path is the sender-to-us path; its reverse is our path back. The first copy of a given `id` wins (usually the shortest); later copies are ignored. We store the reversed path as our path to the sender and answer.

**PATH_REPLY**, a lichen frame in `RAW_CUSTOM` along the reversed path:

```
header = kind 3, index 2, total 1, tag = id[0..2], attempt 0
body   = id (8, in clear) ‖ ciphertext ‖ tag (8)
plain  = path_len (1) ‖ path (the sender-to-us path, as received)
K_pr   = HKDF(id, S, "lichen/1 path-reply" ‖ pub_us ‖ pub_sender, 32)
AAD    = header (7) ‖ id
```

The prober matches `id` with its pending probe, stores the path and goes on (handshake, data). Without an answer, it probes again with a new `id` after a random wait uniform in `[20 s, 40 s)`, 3 probes at most, then reports `NoPath`. Because `id` makes every key single-use, nonces never repeat.

## 5. Nonces and AAD (ADR-0003)

```
nonce (12 bytes) = header[2..7] ‖ 00 00 00 00 00 00 00
                 = tag_lo ‖ tag_hi ‖ (kind << 6 | index) ‖ total ‖ attempt ‖ 0×7
AAD              = header (7)                 (DATA, ACK, ABORT)
```

AEAD: ChaCha20-Poly1305 (RFC 8439), tag truncated to its **first 8 bytes** and compared in constant time.

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
| 0x02 | FILE | reserved | L6 |
| 0x03 | LOCATION | §12 (14 bytes) | L5 |
| 0x04 | ANNOUNCE | reserved (blob announcement, `design.md`) | L7 |
| 0x05 | VOICE | reserved (Codec2 700C) | L6 |
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
gid (16, random) ‖ epoch u32 ‖ admin pub (32) ‖ name_len (1) ‖ name (UTF-8, ≤ 32) ‖ n (1, 2..32)
‖ n × (pub (32) ‖ name_len (1) ‖ name (≤ 32))
```

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
gid_hash = SHA256(gid)[0..2];  sender = pub[0] of the sender
frag    = index << 4 | copy << 3 | total, total ∈ {1, 2}, index < total, copy ∈ {0, 1}, bit 2 = 0
nonce   = counter (4) ‖ key_epoch ‖ frag ‖ 0×6;  AAD = payload[0..11];  key = sender key
```

- **Length rule** (§1 fact 9): a stock node reads a 2-byte MAC after the channel hash, so the rest of the payload must not be a non-empty multiple of 16. When `len(payload) − 3` would be one, the sender appends the pad byte and uses type 0x03 (the type byte is in the AAD).
- **Size**: `len(payload) ≤ 159`, so that the frame still fits `LOG_RX_DATA` (173 bytes) after 8 hops with transport codes (`171 − 8 − 4`). A fragment carries at most 139 bytes of plaintext.
- **Plaintext of a message** (cut into 1 or 2 fragments, all but the last full): `content_type ‖ body ‖ signature (64)`, with `signature = Ed25519(sender identity, "lichen/1 group" ‖ gid ‖ key_epoch ‖ counter ‖ content_type ‖ body)`. A TEXT therefore holds 70 bytes in one fragment, 209 in two.
- **Counter**: one per message, strictly increasing under a sender key; every fragment of a message has the same counter, and `frag` separates their nonces.
- **Two copies**: every message is flooded twice (`copy` 0 then 1, each frame spaced by `10 × t + U[0, 10 × t)`, `t` the airtime of a full envelope). The copy bit changes the nonce and the bytes, so repeaters' duplicate filters forward both; receivers keep the first and drop the second with the replay window. A flood lost at its first hop to a collision with other traffic is otherwise lost for every member.
- **Receiver**: find the group by `gid_hash`, the members by `sender`, try their current and previous keys; check the replay window (highest counter and a 64-bit bitmap per key); reassemble; verify the signature with the member's public key; drop anything that fails. Copies of the same flood (several paths) are dropped by the replay window.
- **Early messages**: an envelope from a listed member whose key (for that `key_epoch`) has not arrived yet is kept, 16 at most, for 10 minutes, and tried again when a new sender key arrives. Sender keys travel pairwise over several hops and often arrive after a member's first post.
- **No acknowledgement** in groups: a member who is not connected misses the message (ADR-0006).

## 11. Parts taken as is from `design.md`

Media, Internet bridge (iroh, Tor), search mode, regulatory constraints and threat model: `design.md` is authoritative until their milestone. Each will be detailed here before being coded.

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

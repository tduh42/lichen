# lichen v1 — application protocol over MeshCore (normative spec)

Source of truth for `lc-meshcore`, `lc-proto` and `lc-core`. The design document is `design.md`: it keeps the goals, the rationale and the parts not detailed yet (media, Internet bridge, location, search). This document fixes the bytes and the corrections decided in ADRs 0002 to 0007.

Integers are **little-endian**. `‖` = concatenation. MeshCore facts come from `oxymesh/docs/spec/{air,companion}.md` (MeshCore `companion-v1.18.0`, commit 81dbae5); `A:` refers to `air.md`, `C:` to `companion.md`.

## 1. What we use from MeshCore

| Use | Send | Receive |
|---|---|---|
| Unicast (everything but groups) | `SEND_RAW_DATA` (25): `path_len ‖ path ‖ payload`, payload ≥ 4 bytes, direct route (C:§2) | push `RAW_DATA` (0x84): `snr×4 i8 ‖ rssi i8 ‖ 0xFF ‖ payload` (C:§4) |
| Groups (L3) | `SEND_RAW_PACKET` (65): raw `GRP_DATA` frame, flood | push `LOG_RX_DATA` (0x88): `snr×4 ‖ rssi ‖ raw frame` |
| Paths | contacts' `out_path` (`GET_CONTACTS`), otherwise lichen's own discovery (§4.5): a flooded probe via `SEND_RAW_PACKET`, read through `LOG_RX_DATA` | PATH_REPLY in `RAW_DATA` |
| Flood scope | `GET_DEFAULT_FLOOD_SCOPE` (64): region name and key, used for the transport codes of our floods | — |
| Identity | `EXPORT_PRIVATE_KEY` (23) → 64 "expanded" orlp bytes; public key in `SELF_INFO` | — |
| Radio | `SELF_INFO`: frequency kHz, BW Hz, SF, CR (5..8); the radio's settings, §1.1 | — |
| Mesh tools (§16) | `GET_CONTACTS`; trace, logins, requests and CLI text to room servers and repeaters | `NEW_ADVERT` (0x8A); pushes 0x85–0x87, 0x89, 0x8C; queued `CONTACT_MSG` of types 1 and 2 |

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
10. **Over Bluetooth, a frame from an ESP32 companion is ≤ 173 bytes whole** (ADR-0031). The firmware sets the ATT MTU to `MAX_FRAME_SIZE` (176) (`src/helpers/esp32/SerialBLEInterface.cpp` at 81dbae5) and sends each frame as one notification, which holds MTU − 3 bytes; a longer one reaches the app cut short (arduino-esp32 `BLECharacteristic::notify`, "Truncating to %d bytes"). The receiving app's link is unknown to the sender, so everything lichen sends must arrive in pushes of ≤ 173 bytes: `RAW_DATA` payload ≤ 169, `LOG_RX_DATA` raw frame ≤ 170. Writes from the app go with response, which Bluetooth carries in several pieces.


### 1.1 Radio settings (ADR-0032)

The settings lichen reads and sets on the companion, as firmware v1.18 parses them (`examples/companion_radio/MyMesh.cpp` at 81dbae5, line of each handler; vectors in `testdata/meshcore/settings.toml`). Integers little-endian. A setting is kept by the radio; lichen stores none and reads them at each connection.

| Command | Frame | Answer, rules |
|---|---|---|
| `SET_RADIO_PARAMS` (11, L1424) | `freq kHz u32 ‖ bw Hz u32 ‖ sf ‖ cr ‖ repeat` | `OK`, applied at once; `ERR 6` outside 150,000–2,500,000 kHz, 7,000–500,000 Hz, SF 5–12, CR 5–8, or a repeat frequency not allowed |
| `SET_RADIO_TX_POWER` (12) | `dBm i8` | `OK` within −9 and the radio's maximum (`SELF_INFO`) |
| `SET_ADVERT_LATLON` (14, L1266) | `lat i32 ‖ lon i32` (degrees × 10⁶) | `OK`; a GPS fix overwrites it |
| `REBOOT` (19, L1512) | `"reboot"` | none: the radio restarts |
| `GET_BATT_AND_STORAGE` (20) | — | `BATT_AND_STORAGE` (12): `mV u16 ‖ used KB u32 ‖ total KB u32` |
| `SET_DEVICE_PIN` (37, L1839) | `pin u32` | `OK` for 0 (default) or 100000–999999, used from the next boot |
| `SET_OTHER_PARAMS` (38, L1488) | `manual_add ‖ telemetry ‖ advert_loc_policy ‖ multi_acks` | `OK`; lichen always sends all four. `telemetry = env << 4 ‖ location << 2 ‖ base`, each 0 nobody, 1 contacts allowed one by one, 2 everyone; `advert_loc_policy` 1 puts the node's position in its adverts |
| `GET_CUSTOM_VARS` (40, L1853) | — | `CUSTOM_VARS` (21): ASCII `name:value` pairs joined by commas, empty without any |
| `SET_CUSTOM_VAR` (41, L1867) | ASCII `name:value` | `OK`, or `ERR 6`; `gps:1` / `gps:0` and `gps_interval:<s>` (0–86,400) on boards with a GPS |
| `SET_AUTOADD_CONFIG` (58, L2023) | `mask ‖ max_hops` (≤ 64) | `OK`; mask 0x01 overwrite the oldest when full, 0x02 chat, 0x04 repeater, 0x08 room, 0x10 sensor (used when manual add is on) |
| `GET_AUTOADD_CONFIG` (59, L2030) | — | `AUTOADD_CONFIG` (25): `mask ‖ max_hops` |

`SELF_INFO` carries the four preferences of `SET_OTHER_PARAMS` at offsets 44–47 (`multi_acks`, `advert_loc_policy`, `telemetry`, `manual_add`), between the position and the radio parameters. A position in adverts or in location telemetry is only ever set by an explicit choice of the user (ADR-0032).

## 2. Identity and static secret (ADR-0013)

- A user's lichen **identity** is a key generated on the phone: a random 32-byte `seed`, kept in the Keychain or Keystore, expanded the MeshCore way into `prv` (64 bytes, A:§2) and `id` (32 bytes, Ed25519). The seed never leaves the phone; its backup is the 24-word BIP-39 phrase (English list) of the seed.
- The MeshCore **node key** `node` only routes: frame headers carry node hashes (§3.1), and the companion's private key is never used.
- A **contact** is a node key plus, once known, an identity key and a verified flag (§4.7).
- **Static secret** `S` (32 bytes) with a contact of identity `id_P`: MeshCore's ECDH (A:§2) between identities, `S = X25519(clamp(prv[0..32]), u(id_P))`, `u` being the Montgomery coordinate of the Ed25519 point `id_P`. An `id_P` that does not decompress, or an all-zero `S`, is rejected.
- The static secret **never** encrypts data directly: it only feeds the handshake (§4).
- A phone's data (history, contacts, groups, media) is saved in a backup file that the 24 words open, under a key of its own derived from the seed: `backup.md` (ADR-0037).

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
| 0 | DATA | 0..total−1 | 1..64 | round (0, 1, …) | `ciphertext (1..146) ‖ AEAD tag (16)` |
| 1 | ACK | 0 | `total` of the acknowledged datagram | ACK sequence for this `tag` | `ciphertext (1..9, §7.2) ‖ tag (16)` |
| 2 | ABORT | 0: "I give up my datagram"; 1: "I refuse yours" | `total` of the datagram | sequence | `ciphertext (1) ‖ tag (16)` |
| 3 | CONTROL | 0: HELLO; 1: HELLO_ACK; 2: PATH_REPLY; 3: INTRO; 4: INTRO_ACK; 5: INTRO_FIN; others reserved, ignored | 1 | try | §4 |

Decoding (otherwise silently dropped): length ≥ 7 + 1 + 16; `total` ∈ 1..64; DATA: `index < total`; ACK: `index = 0` and length = `7 + ⌈(total + 1) / 8⌉ + 16`; ABORT: `index ∈ {0, 1}` and length = 24; CONTROL: `total = 1`, `index ≤ 5`. `dst` must be our hash.

### 3.2 Useful size (ADR-0002)

```
payload_max(path_bytes) = min(169, 171 − path_bytes)   (whole over Bluetooth, §1 fact 10)
chunk(path_bytes)       = payload_max − 23            (7 header + 16 tag)
```

A datagram is at most `64 × chunk` bytes (9,344 bytes up to 2 path bytes). All its fragments are `chunk` bytes long except the last (1..`chunk`). The chunk is fixed when the first fragment is sent; if the path grows during the transfer so that a fragment no longer fits, the datagram is abandoned and resent under a new `tag`.

## 4. Handshake

Every session starts with HELLO → HELLO_ACK (version 2, ADR-0029). Each side's ephemeral key travels in clear; every Diffie-Hellman term between static and ephemeral keys goes into the keys, with a hash of what both sides said. HELLO is sealed under `S ‖ es`, HELLO_ACK and the session under `S ‖ es ‖ ee ‖ se`: the session has forward secrecy, and a stolen static key of the initiator no longer lets anyone answer as the responder.

Notation: `I` = initiator, `R` = responder; `HKDF(salt, ikm, info, L)` = HKDF-SHA256 (RFC 5869); `info` strings start with the ASCII `"lichen/2 "` (version 1 used `"lichen/1 "`). `id_I`, `id_R` are the **identity** keys (§2), `x_X = clamp(prv_X[0..32])` their X25519 secrets and `u(id)` their Montgomery form; every Diffie-Hellman result that is zero is rejected. This handshake needs both identities known; a first contact starts with the introduction (§4.6), which ends with a session too.

```
es = X25519(e_I, u(id_R)) = X25519(x_R, e_I)
ee = X25519(e_I, e_R)
se = X25519(x_I, e_R)     = X25519(e_R, u(id_I))
```

### 4.1 HELLO (I → R)

Header: `kind = 3`, `index = 0`, `total = 1`, `tag = h` (random u16, handshake identifier), `attempt = n` (0, then +1 on each resend).

```
body   = salt_I (16, in clear) ‖ e_I (32, in clear) ‖ ciphertext ‖ tag (16)
plain  = version (1) ‖ return_path_len (1) ‖ return_path (0..64)
salt_I = t_I u32 (I's Unix seconds, 0 when unknown) ‖ 12 random bytes
K_h    = HKDF(salt_I, S ‖ es, "lichen/2 hello" ‖ id_I ‖ id_R ‖ e_I, 32)
AAD    = header (7) ‖ salt_I ‖ e_I
```

- `return_path`: path from R to I in MeshCore encoding (`path_len`: bits 0-5 hop count, bits 6-7 hash size − 1), i.e. the I → R path **reversed**. `0xFF` = unknown (no path bytes). R uses it to answer, since `RAW_DATA` does not tell it where the frame came from.
- `version`: I's protocol version, 2.
- Resends keep `h`, `salt_I`, `e_I` and the return path and increment `attempt`: the payload changes, the duplicate filter lets it through, and the nonce changes.

### 4.2 HELLO_ACK (R → I)

Header: `kind = 3`, `index = 1`, `total = 1`, `tag = h` (copied from the HELLO), `attempt` = that of the HELLO being answered.

```
body   = salt_R (16, in clear) ‖ e_R (32, in clear) ‖ ciphertext ‖ tag (16)
plain  = version (1)
h1     = SHA-256("lichen/2 transcript" ‖ id_I ‖ id_R ‖ salt_I ‖ e_I ‖ version_I ‖ return_path_len ‖ return_path ‖ salt_R ‖ e_R)
K_ha   = HKDF(salt_R, S ‖ es ‖ ee ‖ se, "lichen/2 hello-ack" ‖ h1, 32)
AAD    = header (7) ‖ salt_R ‖ e_R
```

R keeps `(h, salt_I) → (salt_R, e_R)` for the last HELLO it answered from that peer: a resent HELLO gets the same answer, with the new `attempt`. A replay after a newer handshake is not fresh any more (§4.4) and gets none.

### 4.3 Session keys

```
h2       = SHA-256(h1 ‖ version_R)
ikm      = S ‖ es ‖ ee ‖ se
salt     = salt_I ‖ salt_R
K_{I→R}  = HKDF(salt, ikm, "lichen/2 data" ‖ id_I ‖ id_R ‖ h2, 32)
K_{R→I}  = HKDF(salt, ikm, "lichen/2 data" ‖ id_R ‖ id_I ‖ h2, 32)
```

Each side encrypts with its own direction's key and decrypts with the other. `tag` counters start at 0 in each direction. Everything both sides said (salts, ephemerals, versions, the return path) is in `h2`: changed on the way, the two sides get different keys and nothing opens.

### 4.4 Rules

- **Retries**: up to 8 HELLOs, each after a random wait uniform in `[W, 2W)` (`W` from §7.4). A handshake nothing answered drops the path it used: the next attempt finds one again (§4.5). Fixed timers keep two nodes that cannot hear each other, and started together, colliding at the repeater between them on every try.
- **Freshness** (review of 2026-11-15, SR-03): R answers a HELLO whose `t_I` is more than 10 minutes away from its own clock only when either clock is unknown (`t_I = 0`, or R has none). A recorded HELLO replayed later gets no answer; its genuine resends, within the 10 minutes R remembers the handshake, get the same one.
- **Versions**: each side learns the other's version from HELLO, HELLO_ACK or the introduction. A newer one is reported to the app ("update lichen"); there is no older one to fall back to, since version 1 frames do not open (ADR-0029).
- **Crossing**: if I receives a HELLO from R while its own is waiting for an answer, the handshake of the smaller public key (lexicographic order) wins. The losing side drops its own and answers the other.
- **Confirmation**: I adopts the session on HELLO_ACK. R keeps it "pending" and adopts it only on the first valid message from I under the new key; the HELLO's return path and the session's age (for renewal) take effect then too. Until then it keeps the previous session and path if there are any. A replayed HELLO therefore cannot break a running session.
- **No persistence** (ADR-0003): sessions live in memory. After a restart, the first send runs a new handshake.
- **Renewal**: new handshake before the send `tag` reaches `0xFF00`, after 24 h or after 4 MB sent (configurable). The old session is dropped when the new one is adopted; a datagram still in flight under it goes again under the new one (§7.1). (Earlier versions of this text kept the old receive key 10 minutes; the code never did, review of 2026-11-15, SR-10.)

### 4.5 Path discovery (ADR-0008)

When neither the companion nor the session knows a path to the peer, lichen floods a **PROBE** and the peer answers with a **PATH_REPLY** in direct mode.

**PROBE**, a raw on-air frame handed to the companion with `SEND_RAW_PACKET` (priority 1):

```
frame   = header ‖ [tc0 ‖ tc1] ‖ path_len (0) ‖ payload
header  = FLOOD | GRP_DATA << 2 = 0x19, or TRANSPORT_FLOOD | GRP_DATA << 2 = 0x18 with the transport
          codes of the companion's default flood scope (tc0 = code(region key, 6, payload), tc1 = 0)
payload = 0x7C (channel hash, ADR-0008) ‖ 0x02 (probe version) ‖ dst (1) ‖ src (1) ‖ id (8, random) ‖ mac (16)
K_p     = HKDF(id, S, "lichen/1 probe" ‖ id_src ‖ id_dst, 32)
mac     = ChaCha20-Poly1305 tag (16) under K_p, nonce 0, empty plaintext, AAD = payload[0..12]
```

`dst` and `src` are node hashes. When the prober does not know the target's identity yet, the probe is **anonymous**: `K_p = HKDF(id, 0×32, "lichen/1 probe-anon" ‖ node_src ‖ node_dst, 32)`, which anyone can compute; the answer to an anonymous probe is anonymous too (`K_pr` below with `0×32`, `"lichen/1 path-reply-anon"` and the node keys). It is not authenticated: a forged answer can only send the following handshake down a wrong path, which then fails and probes again. The target may not know the prober's identity even when the prober knows the target's (a contact card or an invitation scanned one way, a reinstalled app), and could not check a keyed probe: **only the first probe is keyed, the next ones are anonymous**; a joiner's probes to the admin whose invitation it scanned are anonymous from the first (§10.5), and probes are keyed again once a session with that peer is made.

The payload is 28 bytes: after the 2-byte "MAC" a stock node would read, 25 bytes remain, not a multiple of 16 (§1 fact 9). Version 1 was the 20-byte probe with an 8-byte MAC (before ADR-0028); it is no longer answered. Repeaters forward it as any flood, appending their hash to the path.

**On receipt** (a `LOG_RX_DATA` whose frame decodes as `GRP_DATA`, flood route, 28-byte payload starting `0x7C 0x02`, `dst` = our hash): try the contacts whose `pub[0] = src`; the one whose `K_p` verifies `mac` sent it. The frame's path is the sender-to-us path; its reverse is our path back. The first copy of a given `id` wins (usually the shortest); later copies are ignored. We answer along the reversed path. We store it as our path to the sender only when the probe was keyed by the static secret: an anonymous probe is keyed by public node keys, anyone can send one, and its path would replace one that works (review of 2026-11-15, SR-02).

**PATH_REPLY**, a lichen frame in `RAW_CUSTOM` along the reversed path:

```
header = kind 3, index 2, total 1, tag = id[0..2], attempt 0
body   = id (8, in clear) ‖ r (8, random, in clear) ‖ ciphertext ‖ tag (16)
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
h      = SHA-256(SHA-256("lichen/2 intro" ‖ salt_I ‖ e_I ‖ return_path_len ‖ return_path) ‖ salt_R ‖ e_R)
K_1    = HKDF(salt, ee,           "lichen/2 intro r"  ‖ e_I ‖ e_R ‖ h, 32)
K_2    = HKDF(salt, ee ‖ es,      "lichen/2 intro r2" ‖ e_I ‖ e_R ‖ id_R ‖ h, 32)
K_3    = HKDF(salt, ee ‖ es,      "lichen/2 intro i"  ‖ e_I ‖ e_R ‖ id_R ‖ h, 32)
K_4    = HKDF(salt, ee ‖ es ‖ se, "lichen/2 intro i2" ‖ e_I ‖ e_R ‖ id_R ‖ id_I ‖ h, 32)
K_{I→R} = HKDF(salt, ee ‖ es ‖ se, "lichen/2 data" ‖ id_I ‖ id_R ‖ h, 32)
K_{R→I} = HKDF(salt, ee ‖ es ‖ se, "lichen/2 data" ‖ id_R ‖ id_I ‖ h, 32)
```

| Message | Header | Body |
|---|---|---|
| INTRO (I → R) | `index 3`, `tag = h` (random), `attempt` = try | `salt_I (16) ‖ e_I (32) ‖ return_path_len (1) ‖ return_path (0..64)`, in clear |
| INTRO_ACK (R → I) | `index 4`, `tag = h`, `attempt` copied | `salt_R (16) ‖ e_R (32) ‖ seal(K_1, id_R) (32 + 16) ‖ seal(K_2, version (1)) (1 + 16)` |
| INTRO_FIN (I → R) | `index 5`, `tag = h`, `attempt` = try | `seal(K_3, id_I) (32 + 16) ‖ seal(K_4, version (1)) (1 + 16)` |

- `seal` is the AEAD of §5 with the frame's header nonce, and as AAD the header followed by everything before the sealed part (for INTRO_ACK, `salt_R ‖ e_R`, then also the first sealed part for the second); each key is used once.
- INTRO carries nothing secret: the return path is public routing information, but it is in `h`, so a path changed on the way gives other keys and the introduction fails instead of leading the answer astray (ADR-0029). Anyone can send one; R answers each `(src, h, salt_I)` with the same `(salt_R, e_R)` for 10 minutes, so a resend gets the same answer.
- **Which node sent INTRO.** Its header names I by one hash byte. R takes I among its contacts with that byte that are people (MeshCore types chat or unknown: repeaters, rooms and sensors never run lichen), preferring one whose known path equals the return path, else the first in key order. Nothing in INTRO proves the node, and nothing needs to: the node key only routes (ADR-0013), and the answer goes back by the return path. A wrong pick attaches the identity to the wrong node; with identities pinned (below), that wrong identity only waits for the user. R answers along INTRO's return path, but adopts it as its path to I only once INTRO_FIN proves I's identity (SR-02). Observed in the field: the phone's node shared its byte with a repeater.
- I checks INTRO_ACK's second tag, which only the holder of `id_R` could produce, then sends INTRO_FIN and holds the session (`K_{I→R}`, `K_{R→I}`). R checks INTRO_FIN's second tag and holds the session too, pending until I's first message (§4.4): the introduction replaces HELLO / HELLO_ACK for this first session.
- I resends INTRO until INTRO_ACK (as HELLO, §4.4). Until something from R arrives under the new session, I resends INTRO_FIN (re-sealed with the next `attempt`) whenever its data waits for an acknowledgement. A session set up by HELLO / HELLO_ACK in the meantime cancels a pending introduction to the same peer.
- **Trust**: an identity learned this way is new to the receiver and **to verify** (§4.7). The first identity seen for a contact is **pinned**, verified or not: a different one is **not adopted**. The introduction ends without a session, the app is told, and the new identity waits for the user; nothing goes to it until they accept it, and it then starts unverified (review of 2026-11-06, PR-03; review of 2026-11-15, SR-01: before, an unverified identity was replaced by any other, and anyone can send an introduction under a node's hash byte).
- **While another identity waits**, the pinned one is still the contact: messages go to it by HELLO, which only it can answer, and a session with it carries on. When it no longer answers (a reinstalled app), messages fail with "identity changed" until the user decides.
- A peer whose identity is known but who answers no HELLO (8 tries) is introduced again on the next attempt: it may have a new identity, which then waits for the user as above.

### 4.7 Verification

- **Fingerprint** of an identity: the first 12 bytes of `SHA-256("lichen/1 fingerprint" ‖ id)`, shown as 6 groups of 4 hexadecimal digits.
- **Safety number** of two identities `a < b` (lexicographic), ADR-0029: 60 digits, 30 for each identity, `a`'s first. An identity's half: `h = SHA-512("lichen/2 safety" ‖ id)`; group `i` (0..5) is the big-endian integer of `h[5i..5i+5]` modulo 100,000, shown with 5 digits. Both phones show the same 12 groups. A man in the middle must match each half with a key of his own, about 2^100 tries each; the 30-digit number of version 1 hashed both identities together, and two numbers that collide were enough, about 2^50 (review of 2026-11-15, SR-05).
- **Contact card** (QR code): `lichen:1:` followed by the unpadded base64url of `id (32) ‖ node (32) ‖ name (UTF-8, ≤ 32 bytes)`. Scanning it in person adds the contact **verified**, and replaces an identity the contact no longer has; comparing safety numbers aloud verifies it too. A card pasted, or received any other way, proves nothing about who sent it: it adds the contact **unverified**, and an identity different from the one known waits for the user as in §4.6 (review of 2026-11-15, SR-23).

## 5. Nonces and AAD (ADR-0003)

```
nonce (12 bytes) = header[2..7] ‖ 00 00 00 00 00 00 00
                 = tag_lo ‖ tag_hi ‖ (kind << 6 | index) ‖ total ‖ attempt ‖ 0×7
AAD              = header (7)                 (DATA, ACK, ABORT)
```

AEAD: ChaCha20-Poly1305 exactly as RFC 8439 defines it, with its full **16-byte tag**, compared in constant time; nothing is decrypted before the tag verifies (ADR-0028). Until 2026-11-15 the tag was cut to 8 bytes (ADR-0014, superseded): that variant had no proof beyond about `2^-35` per attempt for a frame the attacker had heard, and a forgery at `2^-52` was found.

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
| 0x0A | STICKER | §6.3 | ADR-0025 |
| 0x0B | GIF | §6.3 | ADR-0025 |
| 0x0C | EDIT | `ts u32 ‖ ref (8) ‖ UTF-8 text (≥ 1 byte)`, §6.4 | ADR-0027 |
| 0x0D | DELETE | `ts u32 ‖ ref (8)`, §6.4 | ADR-0027 |
| 0x0E | TIMER | `ts u32 ‖ seconds u32`, §6.5 | ADR-0030 |
| 0x10 | SENDER_KEY | §10.2 | L3 |
| 0x11 | GROUP_STATE | §10.1 | L3 |
| 0x12 | GROUP_LEAVE | §10.4 | L3 (ADR-0017) |
| 0x13 | GROUP_JOIN | §10.5 | ADR-0020 |
| 0x14 | SENDER_KEYS | §10.2 (from the admin) | ADR-0021 |
| 0x15 | GROUP_MISSING | §10.6 | ADR-0024 |
| 0x16 | GROUP_RESEND | §10.6 | ADR-0024 |
| 0x17 | GROUP_SYNC | §10.7 | review of 2026-11-15, SR-12 |
| 0x18 | GROUP_HANDOVER | §10.8 | ADR-0038 |
| 0x19 | GROUP_DIGEST | §10.6 (group messages only) | ADR-0040 |
| 0xE0–0xEF | (local) | never on the air: notices the phone records for itself (a member joined, the group was renamed, a contact's radio took another name) | ADR-0021 |

An unknown type is handed to the app as "unsupported", with no error on the network. A received datagram or group message whose type is in the local range 0xE0–0xEF is dropped: nobody can make another phone record a notice.

Delivery is at least once: a datagram that was delivered but whose ACKs were all lost may be sent again after a new handshake (§7.1, ADR-0006), or after a restart (§6.1). The receiver drops a datagram byte-identical to one received from the same peer within the last hour. TEXT carries the sender's timestamp, so two genuine identical messages are told apart unless they are sent within the same second.

### 6.1 Outbox and restart (ADR-0018)

What the user sends is a **message intent**: the conversation, the datagram's plaintext (`content_type ‖ body`, the sender's timestamp fixed when the user sends) and an **expiry**, 24 hours later. An app that keeps a history persists the intents of its unfinished TEXT, REPLY, STICKER and GIF messages (§6.3), direct or group, and of its own REACTIONs (§6.2), EDITs and DELETEs (§6.4), and nothing else: no session, sender key, counter, nonce or ciphertext (ADR-0003, ADR-0010). After a restart a message is encrypted afresh, under whatever session or sender key exists then.

1. **States.** *Queued*: accepted, nothing transmitted; the user may cancel it, and it disappears. *Sending*: at least one frame of it went out; it can no longer be cancelled, since it may already have arrived. Then one of: *delivered* (direct: the `COMPLETE` ACK, §7.1), *sent to the group* (the first copy of every fragment has left, §10.3; nobody acknowledges), or *failed* with a reason.
2. **Radio away.** A TEXT is accepted while the companion is disconnected. Nothing is transmitted and no timer or retry counter advances until it is back (no frame can leave, so none is counted lost); a transfer in progress resumes where it stopped. When the companion that comes back is another node, sessions and paths, which belonged to the old node, are dropped; the messages stay queued.
3. **Restart.** At start, unfinished TEXT, REPLY, STICKER, GIF, REACTION, EDIT and DELETE intents (queued or sending) are submitted again with their stored plaintext. Unfinished media fail ("interrupted"): media are never queued for later (§13.4).
4. **Expiry.** An unfinished message that is not being transmitted when its expiry passes stops and fails ("expired"): never sent if it was queued; if it was sending, its transfer is abandoned (ABORT `CANCELLED`, §7.2) and it may have arrived. A message being transmitted at that moment finishes normally.
5. **Retry.** A failed message sent again by the user is the same message: the same stored plaintext, a new expiry. No failure is retried automatically: an unreachable peer would cost shared airtime for nothing.
6. **Duplicates.** A receiver that keeps a history drops a TEXT, REPLY or VOICE whose plaintext equals one it already stored from the same sender in the same conversation (direct, or group and author), however long ago. With the sender's timestamp in the plaintext, this covers a resend after a restart on either side and a group message sent again under a new sender key. It changes nothing on the air.

### 6.2 Message references, replies and reactions (ADR-0019)

**Reference.** A message is referred to by `ref = SHA-256("lichen/1 ref" ‖ author ‖ plaintext)[0..8]`, where `author` is its author's identity key (§2, 32 bytes) and `plaintext` its datagram plaintext as sent, `content_type ‖ body`: without the signature in a group (§10.3), the preview for an image (§13.2, which the full variant later replaces). Sender and receivers compute it, nothing carries it; a resend (§6.1) has the same reference. References are compared within one conversation (a contact, or a group); one that matches no stored message refers to "a message not on this phone". A receiver computes the reference on the bytes it received, never on a re-encoding of what it decoded: a text with an invalid UTF-8 byte is shown with U+FFFD, and a file's compression is the sender's choice (review of 2026-11-15, SR-20). Vectors: `testdata/lichen/content.toml`.

**REPLY (0x08)**: `ts u32 ‖ ref (8) ‖ UTF-8 text (≥ 1 byte)`, a text answering the message `ref` of the same conversation. It carries no excerpt: the receiver shows the original when it has it. Otherwise it follows TEXT: outbox (§6.1), duplicates, one group fragment up to 62 bytes of text.

**REACTION (0x09)**: `ts u32 ‖ ref (8) ‖ emoji`, the emoji in UTF-8, 0 to 32 bytes; empty removes the sender's reaction. Anything else (not UTF-8, longer) is dropped. Not a message of the history:

1. **State.** Per conversation, target `ref` and sender identity, the reaction with the greatest `ts` holds; equal `ts`, the greater bytes. Repeats and any order of arrival converge. A sender gives each change to the same target a greater `ts` than its previous one, within the same second too.
2. **Out of order.** A reaction whose target is not stored yet is kept and applies when the target arrives. A received one waits two days at most: its target was sent at most 24 hours before and arrives within the outbox's 24 hours (§6.1), so it never will after that (review of 2026-11-15, SR-15).
3. **Outbox.** The sender's own reaction is applied at once, kept until delivered or sent to the group (§6.1), submitted again after a restart, and given up after 24 hours.

**Authorship.** A reply or reaction is authenticated as its sender's by its session or signature (§4, §10.3). The target's author is part of `ref`: a reference to a message names its author, and only that author's message matches it.

**Older clients** show REPLY and REACTION as unsupported content (§6); nothing else changes on the air.

### 6.3 Stickers and GIFs (ADR-0025)

Both are messages like TEXT: the outbox (§6.1), groups (§10.3), references, replies and reactions (§6.2), recovery (§10.6). Neither carries an image: a sticker names an animation every phone ships, a GIF names one a provider serves.

**STICKER (0x0A)**: `ts u32 ‖ set u8 ‖ emoji (UTF-8, 1..=32 bytes)`.

- `set` 1: Noto Animated Emoji (Google, CC BY 4.0), the animation of that emoji's characters. The apps ship a choice of them; a receiver without the animation of a sticker shows its emoji, large. Another `set` is shown the same way.
- A sticker never makes the phone fetch anything.
- One fragment, direct or in a group.

**GIF (0x0B)**: `ts u32 ‖ provider u8 ‖ width u16 ‖ height u16 ‖ id_len u8 ‖ id (1..=32 bytes of [A-Za-z0-9]) ‖ title (UTF-8, 0..=64 bytes)`.

- `provider` 1: GIPHY. The receiver builds the address itself, with a host and a rendition fixed here: `https://media.giphy.com/media/{id}/200w.webp` (animated WebP, 200 pixels wide), `200w.gif` as a fallback. No address travels: a contact cannot make a phone load a resource of its choosing. An unknown provider is shown as unsupported, with the title.
- `width` × `height`: the shape to reserve before it loads, 0 × 0 when unknown. `title`: what it shows, for screen readers and the conversation list.
- Loading it tells the provider the phone's IP address. Through Tor (§14) when the user has Tor on; otherwise the apps load it when the user taps, unless they chose to load GIFs automatically. Off-grid, the receiver shows its title and loads it once online.
- One fragment direct; in a group, one fragment with a title of about 45 bytes, two beyond.
- A sender turns a GIPHY link into a GIF: `giphy.com/gifs/[slug-]{id}`, `media[0-9].giphy.com/media/[v1.…/]{id}/…`, `i.giphy.com/{id}.(gif|webp)`, over HTTPS; the slug gives the title ("happy-dance" → "happy dance").

Vectors: `testdata/lichen/content.toml` (`[[sticker]]`, `[[gif]]`, `[[bad_content]]`, `[[gif_link]]`).

### 6.4 Editing and deleting (ADR-0027)

An author can change the text of a message it sent, or take a message back, on the phones of the others. Neither is a message of the history: like a REACTION (§6.2), each names its target by reference and changes how the target is shown.

**EDIT (0x0C)**: `ts u32 ‖ ref (8) ‖ UTF-8 text (≥ 1 byte)`, the new text of the TEXT or REPLY `ref` of the same conversation.

**DELETE (0x0D)**: `ts u32 ‖ ref (8)`: the message `ref`, of any type that has a reference, is taken back.

1. **Only its author.** An edit or a delete applies only when its sender is the target's author: the sender's identity is part of `ref` (§6.2), and the receiver also checks the target's stored author before applying. Anything else is ignored.
2. **Within 24 hours** of the target's own timestamp, by the sender's clock: `ts − target ts ≤ 86,400`. A later one is ignored; the apps no longer offer it. Both times come from the author, so the receiver also holds the window to its own clock: a change that arrives more than 25 hours after the target arrived is ignored (review of 2026-11-15, SR-15).
3. **State.** Per conversation and target, the edit with the greatest `ts` holds; equal `ts`, the greater bytes. A delete wins over every edit, whatever their order. The reference stays the original's: replies and reactions stay attached, and a reply's quote shows the current text.
4. **Out of order.** An edit or a delete whose target is not stored yet is kept, and applies when the target arrives with the right author (a delete stores it taken back). A received one waits two days at most, like a reaction (§6.2). An edit's text is bounded like a TEXT's, by the datagram.
5. **What a receiver does.**
   - An edited message shows its current text with "edited". Searching finds the current text.
   - A deleted message keeps its place in the history as "message deleted". Its content, its media, its reactions and its edits are dropped from the phone, and a notification for it is withdrawn.
6. **Outbox.** Like a reaction: the sender applies it at once, keeps it until delivered or sent to the group (§6.1), submits it again after a restart, and gives up after 24 hours.
7. **Groups.** Both are recoverable (§10.6). An edit fits one group fragment with 54 bytes of text, like a REPLY; a delete always fits one.
8. **What it cannot do.** A delete reaches only the phones that receive it: whoever read the message has read it, and a modified app can keep it. The apps say so before sending one.

Deleting a message **for oneself** sends nothing: the phone removes it, its media, reactions and edits.

**Older clients** show EDIT and DELETE as unsupported content.

Vectors: `testdata/lichen/content.toml` (`[[edit]]`, `[[delete]]`, `[[malformed]]`).

### 6.5 Disappearing messages (ADR-0030)

A conversation can have a **timer**: each phone removes a message of that conversation a fixed time after storing it. It is a setting of the conversation, shared by its phones, not a property of each message.

**TIMER (0x0E)**: `ts u32 ‖ seconds u32`, exactly 8 bytes: from now on, messages of this conversation go `seconds` after they are stored; `0` turns the timer off. `seconds` above 2,419,200 (28 days), or any other length, is dropped. The apps offer off, 5 minutes, 1 hour, 8 hours, 1 day and 1 week.

1. **Who.** Either side of a direct conversation; any member of a group.
2. **State.** Per conversation, the TIMER with the greatest `ts` holds; equal `ts`, the greater `seconds`. Repeats and any order of arrival converge. A sender gives each change a greater `ts` than the setting it holds, within the same second too. Not a message of the history: a change records a local notice (§6, kind 11, the seconds as text), and a repeat records nothing.
3. **What goes.** A message stored while the timer is on, sent or received, of any type, goes when its time is up, counted from the moment this phone stored it: with its media, reactions and edits, as a delete for oneself (§6.4). A message stored before the timer was set stays. Notices stay. The app is told, and removes it from the screen.
4. **Delivery.** The sender keeps its own setting until it is delivered, or sent to the group (§10.3), and submits it again at each start until then. In a group a TIMER is recoverable (§10.6) and fits one fragment.
5. **MeshCore chats** (§15) keep a timer on this phone only: stock MeshCore has nothing to carry it.
6. **What it cannot do.** It removes messages from the phones that hold the setting. A phone that never received it keeps its messages; a screenshot, a copy or a modified app keeps anything. The apps say so where the timer is set.

**Older clients** show TIMER as unsupported content.

Vectors: `testdata/lichen/content.toml` (`[[timer]]`, `[[malformed]]`).

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
2. Store the fragment in its slot (64 slots of 149 bytes). A duplicate is not stored, but it shows the sender is alive (it resets the give-up counter of step 4), and a repeated fragment that closes the round (step 3) triggers an ACK: it is the sender asking for an ACK it missed (§7.1 step 5).
3. **ACK**: a little-endian bit string of `total + 1` bits in `⌈(total + 1) / 8⌉` bytes. Bit 0 is `COMPLETE`; bit `i + 1` says fragment `i` was received; the bits above are 0, and an ACK where one is set is dropped. Up to 7 fragments the body is one byte and the frame 24 bytes; 64 fragments take 9 bytes and 32 (ADR-0028):
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

`hops` = number of repeaters; `t_ack` = airtime of the longest ACK frame (32-byte payload, 64 fragments), whatever the datagram. `g` = measured gap between the last two fragments received, clamped to 1..60 s; 10 s while only one fragment has been received.

## 8. Replay protection

Per session and receive direction:
- `H` = highest `tag` seen (with at least one valid fragment); window of 64 `tag`s below `H` with a bitmap of completed datagrams.
- Accepted: `tag > H`, or `H − 64 < tag ≤ H` and not completed. A `tag ≤ H − 64` is ignored. In practice a session receives one datagram at a time (§7.1 step 8), so a `tag` below the one being reassembled is a finished or abandoned datagram: the receiver answers a completed one with `COMPLETE` and ignores the rest.
- The sender starts at `tag = 0` and never goes back; it renews the session before `0xFF00` (§4.4).

## 9. Airtime and budget

- LoRa airtime (Semtech AN1200.13), explicit header, CRC on, LDRO when a symbol lasts 16 ms or more. MeshCore preamble: 32 symbols when SF ≤ 8, otherwise 16 (A:§6).
- Example, 869.618 MHz / 62.5 kHz / SF8, 255-byte frame: 1.51 s at CR 4/5; 2.31 s at CR 4/8 (French preset). The original design counted 1.4 s with a short preamble.
- The budget follows the `config/eu868.toml` rule that contains the frequency (g3: 10 %, i.e. 360 s per sliding hour). The host counts its own transmissions and only refuses or defers costly sends (ADR-0007).
- **Answers to frames anyone can send** (INTRO_ACK, PATH_REPLY, HELLO_ACK, and the INTRO that a HELLO we cannot open prompts) are not deferred by the budget but limited: at most 8 per node in ten minutes, 60 in all in an hour. Beyond, the frame is dropped unanswered; a genuine peer retries (review of 2026-11-15, SR-04).

## 10. Groups (ADR-0009)

A group is a set of members who each hold the **sender key** of every other member. A message is encrypted once with the sender's key, **signed** with the sender's identity, and flooded. Group state and sender keys travel over the pairwise sessions (§4–§8), which are authenticated and forward-secret.

### 10.1 Group state (pairwise datagram, type 0x11 GROUP_STATE)

```
gid (16, random) ‖ epoch u32 ‖ admin id (32) ‖ name_len (1) ‖ name (UTF-8, ≤ 32) ‖ n (1, 1..32: 1 only when the admin closes the group)
‖ n × (id (32) ‖ node (32) ‖ name_len (1) ‖ name (≤ 32)) ‖ [cause (1)]
```

`cause`, present when the list lost members compared with the previous state: 1 = they left (GROUP_LEAVE, §10.4), 2 = the admin removed them. Absent or another value: "no longer in the group". The phones show it in the conversation ("Bob left", "Camille removed Bob").

Members are identities (§2); `node` is where to reach them (MeshCore routing).

- **Delivery**: GROUP_STATE and the sender keys (§10.2) go pairwise like any datagram, but are not given up after five tries: they go again every hour for a day. A member who misses a state keeps a removed member; one who misses a key loses a sender (review of 2026-11-15, SR-12).
- **One node, one member**: a state whose list names a node twice is dropped. A member's identity is learned for its node by the rule of §4.6: a node already known under another identity keeps it, and the listed one waits for the user. The phones show a member by its node, so a member's messages (and its reactions, edits, deletes and positions) are shown only while that node is known under the member's identity (review of 2026-11-15, SR-11).

- The creator is the **admin**. Only the admin sends GROUP_STATE. A receiver accepts it when it comes from the group's admin with a higher `epoch` than the one it holds. The admin can hand the group over to another member (§10.8): a member takes another admin only from a GROUP_HANDOVER signed by the admin it holds.
- **Joining needs consent** (review of 2026-11-15, SR-13). A state for an unknown `gid`, from a contact that lists itself as admin and lists the receiver, is joined at once when the user asked for it (a scanned invitation, §10.5) or the admin is a contact verified in person. Otherwise it is an **invitation**: kept (at most 8, for a week, stored with the keys that came for it, so that a restart loses nothing), shown to the user, and joined only if they accept; nothing of it is used meanwhile (its members do not become contacts, keys for it wait with it). A newer state from the same admin replaces a held one; one that no longer lists the receiver withdraws it. Declining tells nobody. A phone belongs to at most 32 groups. Since group positions, SOS and search go to the groups one belongs to, they now only go to groups the user chose.
- Once joined, members who are not contacts yet are added as contacts from the list (their pairwise traffic needs a path, found by §4.5).
- A receiver that is no longer listed forgets the group (reported to the app).
- The admin adds members (an invitation) with a new `epoch` listing them, sent to every member, newcomers included; the newcomers must be contacts whose identity is known. The keys follow the "only adds" rule of §10.2 (ADR-0017): they step forward (§10.9).
- The admin leaves by handing the group over (§10.8: the handover's state no longer lists it, `cause` 1) or by closing it: a last GROUP_STATE (new `epoch`) that lists the admin alone, so every member forgets the group; then the admin forgets it too. The apps ask which.
- The admin renames the group with a new `epoch` carrying the new name, sent to every member; nobody is gone, so no key is drawn (they step, §10.9). A group may start with the admin alone (to invite in person, §10.5); it sends no state until it has members.
- Any new `epoch` a member takes, whatever it changes, steps every sender key it holds (§10.9).

### 10.2 Sender keys (pairwise datagram, type 0x10 SENDER_KEY)

```
gid (16) ‖ step u32 ‖ key_epoch (1) ‖ key (32) ‖ next_counter u32
```

`step` is the state epoch the key belongs to (§10.9): a member sends its key at its own epoch.

- Every member draws a random sender key per group. A new GROUP_STATE whose member list **lost** someone makes every remaining member draw a new key (`key_epoch + 1`, at the new epoch) and send it to the remaining members only: the departed member cannot read what follows. A GROUP_STATE that only **adds** members draws no key: every key steps forward one-way (§10.9, ADR-0039), so newcomers receive keys that open nothing sent before the state that added them.
- **The admin relays keys on additions** (ADR-0021), so that newcomers and members need no session with each other, only with the admin:
  - A member that joins (a state for an unknown `gid` that lists it) sends its first key to the **admin only**. On a state that only adds members, the others send nothing.
  - On admitting members, the admin sends each newcomer, after the state, one **SENDER_KEYS** with every member's current key it holds, its own included. Each entry's `next_counter` is the next message counter the admin expects from that owner: one past the highest it received under the key, or the key's own `next_counter` when it received none (§10.6).
  - On a SENDER_KEY from member X, the admin forwards X's key in a SENDER_KEYS: to every other member when it held no key of X (X joined, or the group was just created) or X joined again in the last 10 minutes (§10.5); otherwise to the members it admitted in the last 10 minutes (a key in flight during an admission). The admin remembers its admissions in memory only.
  - A key rotation after a departure, and the new key after a restart, still go from the member to every other member directly: they do not depend on the admin being reachable.

```
SENDER_KEYS (0x14) = gid (16) ‖ n (1, 1..32) ‖ n × (owner id (32) ‖ step u32 ‖ key_epoch (1) ‖ key (32) ‖ next_counter u32)
```

- The admin relays and bundles every key at its own epoch, stepped forward first (§10.9): a newcomer admitted by state `E` gets no key from before `E`.
- A receiver accepts SENDER_KEYS only from the group's admin (the session's identity, §4.6), and keeps an entry for an owner not listed yet as a key that came before its state. An entry for itself is its own last key, from before it lost the group (§10.5): when that `key_epoch` is the one it holds or newer, it draws a new key one epoch past it and sends it as after a restart, so the others take it as newer. A relayed key never replaces a newer one: an entry with the `key_epoch` it holds is ignored, and it is adopted only when the receiver holds no key of that owner or the entry is newer (`(e − current) mod 256` in 1..127). A SENDER_KEY from its owner is always adopted, also another key at the epoch held (its owner lost its state); the same key again changes nothing (review of 2026-11-15, SR-20). Two keys are compared at the same step: the earlier one is stepped forward to the later one's.
- Our own sender key and its counter are never stored: after a restart, the app draws a new key (`key_epoch + 1`) and sends it to the members once the node is connected, so a counter is never reused under a key (as for sessions, ADR-0003). The other members' keys and replay windows are stored with the group (ADR-0010): they do not resend them.
- A receiver keeps the current and the previous key of each member (messages in flight during a rekey), and drops keys of members who left. It also remembers, in memory, the last 8 keys before those with their replay windows: a key it held before comes back with its window, under whatever `key_epoch`, so that relaying keys around the epoch ring cannot replay old traffic. A SENDER_KEYS counts only its first entry per owner (review of 2026-11-15, SR-18).

### 10.3 Group message (flooded envelope)

Sent with `SEND_RAW_PACKET` as a `GRP_DATA` flood (transport codes of the default flood scope when set), read through `LOG_RX_DATA`.

```
payload = 0x7C ‖ type (1) ‖ gid_hash (2) ‖ sender (1) ‖ key_epoch (1) ‖ counter u32 ‖ frag (1)
          ‖ ciphertext ‖ tag (16) ‖ [0x00 if type = 0x03]
type    = 0x02, or 0x03 when a trailing pad byte is present
gid_hash = SHA256(gid)[0..2];  sender = id[0] of the sender's identity
frag    = index << 4 | copy << 3 | total, total ∈ {1, 2}, index < total, copy ∈ {0, 1}, bit 2 = 0
nonce   = counter (4) ‖ key_epoch ‖ frag ‖ 0×6;  AAD = payload[0..11];  key = sender key
```

- **Length rule** (§1 fact 9): a stock node reads a 2-byte MAC after the channel hash, so the rest of the payload must not be a non-empty multiple of 16. When `len(payload) − 3` would be one, the sender appends the pad byte and uses type 0x03 (the type byte is in the AAD).
- **Size**: `len(payload) ≤ 156`, so that its `LOG_RX_DATA` push still crosses a Bluetooth link whole after 8 hops with transport codes (`173 − 3 − 1 − 4 − 1 − 8`, §1 fact 10). A fragment carries at most 128 bytes of plaintext.
- **Plaintext of a message** (cut into 1 or 2 fragments, all but the last full): `content_type ‖ body ‖ signature (64)`, with `signature = Ed25519(sender identity, "lichen/1 group" ‖ gid ‖ key_epoch ‖ counter ‖ content_type ‖ body)`. A TEXT therefore holds 62 bytes in one fragment, 193 in two. The signature is what authenticates a group message; the tag keeps frames that are not from a holder of the sender key away from it.
- **Counter**: one per message, strictly increasing under a sender key; every fragment of a message has the same counter, and `frag` separates their nonces. Messages (what the user sends: texts, replies, reactions, announcements) take counters from 0 to 2^31 − 1; what the core sends by itself (positions, search packets) takes counters from 2^31, each range increasing on its own (ADR-0024). A receiver keeps one replay window per range, so a gap among message counters is a lost message (§10.6), never a stale position. The `next_counter` of SENDER_KEY counts messages.
- **Two copies**: every message is flooded twice (`copy` 0 then 1, each frame spaced by `10 × t + U[0, 10 × t)`, `t` the airtime of a full envelope). The copy bit changes the nonce and the bytes, so repeaters' duplicate filters forward both; receivers keep the first and drop the second with the replay window. A flood lost at its first hop to a collision with other traffic is otherwise lost for every member.
- **Receiver**: find the group by `gid_hash`, the members by `sender`, try their current and previous keys; check the replay window (highest counter and a 64-bit bitmap per key and counter range); reassemble; verify the signature with the member's identity key; drop anything that fails. Every member holds the sender key, so a fragment that opens may not be the sender's: fragments of a two-fragment message are kept side by side (6 at most), and the pair whose signature verifies is the message (review of 2026-11-15, SR-17). Copies of the same flood (several paths) are dropped by the replay window.
- **Early messages**: an envelope from a listed member whose key (for that `key_epoch`) has not arrived yet is kept for 10 minutes, and tried again when a new sender key arrives: only when that key can still come (none held for the member, or a newer `key_epoch`), 64 at most, and 8 at most per group hash and sender byte, so that garbage under one member's byte, which nothing can check before the key comes, cannot push out the others' (review of 2026-11-15, SR-20). Sender keys travel pairwise over several hops and often arrive after a member's first post.
- **No acknowledgement** in groups: a member who is not connected misses the message (ADR-0006). For the sender, a message is "sent to the group" once the first copy of every fragment has left (§6.1); until then it waits like any queued message.


### 10.4 Leaving (pairwise datagram, type 0x12 GROUP_LEAVE, ADR-0017)

```
gid (16) ‖ epoch u32
```

- A member who is not the admin leaves by sending GROUP_LEAVE to the admin (`epoch` the state it holds), and forgets the group at once: its state, the members' sender keys and its own (reported to the app). The conversation already received stays on the phone.
- The admin, on a GROUP_LEAVE from a listed member other than itself (the sender's identity is the session's, §4.6), removes it as an admin removal does (§10.1: new `epoch`, every remaining member rotates its key, §10.2). A GROUP_LEAVE for a group the receiver does not administer, or from someone not listed, is ignored; so is one whose `epoch` the admin never issued, or one from before the state that last added the member (a leave resent after it was added back; the admin remembers its additions in memory only, review of 2026-11-15, SR-14).
- Until the admin has removed the member, the others keep sending under keys the leaver held: it deleted them, and a leaver that kept them could read only what is sent before the rotation (the same exposure as a removal waiting for the admin).
### 10.5 Invitation by QR code (pairwise datagram, type 0x13 GROUP_JOIN, ADR-0020)

An admin can let people join in person, without knowing them first: their app shows an **invitation** that people scan.

- **Invitation**: `lichen:1:g:` followed by the unpadded base64url of `admin identity (32) ‖ admin node (32) ‖ token (16) ‖ expires u32 (Unix seconds) ‖ n (1) ‖ admin name (n ≤ 32 bytes of UTF-8) ‖ m (1) ‖ group name (m ≤ 32 bytes of UTF-8)`. It carries no key and no group id: only the admin's public identity and an unguessable token (random, 16 bytes) that the admin's app maps to the group.
- **Validity**: 10 minutes from its creation, for any number of joiners until the group is full. Closing the invitation (or the app stopping) revokes the token at once. The joiner's app refuses an invitation more than 2 minutes past `expires` by its own clock (phone clocks differ); the admin's app decides by its own timer.
- **Joining**: the joiner's app adds the admin as a contact, **verified** when the invitation was scanned in person (as a contact card, §4.7). A pasted invitation proves nothing: the admin is added unverified, and one that names another identity than the one the admin's node has is refused, the change waiting for the user (§4.6). It then sends **GROUP_JOIN** `token (16)` to the admin over a pairwise session (introduced if needed, §4.6). The request is an internal message, sent again with a backoff like GROUP_LEAVE. The admin's node may never have heard the joiner's: the joiner's app first sends a **zero-hop advert** (they stand next to each other), which the admin's companion adds as a contact, so that the admin can answer the probe and the handshake; the joiner's probes are anonymous (§4.5), since the admin does not know its identity yet.
- **Admission**: the admin's app accepts a GROUP_JOIN whose token is known, not expired or revoked, for a group it still administers and that is not full; it then adds the sender as an invitation does (§10.1, ADR-0017: new epoch, state to everyone, sender keys). From a member already listed (who lost the group, e.g. a phone restored from its phrase), it sends the state again to that member alone, then one SENDER_KEYS with every current key it holds, that member's own last key included (§10.2: the member moves past it), and forgets that member's key: its next keys go to everyone for 10 minutes, as a first key does (review of 2026-11-15, SR-20). Anything else is dropped without an answer. The joiner's identity is the one of the authenticated session; holding the token is the authorization.
- **Keys**: as for any addition, newcomers receive the members' sender keys at the step of the state that added them (§10.9): nothing recorded before opens under them.

Vectors: `testdata/lichen/content.toml` (`[[invite]]`, `[[group_join]]`; `[[group_state]]` and `[[sender_keys]]` for §10.1–10.2).

### 10.6 Recovering lost group messages (pairwise datagrams 0x15 GROUP_MISSING, 0x16 GROUP_RESEND, ADR-0024)

Group messages are flooded twice and never acknowledged (§10.3): when both copies collide at a busy repeater, the message is lost for every member behind it. A member that notices a gap asks for it over a pairwise session. An envelope is encrypted under its author's sender key and **signed** by the author, so any member who holds it can hand it on: the receiver authenticates it exactly as if heard on the air.

**Keeping.** Every member keeps the envelope payloads (§10.3, as heard: any copy) of the last 64 group messages of each group, for 1 hour, its own included: one payload per fragment, with the step it opened at (§10.9). Only TEXT, REPLY, REACTION, ANNOUNCE, STICKER, GIF, EDIT, DELETE and TIMER are kept; positions, search packets and digests go stale, are not worth resending, and do not use message counters anyway. They are stored with the group, with the Unix time they were kept, and come back after a restart for what is left of their hour (ADR-0040): ciphertext under keys the members hold; our own sender key is still never stored.

**Gaps.** When a member's message (a counter below 2^31, §10.3) is accepted under a key, the message counters it skipped are missing: from the highest message counter received under that key plus one up to the accepted counter minus one, and only those still inside the 64-counter replay window. For the first message received under a key, they start at the key's `next_counter` (§10.2): what its owner sent from there on was sent after the key reached us, by the owner itself or relayed by the admin with the counter it expected next. Nothing before that is missing: it may predate our membership. A receiver remembers at most 16 missing counters per group and forgets one when it arrives. A loss at the end of a burst is noticed when that member sends another message, or its digest.

**Digest (group message, type 0x19 GROUP_DIGEST, ADR-0040).** `key_epoch (1) ‖ counter u32`, the author's last message. Two minutes after a member's last message to a group, if it sent no other message there meanwhile, it floods one digest: signed and copied as any group message (§10.3), under a counter of the core's own range, so it opens no gap. A receiver that verifies it takes as gaps the message counters it lacks under that author's key of `key_epoch`: after the highest it received (from the key's next counter if none) up to `counter` included, inside the replay window. Every message restarts the two minutes: one digest per burst. A digest is 70 bytes of plaintext, one fragment.

```
GROUP_MISSING (0x15) = gid (16) ‖ author id (32) ‖ key_epoch (1) ‖ n (1, 1..8) ‖ n × counter u32
GROUP_RESEND  (0x16) = gid (16) ‖ n (1, 1..16) ‖ n × (len (1, 1..156) ‖ envelope payload)
```

**Asking.** 20 seconds after a gap is seen, plus `U[0, 20 s)` (the second copy and slower flood paths arrive meanwhile, and members who lost the same message do not ask at once), the receiver sends GROUP_MISSING for the counters still missing under one author and key: to the author when a session with it is open, otherwise to the group's admin, which has a session with every member (§10.2); to the author when the receiver is the admin. If no GROUP_RESEND arrives within 2 minutes, or one from another member than the author lacks some of the counters, it asks the other one once; then it gives up. GROUP_MISSING is an internal datagram, deferred by the airtime budget like text (§9).

**Answering.** A member accepts GROUP_MISSING only from a listed member of that group (the session's identity, §4.6), for a group it belongs to, and answers with GROUP_RESEND holding the kept payloads of those counters (both fragments of a two-fragment message), 16 at most, and only those sent at a step no earlier than the state that added the asker (as far as it saw, §10.9): nobody gets back what was sent before it was a member, which its keys would not open anyway. Counters it is missing too (they are among its own gaps: a busy repeater drops a message for every member behind it, the admin included) it owes the asker for 2 minutes: once it recovers one, it hands it on to the asker as well, in its own GROUP_RESEND. Otherwise it sends nothing for counters it does not hold; they are not available from it. A kept message goes **once** to each member that asks: a request is a tiny frame and its answer up to 16 payloads, so a repeated request costs the answerer nothing more (review of 2026-11-15, SR-14).

**Receiving.** GROUP_RESEND is accepted only from a listed member of a group the receiver belongs to. Each payload goes through §10.3's receiver unchanged (key, replay window, reassembly, signature), so a relayed payload can be neither forged nor replayed; only, it was not heard on the air, so it is not a search measurement (§11). A message recovered this way is reported as **late**: the phones show it where it arrives, with "arrived late" and its sending time.

**Airtime.** A request is one fragment; an answer for a one-fragment message (any text up to 62 bytes) is two fragments on the requester's path, for a two-fragment one three. Nothing is flooded again.

### 10.7 Asking the admin for the keys (pairwise datagram, type 0x17 GROUP_SYNC)

```
gid (16) ‖ epoch u32
```

A member that missed the keys (a SENDER_KEYS lost on the way, a phone away during an admission) would stay unable to read the group until the admin sends them again, an hour later (§10.1). GROUP_SYNC asks for them at once (review of 2026-11-15, SR-12).

- **Asking.** A member that is not the admin, holding an envelope from a listed member for a minute without the key to open it (§10.3, early messages), sends GROUP_SYNC to the admin over a pairwise session: `epoch` is the state it holds. At most once every 10 minutes per group.
- **Answering.** The admin accepts GROUP_SYNC only from a listed member (the session's identity, §4.6), for a group it administers. It sends that member alone the handovers it keeps that are newer than `epoch` (one GROUP_HANDOVER, §10.8), the current GROUP_STATE when `epoch` is older than its own, then one SENDER_KEYS with every current key it holds, its own included, at its own step (§10.2, §10.9). Unlike a newcomer's, each entry's `next_counter` is the key's first counter (the one its owner announced; 0 for the admin's own), not the next one the admin expects: the member holds what was sent under those keys at that step and reads it. At most once per member every 10 minutes; anything else is dropped without an answer.
- **Cost.** 21 bytes asked; the answer is what an admission costs that member. A member cannot make the admin send anything to anyone else.

Vectors: `testdata/lichen/content.toml` (`[[group_sync]]`, `[[bad_group_control]]`).

### 10.8 Handing the group over (pairwise datagram, type 0x18 GROUP_HANDOVER, ADR-0038)

```
GROUP_HANDOVER (0x18) = n (1, 1..4) ‖ n × (len u16 ‖ signature (64) ‖ state (len bytes))
state     = a GROUP_STATE datagram (§10.1, its type byte 0x11 included) whose `admin` is the successor
signature = Ed25519(the identity of the admin that hands over, "lichen/1 handover" ‖ state)
```

Each `signature ‖ state` is a **link**: the admin of the state before it names the admin of this one.

- **Handing over.** The admin names a listed member other than itself its successor, with a new `epoch` whose `admin` is the successor: with the same members ("make admin": the former admin stays a member), or without itself (the admin leaves: `cause` 1, §10.1). It signs the state and sends one GROUP_HANDOVER (one link) to every other member, delivered as a state is (§10.1, every hour for a day). Its invitations stop at once (§10.5). An admin that leaves either hands over or closes the group; the apps ask which.
- **Accepting.** A receiver takes the links in order, against the admin and `epoch` it holds. A link whose state's `epoch` is not above the held one is skipped (it was taken). Otherwise the state must decode, be for the receiver's group (or a held invitation, §10.1), name another admin than the held one, and its signature must verify under the held admin's identity; if not, the receiver stops there. A link that passes is taken as a state from the admin (members, `cause`, key step or rotation, notices, §10.1, §10.9) and its successor is the admin the next link must be signed by. Whoever delivers it, the signature is the authority: no handover makes an admin the current one did not name, and none can be replayed (epochs only grow).
- **Keeping.** Every member keeps the last 4 links it took, stored with the group.
- **Catching up.** A member that missed a handover refuses the new admin's states (§10.1). When a listed member sends it a GROUP_STATE naming itself admin with a newer `epoch`, it keeps that state aside and sends that member GROUP_SYNC (§10.7), at most once every 10 minutes per group. The admin answers a GROUP_SYNC whose `epoch` is older than a link it keeps with one GROUP_HANDOVER holding those links, oldest first, before the state and the keys (§10.7). A member that is no longer the admin (it handed over) answers GROUP_SYNC from a listed member with the links newer than its `epoch`, alone, once per member every 10 minutes. The state kept aside, and SENDER_KEYS from its sender that come before the handover, are used once the handover makes that member the admin, and the member's next GROUP_SYNC may go to its new admin at once, without waiting the 10 minutes.
- **Limits.** A member that missed more than four handovers in a row cannot follow: it joins again with an invitation (§10.5). A GROUP_LEAVE that reaches the former admin after the handover is ignored (§10.4): the leaver stays listed until the new admin removes it.
- **Notices** (local, §6): kind 40, the former admin as actor and the successor as subject, on every phone ("Camille made Julien admin", "You made Julien admin", "Camille made you admin"); "Camille left" follows when the former admin left with it.

Cost: a GROUP_STATE plus 68 bytes to each member, as any state change.

### 10.9 Key step (ADR-0039)

```
K(e) = HKDF-SHA256(salt = gid, ikm = K(e − 1), info = "lichen/1 key step" ‖ e u32, 32 bytes)
```

- **A step per state epoch.** Every sender key is held at a **step**, the state epoch it belongs to: a key drawn at state `e` (creation, joining, a rotation after a departure, a restart) is at step `e`. A member that takes a state with a newer `epoch` `E` (from the admin, §10.1, or a handover, §10.8) replaces every key it holds at an earlier step, its own included, with its derivation at `E`, one HKDF per epoch in between. Key epochs, counters and both replay windows (messages and the core's own traffic, §10.3) carry on: the derived key continues the counter ranges of the key it replaces. Nothing travels: a step costs no airtime.
- **Keys travel with their step** (SENDER_KEY, SENDER_KEYS, §10.2). A received key at an earlier step than the receiver's epoch is stepped forward to it; one at a later step is kept there until the receiver's epoch reaches it. A key more than 64 steps from the receiver's epoch is dropped. A newcomer admitted by state `E` gets every key at `E` or later: HKDF is one-way, so nothing sent at an earlier step opens under its keys.
- **Every epoch steps, not only additions.** A member that missed states (they merge while they wait, §10.2) knows how many epochs it skipped, not what each one changed. A rename or a handover steps the keys too; a departure rotates them (§10.2).
- **Receiving across a step.** Members take a state at different times, and the envelope does not carry the step. For a member's key of the envelope's `key_epoch`, a receiver tries the key at its own step, then the next 8 steps (a sender that took a newer state first), then the steps from the one it held before its last step up to its own (frames sent before the step, late or recovered, §10.6); after several steps within 8 epochs it keeps the oldest. What opens is then checked as any envelope (replay window, signature, §10.3).

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

A direct copy counts as a reception of its member only once it verifies under that member's sender key (each copy is checked, the second one too): an envelope's clear header can be re-emitted by anyone near a searcher, with any signal strength (review of 2026-11-15, SR-16).

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

One direct position costs 38 bytes on air (7 header + 1 type + 14 + 16 tag).

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
- At most 375 frames (15 s): 1,313 bytes in 700C, 2,250 in 1200, 3,000 in 1600, 4,500 in 2400. Ten seconds on one hop: 875 bytes (6 fragments) in 700C, 2,000 (14) in 1600, 3,000 (21) in 2400.
- Before encoding, the sender removes DC and rumble (high-pass at about 100 Hz) and brings the speech to about -20 dBFS (at most +24 dB, peaks softly limited); in 700C the encoder's equaliser is on. Codec2 is sensitive to level: a quiet recording comes out muffled.

### 13.2 FILE (0x02): images and small files

`ts u32 ‖ kind u8 ‖ media_id [8] ‖ variant u8 ‖ width u16 ‖ height u16 ‖ full_len u16 ‖ data`

- `kind` 1: an AVIF image; `kind` 2: a small file, `kind` 3: a compressed small file (below). Other kinds are shown as unsupported.
- `media_id`: random, chosen by the sender; the same for both variants.
- `variant` 0: the **preview**, at most 426 bytes of AVIF (with the header above, exactly 3 fragments on one hop); `variant` 1: the **full** thumbnail, at most 4,096 bytes. Both fit in 160 × 120 pixels.
- `full_len`: size of the full variant, so the receiver can show its cost before asking; 0 when there is none.
- The sender sends the preview only. The full variant goes out when the receiver asks for it (§13.3), and the receiver replaces the preview with it.

**A small file** (`kind` 2, ADR-0022): any type, up to 8 KB, in one datagram, to a contact only (never flooded).

`ts u32 ‖ kind u8 (2) ‖ name_len u8 ‖ name (UTF-8, 1..=64 bytes) ‖ data (1..=8,192 bytes)`

- The name says what it is (its extension); the receiver opens it with whatever handles that type, and never runs it.
- The name is a label, never a path: whoever writes a copy (to open or share it) drops separators, control characters and leading dots and spaces, writes it into a new folder of its own, and never removes or overwrites anything at a path derived from it. A name such as `..` must not reach the file system.
- It is a medium: refused when its first round does not fit in the hour (§13.4). Its cost is shown before sending: 8 KB is about 56 fragments, measured in `docs/measurements.md`.
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
- An accept-list of contacts' endpoints is deliberately not used: the connection, and with it the knowledge that the provider is online, comes before any check the provider could make (review of 2026-11-15, SR-29). Anyone who knows a provider's endpoint id can tell whether iroh runs (§14.4); relay only hides the address.
- The requester writes the content to a file as it arrives (`<name>.part`), renamed only once the hash matches and removed on any failure or cancellation: a download never sits in memory (SR-33).
- The provider offers a blob while the app runs (synchronous mode), for 7 days after the announcement.
- An onion service accepts only `BEGIN` to port 1 and answers anything else with `END` (reason `DONE`), as other onion services do.

### 14.3 Keys

The iroh key is `HKDF(seed, "lichen/1 iroh")` (§2): stable for a phone, so connections reuse what the network learned, but distinct from the lichen identity. The Tor key is derived the same way with `"lichen/1 onion"`: those 32 bytes are the Ed25519 secret of the onion service, whose public key is the Tor `key` of announcements (the `.onion` address follows from it, rend-spec-v3 §6). Neither key is stored: both are derived again from the seed at every start.

### 14.4 Choosing a transport

- **iroh runs on demand** (review of 2026-11-15, SR-29): while something we offer is still served (7 days), while the user is about to send, or while a download goes through it. A phone that offers nothing is not reachable over iroh.

- **Sender**: an announcement lists iroh (when on and reachable) and Tor (when our onion service is reachable). Sending "via Tor" lists Tor only: the recipient never learns the sender's IP address.
- **Recipient**: tries iroh first (when on), then Tor, moving on only when the provider is unreachable or too slow; any other failure (unknown hash, size, hash mismatch) ends the fetch. A recipient without Tor connects to it on demand, without publishing an onion service.
- **Relay only**: iroh without direct paths; contacts see the relay, never our IP address (the relay does).

## 15. MeshCore chats (ADR-0026)

lichen also exchanges **plain MeshCore text** with people who use a stock MeshCore client: direct messages with any node the companion knows, and posts on the companion's channels. MeshCore encrypts and acknowledges these itself; lichen only drives the companion. They are **separate conversations**: a MeshCore chat never carries lichen content (no session, reply, reaction, media or edit), and a lichen chat never falls back to plain text. The user can still forward a text into a MeshCore chat, as they could paste it: it then leaves lichen's encryption, and the apps say so on the chat (review of 2026-11-15, SR-39). The two kinds of chat with the same person are never merged.

### 15.1 Receiving

The companion decrypts plain messages for us and keeps them in its offline queue, also while the phone is away.

- At connection (after the contacts), and on every push `MSG_WAITING` (0x83), the app sends `SYNC_NEXT_MESSAGE` (10) one at a time until `NO_MORE_MESSAGES` (10).
- `CONTACT_MSG` (0x10: `snr×4 i8 ‖ reserved (2) ‖ key prefix (6) ‖ path_len ‖ txt_type ‖ ts u32 ‖ text`, or legacy 7 without the first three bytes) is a direct message from the contact whose key starts with the prefix. A prefix that matches no known contact is held until the contacts are fetched again, then dropped.
- `CHANNEL_MSG` (0x11: `snr×4 ‖ reserved (2) ‖ channel index ‖ path_len ‖ txt_type ‖ ts u32 ‖ text`, or legacy 8) is a post on that channel slot. Its text is `name: body`; the name, as the sender's radio advertises it, is the author's label. A text without ": " has no label.
- Only `txt_type` 0 (plain text) is shown. Type 1 (CLI command) and type 2 (a room server's signed post) are dropped.
- `path_len` 0xFF means the message came direct; otherwise its low six bits are the flood path's hop count.
- A message with the same sender (or channel), timestamp and text as one received in the last hour is a retry and is dropped.
- Text is UTF-8; invalid sequences are replaced (U+FFFD). Plain messages are never parsed for lichen content.

### 15.2 Direct messages

- **Sending**: `SEND_TXT_MSG` (2): `txt_type 0 ‖ attempt ‖ ts u32 ‖ recipient key prefix (6) ‖ text`, text ≤ **160 bytes**. The companion answers `SENT` (6): `flood u8 ‖ ack u32 ‖ timeout_ms u32`; push `SEND_CONFIRMED` (0x82) with the same `ack` means delivered.
- **Retries**, as stock clients do: when no confirmation arrives within `timeout_ms` + 2 s, the same text and timestamp go again with `attempt` + 1. Before the third and last attempt the app sends `RESET_PATH` (13), so that one floods. With no confirmation after it, the message failed. An `ERR` to `SEND_TXT_MSG` fails it at once.
- One plain direct message is in flight at a time; the others wait in order. They wait while the companion is away, like lichen texts (§6.1), and expire after 24 hours.

### 15.3 Channels

A channel is a 16-byte secret held in one of the companion's channel slots, with a name of at most 31 bytes.

- **Reading**: at connection the app reads the slots with `GET_CHANNEL` (31) → `CHANNEL_INFO` (0x12: `index ‖ name (32, NUL-padded) ‖ secret (16)`), from index 0, until `ERR`, the slot count in `DEVICE_INFO`, or four empty slots in a row. An empty name and a zero secret is a free slot.
- **Identity**: a channel is `SHA-256(secret)[0..8]` in the app, so its history follows the channel, not the slot.
- **Joining**: `SET_CHANNEL` (32): `index ‖ name (32) ‖ secret (16)` into the first free slot.
  - *Public*: name `Public`, secret `8b3387e9c5cdea6ac9e5edbaa115cd72` (MeshCore's public channel).
  - *Hashtag*: name `#` + the tag, lower-cased (letters, digits and `-`), secret `SHA-256("#" ‖ tag)[0..16]`, as stock clients derive it.
  - *Private*: a name and 16 bytes given by whoever created it (32 hex digits).
  - A channel already in a slot is not added twice.
- **Sharing by QR code** (ADR-0043), in the stock app's format (MeshCore's `docs/qr_codes.md` at 81dbae5): `meshcore://channel/add?name=<name>&secret=<32 hex digits>`, the name percent-encoded (UTF-8; lichen escapes every byte but letters, digits and `-._~`; `+` reads as a space). Other parameters (`region_scope`) are ignored. A code whose name is empty or longer than 31 bytes, or whose secret is not 16 bytes or is all zero, is refused. Anyone who scans it reads and posts on the channel: the apps say so before showing it. Vectors in `testdata/meshcore/mesh.toml` (`[[qr]]`).
- **Leaving**: `SET_CHANNEL` with an empty name and a zero secret.
- **Posting**: `SEND_CHANNEL_TXT_MSG` (3): `txt_type 0 ‖ index ‖ ts u32 ‖ text`. The companion floods `name: text`, so `text` ≤ 160 − 2 − the length of our node's name. `OK` means the post left; nobody acknowledges it.

### 15.4 Our name

What MeshCore users see is our node's advertised name, and channel posts start with it. The app sets it with `SET_ADVERT_NAME` (8), at most 31 bytes, then sends a flooded advert so that other radios learn it.

### 15.5 Airtime

Every plain transmission counts in the hourly budget (§9) like lichen's:

- a direct message, each attempt: `TXT_MSG` with a header and path length byte, the path, two 1-byte hashes, a 2-byte MAC and the ciphertext, `5 + text` bytes rounded up to 16;
- a channel post: `GRP_TXT` with a header and path length byte, the channel hash, a 2-byte MAC and `5 + name + 2 + text` bytes rounded up to 16.

The apps show the estimate before sending, as for lichen messages.

### 15.6 What the apps show

- A MeshCore chat is marked as such in the conversation list.
- Its header says it is not lichen-encrypted:
  - **direct messages** are encrypted by MeshCore between the two radios: AES-128, a 2-byte MAC, no forward secrecy, and the companion sees the text;
  - **a channel** is readable by everyone who has its secret, and Public by anyone in range.
- The composer is text only and counts bytes.
- A channel joined from lichen starts muted.
- A MeshCore direct message notifies as "Name · MeshCore", never under the bare name a lichen conversation uses: a node can send plain text under any name (review of 2026-11-15, SR-30).

### 15.7 History from the MeshCore app (ADR-0033)

The official MeshCore app's exported database can be imported once: its plain direct messages go to the MeshCore chat with their node, its channel posts to the channel of their secret (`channel_id(secret)`, §15.3), as received or sent, with the sender's time, read. Other text types are left out; a message already there (same chat, direction, time and text) is skipped. The file is read from a private copy, read-only and immutable, in SQLite's defensive mode.


### 15.8 Room servers (ADR-0043)

A room server is a MeshCore node (type 3) that keeps the last 32 posts of a room and sends them to the clients logged in; stock clients show its posts as a conversation. Its posts are as private as a direct message to it (MeshCore's AES-128 between our radio and the room, readable by the room and by everyone logged in): a room chat is a MeshCore chat (§15.6), with its own kind in the store.

- **Login**: `SEND_LOGIN` (26, `MyMesh.cpp` L1577): `key (32) ‖ password` (the room's guest or admin password, 15 bytes at most: `BaseChatMesh.cpp` L603). The companion sends an anonymous request with our `sync_since` for that room, the time of the newest post it acknowledged (L611, L291–L293), and answers `SENT` (`ack` = the room key's first 4 bytes). The room answers a known password with push `LOGIN_SUCCESS` (0x85, `MyMesh.cpp` L704): `is_admin ‖ key prefix (6) ‖ server time u32 ‖ permissions ‖ firmware level`, the role in permissions bits 0–1 (`ClientACL.h`: 0 guest, 1 read only, 2 read and write, 3 admin; `simple_room_server/MyMesh.cpp` L343–L387: the guest password gives read and write). A wrong password gets no answer at all, so the login gives up after `SENT`'s estimate plus 4 s ("no answer: a wrong password, or out of reach"); `LOGIN_FAIL` (0x86, L717) is for an answer that is not a welcome.
- **Catch-up**: once logged in, the room sends every post newer than `sync_since`, one at a time, each acknowledged by the companion (`simple_room_server/MyMesh.cpp` L995–L1030), then each new one; it never sends us our own. They are queued like direct messages: `CONTACT_MSG` from the room's key prefix with `txt_type` 2 (`MyMesh.cpp` L550), the text being `author key prefix (4) ‖ post`. The author is named after the contact whose key starts with it, the room itself for its own notices, else shown in hex. A signed post from a node that is not a room server is dropped. Posts are stored as "author: text", like channel posts (§15.1), and a repeat within the hour is dropped.
- **Posting**: a direct message to the room (§15.2, `txt_type` 0); the room acknowledges what it keeps (`SEND_CONFIRMED`), a reader without the right to post gets no acknowledgement and the post fails after its three attempts.
- **Remembering**: the user may keep a room's password on the phone; lichen then logs in to it at each connection, once the radio's contacts are read, so that the posts since the last time come. A repeater's password is never kept (§16.5).
- One login, status or binary request is out at a time: the companion forgets the one it waits for when it gets another (`clearPendingReqs`, `MyMesh.cpp` L1588). lichen queues them.

## 16. Mesh tools (ADR-0041)

What lichen shows of the MeshCore network itself, from the companion, as the stock app does. MeshCore facts come from the firmware's sources at v1.18 (`https://github.com/meshcore-dev/MeshCore/blob/81dbae5/…`; `MyMesh.cpp` is `examples/companion_radio/MyMesh.cpp`, `BaseChatMesh.cpp` is `src/helpers/BaseChatMesh.cpp`), with the line of each handler; vectors in `testdata/meshcore/mesh.toml`, written from them by `mesh.py`. Nothing here is lichen-encrypted, and nothing is stored: the radio is the source of truth and is read again at each connection.

### 16.1 Nodes

- **The radio's contacts** come with `GET_CONTACTS` (§1). Each record (`MyMesh.cpp` L171, `writeContactRespFrame`) is `key (32) ‖ type ‖ flags ‖ out_path_len ‖ out_path (64) ‖ name (32, NUL-padded) ‖ last_advert u32 ‖ lat i32 ‖ lon i32 ‖ lastmod u32`: `type` 1 companion, 2 repeater, 3 room server, 4 sensor; `last_advert` is the node's own clock at its last advert, `lastmod` the radio's clock when it last heard from it (an advert, a message: `BaseChatMesh.cpp` L198, L240, L294), shown as "last heard". `lat`, `lon` are degrees × 10⁶ from the node's last advert that carried a position (L193); both 0 means none.
- **Nodes heard but not added.** An advert from a node that the radio does not add (manual add without its type in the auto-add mask, more hops than the auto-add limit, or a full table: `BaseChatMesh.cpp` L156–L184) comes as push `NEW_ADVERT` (0x8A, `MyMesh.cpp` L361) with the same record; push `ADVERT` (0x80) is for a contact, read again with `GET_CONTACT_BY_KEY`. lichen lists them, marked as not on the radio, the 512 most recently heard at most.
- The list is the radio's contacts and those nodes, the most recently heard first, with their type, hops (`out_path_len`, §1) and last heard. A contact lichen keeps that the radio did not report (someone known from a group) is not a node of this radio.
- **Map layer:** the nodes whose record carries a position, at that position, with their type. Only what their owners chose to advertise in clear (ADR-0032); lichen never derives a position for a node.

### 16.2 Paths

A contact's `out_path` (§1) is its route from our radio: one hash per repeater, the first bytes of its key (1 to 3 bytes, `path_len` bits 6–7). The apps show it hop by hop, each hash named after the repeaters the radio reports (type 2, §16.1) whose key starts with it: none known shows the hash in hex, several show all of them (a 1-byte hash is often shared). `None` (0xFF) means the radio floods to reach the node; 0 hops that it hears it directly.

### 16.3 Traces

- **Sending**: `SEND_TRACE_PATH` (36, `MyMesh.cpp` L1813): `tag u32 ‖ auth u32 ‖ flags ‖ path`. `flags & 3` gives the size of each hash in `path`, `1 << (flags & 3)` bytes; the path is a whole number of hashes, at most 64 of them and 178 bytes. The companion answers `SENT` (`flood` 0, `ack` = `tag`, `timeout_ms` its estimate) or `ERR 6`. lichen draws `tag` at random and sends `auth` 0 (v1.18 carries it without checking it).
- **On the air** (`src/Mesh.cpp` L42–L68): a direct `TRACE` (type 9) whose payload is `tag ‖ auth ‖ flags ‖ hashes`; the repeater whose key matches the next hash appends the SNR × 4 at which it heard the packet to the packet's path and forwards it; once every hash is used, any node that hears it reports it. Companions do not forward, so a trace only goes through repeaters.
- **The path traced**: the contact's `out_path`, plus the node's own hash when it is a repeater, then the same hashes back without the last one, so that the last transmission reaches our radio again (`h1 h2 h1` for a contact two repeaters away). 1-byte paths are traced with 1-byte hashes (`flags` 0); 2- and 3-byte paths with their first two bytes (`flags` 1). Nothing to trace (a node heard directly that is not a repeater, or one the radio floods to) is refused; so is a trace whose answer would not fit a Bluetooth frame (§1, 10).
- **The answer**: push `TRACE_DATA` (0x89, `MyMesh.cpp` L855): `reserved ‖ path_len ‖ flags ‖ tag u32 ‖ auth u32 ‖ hashes (path_len bytes) ‖ SNR × 4 per hop (path_len >> (flags & 3), i8) ‖ final SNR × 4 (i8)`, the last one ours. It is matched by `tag`. No answer within `timeout_ms` + 3 s, or the radio gone before it came, is "no answer"; the apps show where each SNR was measured, out and back.
- **Airtime**: our transmission, `2 + 9 + path` bytes, counts in the budget (§9); a trace that does not fit is refused with the wait. One trace per node at a time.

### 16.4 Saved radios (ADR-0042)

The apps keep the radios the phone was paired with (Bluetooth identifier, USB device or TCP address, link, name, node key, last use) in the database, to switch between them. The history, contacts, groups and keys stay with the phone's identity (ADR-0010, ADR-0013), whichever radio carries them; each radio keeps its own MeshCore contacts, channels and settings, and the people a lichen user writes to see another node when the radio changes. The radio's battery is read at connection and again at most every 10 minutes while connected, when the app runs anyway, so that it never wakes the app by itself (`GET_BATT_AND_STORAGE`, §1.1); at or below 15 % (3.3 V empty, 4.2 V full) the apps warn.

### 16.5 Repeaters (ADR-0044)

A repeater (type 2) answers its admin and guests (`examples/simple_repeater/MyMesh.cpp` at 81dbae5). lichen offers: login, status, neighbours and a small command line. Requests are queued one at a time (§15.8), and each counts in the budget as a direct packet of a few blocks.

- **Login**: as for a room (§15.8), without `sync_since` (`BaseChatMesh.cpp` L615–L618): the admin password gives role 3, the guest password role 0 (`MyMesh.cpp` L90–L143). An empty password logs in a client the repeater already lists.
- **Status**: `SEND_STATUS_REQ` (27, `MyMesh.cpp` L1630): `key` → `SENT`, then push `STATUS_RESPONSE` (0x87, L724): `reserved ‖ key prefix (6) ‖ stats`, matched by the prefix. A repeater's stats are `RepeaterStats` (`simple_repeater/MyMesh.h` L44, 56 bytes, little-endian): `battery mV u16 ‖ TX queue u16 ‖ noise floor i16 ‖ last RSSI i16 ‖ packets received u32 ‖ sent u32 ‖ airtime s u32 ‖ uptime s u32 ‖ sent flood u32 ‖ sent direct u32 ‖ received flood u32 ‖ received direct u32 ‖ error events u16 ‖ last SNR × 4 i16 ‖ direct duplicates u16 ‖ flood duplicates u16 ‖ receive airtime s u32 ‖ receive errors u32`. A room server's (`ServerStats`, `simple_room_server/MyMesh.cpp` L24, 52 bytes) has the same first 48 bytes, then `posts u16 ‖ post pushes u16`. Guests may ask for it.
- **Neighbours**: `SEND_BINARY_REQ` (50, `MyMesh.cpp` L1720): `key ‖ request` → `SENT` (`ack` = a tag), then push `BINARY_RESPONSE` (0x8C, L750): `reserved ‖ tag u32 ‖ answer`, matched by the tag. The request is `GET_NEIGHBOURS` (`simple_repeater/MyMesh.cpp` L276): `0x06 ‖ version 0 ‖ count ‖ offset u16 ‖ order ‖ key prefix length ‖ 4 random bytes`; lichen asks for 255 from offset 0, the strongest first (order 2), with 6-byte prefixes. The answer is `total u16 ‖ count u16 ‖ count × (key prefix ‖ heard seconds ago u32 ‖ SNR × 4 i8)`, as many as fit 130 bytes (11 with 6-byte prefixes); lichen asks for the next page while the total is not reached, four pages at most. A neighbour is named after the node the radio knows with that prefix.
- **Command line**: `SEND_TXT_MSG` (§15.2) with `txt_type` 1 (CLI data, `MyMesh.cpp` L1132): the companion sends it once, without acknowledgement, under its own clock. A repeater runs it for an admin only (`simple_repeater/MyMesh.cpp` L702) and answers with a `txt_type` 1 message (L749), queued as `CONTACT_MSG` from its prefix: a console line, never stored and never in a conversation. A type 1 message from a person's radio (type 0 or 1) is dropped. Commands are at most 160 bytes; the apps show what was sent and what came back, in order.
- **Logout**: `LOGOUT` (29, L1748) stops the radio's keep-alive with that server; lichen sends it when the user logs out, and forgets the password.

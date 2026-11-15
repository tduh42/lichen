# Specification — Application protocol over MeshCore

Nov 6, 2026 · Tanguy · English translation of [`design.fr.md`](design.fr.md). Where the two differ, the French original is the author's text; the corrections decided since are in [`protocol.md`](protocol.md) and the ADRs.

## Context and goals

An encrypted application layer on top of MeshCore, running on the host (phone or PC), that requires no change to the repeaters' or companions' firmware. It adds what MeshCore does not do: long messages, modern crypto, private groups, media, location and person search.

**Goals**

- Reliable fragmentation and reassembly of datagrams up to about 10 KB.
- Modern authenticated encryption (AEAD), replacing MeshCore's crypto.
- Private groups with sender authentication.
- Images and voice, over LoRa as a fallback and through a temporary Internet bridge (iroh or Tor) when available.
- Location sharing with per-recipient precision control.
- Off-grid search mode: find a group member with ThinkNode M5 devices (RSSI, bearings, SOS).
- iOS (SwiftUI) and Android (Jetpack Compose) apps.

**Non-goals**

- Real-time voice calls over LoRa (duty cycle).
- Compatibility with official MeshCore clients for the new features: they simply ignore our packets. Plain text with them is supported, in separate "MeshCore" chats that never carry lichen content (ADR-0026, protocol §15).
- Replacing MeshCore routing: we use it as is.

## Overall architecture

All logic (fragmentation, crypto, groups, media, location, search) lives in a Rust core shared by the iOS and Android apps. The companion and the MeshCore repeaters stay on stock firmware. The only planned exception is the search mode embedded on the ThinkNode M5.

The core picks the transport in order of preference: iroh first, Tor as a fallback, LoRa when there is no Internet. Only the LoRa path goes through the companion and the MeshCore network.

## MeshCore constraints

MeshCore repeaters forward any payload in direct mode except four special types, which makes `RAW_CUSTOM` usable without touching the network. Findings from the `meshcore-dev/MeshCore` firmware, commit `81dbae5` (October 15, 2026), files `src/Mesh.cpp`, `src/Packet.cpp` and `examples/companion_radio/MyMesh.cpp`.

**On-air frame**: `header(1) | transport_codes(4, optional) | path_len(1) | path(0–64) | payload(0–184)`, 255 bytes in total at most. The 255-byte ceiling comes from the radio (1-byte LoRa length field): the MTU cannot grow, we can only fragment.

| Payload type | Forwarded in direct | Forwarded in flood | Use in this protocol |
| --- | --- | --- | --- |
| `RAW_CUSTOM` (0x0F) | Yes | No | Unicast transport for all our messages |
| `GRP_DATA` (0x06) | Yes | Yes, even without decrypting | Flood envelope for groups |
| Reserved types 0x0C–0x0E | Yes | No | Avoid: dropped by endpoints, possible clash with a future type |
| `MULTIPART` (0x0A) | ACK subtype only | No | Avoid: any other subtype is dropped at the first repeater |

**Pitfalls to respect**

1. **Deduplication**: the anti-duplicate hash covers the type and the payload, not the path (`Packet::calculatePacketHash`), with about 160 entries per repeater. A fragment resent identically is dropped: each resend must differ by at least one byte.
2. **No addressing in RAW**: at the last hop, every node in range passes the packet up. Destination and source are in our header.
3. **No crypto in RAW**: all encryption is our job.
4. **MeshCore crypto is insufficient**: AES-128 with independent blocks (ECB-like) and an HMAC truncated to 2 bytes. We never rely on it.
5. **Location in clear**: adverts carry lat/lon in clear and are flooded. Our protocol never puts a location in an advert.

**Companion interface used (stock firmware)**

| Command | Role |
| --- | --- |
| `CMD_SEND_RAW_DATA` (25) | Send a RAW payload of 4 to 184 bytes in direct mode, path supplied |
| `CMD_SEND_RAW_PACKET` (65) | Inject an arbitrary packet, flood included |
| `PUSH_CODE_RAW_DATA` | RAW received, with SNR and RSSI |
| `PUSH_CODE_LOG_RX_DATA` (0x88) | Every raw frame received, with SNR and RSSI |
| Repeat option (`isRepeatEn`) | The companion forwards other nodes' packets |

## Fragmentation layer

A datagram is cut into at most 64 fragments of about 160 useful bytes each, i.e. about 10 KB. Fragments are acknowledged by bitmap, in a mode inspired by SCHC ACK-on-Error (RFC 8724). Fragmentation is only allowed with direct routing.

**Common header (7 bytes, at the start of the RAW payload)**

| Field | Size | Content |
| --- | --- | --- |
| `dst` | 1 B | Hash of the destination (or group) |
| `src` | 1 B | Hash of the sender |
| `tag` | 2 B | Random datagram ID, unique per src/dst pair |
| `kind + index` | 1 B | Bits 7-6: type (00 data, 01 ACK, 10 abort, 11 control). Bits 5-0: fragment index (0–63) |
| `total` | 1 B | Number of fragments (1–64), repeated in every fragment |
| `attempt` | 1 B | Attempt number: makes every resend unique for deduplication, and is part of the nonce |

Then come the fragment's ciphertext and an 8-byte AEAD tag. Budget: 184 − 7 − 8 = 169 useful bytes per fragment at most.

**ACK (9-byte encrypted payload)**: `bitmap(8) | flags(1)`. Each bitmap bit stands for a received fragment.

**Rules**

1. The receiver only sends the ACK when the last fragment arrives or when a reassembly timer expires.
2. The sender only resends the fragments missing from the bitmap, with `attempt` incremented. Beyond a maximum number of rounds, it sends an abort.
3. Reassembly is end to end. Repeaters are stock MeshCore repeaters and keep no state.
4. The reassembly buffer is allocated on receipt of the first fragment, whatever its index, thanks to the `total` field.
5. A fragment is only accepted after its AEAD tag is verified: a forged fragment cannot poison the buffer.
6. No fragmentation in flood: a 30-fragment flooded datagram would cost 30 transmissions per repeater. A path discovery (`PATH`) precedes the first send.

Order of magnitude of reliability without selective ACK: with 10 % loss per hop, 10 fragments over 3 hops all arrive only about 4 % of the time (0.9³⁰). Hence the bitmap.

## Cryptography

Each fragment is encrypted separately with ChaCha20-Poly1305, with an implicit nonce (0 bytes transmitted) and, since ADR-0028, the full 16-byte tag (the original design truncated it to 8 bytes), under a session key derived by HKDF.

**Choice of algorithm**

| Option | Verdict | Reason |
| --- | --- | --- |
| ChaCha20-Poly1305 | Chosen | Fast in software on the host, constant time, mature `chacha20poly1305` crate (RustCrypto) |
| Ascon-AEAD128 | For embedded | NIST standard for constrained MCUs, to keep if the layer moves down to the node |
| AES-GCM | Rejected | Fragile with a truncated tag (Ferguson's attack) |
| MeshCore crypto | Rejected | Independent AES blocks and a 2-byte MAC, forgeable in about 65,536 tries |

**Implicit nonce (96 bits)**: `src ‖ tag ‖ index ‖ attempt ‖ session counter`. All fields are in the header or known to both sides; nothing extra is transmitted. The whole header is the AAD.

**Absolute rule**: `attempt` is in the nonce, not only in the AAD. Resending with the same nonce and a different AAD would give two Poly1305 tags under the same one-time key, which enables forgery. An automated test must check that no nonce is ever reused.

**Tag**: the original design accepted an 8-byte tag, on the grounds that each forgery attempt costs a radio transmission of at least 100 ms subject to the duty cycle. The review of 2026-11-15 showed that this bound does not hold for a frame the attacker has heard, and that frames can also arrive at wire speed through the companion link; the tag is now the full 16 bytes of RFC 8439 (ADR-0028).

**Key management**

1. Starting secret: the static X25519 secret MeshCore already derives between two contacts.
2. Session opening: exchange of random salts, session key = HKDF-SHA256(secret, salts, context). The nonce counter restarts at zero with a new key, never with the old one, even after a reboot.
3. Forward secrecy, recommended as soon as a transfer exceeds a few fragments: ephemeral X25519, 32 bytes each way, i.e. a single packet.
4. Session keys expire by time or volume, to be set.

## Private groups

Each member encrypts with their own sender key, on the Signal model. Group messages travel by flood in a `GRP_DATA` envelope and are limited to text and short announcements.

**Transport**

- Envelope: `GRP_DATA` = `channel_hash(1) | our data`. Repeaters flood it without decrypting.
- Sent with `CMD_SEND_RAW_PACKET`, received through `PUSH_CODE_LOG_RX_DATA`. No modified firmware.
- Known side effect: a stock node subscribed to a channel with the same hash will show noise about once in 65,536. Pick a rarely used `channel_hash`.
- Limits: 1 to 2 fragments per message, capped hop count. Images and voice are never flooded: we flood an announcement, and the content goes through the bridge (see below).

**Group crypto**

1. **One sender key per member**: a single shared key would let any member impersonate the others. Each member distributes their sender key to the others over the pairwise X25519 channels.
2. **Nonce**: it includes the sender ID, so that no nonce is shared between two members.
3. **Rekey**: sender keys are redistributed whenever a member leaves. MLS is too heavy for LoRa.
4. **Non-repudiation (optional)**: 64-byte Ed25519 signature, on the reassembled datagram only, never per fragment.

## Internet bridge and transport ladder

LoRa serves as an authenticated signalling channel: an announcement of about 70 bytes, a single packet, lets the recipient fetch heavy content over the Internet through iroh or Tor. The content's integrity is guaranteed by the LoRa message.

**Announcement message (encrypted, 1 fragment)**

| Field | Size | Content |
| --- | --- | --- |
| `content_type` | 1 B | Image, voice, file, other |
| `transports` | 1 B | Bitmask: iroh, onion, LoRa |
| `peer_key` | 32 B | Ed25519 public key: iroh NodeId and/or v3 onion service key |
| `blob_hash` | 32 B | BLAKE3 hash of the content |
| `meta` | ~4 B | Size, codec, duration |
| `blob_key` | 32 B, optional | Decryption key, in asynchronous mode |

**Transport ladder** (the recipient takes the first that works)

1. **iroh direct**: QUIC connection by public key, NAT hole punching, relay as a fallback. Transfer with `iroh-blobs`, verified in BLAKE3 as it streams.
2. **Tor onion**: the onion address derives from the 32-byte public key. Target library: Arti, the official Rust implementation. The maturity of onion-service hosting there is to be checked.
3. **Fragmented LoRa**: progressive direct send, off-grid.

**Modes**

- **Synchronous**: the sender serves the blob until it has been fetched.
- **Asynchronous**: the sender drops the encrypted blob on any storage and sends `blob_hash + blob_key`.
- **Relay only or Tor only**: for users who do not want to expose their IP.

**Security**: the peer's identity and the hash are pinned by an authenticated LoRa message (pairwise channel or group sender key). A malicious Internet relay can neither impersonate the peer nor substitute the content. No PKI needed.

## Media: images and voice

Media go through the Internet bridge as soon as it is available. Over LoRa, only small media in direct mode are allowed: thumbnails and voice messages, never calls.

| Medium | Over LoRa | Over iroh direct | Over Tor |
| --- | --- | --- | --- |
| Image | 160×120 AVIF or WebP thumbnail, 2 to 4 KB, 15 to 25 fragments | Full resolution | Full resolution |
| Voice message | Codec2 700C, ~875 B for 10 s, 6 fragments | Opus | Opus |
| Real-time call | No (duty cycle) | Yes, QUIC + Opus | Not recommended: latency often above 1 s |

**Reference LoRa cost (SF8, 62.5 kHz)**: about 1.4 s of airtime per full fragment. A thumbnail takes about 30 s of transmission per hop, and each repeater spends its own as well.

**Rules**

1. Never flood media. In a group, the announcement is flooded and each member fetches the content through the bridge.
2. Progressive send over LoRa: a heavily compressed preview of 3 or 4 fragments first, then the rest only if the recipient asks for it.
3. Built-in rate control: the sender keeps a sliding airtime counter and refuses or defers a send that would exceed the duty cycle.
4. Link adaptation: if the SNR reported by the companion allows it, switch to a lower SF.
5. Codec2 gives a robotic but intelligible voice. To check: which Rust binding or port supports the 700C mode.

## Location sharing

A position fits in 13 encrypted bytes, about 30 bytes on air with the header and the tag (≈ 0.3 s at SF8/62.5 kHz). It is only sent when the deviation from the extrapolated trajectory exceeds a threshold, and each recipient gets the precision allowed to them.

**Format**

| Field | Size | Encoding |
| --- | --- | --- |
| `lat` | 3 B | 24 bits, ≈ 1.2 m |
| `lon` | 3 B | 24 bits, ≈ 2.4 m at the equator |
| `alt` | 2 B | Metres, signed |
| `dt` | 2 B | Seconds since the session timestamp |
| `heading + speed` | 2 B | Heading on 9 bits, speed on 7 bits |
| `precision` | 1 B | Uncertainty radius, log scale |

**Send rate**

- Trajectory estimation: sender and receiver extrapolate with heading and speed. Send when the deviation exceeds the threshold (for example 50 m). Nothing is sent while stationary.
- Slow heartbeat to guarantee a minimal update rate.
- In a group (flood): minimum interval of several minutes, or pull mode (a member asks for the position in direct mode).
- Bridge available: real-time tracking over iroh or Tor, LoRa remains the fallback.

**Privacy**

1. **Per-recipient precision**: rounding on a fixed grid (for example 1 km for a wide group). Never random noise: averaging several messages would give back the exact position.
2. **Freshness**: the timestamp is in the nonce and the AAD. A position older than the last one received is rejected, which prevents replay.
3. **Time-limited sharing** (1 h, 8 h…): the sharing key expires with it.
4. **Never in MeshCore adverts.**
5. **Physical limit**: encryption protects the content, not the emission. An observer can locate the sender by direction finding or RSSI, and regular beacons are recognisable. For sensitive use: irregular intervals and pull mode.

**SOS**: a flag that triggers maximum precision, high priority and an exceptional right to flood to all trusted contacts.

## Search mode

Reference scenario: no repeater left, and several group members equipped with ThinkNode M5s are looking for another member. A single packet sent by the target is received by every searcher in range: one transmission gives N measurements, fused in a particle filter.

**Hardware: ThinkNode M5 (Elecrow)**

| Item | Present | Use |
| --- | --- | --- |
| SX1262, up to +22 dBm | Yes | Beacon, pings, RSSI and SNR measurements |
| Multi-constellation GPS | Yes | Position of searchers (anchors) and target |
| 200×200 e-paper, 2 buttons | Yes | Arrow and estimated distance without a phone |
| ESP32-S3, 8 MB PSRAM | Yes | On-device particle filter (~1,000 particles) |
| RP-SMA connector | Yes | 868 MHz Yagi antenna for bearings |
| 1,200 mAh battery | Yes | Constrains the beacon interval |
| Compass, IMU | No | Bearings from the phone or compass-free techniques |

**Operation**

1. **Searchers form the network**: repeat option enabled on their companions. An M5 placed on high ground serves as a temporary repeater.
2. **Target's SOS beacon**: GPS if available, battery, counter, adaptive interval. It also answers the searchers' pings.
3. **Shared measurement (~16 B, flooded in the group)**: searcher's GPS position, RSSI, SNR, timestamp, optional bearing.
4. **Fusion**: particle filter (weighted least squares for initialisation), combining RSSI distances, bearings and measurements accumulated while moving. Shown as a probability map that tightens.

**Measurement sources**

| Method | Hardware | Realistic accuracy |
| --- | --- | --- |
| RSSI → distance | Stock M5 | Area of hundreds of metres to a few km. ±1 dB ≈ ±8 % distance, and obstacles add 6 to 8 dB |
| Body shadowing | Stock M5 | Coarse direction: signal minimum when the body blocks |
| Gradient while walking | Stock M5 + GPS track | Tells whether you are getting closer, without a compass |
| Yagi + phone compass | Yagi on RP-SMA | Bearing ±10 to 20° in open terrain |
| Switched sector antennas | Dedicated DF station, modified firmware (`GetRssiInst`) | Bearing ±20 to 30° in a single packet |
| Phase angle of arrival | Coherent SDR (e.g. KrakenSDR) | A few degrees, fixed station |
| Time of flight | SX1280 (2.4 GHz) | Good, but short range |

Order of magnitude: at 1 km, a ±10° bearing gives about 175 m of lateral error. Two bearings crossing at right angles give an area about 200 m across.

**Field pitfalls**

- Saturation below about 100 m: the target lowers its power on request, or the searcher adds an attenuator.
- Beacon SF: SF12 to detect (about +10 dB of range compared to SF8, but about 6 times more airtime), then a low SF for close tracking.
- The duty cycle applies to search too.

**Consent**: locating a member requires their prior opt-in. Hunting a hostile transmitter (jammer, malicious node) falls under a different policy.

## Radio and regulatory constraints

Airtime limits the protocol, not bandwidth. Every feature must fit the European duty cycle applicable to its sub-band, with a budget computed on the host.

| Load | Size on air | Airtime (SF8, 62.5 kHz) |
| --- | --- | --- |
| Position | ~30 B | ~0.3 s |
| Full fragment | 255 B | ~1.4 s |
| 10 s voice message | 6 fragments | ~8 s per hop |
| Image thumbnail | 15 to 25 fragments | ~20 to 35 s per hop |
| Full fragment at SF12, 125 kHz | 255 B | ~9 s |

- In the 10 % sub-band, a node has about 360 s of transmission per hour. Repeaters also spend their budget on our packets.
- The app keeps a sliding counter per node and shows the remaining budget before costly sends.
- Values to confirm: the sub-band and preset (SF, BW) actually used by the target MeshCore network.

## Threat model and security

The protocol protects message content and authenticity against an active radio attacker. It protects neither against physical location of transmitters nor against traffic analysis.

| Threat | Countermeasure | Residual risk |
| --- | --- | --- |
| Passive eavesdropping | AEAD per fragment | Visible metadata: src/dst hash, size, rhythm |
| Forged fragment or buffer poisoning | Tag verified before accepting a fragment, random datagram tag | DoS by channel saturation |
| Replay | Timestamp and counter in the nonce, older messages rejected | None known |
| Nonce reuse | `attempt` in the nonce, new session key after reboot, automated test | Implementation bug |
| Impersonation within a group | Sender key per member, optional signature | A legitimate member can still lie about their own position |
| MITM on the Internet bridge | Peer key and hash pinned by the authenticated LoRa message | The relay operator sees who talks to whom (avoidable with Tor) |
| IP exposure | Relay-only or Tor-only mode | None in those modes |
| Transmitter location | Irregular intervals, pull mode | Direction finding and RSSI remain possible |
| Position spoofing | Consistency with the group's RSSI and bearing measurements | Detection only, no proof |
| False RSSI distance (boosted power) | Time of flight (distance bounding) with SX1280 | RSSI alone is unreliable against an attacker |
| Jammer or malicious node | Direction-finding hunt (search mode) | No prevention |

Priority fuzzing targets: the fragment header, ACK, announcement and companion serial protocol parsers. Everything else is encrypted and authenticated before being parsed.

## Open questions and next steps

Next step: the app mockups (SwiftUI for iOS, Jetpack Compose for Android), then a Rust core shared by both.

**Open questions**

- [ ] Sub-band and LoRa preset (SF, BW) of the target network: they set every airtime budget.
- [ ] Maturity of onion-service hosting in Arti.
- [ ] Rust binding or port of Codec2 supporting the 700C mode.
- [ ] `channel_hash` value for the group envelope.
- [ ] Lifetime and maximum volume of a session key.
- [ ] Real RSSI accuracy in the target terrain (to measure before designing the solver).

**Plan**

1. iOS and Android mockups: conversations, groups, map and location, search mode, airtime budget.
2. Rust core (`no_std` for fragmentation and crypto), exposed to Swift and Kotlin through UniFFI. Client for the companion serial/BLE protocol.
3. Test harness with simulated losses and parser fuzzing.
4. Field recording tool: RSSI, SNR, GPS and bearing, with 3 or 4 nodes at known positions.
5. iroh bridge, then Tor.
6. "Search mode" firmware for the `thinknode_m5` variant: SOS beacon, pings, on-device filter, e-paper display.

**Sources**

- [MeshCore — Packet Format](https://docs.meshcore.io/packet_format/)
- [MeshCore — Payload Format](https://docs.meshcore.io/payloads/)
- [MeshCore Protocol Specification, section 11 Multipart (community)](https://swaits.github.io/meshcore-spec/v0.1.0/11-payload-multipart.html)
- [MeshCore source code](https://github.com/meshcore-dev/MeshCore), commit `81dbae5`
- [ThinkNode M5 — datasheet (Passion Radio)](https://www.passion-radio.com/lpwan-iot/ilm13205d1-3175.html)
- [ThinkNode M5 — datasheet (Ameridroid)](https://ameridroid.com/products/thinknode-m5-meshtastic-radio-1)

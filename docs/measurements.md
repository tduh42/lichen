# Measurements

Dated log of measurements (fuzzing, fake network, field). One entry = the command, the context, the numbers.

## 2026-11-06 — ACK-on-Error on a single lossy link (`lc-proto`)

`cargo test -p lc-proto --release --test transfer -- --ignored --nocapture`: 1,000 transfers of a full datagram (64 fragments of 157 bytes, 10,048 bytes) between two `Session`s, each frame lost independently with the given probability in both directions (data, ACKs, aborts), default `Timing`. "Frames" counts every frame put on the air, both directions, for delivered datagrams. 30 % end-to-end is about what 10 % loss per hop gives over 3 hops (0.9³ = 0.73).

| Loss | Delivered | Frames (mean, max) |
|---|---|---|
| 0 % | 1000/1000 | 65.0, 65 |
| 5 % | 1000/1000 | 69.6, 79 |
| 10 % | 1000/1000 | 74.2, 95 |
| 20 % | 998/1000 | 85.1, 108 |
| 30 % | 965/1000 | 100.2, 136 |
| 40 % | 825/1000 | 119.8, 162 |

How the rules got there (delivered at 20 % / 30 % / 40 %):

1. Fixed cap of 8 rounds, ACK only on the datagram's last fragment: 943 / 647 / 235. A retransmission round rarely contains the last fragment, so the sender times out and spends a second round soliciting the ACK.
2. Plus "a fragment at or above the highest index reported missing closes the round" (§7.2 step 3): 968 / 731 / 302.
3. Plus giving up only on stalls, with no-ACK rounds counted apart (4 ACKs without progress, 6 rounds without ACK, hard cap 32, §7.1 step 7): 998 / 965 / 825.

## 2026-11-06 — 10 KB over the fake MeshCore network (`lc-core`)

`cargo test -p lc-core --release --test fakenet -- --ignored --nocapture`: 50 runs per row, a 9,800-byte datagram (64 fragments) between two lichen apps over a chain of stock-behaving companions and repeaters (`lc_core::fakenet`): MeshCore direct delays (`rand[0, 5·t·0.2]` companion, `rand[0, 5·t·0.3]` repeater), half duplex, listen before talk, collisions between hidden neighbours, 160-entry duplicate filter, independent loss on every link and frame. Preset 869.618 MHz / 62.5 kHz / SF8 / CR 4/5. Time is from the send request (handshake included) to the sender's `Delivered`; frames are all transmissions on the network, repeaters included.

| Repeaters | Loss per link | Delivered | Mean time | Frames | Receptions lost to collisions |
|---|---|---|---|---|---|
| 0 | 0 % | 50/50 | 76 s | 66 | 0.0 % |
| 0 | 10 % | 50/50 | 95 s | 77 | 0.0 % |
| 0 | 15 % | 50/50 | 104 s | 83 | 0.0 % |
| 1 | 0 % | 50/50 | 217 s | 132 | 0.0 % |
| 1 | 10 % | 50/50 | 295 s | 168 | 0.4 % |
| 1 | 15 % | 48/50 | 348 s | 190 | 0.8 % |
| 3 | 0 % | 50/50 | 500 s | 264 | 0.0 % |
| 3 | 10 % | 49/50 | 943 s | 424 | 0.9 % |
| 3 | 15 % | 42/50 | 1173 s | 453 | 1.0 % |

With 3 repeaters, 10 % per link is about 34 % end to end in each direction, 15 % about 48 %.

What it took (3 repeaters, 10 % per link, 12 to 20 runs each):

| Change | Delivered | Collisions |
|---|---|---|
| Pacing `k = 5` (first design) | 7/12 | 12.2 % |
| `k = 7` | 9/12 | 0.6 % |
| + ACK sent twice, new handshake only after 3 silent rounds of a never-acknowledged datagram | 19/20 | 1.5 % |
| + sender waits out the ACK copy before resending (it collided with the copy at the repeater, systematically, with 1 repeater) | 18/20, and 20/20 with 1 repeater | 1.0 % |
| + 8 silent rounds and 8 HELLOs before giving up | 49/50 | 0.9 % |

Bugs the fake network found, all fixed: HELLO retries on fixed timers collided forever between two hidden nodes that started together (now a random backoff); a responder sending on its pending session resent the same datagram when the session was confirmed (duplicate delivery); a single lost ACK on a long path restarted a whole 64-fragment transfer.

## 2026-11-06 — Cross-check against oxymesh's MeshCore node logic (`sim/`)

`(cd sim && cargo test --release -- --nocapture)`: two lichen apps on oxymesh's `mc-sim`, which runs oxymesh's port of the MeshCore v1.18 node logic (alice — companion with repeat on — bob). Contacts come from a real advert flood with auto-add.

- First run: `NoPath`. MeshCore's `SEND_PATH_DISCOVERY_REQ` is a flooded telemetry request, and a node answers only when its owner allows base telemetry, off by default. lichen's own fake network had answered discovery from the topology and hidden this. Hence ADR-0008: lichen floods its own PROBE as `GRP_DATA` and reads it through `LOG_RX_DATA`.
- With the probe: text delivered 26 s into the simulation (probe from alice, re-flooded by the relay with its hash, PATH_REPLY, handshake, data, all in direct mode); 3 KB delivered at 5 % random loss.

## 2026-11-06 — Groups, shared radio slot, timing jitter (L3)

**Group of five** (`cargo test -p lc-core --release --test groups -- --nocapture`): five companions each linked only to one repeater (none hears another), every member knowing only the admin. Creating the group (state to 4 members, 20 sender-key datagrams, path discovery and handshakes between members who had no session) took 534 s until every member held every other member's key. All posts arrived, including a two-fragment one; after removing a member, the four others rotated their keys and the removed member could no longer read.

First attempt: two members never joined and the setup stalled after 206 transmissions with 36 % of receptions lost to collisions. Two causes: each session paced itself as if it had the radio alone (the admin ran four transfers at once), and a failed internal datagram (group state, sender key) was dropped for good. Fixed by one shared transmit slot per node (round robin between sessions, ACKs exempt, protocol.md §7.3) and by retrying internal datagrams after 1, 2, 4, 8, 16 minutes.

**Jitter.** Both sides of a conversation behind one repeater, running the same deterministic pacing and ACK wait, stayed in lockstep and collided at the repeater on every round (the crossing test). Every gap now gets `U[0, t_frame)` and the ACK wait `U[0, W/2)`. Cost, same 10 KB measurement as above:

| Repeaters | Loss per link | Delivered | Mean time | Frames | Collisions |
|---|---|---|---|---|---|
| 0 | 0 % | 50/50 | 109 s | 66 | 0.0 % |
| 0 | 10 % | 50/50 | 130 s | 77 | 0.0 % |
| 0 | 15 % | 50/50 | 140 s | 82 | 0.0 % |
| 1 | 0 % | 50/50 | 250 s | 132 | 0.0 % |
| 1 | 10 % | 50/50 | 342 s | 169 | 0.3 % |
| 1 | 15 % | 50/50 | 403 s | 194 | 0.7 % |
| 3 | 0 % | 50/50 | 533 s | 264 | 0.0 % |
| 3 | 10 % | 48/50 | 981 s | 412 | 0.8 % |
| 3 | 15 % | 38/50 | 1216 s | 433 | 1.0 % |

About half a frame time more per gap, and slightly fewer deliveries at the harshest settings, in exchange for no deadlock in two-way traffic.

**Against oxymesh** (`sim/tests/groups.rs`): a group of three on oxymesh's MeshCore node logic, next to a stock node that follows a named channel whose hash is `0x7C`. The message reached both other members; the relay re-flooded the envelopes; the stock node pushed no channel message, channel data or "message waiting" to its app.

**Fuzz** `discovery_open`: 96.7 M runs in 20 min, no crash.

## 2026-11-06 — Group messages: one or two floods

`cargo test -p lc-core --release --test groups -- --ignored --nocapture`: the five-member star above with 10 % loss per link, 10 seeds; after setup, every member posts 8 times (one post a minute overall), 1,280 expected deliveries per row.

| Floods per message | Delivered | Duplicates shown |
|---|---|---|
| 1 | 1045/1280 (81.6 %) | 0 |
| 2 | 1249/1280 (97.6 %) | 0 |

A flood lost at its first hop (collision with other traffic, or the 10 %) is lost for every member, hence the second copy (protocol.md §10.3). It was found with the demo network: Julien's first post collided at the first repeater with a forwarded ACK.

## 2026-11-06 — Fuzzing, two hours per target

Two one-hour campaigns per target (`cargo +nightly fuzz run <target> -- -max_total_time=3600`), from the same corpus, alongside other work: no crash. Slow units reported while the machine was busy run in 0 to 3 ms when replayed.

| Target | Runs |
|---|---|
| `air_packet` | 445 M + 624 M |
| `companion_response` | 121 M + 196 M |
| `frame_header` | 447 M + 626 M |
| `handshake_open` | 228 M + 326 M |
| `session_peer` | 19 M + 15 M |
| `discovery_open` | 96.7 M (20 min) |

`session_peer` found one bug before these campaigns (unbounded receiver gap estimate), fixed in L1.

## 2026-11-06 — Location sharing (L5)

`cargo test -p lc-core --release --test location -- --nocapture`: Alice, Bob and Carol around one repeater, no loss.

| Scenario | Positions sent | Note |
|---|---|---|
| Exact share, 20 min walk at 1.4 m/s, one 90° turn at 10 min, a fix every 5 s | 4 (0, 495, 625, 1130 s) | worst distance between Bob's extrapolation and Alice: 60 m |
| 1 km share, 30 min east at 2 m/s (3.6 km) | 5 (0, 30, 520, 1020, 1520 s) | one per grid cell entered, all cell centres |
| Exact share, stationary 50 min | 4 | heartbeats at 0, 15, 30, 45 min |
| SOS, 5 min, group of 3 without a share | 3 per member | then one end-of-sharing position |

The sends at 495 and 1130 s come from the speed quantum (0.5 m/s): 1.4 m/s is sent as 1.5, so the extrapolation drifts 50 m in about 8 minutes on a straight line. The send at 625 s is the turn (25 s after it). One direct position over one hop: 33 bytes, 365 ms at SF8 / 62.5 kHz / CR 4/5.

With 25 % loss per link (Alice and Bob around one repeater, 40 min zigzag at 2 m/s, a turn every 4 min): 10 positions received; Bob's extrapolation was more than 250 m off at 1 of 455 checks; no stale position shown (a failed position is never resent, the share sends a fresh one).

**Fuzz** `location_decode`: 362 M runs in 20 min, no crash.

## 2026-11-06 — Media over LoRa (L6)

`cargo test -p lc-core --release --test media -- --nocapture` and `cargo test -p lc-media --release -- --nocapture`, SF8 / 62.5 kHz / CR 4/5, one hop.

| Medium | Bytes | Fragments | Airtime (ours, first round) |
|---|---|---|---|
| Voice, 10 s, Codec2 1200 | 1,500 (+ 8) | 10 | 10.7 s |
| Voice, 10 s, Codec2 700C (vendored C library, sent since) | 875 (+ 8) | 6 | — |
| Voice, 6.2 s (demo, macOS speech synthesis), 1200 then 700C | 924, then 539 | 6, then 4 | — |
| Image preview, 160 × 120 AVIF (synthetic landscape) | 409 (+ 21) | 3 | — |
| Image full, 160 × 120 AVIF (same) | 975 (+ 21) | 7 | — |

Budget: 15-second voice messages to one contact, back to back: 22 accepted (16 s of airtime each, 352 s of the 360 s an hour), the 23rd refused with "fits in 60 min" (nothing sent yet, so the queued airtime frees up an hour from now); after 30 minutes, the refusal points to one hour after the first transmission.

**Fuzz** (10 min each): `content_decode` 35.1 M runs, `voice_decode` (Codec2 1200, pure Rust) 168 k runs, no crash.

**Codec2 700C** (vendored C library): 10 s of voice in 875 bytes (6 fragments); Camille's 6.2 s clip in 539 bytes. `voice_decode` fuzzed 10 min with AddressSanitizer compiled into the C code (`-mllvm -asan-guard-against-version-mismatch=0`, Apple clang against Rust's ASan runtime): 106 k runs, no error.

## 2026-11-06 — Phone identities (ADR-0013)

`cargo test -p lc-core --release --test identity -- --nocapture`, SF8 / 62.5 kHz / CR 4/5, one hop.

| Message | Bytes | Airtime |
|---|---|---|
| INTRO | 57 (no return path bytes beyond one hop) | 508 ms |
| INTRO_ACK | 104 | 754 ms |
| INTRO_FIN | 56 | 508 ms |
| HELLO | 66 | 549 ms |
| HELLO_ACK | 64 | 549 ms |

A first contact costs about 1.8 s of airtime for its three frames, once per pair of identities; later sessions use HELLO / HELLO_ACK (1.1 s). Strangers on 15 % lossy links (10 seeds, three messages each): every message delivered once. A one-sided card (Alice scanned Bob, not the reverse) and a reinstalled app with a new identity both converge, the latter reported as changed.

The demo network showed a cost of congestion: an introduction still retrying after a HELLO had succeeded replaced the working session, and INTRO_FIN resends without data to acknowledge only added collisions at the hub. Both are now ruled out (§4.6).


## 2026-11-06 — Internet bridge over iroh (L7a)

`cargo test -p lc-bridge --release -- --ignored --nocapture` and `cargo test -p lc-ffi --release --lib -- --ignored bridge --nocapture`, both ends on the same Mac, over n0's public relays and address lookup (iroh 1.4.1).

| Transfer | Bytes | Time |
|---|---|---|
| Blob fetched by endpoint id alone (`lichen/blob/1`, BLAKE3 checked) | 300,000 | 135 ms |
| Demo: Camille's full photo, 1280 × 960 AVIF, after her LoRa announcement | 8,958 | 191 ms |

The announcement is one LoRa datagram: 52 + 33 per provider + name bytes of content, 101 bytes for Camille's (one iroh provider, a 16-byte name), against 975 bytes for the full 160 × 120 thumbnail over LoRa. Substituted content (a provider answering other bytes) is dropped by the hash check (`substitution_is_caught`).

iOS simulator, demo network (`-demo YES -open Camille -media YES`): the announcement arrives after the LoRa preview, the app fetches over iroh and shows the full-resolution photo with "hash verified" (`docs/screenshots/ios-announce-en.png`, `ios-iroh-fetched-en.png`).

## 2026-11-06 — Internet bridge over Tor (L7b)

`cargo test -p lc-bridge --release -- --ignored tor --nocapture`: two bridges in one process, each with its own Arti 0.48 client, from a home connection (France).

| Step | Time |
|---|---|
| Two Tor clients bootstrapped, fresh directories | 2.2 s and 19.9 s (two runs) |
| Onion service reachable (descriptors published) | 6.0 s |
| 300 kB fetched over Tor, BLAKE3 checked | 3.9 s |
| A second request on the same circuit (unknown hash) | 0.75 s |

Relay only (`--ignored relay_only`): no direct address published, 300 kB through n0's relays in 343 ms (135 ms with direct paths, L7a).

iOS simulator, demo network with iroh off and Tor on (`-demo YES -open Camille -media YES -lichen.tor YES -lichen.iroh NO`): Camille publishes her own onion service, announces with both providers (138 bytes of content), and the app fetches her 9 KB photo over Tor, hash verified (`docs/screenshots/ios-tor-fetched-en.png`).

## 2026-11-07 — First real-radio test

Paris, indoors, the two radios a few metres apart on the same desk. An iPhone 17 Pro with the lichen app, paired over Bluetooth with the ThinkNode M5 (oxymesh firmware), against the Heltec V4 (stock MeshCore companion over USB) driven by `lichen listen --echo --save`. Preset 869.618 MHz / 62.5 kHz / SF8 / CR 4/8, direct path (0 hops).

| What | Frames | Result |
|---|---|---|
| First text, from a phone the Heltec did not know (introduction, then the message) | INTRO, INTRO_ACK, INTRO_FIN, 1 data | received 5 s after the listener started; the echo delivered 9 s later |
| Voice, 2.2 s of Codec2 700C (193 bytes) | 2 | received at 12.5 dB SNR, saved as WAV; the bot's reply delivered 2 s later |
| Photo preview, 160 × 120 AVIF (420 bytes) | 3 | received at 11.5 dB, decodes |
| Full thumbnail asked for by the bot (3,793 bytes) | 25 | 48 s from the request to the last fragment, 10.5 dB |
| Announcement of a 2 MB photo over iroh | 1 | received; the CLI has no bridge to fetch it |

SNR 10.5 to 13 dB throughout. The Heltec reports RSSI 0 dBm in every push (`RAW_DATA` and `LOG_RX_DATA`): its stock firmware's reading, not lichen's.

Found and fixed: an introduction names its sender by one hash byte, and the core took the first contact with that byte, a repeater sharing the phone's node byte (`7d9fcd8`, spec §4.6).

## 2026-11-08 — Android on the real radio, from the emulator

The Android emulator cannot use the Mac's USB devices: `lichen bridge` relays the Heltec's serial port on 127.0.0.1:5000 and the Android app's TCP link reaches it at 10.0.2.2. Same desk, same preset, against the iPhone and the M5 as above.

- Texts both ways, delivered; the safety numbers matched on both phones.
- The iPhone's LoRa full thumbnail (2.2 kB) displays on Android (AVIF decoded by the system).
- A 2.0 MB photo announced by the iPhone, fetched over iroh by Android, hash verified.
- Voice needed the emulator's audio (`-allow-host-audio`); without it, silent playback and no recording.
- Cold start of the debug build on the emulator: 5.7 s, then 2.5 s (a 48 MB native library); to measure on a phone.

## 2026-11-08 — Search mode on the fake network (L8a)

`cargo test -p lc-core --release --test search -- --ignored --nocapture`. Fake network with geometry: log-distance path loss (n = 2.8, 1 m offset −32 dB, TX 22 dBm), 6 dB shadowing drawn per reception, noise floor −118 dBm, SNR readings capped at 13 dB, frames below −10 dB SNR lost. Julien in SOS **without GPS** (a BEACON every 2 minutes, more when pinged); three searchers starting 2.0 to 2.9 km away, walking toward him at 1 m/s, pinging when allowed, sharing their measures; a repeater on a hill linked to everyone. Alice's estimate, median over 10 seeds:

| Minute | Reported RSSI (M5): error / 95 % semi-axis | Noise + SNR (stock Heltec) | Reported RSSI + body bearing ±40° each minute |
|---|---|---|---|
| 3 | 607 m / 2,472 m | 771 m / 2,462 m | 665 m / 1,495 m |
| 6 | 692 m / 1,969 m | 723 m / 1,995 m | 563 m / 967 m |
| 9 | 401 m / 1,143 m | 637 m / 1,485 m | 353 m / 749 m |
| 12 | 339 m / 978 m | 578 m / 1,332 m | 214 m / 670 m |
| 15 | 298 m / 752 m | 533 m / 1,082 m | 187 m / 599 m |

- Airtime per searcher: 1.27 s a minute (2.57 s with a bearing a minute), against 6 s a minute at 10 % duty.
- The estimated RSSI saturates: at 2 km the mean SNR is 16 dB here, above the 10 dB where a reading becomes a lower bound only. It is the realistic worst case for stock Heltecs at short range; with a steeper real path loss, fewer readings saturate.
- Self-calibration: on a field with n = 3.4 (defaults 2.8), while Julien reports GPS the fitted path loss predicts the true mean RSSI within 4 dB from 300 m to 2 km (the default is more than 10 dB off at 1 km). The exponent and the offset trade against each other over a limited span of distances, so the fit is judged by its predictions.
- The 95 % ellipse held Julien for every searcher in the end-to-end test; the error falls as the searchers close in (RSSI ranging at 6 dB is ±60 % per reading).
- These are simulated: the field test (L8c) fits the path loss and the shadowing for real, with `lichen search --csv` on each radio.

## 2026-11-09 — Replies and reactions on the air (ADR-0019)

`cargo test -p lc-core --release --test groups -- --ignored reply_and_reaction_costs --nocapture`: alice sends to bob through one repeater on an established session, then to a group of five around the same repeater. Every transmission is counted: the sender's frames, the repeater's relays, the receiver's acknowledgements (sent twice, §7.3). SF8 / 62.5 kHz.

| Content | Plaintext | Direct, 1 hop | Group of 5, 2 copies |
|---|---|---|---|
| Text, 20 characters | 25 B | 6 frames, 2.17 s | 4 frames, 3.06 s |
| Reply, same text | 33 B | 6 frames, 2.25 s | 4 frames, 3.22 s |
| Reaction 👍 | 17 B | 6 frames, 2.09 s | 4 frames, 2.89 s |

A reaction costs about what a short text does: in a direct conversation the relayed data frame and the two relayed acknowledgements dominate (the 8 bytes saved are 4 % of the airtime); in a group, the 64-byte signature and the two copies do. The apps show this cost before a reaction is sent, as before any send. The 8-byte reference adds 3 to 5 % to a reply.


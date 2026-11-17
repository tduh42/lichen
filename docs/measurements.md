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

Indoors, the two radios a few metres apart on the same desk. An iPhone 17 Pro with the lichen app, paired over Bluetooth with the ThinkNode M5 (oxymesh firmware), against the Heltec V4 (stock MeshCore companion over USB) driven by `lichen listen --echo --save`. Preset 869.618 MHz / 62.5 kHz / SF8 / CR 4/8, direct path (0 hops).

| What | Frames | Result |
|---|---|---|
| First text, from a phone the Heltec did not know (introduction, then the message) | INTRO, INTRO_ACK, INTRO_FIN, 1 data | received 5 s after the listener started; the echo delivered 9 s later |
| Voice, 2.2 s of Codec2 700C (193 bytes) | 2 | received at 12.5 dB SNR, saved as WAV; the bot's reply delivered 2 s later |
| Photo preview, 160 × 120 AVIF (420 bytes) | 3 | received at 11.5 dB, decodes |
| Full thumbnail asked for by the bot (3,793 bytes) | 25 | 48 s from the request to the last fragment, 10.5 dB |
| Announcement of a 2 MB photo over iroh | 1 | received; the CLI has no bridge to fetch it |

SNR 10.5 to 13 dB throughout. The Heltec reports RSSI 0 dBm in every push (`RAW_DATA` and `LOG_RX_DATA`): its stock firmware's reading, not lichen's.

Found and fixed: an introduction names its sender by one hash byte, and the core took the first contact with that byte, a repeater sharing the phone's node byte (`7b17a37`, spec §4.6).

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


## 2026-11-09 — Joining a group by invitation (ADR-0020)

`cargo test -p lc-core --release --test invite -- --nocapture two_people`: alice, bob and carol form a group, each linked to one repeater; dave and eve stand next to alice (linked to her and to the repeater), known to no one, and scan one invitation at the same moment. SF8 / 62.5 kHz, no loss.

| Step | Time after the scan |
|---|---|
| Both admitted (group state received) | 72 s |
| Group of 5, every member holding every sender key | 489 s |

The joiner's zero-hop advert, an anonymous probe and an introduction come before GROUP_JOIN; admission then sends the new state to the four others. The joiners read alice's messages once admitted (her key travels with the state); bob's and carol's keys need a session between them and each newcomer, through the repeater, which is most of the 489 s. A message posted before a key arrives is held and read when it does (§10.3). Before the anonymous probe (§4.5), the joiners never found a path: their keyed probes could not be checked by alice.

## 2026-11-09 — The admin relays sender keys (ADR-0021)

Same scenarios as above, one run each (fake network, SF8 / 62.5 kHz, no loss), before and after the relay of §10.2 and the merging of group control datagrams still waiting for a peer (a newer state replaces the queued one, relayed keys for the same group join one SENDER_KEYS):

| Scenario | Before | After |
|---|---|---|
| Two people join a group of 3 by invitation (`--test invite`, seed 21): every key everywhere | 489 s | 194 s |
| A group of 5 created from contacts, around one repeater (`--test groups group_of_five`, seed 11): every key everywhere | 718 s, 1108 frames | 305 s, 872 frames |

No member opens a session with a newcomer any more. Most of the remaining time is spent at the admin: its frames to one member collide, at the shared repeater, with another member's answers (ACKs, HELLO_ACK), and a transfer that times out starts a new handshake, whose answers collide in turn (`cargo test -p lc-core --release --test invite -- --ignored key_timeline --nocapture` prints who holds which keys over time). That is the next thing to fix.

## 2026-11-09 — Collisions at a shared repeater: keep the session, leave room for answers (§7.1, §7.3)

The rest of the delay above, and the slow first minutes of the demo (Camille's 👍 and answers taking about 2 minutes), came from the same place. Nodes behind one repeater cannot hear each other; their frames collide at the repeater. Two things made it worse:

- After 3 silent rounds the sender dropped its session and started a handshake. The ACK that arrived a moment later under the old session was thrown away, and the HELLO / HELLO_ACK exchange fought the same collisions: several tries, about 12 s apart. Now the session is kept while the handshake runs, and anything that arrives under it ends the handshake (§7.1).
- A node sent its next data frame to another peer while the answer it was waiting for (an ACK, a HELLO_ACK) was coming back through the shared repeater. Now the next data frame waits for that answer too (§7.3).

Ten seeds each, median / worst, seconds (`cargo test -p lc-core --release --test invite --test groups -- --ignored join_timings setup_timings --nocapture`):

| | Relay only | + keep the session | + room for answers |
|---|---|---|---|
| A group of 5 from contacts: every key everywhere | 412 / 485 | 298 / 435 | 237 / 269 |
| Two people join by invitation: both admitted | 78 / 191 | 79 / 191 | 108 / 129 |
| Two people join by invitation: every key everywhere | 195 / 222 | 159 / 222 | 130 / 171 |

Admission is about 30 s slower in the median (the admin waits for answers between frames to different members) but never 191 s; what matters to the conversation, everyone reading everyone, comes sooner.

The demo (`lc-ffi` test `reactions_and_a_reply_with_camille`, with Camille's 👍 back at 8 s after her answer): her answer to the app's 60 s message arrives at 90 s instead of 185 s, without a new session; the 👍 at 166 s instead of 218 s. What remains is the demo's busiest minute: the app, Camille and the crest repeater all talk through the hub, Camille's exchanges with Julien (group setup through the hub and the crest) collide at the hub with the app's frames, four retries in a row.

## 2026-11-09 — Small files over LoRa (ADR-0022)

`cargo run --release -p lc-cli -- sim --repeaters 1 --loss 0 --bytes N --cr 8`: one datagram the size of a FILE kind 2 with a short name, alice → one repeater → bob, French preset (SF8 / 62.5 kHz / CR 4/8), from a cold start (path, handshake).

| File | Frames sent by alice | Her airtime | Delivered after |
|---|---|---|---|
| 1 KB | 11 | 13.9 s | 54 s |
| 4 KB | 36 | 50 s | 3.0 min |
| 8 KB (the most) | 62 | 93 s | 5.6 min |

The repeater spends about as much airtime as the sender. An 8 KB file is a quarter of the sender's hour (360 s), and the app shows its fragments and airtime before sending, as for voice and photos; anything larger goes over the Internet bridge.

## 2026-11-09 — Compressed small files (ADR-0023)

Sizes from `cargo test -p lc-ffi --release --lib -- --ignored file_sizes --nocapture` (the app's encoder, DEFLATE level 9); airtime and delivery from `lichen sim --repeaters 1 --loss 0 --bytes N --cr 8` as above (one hop, French preset, cold start).

| File | Size | On the air | Frames sent by alice | Her airtime | Delivered after |
|---|---|---|---|---|---|
| Hiking track (GPX, 400 points, like a Visorando export) | 24.9 KB | 2.3 KB, compressed | 22 | 29 s | 1.8 min |
| A note (6 KB of English prose) | 6.0 KB | 2.9 KB, compressed | 27 | 36 s | 2.2 min |
| A PDF (already compressed: random bytes) | 6.0 KB | 6.0 KB, as is | 48 | 70 s | 4.2 min |

The track that could not leave the phone in the first field test (25 KB, over the 8 KB limit) now costs 29 s of the hour: 8 % of it, less than an uncompressed 2 KB file. Text halves; a PDF, a photo or an archive is sent as it is.

## 2026-11-09 — Recovering lost group messages (ADR-0024)

`cargo test -p lc-core --release --test groups -- --ignored measure_group_recovery --nocapture`: five members around one repeater (none hears another), random loss on every link, 40 seeds. Every member posts 8 times, a post a minute overall, then once more (a later message that reveals the last gaps; not counted). Delivered = distinct posts each member received, out of 8 × 4 per member; delay = from a post going out to its recovery (median and 90th percentile, to 10 s); frames = every transmission on the network from the first post to half an hour after the last, repeater included, per run.

| Loss per link | | Delivered | Recovered (late) | Delay of recovered posts | Frames per run |
|---|---|---|---|---|---|
| 10 % | without | 6144/6400 (96.0 %) | — | — | 171 |
| 10 % | with recovery | 6400/6400 (100 %) | 326 | 360 s / 440 s | 313 |
| 20 % | without | 5602/6400 (87.5 %) | — | — | 162 |
| 20 % | with recovery | 5961/6400 (93.1 %) | 1344 | 450 s / 1540 s | 1795 |

At 10 % per link, recovery brings back every loss for 80 % more frames (a request and an answer through the repeater, unicast and acknowledged). At 20 %, each exchange itself needs retries, often a new handshake, and every member that lost the same post asks on its own: eleven times the frames for less than half the losses, and the admin, which answers most requests, runs into its airtime budget (the 90th percentile of 26 minutes). A gap is only seen when its author posts again, here every 5 minutes: that wait is most of the delay; then 20–40 s for the second copy, then the exchange. What is still lost: requests that fail twice, and losses not followed by another message from the same author.

Two refinements since the first version (§10.6), measured one after the other:

| Rule | 10 %: delivered, delay p90, frames | 20 %: delivered, delay p90, frames |
|---|---|---|
| first version | 99.5 %, 540 s, 298 | 95.3 %, 1080 s, 1134 |
| + a member asked for a post it lost too hands it on once recovered | 99.5 %, 440 s, 309 | 94.5 %, 990 s, 1257 |
| + gaps before a key's first message count from its next counter | 100 %, 440 s, 313 | 93.1 %, 1540 s, 1795 |

At 10 %, the first cuts the slow tail by 100 s and the second recovers members' first posts, which nothing could bring back before: every post arrives, for 5 % more frames. At 20 %, each extra recovery adds collisions and handshakes that cost more than they bring. An empty answer instead (the asker turning to the author at once) did worse: 90.4 % delivered at 20 % for 1928 frames, against 95.7 % and 1187 for the first rule in the same run (frames counted until the network went quiet), every member opening a session with the author at the same moment.

## 2026-11-15 — Full 16-byte tags (ADR-0028)

Each suite was run twice the same day, on the commit before the change (8-byte tags) and on the commit with it, so the two columns differ by the tag alone. The fake network's absolute times are higher than the 2026-11-06 table above, with either tag: pacing rules added since (§7.3) leave room for answers.

**Sizes and airtime per exchange** (computed from §9's formula, SF8 / 62.5 kHz / CR 4/8, no repeater, the exchange with its two ACK copies): in ADR-0028. At SF8 the 8 bytes are two code blocks on every frame: 65.5 ms at CR 4/8, 41 ms at CR 4/5. The packed ACK keeps a short exchange near +5 %.

**Measured on the fake network:**

| | 8-byte tags | 16-byte tags |
|---|---|---|
| 8 KB file, one repeater, CR 4/8 (`lc-cli sim --repeaters 1 --bytes 8211 --cr 8`): fragments | 53 | 56 |
| the same: sender's airtime / delivered after | 93 s / 5.6 min | 99.0 s / 6.0 min |
| 10 s of Codec2 1200 voice, one hop, CR 4/5 (`--test media`) | 10 fragments, 10.7 s | 11 fragments, 11.4 s |
| 15 s voice messages that fit the hour's budget | 22 | 21 |
| INTRO_ACK / INTRO_FIN, one hop, CR 4/5 (`--test identity`) | 104 B, 754 ms / 56 B, 508 ms | 120 B, 836 ms / 72 B, 590 ms |
| HELLO / HELLO_ACK, the same (with ADR-0029's handshake) | 66 B / 64 B, 1.1 s together | 74 B, 590 ms / 72 B, 590 ms |
| text / reply / 👍 one hop, every frame counted (`--test groups -- --ignored reply_and_reaction_costs`) | 2.17 / 2.25 / 2.09 s | 2.25 / 2.33 / 2.17 s |
| the same in a group of 5, two copies | 3.06 / 3.22 / 2.89 s | 3.22 / 3.39 / 3.06 s |

**A full datagram on a lossy chain** (`--test fakenet -- --ignored`, 50 runs per row, 64 fragments: 9,800 bytes with 8-byte tags, 9,400 with 16):

| Repeaters | Loss per link | Delivered (8 / 16) | Mean time (8 / 16) | Frames (8 / 16) |
|---|---|---|---|---|
| 0 | 0 % | 50 / 50 | 113 s / 115 s | 74 / 75 |
| 0 | 10 % | 50 / 50 | 136 s / 137 s | 86 / 86 |
| 0 | 15 % | 50 / 50 | 147 s / 148 s | 91 / 92 |
| 1 | 0 % | 50 / 50 | 264 s / 270 s | 148 / 150 |
| 1 | 10 % | 50 / 50 | 354 s / 356 s | 184 / 184 |
| 1 | 15 % | 50 / 50 | 416 s / 413 s | 207 / 205 |
| 3 | 0 % | 50 / 50 | 564 s / 576 s | 296 / 300 |
| 3 | 10 % | 47 / 49 | 955 s / 998 s | 422 / 433 |
| 3 | 15 % | 40 / 44 | 1314 s / 1293 s | 498 / 478 |

Times grow by 0 to 4 %, as the frames do; delivery is within the spread of 50 runs. On a single lossy link (`lc-proto --test transfer`, 1,000 transfers per row) the figures are identical with either tag: that test counts frames, and a full datagram is 64 fragments either way.

**Group recovery** (`--test groups -- --ignored measure_group_recovery`, the setup of 2026-11-09):

| Loss per link | | Delivered (8 / 16) | Recovered (8 / 16) | Frames per run (8 / 16) |
|---|---|---|---|---|
| 10 % | without | 96.0 % / 96.1 % | — | 171 / 171 |
| 10 % | with recovery | 100 % / 99.98 % | 326 / 300 | 313 / 282 |
| 20 % | without | 87.5 % / 86.5 % | — | 162 / 162 |
| 20 % | with recovery | 93.1 % / 92.5 % | 1344 / 1214 | 1795 / 1914 |

Within the spread of 40 seeds: flooded frames are longer, not more numerous, for posts that still fit their fragments.

## 2026-11-16 — Frames that cross Bluetooth whole (ADR-0031)

Field finding: a photo to a phone on a stock Heltec V4 over Bluetooth never got its acknowledgement, a text did. An ESP32 companion's notification holds 173 bytes and a longer frame arrives cut short; a full fragment was pushed in 176 bytes. The fake network now cuts every push to an app at 173 bytes, as on an ESP32: five transfer tests in `tests/fakenet.rs` failed with the old sizes and pass with the new ones.

| | Before | Now |
|---|---|---|
| Fragment data, up to 2 path bytes | 149 | 146 |
| Largest datagram | 9,536 | 9,344 |
| Group fragment plaintext | 131 | 128 |
| Photo preview | 426 | 417 (3 fragments) |

Earlier fake-network figures in this log were measured with 149-byte fragments; a large transfer now takes about 2 % more fragments.

## 2026-11-16 — SQLCipher's size (ADR-0036, proposed)

Release core for arm64, the same branch built with SQLite (now) and with SQLCipher 4.14.0 (`libsqlite3-sys` 0.38.2): CommonCrypto on iOS, OpenSSL 3.6.3 built from source (`openssl-src` 300.6.1) on Android, the only way `rusqlite` builds SQLCipher there.

| | SQLite | SQLCipher | Difference |
|---|---|---|---|
| Android `liblc_ffi.so`, stripped | 33,142,312 B | 37,113,464 B | +3.97 MB (+12 %) |
| Android debug APK | 161,738,706 B | 165,710,042 B | +3.97 MB |
| iOS app, simulator debug dylib | 74,628,992 B | 74,677,568 B | +49 KB |

The OpenSSL sources hold 548,000 lines of C (344,000 in `crypto/`); only `libcrypto` is linked. On iOS nothing is added but SQLCipher's own code.

## 2026-11-17 — Groups at 20 % loss, before and after the M9 group changes (ADR-0038, 0039, 0040)

`LICHEN_LOSS=20 cargo test -p lc-core --release --test groups -- --ignored measure_group_recovery --nocapture`, the setup of 2026-11-09: five members around one repeater (none hears another), 20 % random loss on every link, 40 seeds; every member posts 8 times, a post a minute overall (each member every five minutes, on the minute). "Last posts" are each member's 8th post. With `LICHEN_REVEAL=1` a 9th round follows and reveals the last gaps (the 2026-11-09 protocol); with `LICHEN_REVEAL=0` nothing follows the 8th post, as at the end of a conversation. `LICHEN_DIGEST=0` turns the digest of ADR-0040 off. Before: `main` at `4158484` (2026-11-16); after: this branch. Frames: every transmission from the first post to half an hour after the last, repeater included, per run.

| | Recovery | Delivered | Last posts | Recovered (late) | Delay p50 / p90 | Frames per run |
|---|---|---|---|---|---|---|
| before, revealed | without | 5567/6400 (87.0 %) | 725/800 (90.6 %) | — | — | 162 |
| before, revealed | with | 6143/6400 (96.0 %) | 671/800 (83.9 %) | 1272 | 420 s / 1190 s | 1547 |
| after, revealed | without | 5603/6400 (87.5 %) | 703/800 (87.9 %) | — | — | 162 |
| after, revealed | with, no digest | 6118/6400 (95.6 %) | 621/800 (77.6 %) | 1242 | 430 s / 1300 s | 1429 |
| after, revealed | with, digest | 6151/6400 (96.1 %) | 665/800 (83.1 %) | 1275 | 430 s / 1390 s | 1789 |
| before, burst ends | with | 5946/6400 (92.9 %) | 509/800 (63.6 %) | 1075 | 410 s / 920 s | 1146 |
| after, burst ends | with, no digest | 5866/6400 (91.7 %) | 445/800 (55.6 %) | 990 | 410 s / 980 s | 1030 |
| after, burst ends | with, digest | 6113/6400 (95.5 %) | 631/800 (78.9 %) | 1237 | 450 s / 1290 s | 1516 |

- **Key step and handover** (the rows "no digest" against "before"): the same within the spread of 40 seeds. A step sends nothing; members try a few keys when one fails.
- **The digest** brings back a burst's lost last post: when nothing follows the 8th post, 78.9 % of them arrive instead of 55.6 %, and 95.5 % of all posts instead of 91.7 %, about what a later message revealing the gaps gives (96.0 %). The 486 more frames per run are mostly the recovery it sets off (247 more posts recovered); the digests themselves are five per run here, one per member, two copies each: 1.43 s of the author's airtime per digest at SF8 / 62.5 kHz / CR 4/5 (a 97-byte envelope, 0.71 s a copy), about 2.9 s with the repeater's relays.
- **A first digest that waited two minutes failed:** every post got one, sent exactly when another member posted on the minute; both floods collided at the repeater and the recovery traffic that followed fed on itself: 61.7 % delivered (77.2 % with the two minutes drawn up to three), 4080 frames per run. Six minutes plus up to three keeps the digest to the end of a burst.
- Recovery still loses posts at 20 % per link: the request and the answer each need a session through a repeater that drops one frame in five, and members who lost the same post ask at the same time (ADR-0024).

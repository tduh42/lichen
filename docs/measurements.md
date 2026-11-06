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


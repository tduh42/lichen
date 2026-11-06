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

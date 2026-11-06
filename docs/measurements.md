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

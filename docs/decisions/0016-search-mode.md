# ADR-0016 — Search mode: shared direct measurements, fused on each phone

- Status: accepted (L8)
- Date: 2026-11-08

## Context

The design's search mode (design.md "Search mode") locates a group member from radio measurements: one transmission of the target is heard by every searcher in range, so one packet gives N measurements, fused in a particle filter. The M5 firmware version (on-device filter, e-paper) is a later step; L8 is the phones' part, over stock radios.

What the radios give: `LOG_RX_DATA` pushes every heard frame with SNR × 4, RSSI and the frame's path. The ThinkNode M5 on oxymesh reports the packet RSSI; a Heltec V4 on stock MeshCore v1.18 reports RSSI 0 in every push (observed 2026-11-07, unexplained: stock reads SignalRssiPkt), while its SNR is right. `GET_STATS` type 1 gives the noise floor. The design warns that RSSI accuracy must be measured before the solver is tuned, and that receivers saturate below about 100 m.

## Decision

- **Only direct copies measure the target**: frames heard with 0 hops in `LOG_RX_DATA`, before the replay window (both copies of a message count), tied to the target by the envelope's clear header once one copy decrypts (§11.2). RAW_DATA (direct messages) carries no path and is not used.
- **RSSI where the radio has none**: noise floor (`GET_STATS`) plus SNR, flagged estimated, and flagged saturated at SNR ≥ 10 dB, where LoRa's SNR stops growing: such a reading only bounds the distance.
- **Shared in the group** as MEASURE (§11.1): the searcher's position, RSSI, SNR, noise and an optional compass bearing. Every searcher runs the same fusion on everyone's measurements; nothing is decided centrally.
- **Pings need consent**: the target's app answers PING and POWER only in SOS or with "let this group locate me" on for that group. The target is told it is being searched.
- **Fusion on the phone, in the Rust core** (`lc-core/src/search.rs`): a particle filter (2,000 particles, injected randomness) on a local east-north plane:
  - RSSI by log-distance path loss, `RSSI = P0(tx_dbm) − 10 n log10(d / 1 m) + N(0, σ²)`, defaults n = 2.8, σ = 8 dB, P0 = tx_dbm − 32 dB (868 MHz free space at 1 m, small antennas); a saturated reading as a lower bound;
  - bearings by class (±15° Yagi, ±40° body shadowing, ±60° other);
  - the target's own GPS position when it sends one (it dominates);
  - a walking target (1.5 m/s);
  - **self-calibration**: while the target reports GPS positions, the (distance, RSSI) pairs refit P0 and n for the rest of the search.
- **Airtime is rationed** (§11.3): pings only when nothing was heard for 60 s, measures at most once a minute per target, beacons at most every 20 s.

## Consequences

- A search needs a group shared by the target and the searchers; a contact-only target cannot be pinged.
- Searchers with a stock Heltec rank on SNR and an estimated RSSI; their readings saturate sooner. The defaults are literature values until the field test (L8c) fits them.
- The solver is local: two searchers may show slightly different estimates until they have the same measurements.
- Rejected: a central solver (one searcher computing for all), which would need another message and a single point of failure; raw RSSI streams (every frame shared), too costly in airtime.

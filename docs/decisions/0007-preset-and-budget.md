# ADR-0007 — Radio preset read from the device, budget kept by the host

- Status: accepted
- Date: 2026-11-06

## Context

The original design counts airtime at SF8 / 62.5 kHz without stating the CR, and leaves the sub-band question open. In France the preset is 869.618 MHz / 62.5 kHz / SF8 / CR 4/8 (meshcore.fr). The firmware default is CR 4/5. MeshCore sends a 32-symbol preamble at SF ≤ 8.

## Decision

- The preset is read from `SELF_INFO` on every connection, never hard-coded. Airtime is computed exactly, MeshCore's preamble included: 1.51 s for 255 bytes at CR 4/5, 2.31 s at CR 4/8.
- The regulatory rule comes from `config/eu868.toml`. 869.618 MHz falls in g3 (869.40–869.65 MHz): 10 %, i.e. 360 s per sliding hour, 500 mW ERP.
- The host counts its own transmissions (one-hour sliding window), shows the remaining budget, and defers a send that would exceed it.

## Consequences

- The UI numbers ("212 s / 360 s") are exact for the device's preset.
- We only count our own transmissions. The companion also applies its own, looser airtime factor.

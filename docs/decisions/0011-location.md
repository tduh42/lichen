# ADR-0011 — Location: 14-byte fixes, fixed-grid precision, dead reckoning

- Status: accepted
- Date: 2026-11-06

## Context

The original design specifies a 13-byte position (lat, lon, alt, dt, heading and speed, precision), per-recipient precision by fixed-grid rounding, sending on deviation from an extrapolated trajectory, time-limited sharing and an SOS flag. It leaves open: where the SOS flag lives (no bit is free in 13 bytes), what `dt` is relative to (lichen sessions have no timestamp), and the precision steps.

## Decision

- 14 bytes: the 13 of the design plus a flags byte (SOS, end of sharing). `dt` becomes `t16`, the sender's Unix seconds mod 65,536, rebuilt by the receiver within `[now − 18 h, now + 10 min]`.
- Precision steps: exact, 100 m, 1 km, 10 km; coarse positions are cell centres of a fixed grid in degrees, without heading, speed or altitude.
- Dead reckoning with a 50 m threshold (exact) or a cell change (coarse), heartbeat 15 / 30 min, at most one group position every 5 minutes.
- Maps: MapLibre Native with OpenFreeMap tiles (no key, no account) and offline packs of the visible area: MapKit has no offline maps.

## Consequences

- One direct position over one hop is 33 bytes on air, 0.37 s at SF8 / 62.5 kHz / CR 4/5 (plus its ACK); a group position is a 2-copy flood with a 64-byte signature (§10.3).
- The grid hides the position inside a cell but reveals when the holder crosses a cell border: for a sensitive use, the coarser grid and the pull mode of the design remain the answer (pull: later).
- Clocks must agree within 10 minutes; phones do.
- Rejected: random noise (averages out); a session timestamp for `dt` (sessions are not persisted, ADR-0003).

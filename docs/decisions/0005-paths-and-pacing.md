# ADR-0005 — Paths, return path and send pacing

- Status: accepted
- Date: 2026-11-06

## Context

- `RAW_CUSTOM` only travels by direct route: a path is needed before any send.
- The `RAW_DATA` push does not say where the frame came from: the recipient does not know the return path.
- `SEND_RAW_DATA` answers as soon as the frame is queued, and repeaters are half duplex. Fragments sent back to back collide at the first or second repeater.

## Decision

- Send path: the contact's `out_path` in the companion, otherwise `SEND_PATH_DISCOVERY_REQ` and its 0x8D answer.
- The HELLO carries the return path (the forward path reversed). The responder uses it, otherwise its own `out_path`, otherwise a discovery.
- Pacing: `Δ = t_frame × k + 100 ms`, with `k` = 1, 3 or 5 for 0, 1 or at least 2 repeaters.

## Consequences

- With 2 repeaters or more, a full fragment leaves about every 7.5 s (SF8, CR 4/5). 64 fragments take 8 minutes, and each repeater also spends about 100 s of airtime. This fits the duty cycle, and is why the Internet bridge exists for large content.
- Pacing and timers are `lc-proto` parameters. They will be tuned by measurement (fake network, then field).

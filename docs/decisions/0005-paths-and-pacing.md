# ADR-0005 — Paths, return path and send pacing

- Status: accepted
- Date: 2026-11-06

## Context

- `RAW_CUSTOM` only travels by direct route: a path is needed before any send.
- The `RAW_DATA` push does not say where the frame came from: the recipient does not know the return path.
- `SEND_RAW_DATA` answers as soon as the frame is queued, and repeaters are half duplex. Fragments sent back to back collide at the first or second repeater.

## Decision

- Send path: the contact's `out_path` in the companion, otherwise lichen's own discovery (ADR-0008; MeshCore's `SEND_PATH_DISCOVERY_REQ` needs the peer to allow telemetry).
- The HELLO carries the return path (the forward path reversed). The responder uses it, otherwise its own `out_path`, otherwise a discovery.
- Pacing: `Δ = t_frame × k + 100 ms`, with `k` = 1, 3 or 7 for 0, 1 or at least 2 repeaters. `k = 7` covers the worst case of MeshCore's repeater delays (up to `1.5 t` per hop); measured, `k = 5` loses 12 % of receptions to collisions over 3 repeaters, `k = 7` under 1 %, at the same mean transfer time.
- ACKs are sent twice, and the sender waits out the copy before resending; HELLO retries use a random backoff (protocol.md §4.4, §7.3).

## Consequences

- With 2 repeaters or more, a full fragment leaves about every 10.7 s (SF8, CR 4/5). 64 fragments take 11 minutes without loss and about 16 minutes at 10 % loss per link over 3 repeaters, and each repeater also spends about 100 s of airtime. This fits the duty cycle, and is why the Internet bridge exists for large content.
- Pacing and timers are `lc-proto` parameters. They will be tuned by measurement (fake network, then field).

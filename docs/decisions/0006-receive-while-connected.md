# ADR-0006 — Reception only while the app is connected

- Status: accepted
- Date: 2026-11-06

## Context

`RAW_DATA` (0x84) and `LOG_RX_DATA` (0x88) are pushes: the stock companion does not put them in its offline queue. If the phone is not connected, they are lost.

## Decision

- The iOS app enables the `bluetooth-central` background mode and CoreBluetooth state restoration, to stay connected in the background. The Android app will do the same with a foreground service.
- Unicast: losses are recovered by the sender's ACK rounds. After 3 rounds without any ACK, the sender runs a new handshake (the recipient may have restarted), then resends the datagram.
- Amended 2026-11-09 (§7.1): the session is kept during that handshake, and anything that still arrives under it ends the handshake. On a busy repeater, frames are lost far more often than sessions: dropping the session threw away the ACK that came a moment later, and the handshake then fought the same collisions (about 2 minutes in the demo's first minutes, `docs/measurements.md`).
- Groups: a message flooded during a disconnection is lost. This limit is documented in the app ("received only while the phone is connected").

## Consequences

- No modified firmware. The M5's on-device search mode (L10) may keep a local queue.
- Rejected: sending group messages as unicast to each member (airtime multiplied by the group size).

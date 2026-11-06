# ADR-0008 — Path discovery with a flooded lichen probe

- Status: accepted
- Date: 2026-11-06

## Context

`RAW_CUSTOM` needs a direct path. Companions only learn `out_path` from PATH packets, which follow a flooded MeshCore message; a fresh pair of contacts usually has none. MeshCore's `SEND_PATH_DISCOVERY_REQ` is a flooded telemetry request, and the target only answers when its owner allows base telemetry, which stock firmware does not by default. Found by running lichen against oxymesh's port of the MeshCore node logic (`sim/`): no answer, `NoPath`.

## Decision

- lichen floods its own PROBE as a `GRP_DATA` frame through `SEND_RAW_PACKET`. Repeaters forward it without decrypting and append their hash to the path. The target reads the frame, path included, from `LOG_RX_DATA`, and answers with a PATH_REPLY in direct mode along the reversed path (protocol.md §4.5).
- Channel hash `0x7C`: none of the common public channels (Public `0x11`, `#fr` `0x25`, `#test` `0xD9`, and 22 others checked) uses it. Payload lengths are chosen so that a stock node rejects the frame before any MAC check (ciphertext not a multiple of 16), so there is no false message even on a matching hash.
- When the companion has a default flood scope (`GET_DEFAULT_FLOOD_SCOPE`), the probe carries its transport codes, so that region-scoped repeaters forward it.
- Both ends are authenticated by the static secret; each `id` gives single-use keys.

## Consequences

- One flood per discovery, then one direct frame. The target learns its path back for free.
- Works only while the target's app is connected (ADR-0006), like everything lichen receives.
- The same envelope (channel `0x7C`, length rule) will carry group messages in L3.
- Rejected: MeshCore DM flood to trigger PATH packets (shows a stray message in stock clients); requiring telemetry permission from every user.

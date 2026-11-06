# ADR-0002 — Fragment size computed from the path

- Status: accepted
- Date: 2026-11-06

## Context

The original design counts 184 − 7 − 8 = 169 useful bytes per fragment. But a companion-protocol frame is **at most 176 bytes** in both directions (`BaseSerialInterface.h:5`):
- `SEND_RAW_DATA` = code ‖ `path_len` ‖ path ‖ payload, so payload ≤ 174 − path bytes;
- the `RAW_DATA` push = code ‖ SNR ‖ RSSI ‖ 0xFF ‖ payload, so payload ≤ 172;
- `LOG_RX_DATA` is only pushed when the raw frame is at most 173 bytes.

## Decision

`payload_max = min(172, 174 − path_bytes)`; useful chunk = `payload_max − 15`, i.e. 157 bytes up to 2 path bytes. We keep at most 64 fragments: a datagram is about 10 KB. For flooded groups, the `GRP_DATA` payload fits in `171 − hops` bytes (4 fewer with transport codes).

## Consequences

- The chunk is fixed on the first send. If the path grows during a transfer, the datagram is resent under a new `tag`.
- The receiver stores fragments in fixed 157-byte slots: it does not need to know the chunk.
- Rejected: a fixed 169 bytes, which the companion would truncate or refuse.

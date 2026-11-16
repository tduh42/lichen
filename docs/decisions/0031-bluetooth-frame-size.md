# ADR-0031 — Frames that cross Bluetooth whole

- Status: accepted
- Date: 2026-11-16
- Revises the sizes of ADR-0002 and ADR-0028

## Context

The first photo sent to a phone whose radio was a Heltec V4 on stock MeshCore v1.18, connected over Bluetooth, never got its acknowledgement; a text did. Over iroh the announcement was "sent", and the content never fetched.

An ESP32 companion sets its Bluetooth ATT MTU to `MAX_FRAME_SIZE`, 176 (`src/helpers/esp32/SerialBLEInterface.cpp`), and hands each frame to the app as one notification. A notification holds MTU − 3 = 173 bytes; arduino-esp32 cuts a longer value to that and logs "Truncating to %d bytes (maximum notify size)". lichen sized its frames by the companion frame limit alone: a full fragment was a 172-byte `RAW_DATA` payload, pushed in a 176-byte frame, which reached the receiving phone 3 bytes short and failed its tag. Texts fit one short fragment and went through. The limit was never met before: the first field tests had the Heltec on USB, and the other test radio, a ThinkNode M5, runs another firmware.

The fake network modelled the companion protocol, not the Bluetooth link, so no test saw it. It does now: every push to an app is cut at 173 bytes, as on an ESP32, and five transfer tests failed until this change.

## Decision

- **Every frame fits a Bluetooth link:** `lc_meshcore::BLE_FRAME_MAX = 173`. A `SEND_RAW_DATA` payload is at most `min(169, 171 − path_bytes)` (our command and the receiver's `RAW_DATA` push both within 173), so a fragment carries 146 bytes up to 2 path bytes (was 149) and a datagram at most 9,344 bytes (was 9,536). A group envelope is at most 156 bytes, so its `LOG_RX_DATA` push still fits after 8 hops with transport codes: 128 bytes of plaintext per fragment (was 131), a group text of 59 bytes in one fragment and 187 at most (was 62 and 193). A photo preview is at most 417 bytes, still three fragments (was 426).
- **No compatibility with earlier builds:** a receiver refuses the larger fragments and envelopes they sent. Only test phones hold a build, and they update together.
- **Writes from the app** (`SEND_RAW_DATA`, `SEND_RAW_PACKET`) go with response on both apps, which Bluetooth carries in several pieces; they stay within 173 bytes anyway.
- **The fake network models Bluetooth** for every app (`fakenet::BLE_NOTIFY_MAX`), as the phones use it.

## Consequences

- About 2 % more fragments for large transfers; the airtime of a short text does not change.
- Tests that ignore the ESP32's own limits can pass while real radios fail: the fake network's companion now cuts notifications, and the field checklist (M4) sends a photo and a long text to a stock radio on Bluetooth.

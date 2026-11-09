# ADR-0023 — Small files are compressed before LoRa when that helps

- Status: accepted
- Date: 2026-11-09

## Context

The first field test of files (ADR-0022) was a 25 KB hiking track from Visorando (GPX), sent from Tanguy's iPhone to the Heltec bot. It is above the 8 KB of a small file, so the app offered only the Internet bridge, which the bot does not have: the track never left. ADR-0022 had rejected compression because most documents are already compressed, but the file people most want to pass on off-grid is a route, and a GPX is XML: a comparable 25 KB track is 3.2 KB with DEFLATE. Notes, KML, CSV and JSON behave the same way.

## Decision

- **FILE `kind` 3** (§13.2): `ts ‖ 3 ‖ name_len ‖ name ‖ size u32 ‖ raw DEFLATE`. The sender compresses (DEFLATE, RFC 1951, best compression) and sends `kind` 3 when the datagram is smaller, `kind` 2 otherwise. The LoRa limit of 8,192 bytes applies to what goes on the air; the inflated content is at most 64 KiB.
- **Bounded inflation**: the receiver inflates into at most `size` bytes (≤ 64 KiB) and drops anything that is not exactly `size` bytes. A malicious contact cannot make a phone allocate more than 64 KiB per file, and only an authenticated contact can send one.
- **The library**: `miniz_oxide`, pure Rust without `unsafe`, already built into the app through iroh. Encoder output is not part of the spec: vectors fix decoding only.
- **No compression of anything else** for now: texts are short, voice and images are already compressed by their codecs.

## Consequences

- A 25 KB GPX goes over LoRa in about 3 KB; a note or a CSV costs a third to a tenth of its airtime. The send sheet shows the compressed size and its airtime.
- Compressing before encrypting can reveal something through the length when attacker-chosen data is compressed with a secret (CRIME). A file is compressed alone, chosen by its sender: there is nothing to mix it with. Its length was already visible through the number of fragments.
- Older versions show a `kind` 3 file as unsupported.
- Rejected: zstd or xz (smaller output on text, but a C library or a larger dependency for a few hundred bytes on files this size); a higher inflated limit (64 KiB already covers 8 KB of text compressed eightfold).

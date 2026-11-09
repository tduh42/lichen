# ADR-0022 — Files: small ones over LoRa, any size over the Internet bridge

- Status: accepted
- Date: 2026-11-09

## Context

Tanguy asked why a PDF, or any other file, cannot be sent. The apps sent text, voice, photos and positions only. A PDF is usually 100 KB or more: at SF8 / 62.5 kHz that is several minutes of airtime, beyond the 360 s per hour the 869.4–869.65 MHz sub-band allows (§9), and beyond one datagram (10,048 bytes, §3.2). The Internet bridge (ADR-0015) already moves any bytes checked against their BLAKE3 hash, and ANNOUNCE (§14.1) already has a "file" kind, but the apps only used it for photos.

## Decision

- **Any file, up to 64 MiB, over the bridge** when iroh or Tor is on: an ANNOUNCE of kind 3 over LoRa (one fragment), the content over iroh or Tor, to a contact or a group. The name is cut before its extension when longer than 32 bytes.
- **Small files over LoRa, off-grid**: FILE kind 2 (§13.2), the name and up to 8 KB of content in one datagram, to a contact only, never flooded. It is a medium: its cost is shown first, and it is refused when it does not fit in the hour (§13.4).
- The receiver keeps the file as data and opens it with the system's viewer for its type; nothing is executed by lichen.

## Consequences

- An 8 KB file costs about a minute of the sender's airtime on one hop, a sixth of the hour: the send sheet says so before sending.
- Without Internet, larger files cannot be sent; the app says why and how (turn on iroh or Tor) instead of failing.
- Rejected: splitting large files over several datagrams on LoRa (hours of airtime for a common PDF, and the band's budget for everyone else nearby); compressing on the way (most documents worth sending are already compressed). Revisited by ADR-0023: a hiking track (GPX) is text, compresses several times and is the file most often sent off-grid.

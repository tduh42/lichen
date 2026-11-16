# ADR-0032 — Radio settings, and a position in adverts by choice

- Status: accepted
- Date: 2026-11-16
- Amends the constraint "never put a location in a MeshCore advert" (CLAUDE.md, ADR-0011)

## Context

lichen showed the radio's name, its preset (read only) and an advert button. People coming from the official MeshCore app expect to set the radio from their phone: its LoRa parameters, its power, its GPS, which adverts become contacts, who may ask it for telemetry, its Bluetooth PIN. Without them, a lichen user has to keep the other app for the radio's settings.

Stock companion firmware v1.18 (81dbae5) takes all of them over the companion protocol, mostly applied at once and kept across reboots (`examples/companion_radio/MyMesh.cpp`; layouts in protocol §1.1, vectors in `testdata/meshcore/settings.toml`).

The radio's GPS raises a privacy question: with the advert position policy on, every advert carries the node's position in clear, to the whole mesh; with location telemetry open, anyone may ask for it. lichen's rule so far was never to put a location in an advert: positions only travel encrypted, to the people the user picks (ADR-0011).

## Decision

- **The settings lichen offers** (Settings › Radio): a radio card (battery, storage, firmware, model); LoRa parameters by preset or by hand (frequency, bandwidth, spreading factor, coding rate); transmit power within the radio's range; GPS on or off and its interval, when the board lists a `gps` variable; which advert types become contacts by themselves and from how many hops; telemetry permissions; extra ACKs; the Bluetooth PIN; reboot. Factory reset, path hash size, flood scope, Wi-Fi and the command line are left to the official app.
- **A position in adverts is the user's choice**, off by default. Turning it on asks first, saying that every advert then carries the radio's position, unencrypted, for anyone on the mesh. The same goes for letting everyone, or chosen contacts, ask for the location by telemetry; base and environment telemetry are not positions. lichen's own positions still travel only encrypted (§12). The rule becomes: **never a location in an advert or telemetry without the user's explicit choice**.
- **Changes that can cut the radio off ask first:** LoRa parameters ("a radio on other settings no longer hears this mesh") and the PIN (applied at the next boot, which the app offers). After a change, lichen reads `SELF_INFO` again: the airtime budget and the estimates follow the radio's actual parameters.
- **The radio is the source of truth:** lichen stores none of these settings; it reads them at each connection.

## Consequences

- A lichen user no longer needs the official app for the radio's settings.
- A wrong preset can isolate the radio from its mesh; the confirmation says so and the previous values are shown.
- The GPS draws power: its interval is offered with it.

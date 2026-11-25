# ADR-0050 — A node's readings (telemetry)

- Status: accepted
- Date: 2026-11-25

## Context

The redesigned node page has a card "Readings it sends": a node's battery and temperature, when they were sent, and Ask again. Stock companion firmware v1.18 (81dbae5) has the command: `SEND_TELEMETRY_REQ` sends a request to a contact and pushes its answer, CayenneLPP items, as `TELEMETRY_RESPONSE` (protocol §16.7). Whether a node answers is its owner's choice: a person's radio by its telemetry modes (off by default), a repeater or room server only to the clients it lists. lichen had no way to ask.

## Decision

- **The stock command**, not a binary request: its push names the node, and it is what the stock app sends. It joins the queue of server requests (§16.5): the companion waits for one request only, and a new one replaces the one waiting.
- **One request per node at a time**, checked against the airtime budget before it is queued and again before it is sent, as the other requests are.
- **Timeout**: the companion's own estimate plus 4 s (10 s when no `SENT` comes), as for status and neighbours. **No retry**: every try costs airtime, and a node that does not answer has usually chosen not to (telemetry off, not listed by a repeater); the user asks again.
- **A subset of CayenneLPP**: the types MeshCore's sensors write at v1.18 (voltage, temperature, humidity, pressure, altitude, current, power, analog input, distance, frequency, generic sensor, percentage, GPS), plus illuminance. Fixed-point integers (millivolts, tenths of a °C, …): `lc-meshcore` stays without floats. Decoding stops at the first unknown type, keeping what came before: a type's length is never guessed, since a wrong guess would turn the rest into wrong values.
- **The battery's charge** comes from the volts with the curve the radio's own battery already uses (3.3 V empty, 4.2 V full, `lc_core::radio::battery_percent`): one curve. The volts are kept, the charge is computed when shown.
- **Kept**: per radio and node, the decoded items of each answer as sent and when it came, for **7 days**, the newest **100** at most. Readings are asked by hand, so 100 is far more than a week of asking; a week shows a repeater's battery over its solar days. Per radio, like each node's signal (ADR-0049). A node's position, when its owner lets it answer with one, is kept with the rest for as long. Readings stay out of backups (backup §3): they describe the mesh around one radio, once.
- **Our own radio** answers others under its own telemetry setting (§1.1), which only an explicit choice of the user changes (ADR-0032): asking others changes nothing there.

## Consequences

- The node page can show a node's battery and temperature and how old they are, and say why there is nothing: no answer, refused, or answered without a reading.
- A repeater must list us before it answers: the apps should offer to log in (as a guest at least) when a repeater does not answer.
- A request waits behind a login or a status already queued, and holds back the next for up to its timeout.
- Sensors that write types outside the subset are cut off where they start; adding a type is a line in the table, its vectors and its decoder arm.
- Rejected:
  - asking with `SEND_BINARY_REQ` (matched by tag, but the stock app does not use it for telemetry, and it shares the same single wait);
  - retrying by itself;
  - decoding every CayenneLPP type, polyline included (variable lengths the firmware never writes);
  - keeping the readings forever, or in backups.

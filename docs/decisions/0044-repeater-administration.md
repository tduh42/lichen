# ADR-0044 — Repeater administration, tested against the fake companion

- Status: accepted
- Date: 2026-11-16

## Context

Whoever runs a repeater checks on it from their phone: its battery, its uptime, how much it sends and receives, which nodes it hears and how well, and the occasional command. The stock app does it through the companion (`SEND_LOGIN`, `SEND_STATUS_REQ`, `SEND_BINARY_REQ` and text messages of the CLI type, MeshCore v1.18 at 81dbae5). ADR-0032 left the command line to the official app for the user's own radio; a repeater is another node, reached over the air.

## Decision

- **What lichen offers** (protocol §16.5): login with the admin or guest password, the status (battery, uptime, queue, noise floor, last RSSI and SNR, packets and airtime, duplicates, errors), the neighbours with their SNR and how long ago they were heard, named when the radio knows them, and a small command line that shows what was sent and what came back.
- **The admin password is never kept** on the phone; it is typed for each session. Logging out sends `LOGOUT`.
- **Nothing of it is stored**: the status, the neighbours and the console live for the connection.
- **Tested against the fake companion**: the fake network answers logins, status, neighbours and commands as a stock repeater would (simplified: without airtime), and the demo's hub can be administered with the password "password". No real repeater was used.

## Consequences

- A repeater's owner no longer needs the stock app to check on it.
- Each request is a packet on the air and counts in the budget.
- What a real repeater answers is only checked against vectors written from its source; a field test should follow.
- Rejected: region and ACL management, firmware updates, and the full command reference (the stock app and the repeater's own documentation cover them; the console can still send any command).

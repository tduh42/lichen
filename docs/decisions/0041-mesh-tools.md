# ADR-0041 — Mesh tools: nodes, a map layer, paths and traces

- Status: accepted
- Date: 2026-11-16

## Context

lichen showed people (contacts that can talk lichen or plain MeshCore) and hid everything else the radio knows: repeaters, room servers, sensors, and the nodes it hears without adding. People coming from the stock MeshCore app use these to understand their mesh: which repeaters are around, where they are, through which repeaters a contact is reached, and how good each hop is. Stock companion firmware v1.18 (81dbae5) gives all of it over the companion protocol: contact records carry the type, the last time the radio heard the node and the position its adverts carry; `NEW_ADVERT` reports nodes not added; `SEND_TRACE_PATH` sends a trace along a path of repeaters, each of which appends the SNR it heard it at (`src/Mesh.cpp` L42–L68).

## Decision

- **Nodes** (protocol §16.1): a list of what the radio reported since it connected, its contacts and the nodes it heard without adding them, with type, hops and last heard, the most recently heard first. Read at each connection, never stored: the radio is the source of truth, as for its settings (ADR-0032).
- **A map layer** of the nodes that advertise a position, at that position. It shows only what their owners put in their adverts, in clear; lichen derives no position.
- **Paths** (§16.2): a contact's stored path is shown hop by hop, each hop named after the repeaters whose key starts with its hash, when the radio knows them; an unknown hash shows as hex, several matches as all of them.
- **Traces** (§16.3): the stock command, along the contact's path and back, so that the answer comes back to our radio: the SNR of each hop both ways. One trace at a time per node; its airtime counts in the budget; no answer within the companion's estimate (plus a margin) is reported as such.

## Consequences

- A user no longer needs the stock app to see their repeaters and how a contact is reached.
- `lc-meshcore` gains the trace command and push, checked against vectors written from the firmware by an independent Python oracle (`testdata/meshcore/mesh.py`).
- A node's position on the map is as old as its last advert; the list says when it was last heard.
- A trace only measures repeaters: companions do not forward, so a contact reached directly has nothing to trace.
- Rejected: keeping the nodes in the store to show them while the radio is away (they would be stale, and the radio holds them anyway); estimating positions of nodes that advertise none (it would place people without their choice).

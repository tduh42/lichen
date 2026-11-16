# ADR-0001 — Self-contained repository, no dependency on oxymesh

- Status: accepted
- Date: 2026-11-06

## Context

lichen talks to stock MeshCore nodes. oxymesh already contains the MeshCore v1.18 formats, verified byte for byte (`mc-proto`, `mc-air`, `mc-core`), and a companion simulator. Why depend on oxymesh when the app does not depend on its firmware?

## Decision

- No code dependency on oxymesh or loranet. `lc-meshcore` only holds what the app uses: about ten commands, their responses and pushes, USB framing, the on-air frame header, ECDH and airtime.
- **Facts** come from `oxymesh/docs/spec/{air,companion}.md` and are cited in `docs/spec/protocol.md`. The **vectors** in `testdata/meshcore/` are copied from oxymesh: independent Python oracle, and meshcore.js for the companion protocol.
- Only the `sim/` workspace (separate, outside the build and the check script) may depend on oxymesh's simulator.

## Consequences

- The repository builds on its own, including for the iOS and Android apps.
- A MeshCore discrepancy is fixed in two places (oxymesh and lichen). Acceptable: companion protocol v1.18 is frozen, and the shared vectors catch divergences.
- Rejected: path dependency on oxymesh (couples two repositories and two paces).

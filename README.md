# lichen

Encrypted messaging on top of the **stock MeshCore** network: long fragmented messages, ChaCha20-Poly1305 with forward secrecy, private groups, media, location sharing and a search mode. No firmware is modified: the app talks to the MeshCore companion (Bluetooth or USB) and carries its frames in `RAW_CUSTOM` and `GRP_DATA` packets.

- Normative spec: [`docs/spec/protocol.md`](docs/spec/protocol.md); original design: [`docs/spec/design.md`](docs/spec/design.md) (French original: [`design.fr.md`](docs/spec/design.fr.md)).
- Decisions: [`docs/decisions/`](docs/decisions/). Mockups: [`docs/maquettes/`](docs/maquettes/).
- Shared Rust core; iOS app (SwiftUI) first, then Android (Compose).

| Crate | Role |
|---|---|
| `lc-meshcore` | `no_std` app side of the MeshCore companion protocol, USB framing, on-air header, ECDH, LoRa airtime |
| `lc-proto` | `no_std` lichen frame, crypto, handshake, ACK-on-Error fragmentation |
| `lc-core` | sessions, contacts, paths, airtime budget, storage; fake MeshCore network for tests |
| `lc-cli` | `lichen` command-line client over USB |

```sh
scripts/check.sh
```

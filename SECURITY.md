# Security

lichen is **experimental**. No third party has audited it. Do not rely on it where a flaw could put someone in danger.

## What has been checked

- An internal review on 2026-11-15, by three independent reviewers (pairwise cryptography; groups and insiders; bridge, media, parsers, storage, apps, dependencies), and what changed after it: [`docs/security-review-2026-11-15.md`](docs/security-review-2026-11-15.md). Its response table lists every finding, the test that reproduced it, the fix, and what is still open. An independent second read of those fixes, on 2026-11-17, found eighteen more problems (N1 to N18, one High); all are fixed, each with a test that failed before its fix, in the same document.
- An earlier protocol review and its response: [`docs/protocol-review-2026-11-06.md`](docs/protocol-review-2026-11-06.md), [`docs/review-response-2026-11-06.md`](docs/review-response-2026-11-06.md).
- Every decoder of external input has a `cargo-fuzz` target (`fuzz/`); `scripts/check.sh` and CI run `cargo deny` against known advisories (the ignored ones say why in `deny.toml`).

The normative spec, [`docs/spec/protocol.md`](docs/spec/protocol.md), says what each frame carries in clear. In short, a radio listener cannot read what you send, but sees what MeshCore itself shows (node hashes, paths, group channel hashes), lichen's 7-byte frame header (counters), handshake and introduction fields marked "in clear", sizes and timing.

On the phone, the identity seed stays in the Keychain or behind the Android Keystore, on that device only, and no system backup takes it. Messages, contacts and groups are in a SQLCipher database under a random key kept the same way (ADR-0036); media fetched over the bridge are files in the app's own folder, under the system's protection (on iOS, until the first unlock) but not SQLCipher's. An exported backup is encrypted under a key derived from the 24 words (ADR-0037): whoever holds the file and the words reads everything. The link to a radio over TCP is neither encrypted nor authenticated; over Bluetooth, Android uses the radio only once it is bonded, while iOS cannot check that the link is encrypted. MeshCore chats, channels and room posts carry MeshCore's own encryption, not lichen's, and the apps label them so.

## Reporting a vulnerability

Please use GitHub's private reporting: the **Security** tab of this repository, then **Report a vulnerability**. Do not open a public issue for it.

Say what you found, how to reproduce it (a failing test is ideal), and what an attacker gains. lichen is maintained by one person in their spare time: expect an answer within two weeks. Fixes land on `main`; there are no maintained release branches.

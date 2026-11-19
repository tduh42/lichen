# ADR-0033 — History from the MeshCore app

- Status: accepted
- Date: 2026-11-16

## Context

People who move from the official MeshCore app to lichen leave their conversations behind. The radio cannot give them back: it keeps received messages only in RAM (at most 256 on the M5 and Heltec V4 builds of firmware v1.18), hands each one to the first app that asks, and has no command to read older ones (`examples/companion_radio/MyMesh.cpp`, the offline queue). lichen already drains that queue at each connection (§15.1). The rest lives in the official app's own database, which it can export ("export app database", a raw SQLite file, since its version 1.43). The app is closed source and the format undocumented; an export from a real phone shows it (schema version 29): `contact_messages` (full node keys of both sides, status, sender time in seconds, arrival in milliseconds, text), `channel_messages` (the channel's 16-byte secret, text kept as received, "name: text"), `contacts`, `channels`.

## Decision

- **Import, not sync:** Settings › History › "Import from the MeshCore app" reads an exported file once. Direct messages go to the MeshCore chat with their node (§15.2), channel posts to the channel of their secret (§15.3), as lichen would have stored them: same chats, same text, the sender's time, received or sent, delivered or failed. They come in read.
- **Only plain text:** room posts and repeater command lines (other text types) are left out and counted.
- **In their place:** a conversation is shown in time order (the time a message was stored, then its row), not in the order rows were written: imported messages, older than what lichen already holds, go before it, and the conversation's row in Messages keeps its latest message (QA of 2026-11-19, B-core-7).
- **Nothing twice:** a message already in its chat with the same direction, time and text is skipped; importing the same file again adds nothing.
- **Names:** the people talked with become contacts with the name their radio advertised, when the radio does not list them; a name the user gave them in the MeshCore app becomes their nickname in lichen, unless they have one.
- **A file from outside, read defensively:** the app copies the picked file into its own folder, without any `-wal` or journal beside it; the core opens the copy read-only and immutable (no journal or WAL is read), in SQLite's defensive mode with untrusted schema functions off and cell size checks on, and reads only named columns of the known tables; the copy is removed. A file that is not such an export is refused ("not a database exported by the MeshCore app"). This keeps the reason for ignoring RUSTSEC-2026-0459 (a crafted `-wal`) true: no `-wal` from outside is ever opened.
- **Tests never use a real export:** `tests/import.rs` builds a made-up database with the app's tables; a real one is only read locally, counts printed (`LICHEN_IMPORT=path cargo test -p lc-core --test import -- --ignored`).

## Consequences

- Moving to lichen keeps the plain history. lichen's own encrypted conversations are not involved.
- The format may change with the app's versions: a missing table or column refuses the whole file rather than importing half of it.

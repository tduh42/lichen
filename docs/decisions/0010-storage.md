# ADR-0010 — Storage: one SQLite database per node, in the core

- Status: accepted
- Date: 2026-11-06

## Context

The apps need the message history, the contacts learned from groups (the node does not know them) and the groups across restarts. Group members never resend their sender keys on their own: an app that forgets them can no longer read its groups. Android will need the same storage.

## Decision

- SQLite (`rusqlite`, bundled) in `lc-core::store`, driven by `lc-ffi`: the core stays IO-free, the binding layer records what the core reports and restores contacts and groups at start. One database per identity (`id-<first 8 bytes of the identity key>.sqlite` in the app's data directory; the text said per node before the review of 2026-11-15, SR-39), opened at launch with the identity from the Keychain.
- Stored: messages (content, author, time, status, fragments, airtime, SNR, read), contacts, and a snapshot per group: state, members, our key epoch, the other members' sender keys and replay windows.
- Not stored: sessions (ADR-0003), our own group sender key and its counter. After a restart, each group draws a new key under the next epoch and sends it to the members once the node is connected (protocol.md §10.2). Messages still sending when the app stopped are marked failed (`Interrupted`) for the user to resend; since ADR-0018, unfinished texts are kept as intents and sent again, and only media are marked failed.
- The demo network uses a database in memory.

## Consequences

- Sender keys rest on the phone in clear inside the database, protected by the OS (iOS file protection until first unlock, app sandbox). The database and everything lichen keeps beside it are excluded from backups on both platforms: iCloud and Finder backups on iOS (since the review of 2026-11-15, SR-22), `allowBackup=false` and the data extraction rules on Android. A stolen unlocked phone reads past and future group traffic until the next rekey; it already holds the node's key anyway (ADR-0004).
- Schema versioned with `PRAGMA user_version`; a newer database is refused rather than misread.
- Rejected: storage in each app (Swift, then Kotlin: twice the code); encrypting the database with a Keychain key (to revisit if the threat model asks for it).

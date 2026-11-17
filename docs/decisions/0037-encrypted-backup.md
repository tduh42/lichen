# ADR-0037 — Encrypted backup, opened by the 24 words

- Status: accepted
- Date: 2026-11-16

## Context

The 24 words bring an identity back (ADR-0013), but nothing else: a lost or replaced phone loses
its conversations, its contacts' verification, its groups and its media. The database and
everything lichen keeps beside it are excluded from iCloud, Finder and Android backups on
purpose (ADR-0010, SR-22): those would carry the history and the group keys off the phone, under
keys the user does not hold. The apps showed "Encrypted backup: coming later".

## Decision

- **One file**, made on demand (Settings › Backup › Export, and the identity screen): the
  database, the media fetched over the bridge (`downloads`), and the app's settings. The user
  puts it where they like (the share sheet or Files on iOS, a document picked with the Storage
  Access Framework on Android). Format: `docs/spec/backup.md`.
- **The 24 words open it.** Its key is derived from the identity seed with HKDF and a context
  string of its own, under a random salt per file; never the identity key, never a password of
  its own: the words are already what the user must keep, and a second secret would be lost or
  weak. Whoever holds the words can already be the user; the file adds their past to it.
- **Streamed in chunks** of 64 KiB, each sealed with ChaCha20-Poly1305 under a counter nonce and a
  last-chunk flag (the STREAM construction): a 500 MB backup never sits in memory, and truncation,
  reordering or a changed byte fail a tag. Nothing is installed before the whole file verified.
- **Left out** (spec §3): sessions (never stored), the other members' sender keys and replay
  windows, invitations (they hold keys), the outbox, disappearing messages, location shares and
  received positions, search consents, the radio's state. Texts still waiting are marked failed
  ("interrupted"): the user sends them again if they still mean to.
- **Restore** is offered at first launch ("Restore a backup": pick the file, type the 24 words)
  and next to the identity restore. It replaces this phone's data for that identity and makes it
  the phone's identity.

## Consequences

What the user and their contacts see after a restore:

- **Contacts**: the same identity, so nothing to verify again and the same safety numbers. The
  first message to each opens a new session, as after any restart (ADR-0003). With another radio,
  the identity moves to its node as ADR-0013 allows.
- **Groups, as a member**: the phone draws a new sender key under the next epoch and sends it to
  every member once the radio is connected (§10.2), as after a restart. It holds none of the
  others' keys: their next posts wait (§10.3, early messages), and a minute later it asks the
  admin for every key (GROUP_SYNC, §10.7) and reads them. What was posted while it had no group
  is not recovered.
- **Groups, as admin**: the same new key goes out, but nobody hands an admin the members' keys:
  it reads each member again once that member's next key reaches it (their app restarts, or a
  member leaves and keys rotate). A way for the admin to ask is left to the groups work (M9).
- **Texts that were waiting** show failed, the user's reactions and edits not yet delivered are
  gone, disappearing messages are not there, location sharing is off.
- **The file** is as safe as the words: anyone with both reads the history. The apps say so where
  it is made. A backup made before a message was deleted for everyone still holds it: deleting a
  message cannot reach copies the user made.
- Exporting copies the database aside (inside lichen's own folder, `VACUUM INTO` on a connection
  of its own, so the app keeps running), scrubs the copy, and streams it with the media; the copy
  is removed after.
- Rejected: a password of its own (one more secret to lose; a weak one would be the weakest link),
  the identity key as the file key (a key used for two purposes), including sender keys (a file
  that reads the groups' future traffic until the next rotation), syncing to a server (lichen has
  none, and wants none).

# ADR-0043 — Room servers, and channels shared by QR code

- Status: accepted
- Date: 2026-11-16
- Extends ADR-0026

## Context

ADR-0026 brought plain MeshCore chats to lichen and left out room servers and sharing private channels by QR code. Both are common in the stock app: a hut, a club or a valley runs a room server whose posts everyone logged in receives, even when they were away (the room keeps its last 32 posts and the companion remembers the newest it saw); private channels are passed around as QR codes. The stock app's code format is public (MeshCore's `docs/qr_codes.md` at 81dbae5).

## Decision

- **Room servers** (protocol §15.8): a room's posts are a conversation of their own kind (the store's chat kind 4), a MeshCore chat like the others: text only, labelled as not lichen-encrypted, with the posts' authors named. Logging in takes the room's password; the user may keep it on the phone, and lichen then logs in at each connection so that the posts since the last time arrive. Posting is a plain direct message to the room, acknowledged by it.
- **One request at a time**: logins, status and binary requests share one slot in the companion; lichen queues them.
- **Channels by QR code** (§15.3): the apps show a channel's code in the stock format, after saying that whoever scans it can read and post on the channel, and scan such codes to join, showing the channel's name and kind first.

## Consequences

- A lichen user can follow and post on a room as stock users do; what they post is as private as MeshCore makes it, and the room and everyone logged in read it.
- A room's password kept on the phone sits in the database, in clear, like the rest of it (ADR-0010).
- Codes made by lichen are read by the stock app and the other way round (vectors from both directions in `testdata/meshcore/mesh.toml`).
- Messages lists every conversation that has a message, rooms included, whatever the radio's contact list says; it is named after the contact, then the radio's node, else "Unknown radio" and its key (protocol §15.8).
- Rejected: showing room posts in the room node's direct chat (its posts have authors, and the room is not a person); keeping a repeater's admin password (ADR-0044).

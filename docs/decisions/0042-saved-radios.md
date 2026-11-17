# ADR-0042 — Saved radios; the history stays with the phone

- Status: proposed (the history per radio is a question for the maintainer)
- Date: 2026-11-16

## Context

People own more than one radio: one at home, one in the pack, one to test with. lichen remembered only the last Bluetooth device, so switching meant pairing again. The brief for this work asked for a list of paired radios to switch between, each keeping its own history "(ADR-0010: one database per radio)". ADR-0010 was written that way, but since the phone's identity (ADR-0013) and the review of 2026-11-15 (SR-39) it says, and the code does, one database **per identity**: the history, the contacts lichen learned, the verified flags, the groups and the other members' sender keys belong to the phone, whichever radio carries them. A database per radio would lose the groups, their keys and every verification when the user switches radios, and bring them back only when they switch back.

The radio also has a battery, which lichen showed only in Settings › Radio.

## Decision

- **Saved radios** (protocol §16.4): the apps save each radio they connect to (how to reach it: Bluetooth identifier, USB device or TCP address; its name; its node key; when it was last used) in the identity's database, list them in Settings, and switch to one or forget it. The radio connected now is marked.
- **The history stays with the phone's identity** (ADR-0010 unchanged). What each radio keeps is its own: its MeshCore contacts, its channels, its settings, its offline queue. The screen says so: "Your conversations stay on this phone whichever radio you use. Each radio keeps its own MeshCore contacts, channels and settings, and the people you write to see a different node."
- **Low battery**: the radio's battery is read at connection and at most every 10 minutes while connected, when the app runs anyway; at or below 15 % (3.3 V empty, 4.2 V full, as the radio card already showed it) the conversation list shows a warning.

## Consequences

- Switching radios is a tap; nothing of the history is lost or hidden.
- A plain MeshCore chat (§15) shows messages sent through different radios in one conversation; the other side saw them from different nodes.
- To revisit if the maintainer wants a history per radio: the database name would take the node key, and the groups, keys and verifications would have to be shared between those databases or redone per radio.
- Rejected: one database per radio as the brief asked (see above), until the maintainer confirms it; keeping the list in each app's preferences (twice the code, and not with the rest of the phone's data).

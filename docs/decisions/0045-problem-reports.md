# ADR-0045 — Problem reports without content

- Status: accepted
- Date: 2026-11-16

## Context

Before lichen goes to people other than its author, they need a way to say "this did not work" with enough detail to find out why: which radio and firmware, which link, what the core saw (a session that never opened, a transfer that stalled, the companion answering errors). lichen's whole point is that messages, contacts and positions stay on the phones; a log that carried any of them, sent by email to a developer, would undo that. Until now neither the core nor the apps kept a log at all.

## Decision

- **Settings › About › Report a problem** shows the report first, in full, then shares it as a text file through the system's share sheet. Nothing is sent by lichen itself; the user chooses where it goes.
- **What it holds:** the time (UTC), lichen's versions (core, protocol, app, system, phone model), the link (Bluetooth, TCP, USB, demo) and whether a radio session is open, the radio's model, firmware and build date as the radio reports them, its LoRa settings, TX power and battery, the companion frame counters and error answers, the airtime used this hour, whether the database opened, counts (contacts, groups, channels, texts waiting, radio settings waiting for the radio's answer), a few app settings (iroh, Tor, GIF loading, voice mode, app lock, language), then the core's last 300 log lines and the app's last 200.
- **What it never holds:** a message or any part of one, a key or id (node, identity, group, channel), a name (contact, nickname, group, member, channel, the radio's or ours), a position, an address. Not even counts per contact or per group.
- **By construction in the core:** `lc-ffi` keeps its log in memory (`report.rs`); a line is a fixed word written in the code (`&'static str`), with at most one number and its unit (an SNR, a TX power, an error code). Each core event is logged by kind only, and each setting asked of the radio, and its outcome, by its kind, never its value ("radio setting asked: preferences", "radio setting saved: preferences"), so a setting with no outcome line after it shows; progress is left out. No runtime string can enter it.
- **Masked as a safety net for the app:** the app's own lines are fixed words too (link states, foreground, retries), but they come in as strings, as do the settings and versions. Before the report is written, every string from the app or the radio goes through a mask: the names the core knows (contacts, nicknames, groups, members, invitations, channels, the radio's) and the words the app adds (our card's name, Bluetooth device names, the TCP host) become `[name]`, case ignored, whole words; runs of 8 or more hex digits become `[hex]`; numbers with 4 or more decimals (coordinates) become `[number]`.
- **Memory only:** both logs start empty at each launch (and when the core is replaced, for the demo); nothing is written to disk.
- **A test proves it:** `report::tests::a_report_carries_no_secret` runs the demo network for four minutes (messages both ways, a group, positions shared, a nickname), adds app lines that do carry a name, a key and a position, and checks that none of the texts, names, keys (whole or their first 4 bytes), ids or positions appear in the report.

## Consequences

- A user can send something useful without trusting the developer with their conversations, and can read every line before sending.
- The report says nothing about one conversation in particular: a failure to one contact shows as "failed: no path" among others, with its time. When that is not enough, the user describes it in their own words, which they choose to share.
- A crash loses the log (memory only); a persistent, rotated log on disk was rejected for now: it would be another file to protect and to wipe.
- Rejected: a log of the companion frames (they carry names, keys and plain texts); masking a free-form log after the fact without the fixed-words rule (one missed pattern would leak); sending reports to a server (lichen has none).

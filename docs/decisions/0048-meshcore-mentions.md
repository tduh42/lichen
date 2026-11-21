# ADR-0048 — MeshCore mentions and replies

- Status: accepted
- Date: 2026-11-21

## Context

A user asked for the stock app's replies (F-3, decision D-15): "in meshcore you can mention a previous message, it's not handled on the app". The stock app's sources are closed. What its messages show: a reply is a channel text `<sender>: @[<name>] <text>`, and its channels have a "self mention" notification setting next to "new message". Nothing in the text identifies a message: a mention names a person. lichen showed `@[Marc]` as written.

## Decision

- **The form** is `@[name]`, the name one character or more without `]` or a line break (§15.9). That is as loose as the observed messages allow: MeshCore names are free text up to 31 bytes, and a mention typed by hand works the same.
- **A leading mention is a reply**; anywhere else, a mention.
- **The quote is guessed, and said to be**: the latest earlier message of the same conversation whose sender has that name. It is what the stock app's user most likely answered, and the only thing the text allows. It is resolved when messages are listed, so a message arriving late or deleted changes it, and it never reaches across conversations.
- **Our own replies quote what the user answered**: the phone keeps which row a reply sent from lichen answers (`plain_replies`), so a reply to an older message quotes that one, not the latest by that name. It is not in the backup file (ADR-0037): restored, the guess applies.
- **Mentions us**: our radio's current name, compared exactly. Renaming the radio changes which later messages mention us; a looser match (case, spaces) would flag messages meant for a neighbour with a similar name.
- **Muted conversations still notify a mention of us**, as the stock app's separate setting does by default. The core gives the flag; the apps decide, as they already do for muting.
- **Spans from the core**: the apps get the whole text cut into spans (text, link, place, mention), so they parse nothing.

## Consequences

- A quote can be wrong when the person wrote again before the reply came, or answered something older: the apps show it as a quote of that person's latest message, which is what the convention gives.
- Someone can write `@[<our name>]` to get through a muted channel. It is one message, from a sender the user can block, and the stock app behaves the same.
- A sender whose name holds `]` cannot be answered from lichen.
- Rejected:
  - a case-insensitive match for "mentions us" (a neighbour's similar name would match);
  - storing the guessed quote when a message arrives (a late or deleted message would leave it stale);
  - a URL scheme or hidden marker to point at a message (stock apps would show it as text).

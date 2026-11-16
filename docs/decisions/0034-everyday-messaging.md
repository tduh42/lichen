# ADR-0034 — Everyday messaging

- Status: accepted
- Date: 2026-11-16

## Context

Milestone M7 adds the conveniences people expect from a messenger. None changes what travels on the air; each still needs a rule that can be checked later, because each touches what the phone fetches, shows or keeps.

## Decision

### Links and places in a message

- **What is a link**: an address that starts with `http://`, `https://` or `www.` (opened as `https://`), after a character that is not a letter or a digit. It ends at a space, a control character, a character that reorders text (SR-35) or one of `<`, `>`, `"` and the backquote; sentence punctuation and closing brackets it does not open are left out. Nothing else is a link: no `javascript:`, `file:`, `mailto:` or bare domain.
- **Nothing is fetched**: no preview, no title, no favicon. A link opens in the browser when the user taps it, and only then does anyone learn the phone's address.
- **What is a place**: a `geo:` URI (RFC 5870, and the `geo:0,0?q=lat,lon` that maps apps share); two decimal numbers, latitude then longitude, with three decimals at least when neither carries a degree sign or a hemisphere letter; degrees with minutes (and seconds) and a hemisphere (`N`, `S`, `E`, `W`, and `O` for the French "ouest"). Latitude within ±90°, longitude within ±180°. A number glued to a letter, a dot or another number is not a coordinate.
- **A place opens lichen's map**, centred on a pin, with its coordinates to copy; the long-press menu offers "Show on the map". Nothing leaves the phone but the map's tiles, as for any position (ADR-0011).
- **One scanner for both apps**: `lc_core::links::find`, exposed as `text_links` with UTF-16 offsets (what Swift's `NSRange` and Kotlin's strings count). Fuzzed (`text_links`).

### Answering from a notification

- A message's notification offers **Reply** and **Mark as read**; both need the phone unlocked (iOS `authenticationRequired`, Android `setAuthenticationRequired`), so that whoever holds a locked phone neither answers nor hides messages as its owner.
- **With the app lock on**, notifications say "New message" and offer nothing: no sender, no text, no action.
- A reply is an ordinary text to that conversation, through the outbox (§6.1): it waits for the radio like one typed in the app. A reply that is refused (too long, a contact whose identity changed) is said in a notification of its own.
- Invitations, searches and SOS alerts have no actions.
- **What a notification carries is checked as if another app had written it** (review of 2026-11-15, SR-25): the conversation must be a known kind with an id of the right length, the action one of ours, the text non-empty and at most 4,096 bytes. On Android the actions go to a receiver that is not exported, through explicit intents; the reply's is mutable only because the system writes the typed text into it.

## Consequences

- Both apps link the same spans; a test in Rust covers what is and is not a link or a place.
- A pair of numbers with three decimals that is not a place ("version 1.2345, 2.3456") is offered as one: tapping it shows a pin somewhere wrong, nothing worse.
- Rejected:
  - the platforms' own detectors (`NSDataDetector`, `Linkify`): they differ between the apps, find phone numbers and addresses that would hand text to other apps, and know nothing of coordinates;
  - link previews: they fetch a page the sender chose, from every phone that shows the message, and tell its server who reads it.

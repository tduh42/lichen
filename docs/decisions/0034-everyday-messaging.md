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

### Block

- **Local**: nothing is sent, and the person is not told. A block holds a node and the identity it is known under (§4.6), kept in the store (`blocked`) and restored into the core at start.
- **What it drops**, in the core, before anything is answered: every lichen frame from that node (introductions, handshakes, probes, session frames: the node is no longer a candidate for a frame's hash byte); an introduction that proves the blocked identity from another node (INTRO_FIN, INTRO_ACK); MeshCore direct messages from that node (§15.2). Internal datagrams (group state, keys) are no longer queued for it. The other side sees its messages fail like messages to someone unreachable.
- **Groups**: the member's posts (texts, replies, reactions, edits, deletes, stickers, GIFs, timers) are not reported, but kept for the others as §10.6 requires; its positions and SOS still show, since safety comes first. Its posts already on the phone are hidden from the history, the list and search, and show again when unblocked. Blocking a group's admin also stops its group updates reaching us: leave the group instead when that is what is wanted.
- **Ours to them**: refused (`Blocked`); messages waiting for them fail (`Refused`) at the block. The chat shows "You blocked …" with Unblock instead of the composer.
- MeshCore channels are left alone: their authors are names anyone can take.

### Archive

- `chat_prefs.archived`: the conversation leaves the list for an "Archived" row that opens in place. A received message (not a notice) brings it back, unless the conversation is muted then; our own messages do not.

### Drafts

- The message a draft answers (§6.2) is kept with the draft, per conversation and identity, across screens and restarts; it is dropped when that message is no longer on the phone.
- A voice recording made in a conversation stops when the user leaves it and waits there, in memory: coming back shows it under review, to listen to, send or discard. It never goes to another conversation.

### Media and storage

- **Chat info › Media, files and voice**: what a conversation holds — FILE (photos and small files), VOICE and ANNOUNCE (content over the bridge) — newest first; photos in a grid showing the best version on the phone, files and voice messages in lists. MeshCore chats carry text only and have none.
- **Settings › Storage**: per conversation, its messages, how many are media, and its bytes: the database's (messages and the full images kept for others) plus the downloads of what it announced.
- **Clear media older than** 30 days, 90 days or 1 year: each such message goes as a delete for me goes (§6.4): its row, its media, its reactions and edits, with `secure_delete` overwriting them (SR-28), and an announcement's download and offer (`MediaGone`). Its place in the conversation goes too: a placeholder would keep its date and sender, which the user asked to clear. Ours still being sent stay.

### Sharing into lichen

- **Android**: `ACTION_SEND` and `ACTION_SEND_MULTIPLE` of any type. What comes in is another app's data (SR-25): a text of 4,096 characters at most; only `content:` URIs, never `file:` and never one of lichen's own providers (another app could otherwise make lichen send lichen's own files); ten at most. Nothing is read until the user picks a conversation; each file is then read with the 64 MB limit of the file picker, and its name passed through `fit_file_name` before anything shows or sends it.
- **iOS**: a share extension would need an App Group to hand what it receives to the app. Apple's "Supported capabilities (iOS)" table lists App Groups per membership type, but its marks did not survive a text fetch, and reports on free Personal Teams disagree; lichen is built with whatever team the maintainer has, so it does not depend on one. Instead lichen declares document types (any data or content, rank Alternate, not opened in place): "Open in lichen" from the share sheet or Files gives the app a copy in `Documents/Inbox`, read once (64 MB at most), its name sanitised, then deleted. Texts cannot come this way.
- Then the same picker on both: lichen contacts (not blocked) and groups; MeshCore chats too for a text alone. The conversation opens with the text in the draft, a photo on the photo sheet, a file on the file sheet, so that the route and its cost are shown as for anything picked in the app.

## Consequences

- Both apps link the same spans; a test in Rust covers what is and is not a link or a place.
- A pair of numbers with three decimals that is not a place ("version 1.2345, 2.3456") is offered as one: tapping it shows a pin somewhere wrong, nothing worse.
- Store version 13: `blocked`, `chat_prefs.archived`. One event, `ChatPrefsChanged`, when a message brings a conversation back.
- What a blocked person sends while blocked is lost: nothing is kept to show after an unblock, except group posts already received before the block.
- Rejected:
  - blocking in the apps only (frames from the blocked node would still be answered, and introductions would still attach an identity);
  - keeping a blocked person's direct messages hidden (the phone would still store what the user asked not to receive);
  - the platforms' own detectors (`NSDataDetector`, `Linkify`): they differ between the apps, find phone numbers and addresses that would hand text to other apps, and know nothing of coordinates;
  - link previews: they fetch a page the sender chose, from every phone that shows the message, and tell its server who reads it.

# Field checklist

Run it again before each release, on real radios, with the build that will be published. The fake network and the demo test the logic; only this finds what radios, phones and the air do to it.

## Setup

- **Two phones with lichen**, A and B, ideally one iPhone and one Android phone, each with its own stock MeshCore companion radio (firmware v1.18 or later), paired over Bluetooth. Both on the same LoRa preset (Settings › Radio settings).
- **A third radio with the official MeshCore app** (phone or web client), C, on the same preset.
- For 5.6–5.8, optional: **a stock MeshCore repeater and a room server** in range, whose passwords you hold (yours, or lent for the test). For 6.5, optional: **a second companion radio** for A.
- Both lichen apps freshly installed, or with the history of the previous run deleted, so that "new contact" steps start from nothing.
- Start the run in the same room (one hop, radio to radio), except where a line says otherwise.
- Note at the top of the run: date, lichen version and commit, phones and systems, radios, firmware versions, preset. Each line below passes or fails; a failure gets a problem report (Settings › About › Report a problem) from both phones, and an issue.

| Run | |
|---|---|
| Date | |
| lichen version (commit) | |
| Phone A (system) / radio, firmware | |
| Phone B (system) / radio, firmware | |
| Radio C (MeshCore app version) | |
| Preset | |

## Pairing and links

| # | Do | Passes when |
|---|---|---|
| 1.1 | A: welcome › Connect my radio, pair over Bluetooth with the code shown on the radio. | The radio card shows the radio's name and "connected"; the airtime meter reads 0 s of 360 s. |
| 1.2 | A: switch the radio off for 30 s, then on. | The chat list says "Radio disconnected"; within a minute of the radio's return it says connected again, without touching the phone. |
| 1.3 | A (optional, a Wi-Fi companion or `lichen bridge`): Settings › Pair a radio › Network, type `host:5000`, Connect. | The pairing screen states the link is not encrypted; the radio card shows "TCP · … · not encrypted". |
| 1.4 | With 1.3 connected: stop the bridge (or the radio) for 20 s, then start it again. | The app reconnects by itself within 30 s; a text written meanwhile waits, then goes. |
| 1.5 | Quit the app with the TCP link up, open it again. | It reconnects to the same address. Disconnect stops it. |

## lichen ↔ lichen

A and B exchange their contact codes in person first (Add a contact › My code / Scan, both ways).

| # | Do | Passes when |
|---|---|---|
| 2.1 | A sends B a short text. | B shows it within seconds; A shows it delivered. With Settings › Show radio details on, long-press › Info gives its fragments and airtime. |
| 2.2 | B replies to that message (swipe or long press › Reply). | A shows the reply with the quoted original above it. |
| 2.3 | A reacts to B's reply, then changes the reaction. | B shows the first reaction, then only the second. |
| 2.4 | A edits its first text. | B shows the new text, marked "edited". |
| 2.5 | A deletes a message for everyone. | B shows "Message deleted" in its place. |
| 2.6 | B forwards one of A's messages to A. | A receives it as a new message from B, with the same content (text or medium). |
| 2.7 | A sends a sticker. | B plays the animation, with no Internet on either phone. |
| 2.8 | A pastes a GIPHY link and sends the GIF. | B shows a GIF card; with Internet it loads (tap, or automatically if B chose it). |
| 2.9 | A records and sends 10 s of voice (Standard). | B plays it, intelligible; both show the same duration; the airtime meter of A grows by the expected amount. |
| 2.10 | A sends a photo. | B shows the preview first; "Load the full image" brings the full one; with both phones on the Internet the original comes over the bridge. |
| 2.11 | A sends a small file (a few KB) and one too big for the hour's airtime. | The small one arrives and opens on B; the big one is refused on A with the time at which it would fit, not queued. |
| 2.12 | A sets disappearing messages to 5 minutes in the chat. | B shows the timer notice; a message sent then is gone from both phones 5 minutes later. |
| 2.13 | B, out of range or radio off: A sends a text; B comes back 10 minutes later. | A shows it waiting, then delivered once B is back (within 24 h). |
| 2.14 | A sends a text with a web address and a place (`geo:` or decimal coordinates with three decimals). | On B the address is a link that opens the browser only when tapped (no preview); the place opens lichen's map on a pin; long-press › Show on the map does the same. |
| 2.15 | B: lichen in the background, phone unlocked. A sends two texts; B answers the first from its notification (Reply) and marks the second as read. | A receives the answer; B's chat shows it sent and both texts read. With B's phone locked, the actions ask to unlock first. |
| 2.16 | B: Settings › Privacy › app lock on, then A sends a text. | B's notification says only "New message", with no actions. Turn the lock off afterwards. |
| 2.17 | B: start an answer to A's message (Reply), leave the chat, quit lichen, open it again. | The draft and the message it answers are still there. |
| 2.18 | B archives the chat with A; A sends a text. | The chat leaves B's list for "Archived", then comes back with A's text. |
| 2.19 | B blocks A (chat menu › Block); A sends a text; B unblocks A. | B shows "You blocked …" instead of the composer and nothing from A arrives; A's text fails as to someone unreachable. After unblocking, A's next text arrives. |

## Trust

| # | Do | Passes when |
|---|---|---|
| 3.1 | A (new install, or a third lichen phone D that never scanned A) writes first to B through the contact list (radios that heard each other's adverts). | B shows a request in Messages, "Someone using A's radio wants to write to you", with A's message. B accepts: the message moves into a conversation with A, unverified ("key to verify"), and B's answer gets through. |
| 3.1b | Repeat 3.1 with D, and B deletes the request; then again, and B blocks it. | Delete: the request and its messages go; D's next message is a new request. Block: D's messages stop, Settings › Privacy › Blocked lists "Someone using D's radio", and D's radio can still be used by someone else. |
| 3.2 | A and B compare safety numbers (Contact › Safety number), then scan each other's codes. | Both show the same numbers, and the contact becomes "key verified" on both sides. |
| 3.3 | On A: Settings › Your identity › Start a new identity (after noting the 24 words), then write to B. | B refuses the new identity: nothing is shown until B compares safety numbers and accepts it; "They differ" keeps it refused. Restore A's 24 words afterwards. |
| 3.4 | Restore A from its 24 words. | B sees A's original identity again, verified, without accepting anything new. |

## A group of two

| # | Do | Passes when |
|---|---|---|
| 4.1 | A creates a group with B (New conversation › New group). | B gets "A invites you to …" when A is not verified, and joins only after accepting; otherwise B joins at once. |
| 4.2 | Or: A shows the group's invitation QR (Invite with a QR code), B scans it next to A. | B joins within a minute or two, and A shows "B joined with your invitation". |
| 4.3 | A and B each post twice. | Every post shows on the other phone, signed (no warning). |
| 4.4 | B's radio off while A posts twice, then B back on. | B gets both posts back within about five minutes, marked "arrived late" (recovery). |
| 4.5 | A renames the group. | B shows "A renamed the group …" and the new name. |
| 4.6 | A removes B. | B shows "A removed you"; A's next post does not reach B. |
| 4.7 | A adds B again, then B leaves the group. | A shows that B left; B no longer lists the group. |
| 4.8 | A records a voice note in the group (6 s at most) and sends it. | The review shows its cost before sending; B plays it, signed. |
| 4.9 | A: Group info › B › Make admin; then B renames the group; then A leaves. | Both show B as admin; A sees B's new name; A leaves without being asked to choose a successor or close the group (B already is admin). |

## MeshCore

| # | Do | Passes when |
|---|---|---|
| 5.1 | C posts on Public. | A and B show the post in Public (it starts muted), with C's name. |
| 5.2 | A joins the hashtag channel `#lichen-test` (New conversation › Join a channel), C joins it too; A posts once. | C reads A's post in `#lichen-test`, prefixed with A's name on the mesh; C's answer reaches A. |
| 5.3 | C sends A a direct message from the official app. | A shows it in a MeshCore chat with C, marked not lichen-encrypted. |
| 5.4 | A answers C. | C receives it in the official app; A shows it delivered. |
| 5.5 | A shares `#lichen-test` by QR code (the channel's info › Share the channel); B scans it, and C scans it in the official app if it can. | A warns that whoever scans can read and post; B and C join the channel by its name. |
| 5.6 | A: Settings › Nodes, the room server › Log in, password kept. Read its posts, post one. Disconnect the radio, wait for a post from someone else, reconnect. | The room's posts show in a room chat marked as not lichen-encrypted; A's post appears on the server; after reconnecting, A logs in by itself and the new post arrives. |
| 5.7 | A: Settings › Nodes, the repeater › Log in with its admin password; Status, Neighbours, then `ver` in the Console. Quit and reopen lichen. | Status and neighbours match what the official app shows; the console prints the firmware's answer; after reopening, the admin password has to be typed again. |
| 5.8 | A: Settings › Nodes, a contact two hops away or more › Trace the path. | The path shows hop by hop, named where the radio knows the repeaters; the trace comes back with an SNR per hop each way, or says that no answer came. |
| 5.9 | A: Settings › Nodes, then the map's nodes layer. | The repeaters and companions the radio heard are listed with their type and last heard; those that advertise a position show on the map, nothing else. |

## Radio settings

| # | Do | Passes when |
|---|---|---|
| 6.1 | A: Settings › Radio settings. | Model, firmware, battery and storage match what the official app shows for that radio. |
| 6.2 | Change TX power, then put it back. | The value read back from the radio is the one chosen. |
| 6.3 | Turn "Position in adverts" on, send an advert, check on C; then off. | C sees the position only while it is on (and the warning was shown before). |
| 6.4 | Change the preset to another one, then back (the app asks first). | The radio reboots or reconnects with the new settings, then the old ones; 2.1 still passes. |
| 6.5 | A pairs its second radio, then switches back to the first in Settings › Radios. | Both radios are listed, the connected one marked; the conversations, contacts and groups are the same with either radio; each radio keeps its own MeshCore contacts and channels. |

## History import

| # | Do | Passes when |
|---|---|---|
| 7.1 | On a phone that used the official MeshCore app: export its database, then in lichen Settings › History › Import from the MeshCore app. | The counts shown match; the MeshCore chats and channel posts appear with their times; a second import adds nothing. |

## Backup and the encrypted database

| # | Do | Passes when |
|---|---|---|
| 8.1 | Install this build over one from before the encrypted database (2026-11-17), with history on the phone. | It starts in the usual time, with every message, contact and group; a second start too. |
| 8.2 | A: Settings › Backup › Export a backup, saved outside the phone. | The app says how many messages and media it holds. |
| 8.3 | Delete lichen on A, reinstall it; at the welcome screen, Restore a backup with that file, first with a wrong word, then with the 24 words. | A wrong word is refused with its own message and nothing is installed. With the right words: messages, verified contacts, groups and downloaded photos are back; a text that was waiting shows as failed "interrupted"; B's next message arrives (a new handshake). In a group with B (make one if 4.9 left none), B's next post reaches A within a few minutes: A asks for the key the backup left out. |
| 8.4 | Android, with a debug build of the same commit: `adb shell run-as com.tduhamel.lichen rm no_backup/database.key`, then open lichen. iOS: the debug build with `-identityProblem databaseKeyLost`. | The "key gone" screen offers to restore a backup or start with an empty history; restoring 8.2's file brings the history back; starting empty keeps the identity (B still sees A verified). |

## Search and SOS (two devices of the same person only)

Never trigger an SOS or a search towards anyone who has not agreed to the test. Use two phones and radios of the tester, in the same group.

| # | Do | Passes when |
|---|---|---|
| 9.1 | On B: the group's "Let this group locate me" on. A: the Locate tab, B. | A shows measurements from B's answers and an estimate that narrows while A moves. |
| 9.2 | B: SOS on (with GPS). | A gets the full-screen SOS alert with B's exact position, again every 2 minutes. |
| 9.3 | B: SOS on without a GPS fix (indoors, or location off). | A's alert says there is no position and offers to search by radio (Locate); B's beacons arrive every 2 minutes. |
| 9.4 | B: SOS off. | A shows "SOS ended"; B's TX power, if a search lowered it, is back to what it was. |

## Walk test (optional, real hops)

| # | Do | Passes when |
|---|---|---|
| 10.1 | B walks away until A and B only hear each other through a repeater (the official app shows the path). A sends a text, a voice message and a photo preview. | All three arrive; the conversation shows the path's hops; failures, if any, say why (no path, stalled). Note distances, hops and SNR in `docs/measurements.md`. |

# Field checklist

Run it again before each release, on real radios, with the build that will be published. The fake network and the demo test the logic; only this finds what radios, phones and the air do to it.

## Setup

- **Two phones with lichen**, A and B, ideally one iPhone and one Android phone, each with its own stock MeshCore companion radio (firmware v1.18 or later), paired over Bluetooth. Both on the same LoRa preset (Settings › Radio settings).
- **A third radio with the official MeshCore app** (phone or web client), C, on the same preset.
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
| 2.1 | A sends B a short text. | B shows it within seconds; A shows it delivered, with its airtime. |
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

## Trust

| # | Do | Passes when |
|---|---|---|
| 3.1 | A (new install, or a third lichen phone D that never scanned A) writes first to B through the contact list (radios that heard each other's adverts). | B receives it from an unverified contact ("key to verify"); B's answer gets through. |
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

## MeshCore

| # | Do | Passes when |
|---|---|---|
| 5.1 | C posts on Public. | A and B show the post in Public (it starts muted), with C's name. |
| 5.2 | A joins the hashtag channel `#lichen-test` (New conversation › Join a channel), C joins it too; A posts once. | C reads A's post in `#lichen-test`, prefixed with A's name on the mesh; C's answer reaches A. |
| 5.3 | C sends A a direct message from the official app. | A shows it in a MeshCore chat with C, marked not lichen-encrypted. |
| 5.4 | A answers C. | C receives it in the official app; A shows it delivered. |

## Radio settings

| # | Do | Passes when |
|---|---|---|
| 6.1 | A: Settings › Radio settings. | Model, firmware, battery and storage match what the official app shows for that radio. |
| 6.2 | Change TX power, then put it back. | The value read back from the radio is the one chosen. |
| 6.3 | Turn "Position in adverts" on, send an advert, check on C; then off. | C sees the position only while it is on (and the warning was shown before). |
| 6.4 | Change the preset to another one, then back (the app asks first). | The radio reboots or reconnects with the new settings, then the old ones; 2.1 still passes. |

## History import

| # | Do | Passes when |
|---|---|---|
| 7.1 | On a phone that used the official MeshCore app: export its database, then in lichen Settings › History › Import from the MeshCore app. | The counts shown match; the MeshCore chats and channel posts appear with their times; a second import adds nothing. |

## Search and SOS (two devices of the same person only)

Never trigger an SOS or a search towards anyone who has not agreed to the test. Use two phones and radios of the tester, in the same group.

| # | Do | Passes when |
|---|---|---|
| 8.1 | On B: the group's "Let this group locate me" on. A: Search for B. | A shows measurements from B's answers and an estimate that narrows while A moves. |
| 8.2 | B: SOS on (with GPS). | A gets the full-screen SOS alert with B's exact position, again every 2 minutes. |
| 8.3 | B: SOS on without a GPS fix (indoors, or location off). | A's alert says there is no position and offers to search by radio; B's beacons arrive every 2 minutes. |
| 8.4 | B: SOS off. | A shows "SOS ended"; B's TX power, if a search lowered it, is back to what it was. |

## Walk test (optional, real hops)

| # | Do | Passes when |
|---|---|---|
| 9.1 | B walks away until A and B only hear each other through a repeater (the official app shows the path). A sends a text, a voice message and a photo preview. | All three arrive; the conversation shows the path's hops; failures, if any, say why (no path, stalled). Note distances, hops and SNR in `docs/measurements.md`. |

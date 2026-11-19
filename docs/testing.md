# Testing lichen

Every user-facing flow, numbered, with what it must do. Each scenario is automated at the lowest
layer that can show it:

- **FFI**: `crates/lc-ffi/tests/scenarios_*.rs`, the API on the demo network, as the apps call it
  (runs in the Rust CI job).
- **iOS**: XCUITests in `apps/ios/LichenUITests/`, on the demo network (CI job `ios`).
- **Android**: Compose UI tests in `apps/android/app/src/test/` (Robolectric) or `androidTest/`
  (CI job `android`).

The **Covered by** column names the test(s); `real radio` marks what only a real companion shows
(see the checklist at the end). A bug found gets a test that fails before its fix, at the lowest
layer that shows it.

Launch flags are the debug arguments listed in CLAUDE.md (iOS `-flag value`, Android `--ez`/`--es`/`--ei`).

## 1. Onboarding and identity

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| ON-1 | First launch, onboarding to the end | Identity created, recovery phrase shown once, Messages empty with the radio prompt | `-onboarding YES` | |
| ON-2 | Restart after onboarding | Same identity (fingerprint unchanged), no onboarding | | |
| ON-3 | Recovery phrase: show, confirm | Words match the identity; wrong confirmation refused | | |
| ON-4 | My card / QR | Card shows name and QR; the QR decodes to the card | `-screen identity` | |
| ON-5 | Identity failure screens | Each problem shows its screen and its way out | `-identityProblem …` | |
| ON-6 | Verify a contact (safety number, scan) | Contact marked verified; number identical on both sides | `-screen verify` | |
| ON-7 | Add a contact by card (paste, scan) | Contact added, conversation can start | `-screen contact` | |

## 2. Radio link and radio settings

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| RL-1 | Demo radio connects | "ThinkNode M5 connected", contacts and channels arrive | `-demo YES` | |
| RL-2 | Radio drops, comes back | Banner "Radio disconnected"; waiting texts go once it is back | `-offline 60 -online 150` | |
| RL-3 | Text expires while offline | Marked failed after the expiry, can be resent | `-expiry 30` | |
| RL-4 | Saved radios: list, forget, reconnect | The list matches; forgetting removes it | | |
| RL-5 | Radio settings: read, change, read back | Values shown match the radio; a change is applied and shown | `-screen radio` | |
| RL-6 | Refused setting | Error shown, the old value stays | `-screen radio` | |
| RL-7 | Battery and radio card | Battery level shown; radio details when turned on | `-lichen.radioDetails YES` | |
| RL-8 | Radio clock set at connection | The radio's time follows the phone's | | FFI: `lc-core/tests/radio.rs` |
| RL-9 | Contact list read whole / cut short | All contacts, asked again when records are missing | | lc-core unit tests |

## 3. Contacts, nodes, rooms, repeaters, requests, blocks

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| CT-1 | Contacts list | People only, sorted, names and hop badges right | | |
| CT-2 | Rename a contact | Shown everywhere (list, chat title, notifications); reset restores the node name | | |
| CT-3 | Nodes: repeaters, rooms, heard nodes | Each with its kind, position, last heard | | |
| CT-4 | Room: log in, read posts, post | Posts arrive, the room's row is in Messages, a post goes out | `-roomLogin hello -open "Refuge board"` | iOS: `ConversationListTests` |
| CT-5 | Room: wrong password | Refused, said so | | |
| CT-6 | Repeater: admin login, status, CLI | Status read, a CLI command answered | | |
| CT-7 | Trace to a node | Hops and SNRs shown | | |
| CT-8 | Introduction request: accept | Request card in Messages; accept opens the chat with its messages | `-requestDemo YES` | |
| CT-9 | Request: block, delete | Blocked: nothing more from them; deleted: gone | `-requestDemo YES` | |
| CT-10 | Block and unblock a contact | Their messages stop and come back; Settings › Blocked lists them | | |
| CT-11 | A contact the radio files as a non-person | Their conversation stays in Messages under their name | `-marcRepeater YES` | iOS: `ConversationListTests` |

## 4. Direct lichen chats

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| DM-1 | Send a text | Sent, then delivered; shown once | `-open Camille -say …` | |
| DM-2 | Receive a text | Row updates, unread badge, notification | | |
| DM-3 | Long text (fragmented) | Arrives whole | | |
| DM-4 | Reply | Quoted message shown and tappable | `-reply …` | |
| DM-5 | React, change, remove reaction | Shown on both sides | `-react …` | |
| DM-6 | Edit a sent message | Edited text and mark on both sides | | |
| DM-7 | Delete for me / for everyone | Gone here; for everyone gone there too | | |
| DM-8 | Disappearing timer | Messages vanish after the time, both sides | `-timer 300` | |
| DM-9 | Paging older messages | Older pages load, order kept, no duplicates | `-fill 250 -earlier YES` | iOS: `LongChatTests` |
| DM-10 | Search messages | Results across chats; opening one shows it in place | `-screen searchMessages -query …` | |
| DM-11 | Message Info | Times, route, fragments, airtime | `-screen info` | |
| DM-12 | Unread counts and mark read | Badge per row and on the tab; opening clears | | |
| DM-13 | Forward a message | Arrives in the other chat | | |
| DM-14 | Copy, links and places in a text | Links open; places open the map | | |

## 5. MeshCore chats and channels

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| MC-1 | Plain chat with a stock MeshCore user | Marc's message arrives; the answer reaches him; badge "MeshCore" | `-open Marc -sayPlain …` | |
| MC-2 | Public channel: read and post | Posts with their authors; a post goes out | `-open Public` | |
| MC-3 | Channel QR: show and join | The code shows after the warning; scanning one joins the channel | | |
| MC-4 | Import the MeshCore app's history | Messages imported once; a second import adds nothing | | |

## 6. Media

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| MD-1 | Send a photo | Preview goes; cost shown first; refused when over the hour's airtime | `-screen media` | |
| MD-2 | Full photo requested | Card on the photo; Send sends it, Not now doesn't; asking again shows it again | `-media YES` | FFI: demo test (#32) |
| MD-3 | Receive a photo, open the viewer | Thumbnail, viewer, save | `-screen photo` | |
| MD-4 | Voice: record, review, send, play | Plays on both sides | `-screen voiceReview` | |
| MD-5 | Files: small note, big document | Small sent; big refused or offered by bridge | `-screen file` / `fileBig` | |
| MD-6 | GIFs and stickers | Picker, send, shown animated | `-sticker …` | |
| MD-7 | Bridge: fetch a photo over iroh | Fetched, checked, shown | `-media YES` | FFI: `lc-ffi … --ignored bridge` |

## 7. Groups

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| GR-1 | Create a group | Shown in Messages, alone in it | | |
| GR-2 | Invite by QR, someone joins | The member appears; their posts arrive | `-screen invite -inviteDemo YES` | |
| GR-3 | Join by scanning an invitation | Group opens with its members | `-screen join -joinNow YES` | |
| GR-4 | Invitation received, accept / decline | Card in Messages; accepted joins, declined goes | | |
| GR-5 | Post and receive in a group | Authors named, order kept | | |
| GR-6 | A post lost, recovered late | Gap revealed, post comes back marked late | `-demo YES` (Julien) | |
| GR-7 | Group voice note (≤ 5 fragments) | Cost shown, sent, plays | | |
| GR-8 | Rename, members, leave | Changes shown to all; after leaving, nothing more arrives | | |

## 8. Location, map, Locate and SOS

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| MP-1 | Share my position with a contact | Precision chosen; their side sees it; stopping ends it | `-share Camille` | |
| MP-2 | Positions received on the map | Camille exact, Julien coarse (1 km cell) | | |
| MP-3 | Nodes layer | Repeaters and rooms where they advertise | | |
| MP-4 | Offline maps | Download a region; the map works without network | | |
| MP-5 | SOS: send, receive, stop | Beacon shown to the group; stop ends it | `-sosdemo YES` | |
| MP-6 | Locate a member without GPS | Search converges, estimate shown | `-sosdemo YES` | |

## 9. Settings

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| ST-1 | Privacy settings | Each switch applied and kept | `-screen privacy` | |
| ST-2 | App lock | Cover at launch and after the delay; unlock with Face ID / passcode | `-lichen.lock YES` | |
| ST-3 | Backup: export, restore | The restored phone has contacts, groups, messages | | |
| ST-4 | Problem report | Built, shareable, no names or keys in it | | FFI: `report::tests` |
| ST-5 | About and version | Version and build of the release | | |
| ST-6 | French throughout | No English left on any screen | Android `cmd locale …` | |
| ST-7 | Dark mode throughout | Every screen readable | Android `cmd uimode night yes` | |
| ST-8 | Bridge settings (iroh, Tor, relay only) | Applied; Tor starts and stops | `-lichen.tor YES` | |
| ST-9 | Accessibility | Every screen passes the audit | | iOS: `AccessibilityAuditTests` |

## 10. Lifecycle

| Id | Scenario | Expected | Flags | Covered by |
|----|----------|----------|-------|------------|
| LC-1 | Notification opens the right chat | Tapping it opens that conversation (each kind: lichen, MeshCore, channel, room, group) | | |
| LC-2 | Mute, archive, pin | Muted: no notification; archived: back on a message unless muted; pinned on top | | |
| LC-3 | Delete a conversation | Gone; a new message starts it again | | |
| LC-4 | Restart keeps everything | Messages, contacts, groups, invitations, requests, prefs | | |
| LC-5 | Background and back | Messages received meanwhile are there; no duplicates | | |
| LC-6 | Every conversation listed | Any chat with a message has a row, whatever the contact list says | | FFI: `unit.rs`; iOS: `ConversationListTests` |

## Real radio checklist (on the phone)

What the demo network cannot show, to run on the iPhone with a real companion:

1. Bluetooth: pair a new radio, reconnect after the radio restarts, after the phone sleeps.
2. Contacts: every person the radio holds is in Contacts (`lichen contacts <port>` gives the radio's list).
3. A lichen text to and from another lichen user; delivered mark.
4. A photo, then its full variant on request.
5. A MeshCore chat with a stock app user; a Public channel post.
6. Radio settings changed in lichen, seen in the stock app.

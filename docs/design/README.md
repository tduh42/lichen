# lichen — design handoff

The design of the lichen apps (2026-11-06), for iOS (SwiftUI) and Android (Jetpack Compose). French original: [`README.fr.md`](README.fr.md). The mockups in `mockups/` are the reference for layout; this file gives the rules, the tokens and the screens.

## Contents

| Path | Contents |
| --- | --- |
| `mockups/index.html` | Gallery of every screen, open it in a browser |
| `mockups/ios/*.html` | 14 iOS screens, 390×844, linked together |
| `mockups/android/*.html` | the same 14 screens in Material 3 |
| `mockups/epaper/*.html` | 3 e-paper screens of the ThinkNode M5, 200×200, 1 bit (oxymesh, L10) |
| `mockups/logo.html` | logo sheet (icon, lockups, palette, rules) |
| `tokens.json` | dark and light colours, type, spacing, radii, sizes |
| `assets/` | the "chirp" mark in SVG, 1024 app icons (main and alternative) |
| `png/` | the iOS screens rendered (Quick Look), for quick reference |

## Principles

1. **Dark theme by default.** The light theme exists (`light` in `tokens.json`) as an option.
2. **Orange means LoRa and the brand, nothing else.** Primary actions are white in dark (`act_bg`), ink in light. Never orange for a generic button.
3. **The transport is always written**, never carried by colour alone: badge "LoRa · 2 hops", "iroh", "Tor".
4. **Every radio send shows its cost before it starts**: fragments and seconds of airtime. The airtime left is always one tap away: the radio line of Messages opens the radio card, Settings shows it, and a banner says when it runs out.
5. **Type**: the system font for the interface (SF Pro, Roboto). Space Grotesk 700 for the wordmark and big figures (budget, bearing, distance, fingerprints, safety numbers), tabular figures.
6. **Calm chats**: a message shows its time and a status word ("Delivered"); its fragments, airtime and signal are in long press › Info, or under every message with Settings › Show radio details (off by default).
7. **Rich radio screens** (2026-11-25): the Nodes tab, a node's page, the Map and Radio settings show the radio's data at a glance. Each node heard directly carries its signal, rated against your own preset (`SignalMeter`); the others say how many repeaters they go through; a node's page draws the route that reaches you, leg by leg (`RouteRibbon`), its signal over 24 hours or 7 days against your preset's Good and Fair bands, and the readings it sends (`ReadingsCard`); the Map draws that route between the repeaters' positions; Radio settings opens on the radio itself (name, model, link, battery, airtime left, noise floor, firmware) and ends with what it counted since it started; while a connection reads the radio, the radio line says which step it is at. A figure nothing measured is never shown: an unmeasured leg says "not measured", a figure the radio has not reported has no row. A node's readings are asked by hand only: opening its page never asks. Chats stay calm (6).
8. **Accessibility**: targets ≥ 44 pt (iOS) / 48 dp (Android), text contrast ≥ 4.5:1, accessibility labels on every icon button.

## Colours (details in `tokens.json`)

| Token | Dark | Light | Use |
| --- | --- | --- | --- |
| `bg` | #0E0E0D | #F7F7F5 | screen background |
| `s1` | #1A1A18 | #FFFFFF | cards, lists, bars |
| `s2` | #262623 | #EFEFEB | received bubbles, fields, segments |
| `line` | #34342F | #E2E2DC | separators |
| `tx` / `tx2` | #F4F4F0 / #ABABA3 | #141413 / #55554F | text / secondary |
| `act_bg` / `act_fg` | #F4F4F0 / #141413 | #141413 / #FFFFFF | primary button, sent bubble |
| `lora` | #FB923C | #C2410C | LoRa transport and brand |
| `iroh` | #60A5FA | #1D4ED8 | iroh transport |
| `tor` | #A78BFA | #6D28D9 | Tor transport |
| `sos` | #EF4444 | #B91C1C | distress, destructive action, weak signal |
| `ok` | #4ADE80 | #15803D | verified key, delivered, good signal, heard within 2 h |
| `warn` | #FACC15 | #A16207 | warning, to check, fair signal |

Each semantic colour has a `_bg` (badge or banner background) and a `_fg` (text on it).

## Type, spacing, radii

- iOS: large title 34/700, body 16, secondary 13–14, caption 12 (the text style `.caption`: `.caption2` does not get smaller with Dynamic Type, which the accessibility audit flags). Android: bar title 22/400, the rest the same.
- Spacing 4, 8, 12, 16, 24; screen margins 16.
- Radii: cards 14 (iOS) / 16 (Android); buttons 14 and 50 high (iOS) / pill and 48 high (Android); badges 8.

## Screens

| Screen | File | Contents and states |
| --- | --- | --- |
| Welcome | `accueil` | logo, three-point promise, "Connect my radio", "Later" |
| Pairing | `appairage` | BLE scan, radios (name, RSSI, companion version), 6-digit code shown on the radio |
| Identity and keys | `identite` | nickname, key fingerprint (6 groups of 4 hex), backup warning, 24-word phrase, encrypted file |
| Add a contact | `contact` | tabs My code / Scan, QR, safety number (6 groups of 5 digits), "match" / "differ" |
| Conversations | `conversations` | radio state in one line (a tap: airtime budget, available transports); while a connection reads the radio, the step it is at: its settings, its contacts ("contacts 34 of 120", the count the radio announced), its channels, then the messages that waited on it (so many so far), with no time left guessed; list with transport badge and unread |
| Chat | `chat` | text, fragmented send with progress and ACK, announced photo (LoRa preview + iroh/Tor download), Codec2 voice, estimated cost in the composer |
| Send a photo | `media` | mode: LoRa preview, iroh, Tor, deferred; cost per option |
| Nodes | (in the apps) | the nodes the radio knows: "In reach", heard directly, each with its `SignalMeter`, sorted by signal; "Through repeaters", each with its hops, or "path unknown" last; filters (All, People, Repeaters, Rooms, Heard in 2 h); distance and bearing; last heard |
| A node's page | (in the apps) | header with the large meter or the hops; "How it reaches you" (`RouteRibbon`, Trace again, Show on the map); its signal over 24 h / 7 days, a dot per reading in its rating's colour over the Good and Fair bands of your own preset (the core's `signal_limits()`, none until the radio's settings are read); "Readings it sends" (`ReadingsCard`), except on your own radio's page; then its room, repeater admin and MeshCore key cards |
| Map and location | `carte` | members and shared precision, time-limited sharing, SOS button; Positions or Locate at its top; the nodes layer, with a ring in its rating's colour around each node heard directly; a node's route, each leg coloured by its last signal, with a legend; while a route is drawn, its stops always show and the other markers fade, read "not on this route", until its card closes; the layers menu filters the nodes with the Nodes tab's chips and their counts, the Map's choice kept apart from the Nodes tab's |
| Locate (search mode, a mode of the Map) | `recherche` | SOS banner, likely area, searchers' bearings, bearing + distance + uncertainty, measurements, Ping / Add a bearing / Lower power |
| Group | `groupe` | members and their key state, key rotation after a departure, consent to be located, hop limit |
| Settings | `reglages` | connected radio, budget, preset, repeater mode, iroh / Tor / relay only, default precision; a tile on every row |
| Radio settings | (in the apps) | the radio on top (name, model, link, battery, airtime left, noise floor "−118 dBm", firmware); a search that jumps to a setting or a figure; sections as cards; each change says on its row whether the radio saved it; last, "Since it started": on for, packets received and sent, received with errors (and their share of those received), airtime sent and received (and its share of the time), last packet heard (dBm · dB, no rating colour: it may come from anyone), each row only once the radio has reported it, read again every minute while the screen is open |
| SOS | `sos` | hold for 3 s, what is sent, to whom, how |
| SOS received | `sos-recu` | full-screen alert, last position, battery, start the search |
| Degraded states | `etats` | banners: radio disconnected, budget spent, no path, send abandoned, peer unreachable, delivered; empty state |
| M5 screens | `epaper/*` | active SOS beacon, arrow and distance, Yagi bearing |

## Navigation

- First launch: Welcome → Pairing → Identity → Add a contact → Conversations.
- Tabs: Messages, Nodes, Map, Settings. Locate (the radio search mode: "Search" is for messages only) is a mode of the Map, Positions | Locate at its top, next to the SOS button. SOS received → the Map in Locate.
- Chat → Send a photo as a sheet (`.sheet` on iOS, `ModalBottomSheet` on Android).

## Shared components

`TransportBadge` (kind, label), `AirtimeMeter` (remainingS, budgetS), `FragmentProgress` (sent, total, retries, etaS), `Banner` (kind sos/warn/lora/iroh/ok, title, body, optional action), `MediaAnnouncement` (title, size, transports, hashVerified), `VoiceNote` (durationS, codec, waveform), `BearingCard` (bearingDeg, distanceM, uncertaintyM), `MeasurementRow` (who, rssi, snr, bearing, age), `PinEntry` (6 digits), `SafetyNumber` (6 groups of 5 digits), `KeyFingerprint` (6 groups of 4 hex), `SosHoldButton` (holdMs 3000, circular progress), `SignalMeter` (rating good / fair / bad / none, snrDb: three bars in `ok`, two in `warn` or one in `sos`, the rest in `line`, and "7.5 dB" in tabular figures; none shows nothing; small in rows, large in headers; read "Signal good, 7.5 dB"), `HopPill` ("1 hop", "2 hops"), `RouteRibbon` (stops: you, each repeater, the node, a shared hash showing its names joined by " / "; legs: a line in its last signal's colour with its SNR above, dashed `line` and "not measured" when nothing measured it), `ReadingsCard` (rows: battery "3.92 V · about 69 %", temperature "4 °C", each other value by kind, "Humidity · sensor 2" when not the node's own, each row read as one; "Sent 18 min ago" and Ask again; its battery's line over 7 days once two readings have one; with no reading yet, what to do and Ask, or for a repeater or a room not logged in, that it answers only those logged in, as a guest at least, and Log in; "Asking…"; then, until the next request, what came of it: no answer, worded for a person, who answers only if they allow it, or for a server, maybe out of reach; an answer with nothing lichen reads; the radio cannot ask it, not one of its contacts, with no button; a position it sends is listed, never put on the map).

## Implementation notes

- iOS: SwiftUI, `NavigationStack`, `TabView`, `.insetGrouped` lists, `.sheet` for sending a photo. Android: Compose, Material 3, `Scaffold` + `NavigationBar`, dynamic colour (Material You) off to keep the brand.
- The UI observes the states the Rust core exposes through UniFFI; no protocol logic in the UI.
- The mockups' map background is schematic. M5 screens: 200×200 pure black and white, two buttons (short / long press).
- Names and radio values in the mockups are examples. App icon: `assets/app-icon-1024.svg`; alternative `app-icon-alt-1024.svg`.

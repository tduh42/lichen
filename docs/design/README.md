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
4. **Every radio send shows its cost before it starts**: fragments and seconds of airtime. The airtime left is always visible (conversations, settings).
5. **Type**: the system font for the interface (SF Pro, Roboto). Space Grotesk 700 for the wordmark and big figures (budget, bearing, distance, fingerprints, safety numbers), tabular figures.
6. **Accessibility**: targets ≥ 44 pt (iOS) / 48 dp (Android), text contrast ≥ 4.5:1, accessibility labels on every icon button.

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
| `sos` | #EF4444 | #B91C1C | distress, destructive action |
| `ok` | #4ADE80 | #15803D | verified key, delivered |
| `warn` | #FACC15 | #A16207 | warning, to check |

Each semantic colour has a `_bg` (badge or banner background) and a `_fg` (text on it).

## Type, spacing, radii

- iOS: large title 34/700, body 16, secondary 13–14, caption 11–12. Android: bar title 22/400, the rest the same.
- Spacing 4, 8, 12, 16, 24; screen margins 16.
- Radii: cards 14 (iOS) / 16 (Android); buttons 14 and 50 high (iOS) / pill and 48 high (Android); badges 8.

## Screens

| Screen | File | Contents and states |
| --- | --- | --- |
| Welcome | `accueil` | logo, three-point promise, "Connect my radio", "Later" |
| Pairing | `appairage` | BLE scan, radios (name, RSSI, companion version), 6-digit code shown on the radio |
| Identity and keys | `identite` | nickname, key fingerprint (6 groups of 4 hex), backup warning, 24-word phrase, encrypted file |
| Add a contact | `contact` | tabs My code / Scan, QR, safety number (6 groups of 5 digits), "match" / "differ" |
| Conversations | `conversations` | radio state, airtime budget, available transports, list with transport badge and unread |
| Chat | `chat` | text, fragmented send with progress and ACK, announced photo (LoRa preview + iroh/Tor download), Codec2 voice, estimated cost in the composer |
| Send a photo | `media` | mode: LoRa preview, iroh, Tor, deferred; cost per option |
| Map and location | `carte` | members and shared precision, time-limited sharing, SOS button |
| Search mode | `recherche` | SOS banner, likely area, searchers' bearings, bearing + distance + uncertainty, measurements, Ping / Add a bearing / Lower power |
| Group | `groupe` | members and their key state, key rotation after a departure, consent to be located, hop limit |
| Settings | `reglages` | connected radio, budget, preset, repeater mode, iroh / Tor / relay only, default precision |
| SOS | `sos` | hold for 3 s, what is sent, to whom, how |
| SOS received | `sos-recu` | full-screen alert, last position, battery, start the search |
| Degraded states | `etats` | banners: radio disconnected, budget spent, no path, send abandoned, peer unreachable, delivered; empty state |
| M5 screens | `epaper/*` | active SOS beacon, arrow and distance, Yagi bearing |

## Navigation

- First launch: Welcome → Pairing → Identity → Add a contact → Conversations.
- Tabs: Messages, Map, Search, Settings. SOS from the Map; SOS received → Search mode.
- Chat → Send a photo as a sheet (`.sheet` on iOS, `ModalBottomSheet` on Android).

## Shared components

`TransportBadge` (kind, label), `AirtimeMeter` (remainingS, budgetS), `FragmentProgress` (sent, total, retries, etaS), `Banner` (kind sos/warn/lora/iroh/ok, title, body, optional action), `MediaAnnouncement` (title, size, transports, hashVerified), `VoiceNote` (durationS, codec, waveform), `BearingCard` (bearingDeg, distanceM, uncertaintyM), `MeasurementRow` (who, rssi, snr, bearing, age), `PinEntry` (6 digits), `SafetyNumber` (6 groups of 5 digits), `KeyFingerprint` (6 groups of 4 hex), `SosHoldButton` (holdMs 3000, circular progress).

## Implementation notes

- iOS: SwiftUI, `NavigationStack`, `TabView`, `.insetGrouped` lists, `.sheet` for sending a photo. Android: Compose, Material 3, `Scaffold` + `NavigationBar`, dynamic colour (Material You) off to keep the brand.
- The UI observes the states the Rust core exposes through UniFFI; no protocol logic in the UI.
- The mockups' map background is schematic. M5 screens: 200×200 pure black and white, two buttons (short / long press).
- Names and radio values in the mockups are examples. App icon: `assets/app-icon-1024.svg`; alternative `app-icon-alt-1024.svg`.

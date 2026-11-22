# CLAUDE.md — lichen

An **encrypted** application layer on top of the **stock MeshCore** network, running on the host (phone, PC). No repeater or companion firmware is modified: the app talks to the companion through its protocol (BLE or USB) and carries its frames in `RAW_CUSTOM` packets (unicast, direct route) and `GRP_DATA` packets (groups, flood). It adds what MeshCore lacks: long fragmented messages, modern AEAD, private groups, media, location, search mode; and it keeps what MeshCore has: plain chats and channels, room servers, repeaters, the radio's settings.

The original design: `docs/spec/design.md` (French original `design.fr.md`); normative spec: `docs/spec/protocol.md`. Mockups: `docs/maquettes/`. Passages marked **(ADR-xxxx)** refer to `docs/decisions/`.

---

## 1. Non-negotiable constraints

- **Stock MeshCore v1.18** (commit 81dbae5): no assumption of a modified firmware. MeshCore facts come from oxymesh's verified documentation (`../oxymesh/docs/spec/{air,companion}.md`) and are copied into our spec with their source.
- **Self-contained** (ADR-0001): no code dependency on oxymesh or loranet. Only the optional `sim/` workspace, outside the build, may use oxymesh's simulator. The vectors in `testdata/meshcore/` are copied from oxymesh (data, not code), except `settings.toml` and `mesh.toml`, written from the firmware's handlers by `settings.py` and `mesh.py`.
- **Never reuse a nonce** (ADR-0003): every session key is fresh, `tag` is a per-direction counter, `attempt` is part of the nonce. A proptest checks it.
- **The identity is the phone's** (ADR-0013): a seed in the Keychain / Keystore, never the node's key; the node key only routes (frame headers, paths). A contact is a node plus, once known, an identity and a verified flag; strangers meet through the introduction (§4.6).
- **Never put a location in a MeshCore advert or telemetry without the user's explicit choice** (ADR-0032), never flood media (no media in groups before the bridge, L7), except a group voice note of at most 5 fragments, its cost shown before sending (ADR-0035); a medium that does not fit in the hour's airtime is refused, not queued (§13.4). A coarse location is a fixed grid cell centre, never random noise (ADR-0011).
- **Duty cycle**: values in `config/eu868.toml`, never hard-coded; the budget is computed on the host before every costly send.

## 2. Architecture

```
crates/
  lc-meshcore/  no_std  app side of the companion protocol (subset: also radio settings, traces, logins),
                        USB framing, on-air frame header, MeshCore-style ECDH, LoRa airtime; what
                        repeaters and room servers answer (server.rs)
  lc-proto/     no_std  lichen frame (7-byte header), crypto (ChaCha20-Poly1305 as RFC 8439, 16-byte tag, HKDF),
                        handshake, introduction (intro.rs), fragmentation and reassembly machines
                        (ACK-on-Error), fixed capacities
  lc-core/      std     sessions, contacts, paths, introduction requests and blocks (core.rs, ADR-0046,
                        ADR-0034), groups (group.rs), airtime budget, send queue, events;
                        location shares (location.rs, ADR-0011); search mode (search.rs, particle
                        filter in solver.rs, ADR-0016); identity tools (identity.rs:
                        phrase, fingerprint, safety number, card); links and places in a text
                        (links.rs, ADR-0034); plain MeshCore chats and channels (plain.rs, ADR-0026);
                        radio settings (radio.rs, ADR-0032); the MeshCore app's history (import.rs,
                        ADR-0033); nodes, paths, traces (mesh.rs, ADR-0041); room servers and
                        repeaters (server.rs, ADR-0043, ADR-0044); SQLite store (store.rs,
                        ADR-0010); encrypted backup file (backup.rs, ADR-0037); the store under
                        SQLCipher (cipher.rs, feature `sqlcipher`, ADR-0036); fake MeshCore
                        network for tests (fakenet.rs)
  lc-media/     std     media over LoRa (ADR-0012): Codec2 voice frames, AVIF thumbnails (preview + full)
  lc-bridge/    std     Internet bridge (ADR-0015): async (tokio), iroh endpoint and Arti (Tor) onion
                        service, `lichen/blob/1` offers and fetches checked against BLAKE3; the only
                        crate with network IO
  lc-cli/       std     `lichen` binary: USB client for real-world tests (info, contacts, send, listen with
                        the echo bot in bot.rs, advert, card, search recorder, bridge to TCP, sim)
  lc-ffi/       std     UniFFI for Swift and Kotlin: core + store behind one lock, demo network (demo.rs).
                        One module per area: records and events the apps read, `inner.rs` (the state
                        behind the lock), `on_event.rs`, `outbox.rs`, `amendments.rs`, `link.rs` (TCP
                        link rules), `report.rs` (problem reports, ADR-0045), and the exported API in
                        `api_*.rs`; unit tests in `unit.rs` and `unit_mesh.rs`
apps/ios/               SwiftUI, project generated by XcodeGen (L4); map: MapLibre + OpenFreeMap (L5)
apps/android/           Jetpack Compose (L9), Gradle (AGP 9, Kotlin 2.4); the core through UniFFI's Kotlin
                        bindings and JNA (scripts/build-android-core.sh)
fuzz/                   cargo-fuzz targets (separate workspace, nightly)
sim/                    optional: scenarios on oxymesh's simulator (separate workspace)
config/                 regulations (TOML, with sources)
docs/spec/              specifications — source of truth
docs/decisions/         short ADRs
docs/brand/             logo ("Chirp"), palette, icon sources
testdata/               vectors
```

Everything is **IO-free**: time (ms) and randomness are injected, state machines return frames to write and timers. The apps and the CLI are just *runners*.

## 3. Code rules

- `no_std` crates: no allocation, no floats, `#![forbid(unsafe_code)]`; clippy `deny(unwrap_used, expect_used, indexing_slicing, panic)` outside tests.
- The only `unsafe` code is `crates/lc-media/src/codec2_c.rs`, the FFI to the vendored Codec2 (ADR-0012); keep it there, fuzzed.
- **Hardened parsers**: zero-copy, never panic on external input; one `cargo-fuzz` target per decoder, created together with it.
- **Nothing is interpreted before the AEAD tag is verified**, except the 7-byte header (which is in the AAD).
- **Storage stays out of the core logic**: `lc-ffi` records what the core reports and restores contacts and groups (snapshots) at start. Never store sessions or our own group sender key (ADR-0003, ADR-0010).
- Hand-written formats, little-endian. Serde only for config and tools.
- **Spec first**: every rule starts in `docs/spec/`, then vectors, then code. An ambiguity → an ADR.
- Everything in the repository is in **English**: docs, specs, ADRs, comments, identifiers, commit messages. **App UI text is English with a French translation** (String Catalog on iOS, `values-fr` on Android).

## 4. Tests

- Unit tests, `proptest` (round trips, unique nonces, forged fragment rejected), vectors (`testdata/`).
- Fake network in `lc-core`: chain of repeaters, losses, half-duplex collisions, 160-entry duplicate filter on `SHA256(type ‖ payload)`.
- Hardware: Heltec V4 (MeshCore C++ v1.18, USB) and ThinkNode M5 (CH340 bridge); port names change between sessions (`ls /dev/cu.*`, e.g. `/dev/cu.usbmodem2141101`, `/dev/cu.wchusbserial2120`). Both are shared with oxymesh and loranet (echo bot): check `lsof` and ask the maintainer before using them.
- Measurements go to `docs/measurements.md`.

## 5. Roadmap

Every change lands through a pull request (remote: `github.com/tduh42/lichen`, public): one branch per topic, CI (`scripts/check.sh`, the Android build and its UI tests, the iOS UI tests on pull requests; `.github/workflows/check.yml`) must pass before `main` takes it, and it is squash-merged, the PR description being the commit message. A merge to `main` is a release of its own: CI tags it `vMAJOR.MINOR.N` (`scripts/version.sh`: `VERSION` is the series, N the commits since it changed) and publishes the signed APK with the merged PRs as notes (key in `~/.config/lichen/`, password in the Keychain, both in the repo's Actions secrets). See `CONTRIBUTING.md`, "Versions".

- **L0** repository, normative spec, ADRs; **L1** `lc-meshcore` + `lc-proto`; **L2** `lc-core` (direct messages), fake network, `lc-cli`;
- **L3** groups; **L4** `lc-ffi` + iOS app; **L5** location and map; **L6** media over LoRa;
- **L7** iroh bridge then Tor; **L8** search mode; **L9** Android app; **L10** on-device search (in oxymesh).

## 6. Commands

```sh
scripts/check.sh                  # one version, fmt, clippy -D warnings, tests, no_std build, no floats,
                                  # advisories and licences (cargo deny, deny.toml); CHECK_SKIP_FUZZ=1, CHECK_OFFLINE=1
scripts/fuzz.sh N [target…]       # fuzz, N seconds per target
cargo test --workspace
cargo run --release -p lc-cli -- sim --repeaters 3 --loss 10 --bytes 10000   # fake network
cargo run --release -p lc-cli -- info /dev/cu.usbmodem2141101                # a real companion
cargo run --release -p lc-cli -- contacts /dev/cu.wchusbserial2120           # its contact table, record by record
                                  # (type, path byte, readable or not, count announced): read only
cargo run --release -p lc-cli -- send /dev/cu.usbmodem2141101 --to bob --text "hello"
cargo run --release -p lc-cli -- listen /dev/cu.wchusbserial2120
cargo run --release -p lc-cli -- listen /dev/cu.usbmodem14301 --advert --echo --save local/received \
  --outbox local/outbox.txt     # field-test bot: echoes, saves media; lines appended to the outbox are sent
cargo run --release -p lc-cli -- advert /dev/cu.usbmodem14301          # MeshCore advert (flood)
cargo run --release -p lc-cli -- card /dev/cu.usbmodem14301 --svg local/card.svg   # contact card + QR
cargo test -p lc-proto --release --test transfer -- --ignored --nocapture    # measurements
cargo test -p lc-core --release --test fakenet -- --ignored --nocapture
cargo test -p lc-core --release --test location -- --nocapture            # location: dead reckoning figures
cargo test -p lc-core --release --test identity -- --nocapture            # introduction costs
cargo test -p lc-core --test backup                                       # backup file: vectors, round trip, refusals, restore
cargo test -p lc-core --features sqlcipher --test sqlcipher               # the store under SQLCipher (CommonCrypto on macOS,
                                  # OpenSSL built from source elsewhere); the apps' cores are built with it (ADR-0036)
cargo test -p lc-core --features sqlcipher --release --test sqlcipher -- --ignored --nocapture   # first-start migration time
cargo test -p lc-core --release --test search -- --ignored --nocapture    # search: convergence figures
LICHEN_LOSS=20 cargo test -p lc-core --release --test groups -- --ignored measure_group_recovery --nocapture
                                  # group delivery and recovery (LICHEN_SEEDS, LICHEN_REVEAL=0, LICHEN_DIGEST=0)
cargo test -p lc-core --test requests --test block --test mesh --test second_read   # requests, blocks, mesh tools,
                                  # the second read's reproductions (fake network)
python3 -I testdata/meshcore/mesh.py > testdata/meshcore/mesh.toml     # a vector oracle (each says how it runs)
python3 -I scripts/licences.py                                         # the apps' licence list (check.sh: --check)
python3 -I scripts/ios-strings.py --check                              # the iOS text takes ’, never ' (check.sh)
python3 -I docs/readme/gen.py                                          # the README's diagrams
scripts/install-ios.sh                            # Release build onto a USB iPhone (free Apple ID), from a Terminal
cargo run --release -p lc-cli -- search /dev/cu.usbmodem14301 --csv local/rx.csv --at 45.19,5.72   # field recorder
cargo run -p lc-media --release --example encode_voice -- in.wav out.c2 heard.wav   # voice fixtures
cargo test -p lc-bridge                                                   # bridge over loopback (no Internet)
cargo test -p lc-bridge --release -- --ignored n0 --nocapture            # bridge over n0's relays (Internet)
cargo test -p lc-bridge --release -- --ignored tor --nocapture           # bridge over Tor (about 30 s)
cargo test -p lc-ffi --release -- --ignored bridge                        # demo: Camille's photo over iroh
CFLAGS="-fsanitize=address -mllvm -asan-guard-against-version-mismatch=0" scripts/fuzz.sh 600 voice_decode
(cd sim && cargo test --release -- --nocapture)                            # against oxymesh's MeshCore logic
scripts/build-ios-core.sh && (cd apps/ios && xcodegen generate)           # iOS core + project
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcodebuild -project apps/ios/Lichen.xcodeproj \
  -scheme Lichen -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' -derivedDataPath local/dd-sim \
  CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM= build   # ad-hoc signed: unsigned builds have no Keychain (-34018)
python3 -I apps/ios/sync-strings.py                                       # String Catalog keys after a build
swift scripts/render-app-icon.swift apps/ios/Lichen/Assets.xcassets/AppIcon.appiconset   # app icons
scripts/build-android-core.sh                                             # Android core (arm64) + Kotlin bindings
(cd apps/android && ./gradlew lintDebug testDebugUnitTest assembleDebug)  # lint, JVM tests on the host core, APK
(cd apps/android && ./gradlew testDebugUnitTest --tests 'com.tduhamel.lichen.ui.*')   # Compose UI tests on the JVM
                                  # (Robolectric, docs/testing.md ids): the app on the demo network in simulated time,
                                  # no emulator; `@Config(qualifiers = "fr")` / `"night"` for French and dark mode
(cd apps/android && ./gradlew assembleRelease)   # R8 build; signed with the release key when LICHEN_KEYSTORE and
                                  # LICHEN_KEYSTORE_PASSWORD are set (CI), else the debug key; LICHEN_VERSION_NAME / _CODE number it
scripts/version.sh --version|--build              # the release (MAJOR.MINOR.N) and the build number
scripts/android-release-key.sh                    # once: the release key (~/.config/lichen), its password in the
                                  # Keychain (security find-generic-password -s lichen-android-release -w), the CI secrets
$ANDROID_HOME/emulator/emulator -avd lichen-pixel -allow-host-audio &   # with a window, the Mac's audio and mic
adb install -r apps/android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n com.tduhamel.lichen/.MainActivity --ez demo true --es open Camille   # debug extras
cargo run --release -p lc-cli -- bridge /dev/cu.usbmodem2101    # the radio's serial port on 127.0.0.1:5000 (TCP)
adb shell am start -n com.tduhamel.lichen/.MainActivity --es tcp 10.0.2.2:5000   # emulator on the real radio
```

Android: the core's database is SQLCipher on OpenSSL built from source, which needs perl and make (ADR-0036). Toolchain from Homebrew, `JAVA_HOME=/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home`, `ANDROID_HOME=/opt/homebrew/share/android-commandlinetools` (NDK 28.2.13676358, AVD `lichen-pixel`, API 36 arm64); the emulator counts as one of the two simulators allowed at a time. minSdk 31 (AVIF decoding, ADR-0012), compileSdk 37. The bindings are generated into `app/src/main/java/com/tduhamel/lichen/core` (git-ignored); their byte arrays compare by reference, so the app keys maps by `ChatId` / hex strings. Debug extras mirror the iOS launch arguments (`--ez demo true`, `--es open|say|tab|share`, `--es screen contact|identity|sos|locate|group|verify|pairing|file|fileBig|media`, `--ez media|sosdemo|onboarding true`, `--es tcp 10.0.2.2:5000` for the real radio through `lichen bridge`); the per-app language: `adb shell cmd locale set-app-locales com.tduhamel.lichen --locales fr`, dark: `adb shell cmd uimode night yes`.

iOS: at most two simulators at a time (three are too slow). The design reference is `docs/design/` (the theme follows the system by default; orange only for LoRa and the brand). The radio-search tab is "Locate" (FR "Localiser"); fragments and airtime show only with Settings › Show radio details (off by default), then in a message's long-press › Info. Debug launch arguments for screenshots: `-demo YES`, `-tcp 127.0.0.1:5000` (a real radio through `lichen bridge`), `-open <contact, group or room server>`, `-roomLogin <password>` (logs in to the radio's room servers at launch, remembered; the demo's board takes "hello", §15.8), `-say <text>`, `-sticker <emoji>` (a sticker to whoever writes first), `-tab map|locate|settings`, `-share <contact>` (100 m), `-media YES` (voice to Camille, full images requested, announcements fetched), `-autoFull YES` (a contact's request for the full variant of your photo is sent at once instead of waiting for Send, §13.3; Android `--ez autoFull true`), `-onboarding YES` / `-onboarded YES`, `-screen pairing|media|share|sos|group|locate|contact|identity|searchMessages|file|fileBig` (`-query <text>` fills the message search; `file` / `fileBig`: the file sheet with a 3 KB note or a 240 KB document),  `-identityProblem unavailable|corrupt|notSaved|storage|databaseKey|databaseKeyLost` (the identity or database failure screen; Android `--es identityProblem …`, in lower case), `-sosdemo YES` / `-sosdemo2 YES` (Julien in SOS without GPS on the demo network, a real search; Camille in SOS too), `-fill 250` (older messages with Camille, paging, the last five of hers unread; Android `--ei fill 250`; `-focus <text>` then opens on the first message holding it), `-earlier YES` (the earlier page at once), `-reply <text>` / `-react <emoji>` (answer / react to Camille's first message), `-screen invite` (a group of your own, alone, its invitation QR on screen; with `-inviteDemo YES`, Léa next to you scans it and joins), `-screen join` (Camille shows an invitation to "Refuge tonight" and you scan it; `-joinNow YES` joins at once and opens the group; Android `--es screen invite --ez inviteDemo true`, `--ez joinNow true`), `-requestDemo YES` (the demo keeps introduction requests on: Camille's first message arrives as a request in Messages, to accept, block or delete, ADR-0046; her group, sent meanwhile, is dropped; Android `--ez requestDemo true`), `-marcRepeater YES` (the demo radio files Marc as a repeater: he leaves the people, his MeshCore chat stays in Messages under his name, §15.8; Android `--ez marcRepeater true`), `-groupInvitationDemo YES` (Android `--ez groupInvitationDemo true`: the demo keeps group invitations on: Camille's group, at 60 s, arrives as an invitation in Messages, to join or decline, §10.1), `-screen photo` / `-screen voiceReview` (the photo viewer, the voice review bar; Android `--es screen voiceReview` with `--es open …`: a 4 s recording under review in that chat), `-screen info` (long press › Info on your first message once delivered, with `-open` and `-say`), `-lichen.radioDetails YES` (Settings › Show radio details: fragments, airtime and signal under every message, the radio card on Messages), `-timer 300` (the opened chat's messages disappear after 5 minutes, §6.5; Android `--ei timer 300`), `-offline 60` / `-online 150` / `-expiry 30` (the demo radio drops after 60 s with a text waiting for Camille and one for the group, comes back after 150 s; texts expire after 30 s instead of 24 h; Android `--ei offline 60` etc.; Settings › Demo also switches the simulated radio off), `-lichen.tor YES` / `-lichen.iroh NO` / `-lichen.relayOnly YES` (bridge settings; with Tor on, the demo's Camille also publishes an onion service), `-lichen.lock YES` / `-lichen.lockAfter 60` (the app lock on: the cover at launch; enrol Face ID first with `xcrun simctl spawn <device> notifyutil -p com.apple.BiometricKit.enrollmentChanged`, match with `notifyutil -p com.apple.BiometricKit_Sim.fingerTouch.match`, or the lock turns itself off for want of a passcode; `-lockPrompt NO` leaves the system prompt down), `-screen privacy` (Settings › Privacy), `-screen radio` (Settings › Radio settings, ADR-0032; Android `--es screen radio`; the demo's radio has a GPS). For UI tests (iOS only so far): `-demoSkip 20` (the demo's timed events come 20 s sooner, in order: Camille's first message about 18 s in instead of 38 s; more is not faster, `DemoNetwork::skip_ahead`), `-freshInstall YES` (the app starts as just installed: identity, database key, data and settings removed), `-contactDemo scan|paste` (Léa, next to you, shows her contact code: scanned in person, the camera's stand-in, or left on the pasteboard to paste in Add a contact › Scan; `DemoNetwork::lea_card`), `-savedRadios YES` (outside the demo: two radios used before in Settings › Radios, once), `-quietDemo YES` (no group, photo, position share or channel post on the demo's air: Camille answers a text in about 8 s instead of up to a minute and a half, `DemoNetwork::quiet`), `-demoSpeed N` (N up to 4: the demo network and the core run N times faster than the wall clock, the bots' clocks staying on it; demo only, `DemoNetwork::set_speed`; a stall of the app counts at most 0.25 s, so the core's 5 s wait for the radio is never jumped over; the UI tests use 4; Android `--ei demoSpeed N` once it reads it), `-noBanners YES` (no notification banner over the app, still in the list: the UI tests' taps near the top would wait behind them; the tests that look at notifications leave them), `-radioRefuses power` (the demo radio answers ERR to transmit power changes: a setting the radio itself refuses, D-4; `DemoNetwork::refuse_tx_power`), `-radioIgnores preferences` (the demo radio answers OK to one kind of setting and keeps its old value: `preferences`, `power`, `params`, `position`, `gps`, `autoadd` or `name`; the row says the radio kept it, B-user-5; `DemoNetwork::ignore_settings`; Android `--es radioIgnores preferences`), `-radioSavedFor S` (iOS UI tests: "Saved on the radio" stays S seconds on its row instead of 2, for a busy machine's slow screen reads), `-radioSlow S` (the demo radio holds its answers for the first S seconds of the demo's time: its settings are being read, D-9; `DemoNetwork::hold_replies`), `-answerAfter S` (Camille and Julien answer a text S seconds after it reaches them instead of 4: room to act first on a slow machine, CT-10 blocks Camille before she writes; `DemoNetwork::answer_after`), `-radioNoFix YES` (the demo radio's GPS is on but has no fix, indoors: no position of its own until this phone's is given to its adverts, B-user-2; `DemoNetwork::gps_without_fix`; Android `--ez radioNoFix true`), `-scanChannel <code>` (Join a channel › Scan reads this `meshcore://channel/add?…` code, the camera's stand-in), `-importFile <path>` (Settings › Import from the MeshCore app imports this export, the Files picker's stand-in, MC-4), `-saveBackup <name>` / `-restoreFile <name>` (Export a backup › Save to Files saves it as this name in the app's temporary folder, and Restore a backup › Choose the backup file takes it from there: the Files sheets' stand-ins, ST-3). UI tests: `scripts/ui-tests-ios.sh [--lang fr] [--dark] [--only <Class>] [--device <name or UDID>] [--workers N] [--shard K/N]` builds the simulator core and runs `apps/ios/LichenUITests`, one class per area of `docs/testing.md`; `--workers N` runs them on N clones of the simulator (1, the default, on the Mac, D-12); `--shard K/N` runs the K-th of N shares of the classes, balanced by their run times (CI, on pull requests: five runners of one simulator each, `--shard 1/5` to `5/5`, the fifth also running the French pass, the check `iOS` summing them up; clones ran about three times slower there). The demo bots share positions: Camille (exact, direct) after 90 s, Julien (1 km, group) once the group keys are out. The app's radio misses both copies of Julien's second group post ("Mist on the ridge…", a scripted loss: `Net::deaf` in `fakenet`): his GIF reveals the gap, and the post comes back marked "arrived late" about 5 minutes in (§10.6); his first post, often lost to Camille's photo transfer at the hub, comes back the same way. Two people with the stock MeshCore app share the demo (§15): Marc, at the refuge, writes in plain at 45 s and answers once when the user writes back (`-open Marc` opens his MeshCore chat; `-sayPlain <text>` answers him at once; Android `--es sayPlain …`); Lucie, within reach of the user and of the hub, and Marc post on Public (12 s, 110 s, 280 s, 320 s; `-open Public`): Lucie's at 110 s mentions the demo radio (`@[ThinkNode M5]`, "mentioned you"), Marc's at 320 s replies to hers at 280 s (`@[Lucie] …`, a quote; §15.9). Channel posts are flooded once and never retried, so in the busy first minutes one may be lost. The mesh around (§16, ADR-0041 to ADR-0044): the radio knows the repeaters hub and crest and the "Refuge board" room server, which advertise positions (Settings › Nodes, and the map's nodes layer); Lucie's advert is heard, not added, at 3 s; the hub's admin password is "password" (guests and the board: "hello"); the board holds two posts and gets a third at 200 s. Camille asks for the full variant of a photo you send her (§13.3): its card shows on your photo; Send, and she thanks you once it arrives; Not now, and she asks once more 3 minutes after her request (the demo's hub is busy for the first 5 minutes: a photo sent then may fail).

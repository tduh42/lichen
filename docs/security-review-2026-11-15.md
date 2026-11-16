# Security review — 2026-11-15

Three independent read-only reviews of the tree at `a771051`, one per area, each by a reviewer that had not seen the others' work or the earlier reviews until it had its own list:

- **Core:** the pairwise cryptography (spec §1–§9, `lc-proto`, sessions and paths in `lc-core`).
- **Groups:** groups, content semantics and insiders (spec §6, §10–§12).
- **Edges:** the bridge, media, parsers, storage, both apps, dependencies (spec §13–§15).

Nothing was built or run by the reviewers. "Verified" below means the maintainer then read the cited code and found it as described; "reported" means not yet checked. **A finding counts as confirmed only once a test reproduces it** (the response table at the end records that).

No third party has audited lichen. This review does not replace one.

## Summary

- **The ciphers and their use were found sound.** ChaCha20-Poly1305 assembled as in RFC 8439 with the tag truncated to 8 bytes, X25519, Ed25519 (`verify_strict`), HKDF, BLAKE3 for blobs. No (key, nonce) repeat was found; nothing a passive listener can read; no key recovery.
- **No Critical finding.**
- **Seven High findings**, all in how trust and state are handled around the ciphers: SR-01, SR-02, SR-11, SR-12, SR-13, SR-22, SR-23.
- **Fourteen Medium**, twelve Low, six informational.

## Findings

Severity: High, Medium, Low, Info. "Wire" says whether the fix changes what travels on the air or how keys are derived. Line numbers are those of `a771051`.

### Core

**SR-01 (High, verified). Contacts not verified in person are not pinned.**
- Where: §4.6; `crates/lc-core/src/core.rs` `learn_identity` (2002–2028), `on_intro` (2289–2364), `on_intro_fin` (2429–2473).
- An unverified contact's identity is replaced by any new one (`c.identity = Some(identity)`, line 2019). INTRO names its sender by one hash byte. Anyone sends INTRO with Bob's byte, then INTRO_FIN with their own identity: Alice's later messages to "Bob" go to the attacker, in the same conversation. Against a verified contact the same frames set a pending identity and block sending until the user rejects it.
- Fix: pin at first use for every contact; a change waits for consent while traffic continues to the pinned identity; an INTRO from an unknown identity becomes a request. Wire: no.

**SR-02 (High, verified). Paths are stored from unauthenticated frames and never rediscovered.**
- Where: §4.5; `core.rs` `on_probe` (2195–2240, the path is stored at 2239 after a probe keyed only by public node keys), `on_intro` (2361), `on_hello` (2574); the only reset is at 1139.
- Anyone who knows two node keys (adverts are public) floods one anonymous probe with a bogus path; frames to that contact then go nowhere until the peer writes or the app restarts. A malicious repeater can insert itself instead. §4.5's "fails and probes again" is not implemented.
- Fix: answer such frames but never store their path; adopt a path only after an authenticated frame arrives over it; clear and probe again after a handshake failure. Wire: no (binding INTRO's return path into its keys changes derivation, see SR-06).

**SR-03 (Medium, reported). A HELLO has no freshness and changes state before the initiator is confirmed.**
- Where: §4.1, §4.4; `core.rs` 2529–2604, 541–554, 524–529.
- A recorded HELLO stays valid. Replaying it overwrites the path, can cancel the victim's own handshake, can drop a confirmed session while a datagram is "suspect", and resets the session's age and byte counters so the 24 h / 4 MB renewal never fires. A cheap, selective denial of service; no plaintext.
- Fix: a HELLO only creates a receive-only pending session; everything else changes when a valid frame confirms it. Wire: no.

**SR-04 (Medium, reported). Transmissions an attacker triggers bypass the airtime budget.**
- Where: §9, ADR-0014; `core.rs` 1848–1856, 2237, 2359, 2602, 2511–2526.
- A forged INTRO draws an INTRO_ACK, a forged anonymous probe a PATH_REPLY, a replayed HELLO a HELLO_ACK, a HELLO with a bad tag up to 8 INTROs. All are counted in the budget but only DATA is gated: a few hundred frames an hour exhaust it and push the victim past the duty cycle. ADR-0014's "failures cost nothing" is wrong here.
- Fix: rate-limit unauthenticated replies per peer and globally; no INTRO on a failed tag. Wire: no.

**SR-05 (Medium, verified). The safety number gives about 50 bits against a man in the middle.**
- Where: §4.7; `crates/lc-core/src/identity.rs` 41–56.
- One 30-digit number hashes both identities. An attacker replacing both sides looks for M1, M2 with SN(A, M1) = SN(B, M2): a birthday search of about 2^50 key generations per side, offline.
- Fix: 60 digits, one 30-digit half per identity. Wire: no, but both phones must update.

**SR-06 (Medium, reported). HELLO authenticates with static-static Diffie-Hellman only.**
- Where: §4.1–§4.3; `crates/lc-proto/src/handshake.rs` 52–64, 240–272.
- The pattern is NNpsk0 with the static-static secret as the key. Whoever learns Alice's seed can impersonate any contact to Alice and read past HELLO bodies; data keeps forward secrecy (ephemeral-ephemeral). There is no transcript hash: ephemerals, flags and the return path are not bound into the session keys.
- Fix: mix the ephemeral-static terms and a transcript hash; bind INTRO's return path. Wire: key derivation changes, no extra bytes.

**SR-07 (Low, reported). One random generator for the whole process, never erased.**
- Where: `core.rs` 668, 2087–2090; no zeroize anywhere; `Config`, `Initiating`, `Answer` derive `Debug` over secrets.
- A memory image of a running app yields every ephemeral since launch.
- Fix: fresh OS randomness for ephemerals (or re-key after each handshake), zeroize, redact `Debug`. Wire: no.

**SR-08 (Low, reported). Leaks understated in the documents.**
- Flooded probes show source and destination hash bytes network-wide; an anonymous probe's MAC lets anyone confirm the exact pair (`crates/lc-proto/src/discovery.rs` 53–60). INTRO_ACK gives the responder's identity to any active sender: ADR-0013's "a listener cannot link" holds for passive listeners only.
- Fix: say so in §4.5, §4.6, ADR-0013.

**SR-09 (Info). The truncated-tag bound.** 2^-64 per attempt is heuristic; the simple provable bound for a 64-bit truncation is about ⌈ℓ/16⌉·2^-39. The attempt count (about 12,700 an hour) is right; comparison is constant time; no oracle. Fix: state both in ADR-0014.

**SR-10 (Info). Spec and code disagree.** The 10-minute old receive key of §4.4 is not implemented (the session is dropped, `core.rs` 1806–1813); answered-HELLO memory never expires (§4.2); crossing compares node keys, not identity keys; the replay window accepts only the current tag (`lc-proto/src/session.rs` 288), stricter than §8; INTRO-responder sessions never set their start time (`core.rs` 2464); `design.md`'s nonce and replay rows are stale; `LichenCore::new` does not validate its seed.

### Groups

**SR-11 (High, verified). Group authorship follows the admin-supplied node, not the signed identity.**
- Where: §6.2, §6.4, §10.1; `crates/lc-ffi/src/lib.rs` 2145–2176, 2229–2250; `crates/lc-core/src/store.rs` 1040–1078; `crates/lc-core/src/group.rs` 595–603 (only identities must be distinct), 2119–2124.
- The core verifies the signature against the identity, then reports the member's node; the binding layer keys rows, reactions, the edit/delete author check and positions on that node. A malicious admin lists (E's identity, Camille's node): E's posts show as Camille's, and E's EDIT or DELETE of Camille's message passes the author check on every phone. Probably reachable by a QR-token holder too, through SR-01.
- Fix: key everything by identity; reject duplicate nodes in a state; refuse a join whose contact identity just changed. Wire: no (store migration).

**SR-12 (High, verified). Removal and key rotation are best effort.**
- Where: §10.1–§10.2; `core.rs` `fail_message` (1630–1642: internal datagrams are dropped after 5 tries, 31 minutes); `group.rs` 1324–1376, 2050–2061.
- A member out of reach when Bob is removed keeps Bob listed, never rotates, and sends Bob its new key at its next restart: Bob keeps reading that member. A missed rotated key silently loses that sender; nothing carries the group epoch, so nobody notices.
- Fix: never give up on GROUP_STATE and SENDER_KEY (capped backoff, resend on reconnect and restart, the admin sees who was not reached); a sync request (`gid ‖ epoch held`) for members holding envelopes they cannot open. Wire: the sync request is a new datagram.

**SR-13 (High, verified). Joining needs no consent; SOS trusts every group.**
- Where: §10.1, §11, §12.2; `group.rs` `on_group_state` (1610–1656; members are learned at 1622 before the admin and epoch checks of an existing group); `location.rs` 541–565; `search.rs` 629–631, 865–874, 928–941.
- Any contact with a known identity sends a GROUP_STATE listing the victim: the phone joins, answers with a key and imports up to 30 contacts with names the sender chose. In SOS the exact position goes to every group, and PING and POWER are obeyed from any member: the attacker gets the location, can hold TX power at −9 dBm, and can force a BEACON every 20 s. Junk groups also queue ahead of real SOS frames.
- Fix: an invitation waits for the user; a cap on groups; SOS and search only for accepted groups; a floor or a confirmation for POWER. Wire: no.

**SR-14 (Medium, reported). GROUP_MISSING is an unmetered amplifier.**
- Where: `group.rs` 1428–1483, 1098–1125, 1213–1229; `core.rs` 2703–2717.
- An 83-byte request from any member returns up to 16 payloads, repeatably: about 15 an hour exhaust the victim's budget. A token holder repeating GROUP_JOIN gets the state re-sent each time. GROUP_LEAVE's epoch is never checked.
- Fix: answer each (asker, counter) once, a cap per asker, the duplicate filter on control datagrams, check the leave epoch. Wire: no.

**SR-15 (Medium, verified). The 24-hour edit window is the author's word.**
- Where: `store.rs` 1040–1078 (`a.ts − sent_at ≤ 86,400`, both chosen by the author); `crates/lc-core/src/content.rs` 536.
- A modified client rewrites its own messages months later. Rows for unknown targets are never purged; EDIT text has no cap. (A far-future timestamp cannot beat another member: rows are per author.)
- Fix: also require the receiver's clock (target arrival + 24 h + slack); purge orphans; cap the text. Wire: no.

**SR-16 (Medium, reported). Search counts unauthenticated direct copies.**
- Where: §11.2; `search.rs` 636–696; `core.rs` 2184–2190.
- Any zero-hop frame carrying a message's clear header counts as a reception of the target, and the best SNR is shared. An outsider near a searcher re-emits the header and falsifies ranging during a rescue.
- Fix: count a copy only if its tag verifies, and only the first per copy bit. Wire: no.

**SR-17 (Medium, reported). Two-fragment reassembly can be poisoned by an insider.**
- Where: `group.rs` 2162–2203.
- A fragment valid under B's key is stored before any signature check; B's real fragment is then dropped as "another copy" and the message fails. Two timed injections censor B's messages over 70 bytes, recovery included.
- Fix: keep several candidates per index and try the combinations. Wire: no.

**SR-18 (Low, reported). Relayed keys and replay.**
- Where: `group.rs` 487–489, 1832–1860, 155–160.
- A malicious admin can walk a member's key epoch in one SENDER_KEYS and reinstall the real key with a fresh window: authentic old packets (positions, POWER) then replay. Counters ≥ 2^31 have no floor: an outsider can replay recorded positions and search packets to a newcomer.
- Fix: one entry per owner, small forward steps, retired epochs remembered; a floor for the second counter range (wire), which the planned key step on join must also cover.

**SR-19 (Low, reported). Invitations.** Accepting an invitation overwrites an existing contact's identity, even a verified one, as verified, with no conflict prompt (`group.rs` 1163–1186, `core.rs` 738–750). One photographed token admits up to 31 identities. Fix: the conflict flow; a cap per token. Wire: no.

**SR-20 (Low, reported). Spec and code disagree (groups).** A same-epoch SENDER_KEY from its owner is ignored (`group.rs` 1847) where the spec says adopted; a re-admitted restored phone gets only the admin's key and its own is not forwarded (mute and deaf); `held` is 16 entries globally and can be filled by an outsider who knows `gid_hash` and a sender byte (`group.rs` 2131–2145); references are computed on re-encoded content (`lc-ffi/src/lib.rs` 2203, 2232), not on the bytes as sent.

**SR-21 (Info).** The clear counter range and `key_epoch` reveal position or SOS activity, restarts and removals. No forward secrecy inside a key epoch. "Arrived late, sent at" shows a time the author claims. With 8-byte references an author can collide two of its own messages (about 2^32 work); this gives no cross-author edit or delete.

### Edges

**SR-22 (High, verified). iOS backups carry the history and the group sender keys.**
- Where: `apps/ios/Lichen/Model/AppModel.swift` 321–330; ADR-0010. Nothing sets `isExcludedFromBackup`.
- iCloud and Finder backups copy the database (messages, sender keys, channel secrets), `offers/`, `downloads/` and the drafts in UserDefaults. Android excludes everything.
- Fix: exclude lichen's folder from backups; move drafts into it.

**SR-23 (High, verified). A pasted contact code is marked verified.**
- Where: `lc-ffi/src/lib.rs` `add_contact_card` (2675–2692: `set_identity(…, true)` for any card); paste fields in `AddContactView.swift` 329 and `ContactScreen.kt` 240. Spec §4.7 says "scanning in person".
- A code received or altered by mail or SMS gets the verified badge, or silently replaces a verified contact's identity.
- Fix: a pasted code is added unverified; a changed identity goes through the conflict flow.

**SR-24 (Medium, verified). Android: a crafted file name crashes the app after download.**
- Where: `apps/android/…/model/AppModel.kt` 1535–1540, 1610.
- A file announced as `a.b/c` keeps `/` in its extension: the write throws, only `BridgeException` is caught, and each tap on Download crashes again.
- Fix: alphanumeric extensions only; catch IO errors.

**SR-25 (Medium, verified). Android: any installed app can crash lichen.**
- Where: `MainActivity.kt` 79–81; `model/Ids.kt` (`fromRoute`, `unhex`).
- The exported activity takes the `chat` and `sos` extras unvalidated; a non-hex value throws when the chat opens, which also stops reception.
- Fix: validate the kind and the hex length before navigating.

**SR-26 (Medium, verified). GIFs bypass Tor while Tor is on but not ready.**
- Where: `AppModel.swift` 972 (`viaTor: torStatus == .ready`), `AppModel.kt` 1153. §6.3 says through Tor when Tor is on.
- "Ready" needs the onion service published, up to eight minutes per start, or never if publishing fails. Until then a GIF loads directly and GIPHY sees the address.
- Fix: through Tor whenever the setting is on; wait for the client; never fall back.

**SR-27 (Medium, reported). Received images are decoded unbounded on the main thread.**
- Where: `ChatView.swift` 944, 952; `ChatMedia.kt` 205, 270; `content.rs` 788 (the 160 × 120 limit is never checked).
- A 450-byte AVIF declaring huge dimensions, or a bridge image up to 64 MiB, can get the app killed and the chat unopenable. Needs a test on each platform's decoder.
- Fix: read the header first, cap dimensions, decode downsampled off the main thread (imports too).

**SR-28 (Medium, verified in part). Deleting a message does not delete its media.**
- Where: `crates/lc-bridge/src/lib.rs` 112–164; `store.rs` 402 (no `secure_delete`).
- After "delete for everyone" the original stays in `offers/` and `downloads/` and is served for seven days to anyone with the hash; deleted rows stay in free pages. §6.4 says media are dropped.
- Fix: revoke the offer, sweep at start, delete downloads with their message, `secure_delete` on.

**SR-29 (Medium, verified in part). iroh is on by default and serves any caller.**
- Where: `AppModel.swift` 37, `AppModel.kt` 295, `lc-bridge/src/lib.rs` 257–266.
- The relays see the phone's address and a stable endpoint id at every launch, before any media is sent; a former contact can probe presence and probably the current address.
- Fix: start iroh at first use; accept only current contacts' endpoint ids.

**SR-30 (Medium, reported). Names are not pinned; MeshCore notifications look like lichen's.**
- Where: `core.rs` 1432–1450; `AppModel.swift` 675–683.
- A verified contact's name follows its node's advert: Julien renames his node "Camille" and keeps his badge. A plain message from Camille's node notifies as "Camille", like her encrypted chat.
- Fix: pin the name at verification, show renames, label plain notifications "MeshCore".

**SR-31 (Low, reported). The Android TCP radio link is clear and unauthenticated** (`TcpCompanion.kt` 28–45). Someone on the Wi-Fi can become the radio: read and forge plain chats, contacts, channel secrets; lichen content stays sealed. Fix: say so on the pairing screen.

**SR-32 (Low, reported). A hostile radio is trusted too far** (`core.rs` 1399–1527; `crates/lc-core/src/plain.rs` 591, 597). Replies are not matched to the pending command; contacts and held plain messages are unbounded; a slot counter can overflow; the channel scan can loop; neither app checks that the BLE link is bonded. Fix: match replies, bound the lists, cap the scan.

**SR-33 (Low, reported). Bridge robustness** (`lc-bridge/src/lib.rs` 239, 258). A fetch allocates up to 64 MiB from the peer's header and copies it across the FFI; the server has no timeout or concurrency cap. Fix: stream to a file with incremental hashing; caps.

**SR-34 (Low, reported). Secret hygiene.** No zeroize; `Config` (`core.rs` 349) and `LichenBridge` (`lc-ffi/src/bridge.rs` 92) derive `Debug` over keys; the CLI writes its seed before `chmod` (`crates/lc-cli/src/device.rs` 133).

**SR-35 (Low, reported). File names keep bidirectional control characters** (`lc-ffi/src/lib.rs` 4283), which allows extension spoofing; Android takes the MIME type from the raw name.

**SR-36 (Low, reported). GIPHY and maps.** The fixed User-Agent "lichen" marks the app, over Tor too; auto-load may give a sender a read receipt through a unique GIF's view counter (to test); map tiles ignore the Tor setting.

**SR-37 (Info). Fuzzing gaps.** No target drives a hostile companion through `Core`, the blob protocol or store snapshots; `group_open` has no corpus; a `session_peer` crash artifact from 6 November had not been replayed.

**SR-38 (Info). Dependencies.** No pre-releases. Arti is pinned at `=0.48.0`; `rsa 0.9.10` has RUSTSEC-2023-0071; no dependency had ever been checked against the advisory database; no Gradle dependency verification.

**SR-39 (Info). Spec and code disagree (edges).** ADR-0010 names the database `node-….sqlite`, the code `id-…`; forwarding a text into a MeshCore chat contradicts §15's "never carries lichen content".

## Checked and found sound

- **AEAD and nonces:** the RFC 8439 assembly, tested against the `chacha20poly1305` crate; tags capped at 0xFF00; every resend bumps the round; one reassembly per tag; directions and kinds separated; the PR-01 fix holds.
- **Handshake:** mutual authentication and forward secrecy for data; all-zero Diffie-Hellman results and non-decompressing keys rejected; reflection and label separation hold; the header is fully in the AAD; memory per peer is fixed.
- **Groups:** sender keys from the OS-seeded generator, fresh at restart and rotation, never stored; nonce `counter ‖ epoch ‖ frag` unique; the Ed25519 signature over `gid ‖ key_epoch ‖ counter ‖ content` checked before the window moves, before storage and delivery, for recovered envelopes too; only the admin's session changes membership; epochs strictly increase; another member cannot edit or delete by a reference collision; history is ordered by arrival.
- **Bridge and media:** blobs checked against BLAKE3, sizes capped, providers authenticated; the GIF host and path fixed, rustls with webpki roots, no redirects, 8 MiB cap; DEFLATE bounded at 64 KiB; file opening contained to its own folder; the C voice decoder gets fixed 4-byte frames and all modes are fuzzed.
- **Plain chats:** separate kinds in the store and the UI, labelled in the list, the header and the info sheet; the node's private key is never exported.
- **Secrets on the phone:** the seed in the Keychain (after first unlock, this device only) and behind Android Keystore AES-GCM; the phrase never reaches the clipboard or logs; debug arguments are compiled out of release builds; no URL schemes, no WebViews, no other exported components.
- **Randomness:** the apps seed from `SecRandomCopyBytes` / `SecureRandom`, the CLI from `getrandom`; seeded generators stay in the fake network, the demo bots and the solver.
- **Crashes:** no `unwrap`, `expect` or `panic` on network data in the code reviewed.

## Not determined

- Real replay rates on air (they depend on the companion's duplicate filter): a hardware test.
- What iroh's default relays publish, and whether a stranger who connects learns the address: a two-endpoint test.
- How ImageIO and ImageDecoder behave on image bombs: test files.
- Whether QuickLook runs scripts in HTML or SVG files; whether GIPHY counts media-host loads as views; whether stock MeshCore v1.18 always requires BLE pairing.
- The SR-11 token-holder path, the SR-13 queueing effect, the SR-17 timing: fake-network tests.

## Response

Each finding gets a test that fails before its fix and passes after, then the fix, then the spec and ADR text. This table is filled as that happens.

| ID | Verdict | Fix | Test |
|---|---|---|---|
| SR-37, stale `session_peer` crash artifact | **Not reproduced.** Replayed on 2026-11-15: the crash input and both slow units run clean on the current tree. | New targets still to add. | `cargo +nightly fuzz run session_peer <artifact>` |
| SR-38, dependencies | **Confirmed, gate added.** `cargo deny check` (advisories, licences, sources) runs in `scripts/check.sh` and CI. Two advisories reach the build and are ignored with their reasons in `deny.toml`: RUSTSEC-2023-0071 (`rsa`, with Arti; lichen holds no RSA private key) and RUSTSEC-2026-0459 (bundled SQLite and crafted `-wal` files; lichen opens only databases it created; the fixed `rusqlite` is not accepted by Arti 0.48). A third, in `crypto-bigint`, is only in the lock file, not in the build. | Upgrade both with the next Arti release. | `cargo deny check` |
| SR-09, the truncated-tag bound | **Confirmed, and worse than reported.** A follow-up on 2026-11-15 found a forgery at `2^-52` per attempt against the 8-byte tag (a zero byte appended to a frame the receiver has not accepted; verified on 200,000 keys), and that the radio-rate limit is not enforced off the radio path. | Full 16-byte tags, RFC 8439 through the `chacha20poly1305` crate, a packed ACK (ADR-0028, supersedes ADR-0014). | `lc-proto` `tests/aead.rs` (RFC 8439 §2.8.2, `an_appended_byte_is_rejected`, an independent assembly); `lc-core` `tests/vectors.rs` `frames::*` against `testdata/lichen/frames.py`, an independent Python oracle |
| SR-01, unverified identities not pinned | **Confirmed.** An introduction under Bob's node replaced the identity Alice knew, unverified, without asking her. | Every known identity is pinned; another waits for the user, the known one keeps receiving by HELLO, and fails say "identity changed" (§4.6). Both apps' banner says so. | `lc-core` `tests/identity.rs`: `an_unverified_identity_is_pinned`, `a_waiting_identity_does_not_block_the_known_one` (both failed before the fix) |
| SR-23, a pasted card marked verified | **Confirmed.** `add_contact_card` verified every card, and replaced a verified identity without asking. | The apps say whether the camera read the card. Pasted: added unverified, and a different identity waits for the user (§4.6). Scanned: verified, and replaces an identity the contact no longer has (§4.7). | `lc-ffi` `unit::cards` (both failed before the fix) |
| SR-22, iOS backups carry the history and group keys | **Confirmed in code.** Nothing set `isExcludedFromBackup`; drafts were in the preferences. | lichen's folder (databases, offers, downloads, Tor state) is excluded from backups at every start; drafts moved from the preferences into it, once. | Checked in code; a restore test needs a device backup (field checklist) |
| SR-24, Android crash on a crafted file name | **Confirmed in code.** The extension kept any character; a write error escaped the download job. | Only a plain extension of 1 to 10 ASCII letters or digits is kept (else `bin`); a write error ends the download with an error instead of a crash. | Checked in code |
| SR-25, Android crash from another app's extras | **Confirmed in code.** `fromRoute` and the SOS extra took any string; `unhex` threw on non-hex. | A route or extra must be the right kind and length of hexadecimal, or it is ignored. | Android `IdsTest` |
| SR-11, group authorship by the admin-supplied node | **Confirmed.** A state could list Eve with Camille's node; Eve's signed posts were shown as Camille's and her edits passed Camille's author check. | A state naming a node twice is dropped; a member's identity is learned for its node by §4.6's rule (another identity waits for the user); a member's messages are shown only while its node is known under its identity. Keying rows by identity in the store stays possible later; with this rule, node and identity can no longer disagree for anything shown. | `lc-core` `group::tests::a_node_belongs_to_one_member`, `a_member_on_someone_elses_node_is_not_shown` |
| SR-12, removal and rotation given up after five tries | **Confirmed.** A member away for two hours during a removal never received the new state. | GROUP_STATE, SENDER_KEY and SENDER_KEYS go on every hour for a day after the five quick tries. A member holding an envelope a minute without its key now asks the admin (GROUP_SYNC, §10.7), who answers it with the state and every key, from their first counter, at once: a member that lost a rotation reads again in about a minute instead of an hour. The group screen of both apps names the members the latest state or keys have not reached (a datagram for them failed and still waits); answering their GROUP_SYNC drops what it supersedes, so the list clears once they have it. | `lc-core` `tests/groups.rs` `a_removal_reaches_a_member_who_was_away` (failed before); `tests/invite.rs` `a_member_who_missed_a_rotation_asks_and_reads` (failed without the request); `group::tests::a_member_without_a_key_asks_the_admin`, `the_admin_answers_a_sync_once` |
| SR-02, paths from unauthenticated frames, never rediscovered | **Confirmed.** One forged anonymous probe, or one INTRO, under Bob's hash byte with a bogus path cut Alice off from Bob. | A path is adopted from a keyed probe, a PATH_REPLY to our probe, INTRO_FIN, or a HELLO; an anonymous probe or an INTRO is only answered along its path. A HELLO or INTRO that nothing answered drops the path. Binding INTRO's return path into its keys comes with the handshake change (SR-06). | `lc-core` `tests/paths.rs` (both failed before) |
| SR-13, joining needs no consent; SOS trusts every group | **Confirmed.** Any contact with a known identity could put us in a group, import its members as contacts and get our SOS position. | A group that lists us is joined at once only when we asked (scanned invitation) or its admin is verified in person; otherwise it waits for the user (Join / Decline in Messages, a notification), and nothing of it is used meanwhile. At most 32 groups. SOS, positions and search therefore only reach groups the user chose. A floor on POWER is not added: it is only obeyed from such groups now. | `lc-core` `tests/groups.rs`: `a_group_waits_for_the_user`, `a_declined_group_leaves_nothing`, `a_verified_admin_adds_us` |
| SR-26, GIFs around Tor while it starts | **Confirmed in code.** Both apps used Tor only once the onion service was published, and went direct before. | With Tor on, a GIF goes through Tor or not at all; while Tor starts it fails and loads once Tor is up. | Checked in code (the bridge refuses a Tor fetch without a Tor client) |
| SR-15, the edit window is the author's word | **Confirmed.** Only `ts − sent_at`, both from the author, was checked. | A received edit or delete is also held to this phone's clock: not applied more than 25 hours after its target arrived here. A received reaction, edit or delete whose target never comes is forgotten after two days (purged at start); ours wait for their delivery. An edit's text is bounded like a text's, by the datagram: a smaller cap would forbid editing long texts. | `lc-ffi` `unit::edit_window`; `lc-core` `store::tests::orphans_are_purged` |
| SR-14, GROUP_MISSING as an amplifier | **Confirmed.** Each request returned up to 16 payloads, as often as asked. | A kept message goes once to each member who asks (512 remembered per group). The admin ignores a GROUP_LEAVE naming a state it never issued, or one from before it last added the member (a leave resent after the member came back). | `lc-core` `group::tests::a_kept_message_goes_once_to_each_member` (failed without the rule), `a_stale_leave_does_not_remove_a_returning_member` (failed before) |
| SR-04, answers to unauthenticated frames bypass the budget | **Confirmed.** 100 forged INTROs made the phone spend 81.6 s of its 360 s hour answering. | INTRO_ACK, PATH_REPLY, HELLO_ACK and the INTRO a failed HELLO prompts: at most 8 per node in ten minutes, 60 in all in an hour. | `lc-core` `tests/paths.rs` `forged_intros_get_few_answers` (failed before) |
| SR-30, names not pinned; MeshCore notifications look like lichen's | **Confirmed in code.** A verified contact's name followed its node's advert; a plain DM notified under the bare name. | A contact keeps the name it had when verified (kept as its nickname, which the user can change); a MeshCore direct message notifies as "Name · MeshCore". Since then, when a contact's radio advertises another name, each conversation we have with it (lichen or MeshCore) gets a line, "Camille's radio is now called “Julien”", compared with the names last saved, so a rename made while the app was closed shows too; the name shown does not change for a verified contact. | `lc-ffi` `unit::names` |
| SR-28, deletion that does not delete | **Confirmed in part.** SQLite left deleted rows in free pages. | `secure_delete` is on: deleted rows are overwritten. Revoking a deleted message's offer and removing its download stay open. | `lc-core` `store::tests::deleted_text_is_overwritten` (failed without it) |
| SR-27, images decoded unbounded | **Confirmed in code.** Both apps decoded received and picked images whole, at whatever size their bytes declared. | Every image is read header first: more than 16,384 pixels a side or 64 million in all is refused; the rest is decoded downsampled (480 px for LoRa images, 2,048 for downloads and picked photos, 4,096 in the viewer). Since then, decoding runs off the main thread on both apps, once per image (a small cache on iOS, `produceState` on Android), and GIFs are bounded too: a first frame over the same limits is refused, frames are scaled to 480 pixels and at most 120 are kept on iOS. | Checked in code; a test needs image-bomb files on each platform |
| SR-16, search counts unauthenticated copies | **Confirmed in code.** A direct copy whose clear header named an attributed message counted as a reception, whatever its ciphertext. | A copy counts only if it verifies under the member's sender key for its epoch; copies kept while waiting for attribution are checked too. | `lc-core` `group::tests::a_copy_counts_only_if_it_verifies` |
| SR-17, two-fragment reassembly poisoned by an insider | **Confirmed.** A fragment of Carol's under Bob's key and counter, flooded first, made Bob's real one "another copy" and his message was lost. | Fragments are kept side by side (6 per message) and the pair whose signature verifies wins. | `lc-core` `group::tests::an_injected_fragment_does_not_shut_out_the_real_one` (failed before) |
| SR-29, iroh on by default, reachable all the time | **Confirmed in code.** Both apps started iroh at launch: its relays saw the address and the lasting endpoint id at every start, and anyone who once got an announcement could see whether the phone was online. | iroh now runs only while something we offer is still served (7 days), a send sheet is open, or a download goes through it; expired offers are removed when checked. Accepting only current contacts was looked at again and is not done: it would not hide presence, because an iroh connection (and, unless relay only, the hole punching that exchanges addresses) is made before any check a provider can run, so a former contact who knows our endpoint id still learns that iroh runs. What it would protect is the content, and the hash already is its capability (§14.2): it only travels inside announcements sealed for their recipients, who may fetch it. What is left: a former contact who knows our endpoint id can tell when iroh runs, that is for 7 days after we offered something; relay only hides the address from them. Per-offer or rotating endpoint ids would close that, at the cost of several endpoints at once; not done. | `lc-bridge` `offers::live_offers_and_the_sweep`; app behaviour checked in code |
| SR-06, HELLO authenticated by the static secret alone | **Confirmed in code.** | Version 2 handshake: ephemerals in clear, `es`, `ee`, `se` and a transcript hash in HELLO_ACK's key and the session keys; the INTRO's return path bound into the introduction's keys (ADR-0029). | `lc-proto` `tests/handshake.rs` `a_stolen_seed_does_not_answer_as_the_contact`, `the_transcript_is_bound`; `intro::tests::the_return_path_is_bound` |
| SR-03, HELLO without freshness | **Confirmed in code.** | A HELLO carries its sender's time and is not answered more than 10 minutes away from ours (ADR-0029); answers were already limited (SR-04). A HELLO's path and the session's age now change only once the initiator confirms the session; a replayed HELLO no longer moves the path or resets the renewal clock. | Checked in code; the fake-network suites run with the check |
| SR-05, a 30-digit safety number for two identities | **Confirmed (arithmetic).** | 60 digits, 30 per identity (ADR-0029). | `lc-core` `identity::tests::fingerprint_and_safety_number` |
| SR-28, a deleted message's offer and download (rest) | **Confirmed in code.** | Deleting an announcement (for me, for everyone, by its author, or with its conversation) tells the app, which stops offering the content and removes its download. | `lc-ffi` `unit::media_gone` |
| SR-35, file names that reorder themselves | **Confirmed in code.** | Bidirectional control characters are dropped from file names as shown and opened, and from names we send; the name as sent is kept for references. | `lc-ffi` `unit::bidi_names` |
| SR-34, secrets in Debug output; the CLI seed file | **Confirmed in code.** | `Config` and `LichenBridge` print no seed or key; the CLI creates its seed file readable by its owner only before writing it. Since then, ephemeral handshake keys and group sender keys are a type that never prints and is wiped when dropped, and session keys and introduction answers are wiped when dropped (SR-07). After each secret, the random generator is re-keyed from its own output, so a memory image no longer yields the ephemerals drawn before it (the demo's script, which relied on its bots' old timing, now starts Julien's posts after Camille's photo). | `lc-core` `secret::tests::prints_nothing`, `core::tests::the_generator_is_rekeyed_after_a_secret` |
| SR-19, invitations | **Confirmed in code.** | A scanned invitation goes through the same identity steps as a scanned card (the app is told when it replaces an identity); one invitation code admits at most 12 people. | The invitation tests run through it |
| SR-31, the Android TCP radio link | **Confirmed in code.** | The pairing screen says the network link to the radio is not encrypted and what that exposes (EN, FR). | — |
| SR-32, a hostile radio trusted too far (in part) | **Confirmed.** An unasked channel info set a channel and its secret. | Contacts taken from the companion are capped (4,096), and the channel scan's counter saturates; held plain messages were already bounded and the scan ends at 256 slots. Since then, a reply counts only as the answer to the command awaited, by its type (the protocol has no request id): one nobody asked for (a channel's secret, the radio's identity, a contact) is dropped and does not end the wait; an error answers any command, a type the decoder does not model still ends it. Checking that the Bluetooth link is bonded stays open. | `lc-core` `core::tests::a_reply_nobody_asked_for_is_dropped` (failed before); `core_companion` fuzz target |
| SR-08, SR-10, SR-20, SR-39, documents and code apart | **Confirmed.** | The spec now says what the code does: no old receive key kept (§4.4), answered HELLOs remembered until the next, the replay window as used (§8), forwarding into a MeshCore chat leaves lichen's encryption (§15); ADR-0013 says what an INTRO answer and a probe give away; ADR-0010 names the database by identity. SR-20's other points are fixed in code, in the next row. | — |
| SR-20, group mismatches | **Confirmed.** A phone restored from its phrase and admitted again got only the admin's key, and its new key, at an epoch the others held, was ignored: it could neither read nor be read. Twenty garbage envelopes under one member's byte flushed another member's envelope waiting for its key. A text with an invalid UTF-8 byte got a reference other than its author's. | A SENDER_KEY from its owner is adopted at the epoch held when the key differs. A member asking to join again gets every key, its own last one included, moves its epoch past it, and the admin relays its next keys to everyone for 10 minutes. Envelopes wait for their key only when it can still come, 8 per sender byte, 64 in all. Received messages carry their plaintext as received, and references are computed on it. | `lc-core` `tests/invite.rs` `a_restored_phone_joins_again_and_is_heard` (failed before), `group::tests::an_owners_key_at_the_same_epoch_is_adopted`, `a_restored_member_gets_every_key_and_is_heard_again`, `our_own_entry_moves_our_epoch_past_it`, `an_outsider_cannot_flush_held_envelopes` (all failed before); `lc-ffi` `unit::reactions::a_reference_uses_the_bytes_as_sent` (failed before) |
| SR-18, relayed keys and replay | **Confirmed.** One SENDER_KEYS walked a member's key epoch around the ring and reinstalled its real key with an empty replay window: an old envelope was delivered again. | A SENDER_KEYS counts its first entry per owner only; a key held before (current, previous, or one of the last 8, in memory) comes back with its replay window under any epoch. The second counter range's missing floor for newcomers is left to M9's key step on join: newcomers then hold keys that old recordings were not made under, which closes it without a field in SENDER_KEY; a position also carries its own time (§12.1). | `lc-core` `group::tests::a_relay_cannot_reinstall_a_key_with_a_fresh_window` (failed before) |
| SR-33, bridge robustness | **Confirmed in code.** A fetch set aside the whole announced size (up to 64 MiB) before any byte came; serving over iroh had no time limit, and neither transport capped the requests served at once. | A fetch reads in 64 KiB chunks, hashed on the way, and keeps only what arrives; a provider serves at most 8 requests at once (one more is closed), a request must arrive within 30 s and an answer go within 10 minutes, on both transports. Since then, a download goes straight to a file: written to `<name>.part` as it arrives, renamed only once its hash matches, removed on any failure or when the download is cancelled; the content no longer crosses the bindings or sits in memory, on both apps. | `lc-bridge` `offers::serving_is_limited` (new limit), `offers::a_provider_cannot_make_us_reserve_what_it_does_not_send` (behaviour; the memory is not measured), `tests/local.rs` `a_download_goes_to_a_file`, `substitution_leaves_no_file`, `a_cancelled_download_leaves_nothing` |
| SR-36, GIPHY and maps | **Confirmed in code.** | GIF requests no longer carry a header naming the app (it said "lichen", over Tor too); GIPHY still serves them (live test, direct and over Tor). Since then, with GIFs loading by themselves (asked for, or Tor on), a received GIF loads as it arrives, not when it is read: its view counter tells the sender no more than the delivery lichen acknowledges anyway; with GIFs on a tap, loading one is the reader's choice. With Tor on, the map waits instead of loading tiles from OpenFreeMap outside Tor, until allowed for that run of the app (routing tiles through the in-process Tor client would need a proxy for MapLibre). | `lc-bridge` `gif_by_id` (ignored, Internet); both apps, by hand on the demo network |
| SR-37, fuzzing gaps (in part) | **Confirmed.** | A `core_companion` target drives the whole core with any sequence of companion frames, time going on, seeded with every companion vector: 359,000 runs in three minutes, clean, contacts bounded. Since then, `snapshot_restore` restores arbitrary bytes as a stored group and invitation (a restored group's own snapshot must restore again), seeded with real snapshots from the fake network (`tests/fuzz_seeds.rs`, run by `scripts/fuzz.sh`): 350,000 runs in two minutes, clean. The blob protocol is two fixed-size reads (a 33-byte request, a 9-byte head); its limits are tested instead (SR-33). | `fuzz/fuzz_targets/core_companion.rs`, `snapshot_restore.rs` |

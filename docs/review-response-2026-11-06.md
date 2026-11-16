# Response to the protocol review of 2026-11-06

The review (Codex, at Tanguy's request) reported five findings on the tree as it was during the phone-identity migration. Each was checked against the current source; every fix has a regression test that fails on the old code.

| ID | Verdict | Evidence | Fix | Regression test |
|---|---|---|---|---|
| PR-01, PATH_REPLY key and nonce reuse | **Confirmed** | `reply_key` and the header nonce depended only on the probe id, so a probe answered twice (another copy, or a replay after the 64-entry cache or a restart) reused both with a different path. | Spec §4.5: 8 random bytes `r` in every reply, bound into the key (`HKDF(id ‖ r, …)`) and the AAD; the core draws them per reply. | `lc-proto/tests/discovery.rs`, `replies_to_the_same_probe_never_share_a_keystream` |
| PR-02, group replay window before the signature | **Confirmed** | `on_group_envelope` marked the counter, then checked the signature: a member holding another's sender key could burn that sender's counters. | The signature is checked first; the replay window moves only for a signed message. | `lc-core` `group::tests::forgery_does_not_move_the_replay_window` |
| PR-03, verified identity replaced without consent | **Confirmed** (design) | `learn_identity` replaced any identity and the introduction installed the session at once. | Spec §4.6: a verified contact's new identity is not adopted; it waits (`IdentityConflict`), no session is made with it, and messages fail with `IdentityChanged` until the user accepts or rejects it. Unverified contacts keep the replace-and-report rule. | `lc-core/tests/identity.rs`, `verified_identity_is_not_replaced_without_consent` |
| PR-04, a replay for one candidate ends the search | **Confirmed** | `return` on a candidate's replay rejection, before trying the other members sharing the sender byte. | `continue` to the next candidate. | `lc-core` `group::tests::colliding_senders_are_both_heard` (real colliding key pairs) |
| PR-05, 8-byte tags | **Partially confirmed** | The construction is a variant of RFC 8439; the spec now says so. | Kept, with its bounds written down: ADR-0014 (forgery about `2^-64` per attempt, attempts rate-limited by LoRa, short-lived keys, full-strength authentication for future Internet transports). | — |

The review's observation that the build was broken mid-migration is historical: `scripts/check.sh` (fmt, clippy, all tests, no_std) passes on the commit that carries these fixes, and the oxymesh cross-check (`sim/`) passes too.

## Second recheck (Codex, at `eba76eb`)

The recheck confirmed PR-01, PR-02, PR-04 and PR-06 fixed, and PR-03's automatic adoption gone. It reported a new finding and two items still open. Each was checked against the source.

| ID | Verdict | Evidence | Fix | Regression test |
|---|---|---|---|---|
| PR-07, approval of an identity other than the one compared | **Confirmed** | `accept_identity(node)` adopted whatever was pending at the time of the tap, and `set_verified(node, true)` verified whatever identity the contact had. A second introduction while the verify sheet was open was approved unseen. | Approval names the compared identity. `accept_identity(node, expected, verify)` adopts only if the pending identity is still `expected`, and verifies in the same call. `set_verified(node, identity, …)` verifies only that identity. `reject_identity(node, expected)` leaves a newer pending identity alone. The app keeps the identity on screen, and on a refusal shows "Their key changed again" with the new numbers. | `lc-core/tests/identity.rs`, `stale_approval_is_refused` (Codex's sequence: nothing adopted or sent on the stale approval, then the right one works) |
| Session renewal after 24 h / 4 MB (ADR-0014) | **Confirmed** | §4.4 required it, but only the `tag` limit was enforced. | Enforced: the core renews a session before a new datagram once it is 24 h old or has carried 4 MB (`SESSION_MAX_AGE_MS`, `SESSION_MAX_BYTES`, `set_session_limits`). | `lc-core/tests/fakenet.rs`, `sessions_renew_after_their_limits` |
| ADR-0014's attacker budget and key lifetimes | **Confirmed** | It assumed the attacker keeps the duty cycle, and it said group keys change at every membership change. | Rewritten. Attempts are bounded by one receiver's demodulation: the 17-byte shortest frame takes 283 ms, at most about 12,700 frames an hour, `≈ 2^34.7` tries a year even with 256 candidate keys per frame. Lifetimes are those enforced: sessions 0xFF00 / 24 h / 4 MB / restart; group keys rotate when someone leaves and at restart, and an added member receives the current keys (stated in §10); handshake keys use unique nonces and bounded use. | — |
| Discovery: "never reuses a key" | **Confirmed** (wording) | `r` is 64 random bits, so freshness is probabilistic. | §4.5 states the bound: about `q(q−1)/2^65` for `q` replies to one `id`, below `2^-33` for `q ≤ 2^16`. | — |

Both tests assert what the old code did not do. The old `accept_identity(node)` would adopt the replacement identity; the old session code never renewed before `0xFF00`.

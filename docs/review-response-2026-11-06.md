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

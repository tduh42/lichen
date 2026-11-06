# Protocol review — 2026-11-06

## Second recheck — fixes committed, 76 tests pass

Rechecked on 2026-11-06 at HEAD `7709c7c`, including protocol-fix commit `bb808f8`. This is the latest status and supersedes both earlier assessments below. The implementation tree was clean when this recheck began; this report was untracked.

**Assessment: the original concrete defects are addressed, but an additional identity-approval race was reproduced in this recheck (PR-07, high priority). The truncated-tag security rationale also remains only partially addressed.**

| ID | Current status | Verification |
|---|---|---|
| PR-01 | **Original deterministic reuse fixed** | Each reply now carries fresh responder randomness `r`, used in HKDF and AAD; the core draws it for each reply. `replies_to_the_same_probe_never_share_a_keystream` passes. |
| PR-02 | **Fixed** | Signature verification precedes replay-state updates. The forgery regression passes. |
| PR-03 | **Original automatic-adoption bug fixed; see PR-07** | `learn_identity` refuses adoption, both INTRO acceptance paths stop before installing the replacement session, and explicit acceptance is required. The existing regression passes, but approval is not bound to the identity compared by the user. |
| PR-04 | **Fixed and regression-tested** | Replay rejection skips the candidate. `colliding_senders_are_both_heard` now completes and passes with actual colliding identity keys. |
| PR-05 | **Still partially addressed** | The construction is accurately named as a variant. ADR-0014's attacker-rate and key-lifetime claims remain unchanged from the first recheck. |
| PR-06 | **Fixed** | The helper now searches for any collision across incrementing seeds instead of cycling through 256 seeds looking for a possibly absent fixed prefix. The formerly hanging test passes. |
| PR-07 | **New, high priority: reproduced** | A second introduction changes the pending identity while the user compares the first; the UI's node-only accept/verify calls approve the replacement instead. |

### PR-07 — Approval can verify a different identity from the one displayed

**Locations:** `Core::learn_identity` and `Core::accept_identity` in `crates/lc-core/src/core.rs`; FFI `accept_identity`/`set_verified`; `ChatView`'s `verifying` state; `VerifyPanel` in `apps/ios/Lichen/Views/AddContactView.swift`; `AppModel.acceptIdentity`.

The verification sheet captures a pending identity in `ChatView.verifying` and displays the safety number for that identity. A later conflict overwrites the core's `pending_identities[node]`. The UI then calls `accept_identity(node)` without supplying the identity it displayed, and follows it with `set_verified(node, true)`. `accept_identity` removes and adopts whichever identity is currently in the pending map.

**Reproduced sequence:**

1. Start with a verified contact A. A new identity B is presented and held pending.
2. Capture B as the identity displayed in the verification sheet.
3. Before approval, complete another introduction at the same node with identity C. `pending_identity(node)` now returns C, while the sheet's captured value remains B.
4. Execute the same core calls as the UI: `accept_identity(node)`, then `set_verified(node, true)`.
5. The contact is now C and verified, although the user compared B. A subsequent message is delivered to C.

I reproduced steps 1–5 in a temporary integration test using the fake network, adapting `verified_identity_is_not_replaced_without_consent`. After its existing blocked-message checks and before acceptance, I inserted:

```rust
let displayed_identity = new;
net.reinstall_app(b, 44);
net.connect(b);
net.run_until(net.now + 20 * S, |_| false);
let replacement = identity(&net, b);
assert_ne!(replacement, displayed_identity);
let alice = net.key(a);
net.with_core(b, |c, now| c.send_text(&alice, "second introduction", 3, now))
    .unwrap().unwrap();
net.run_until(net.now + 10 * MIN, |_| false);
assert_eq!(
    net.nodes[a].core.as_ref().unwrap().pending_identity(&bob),
    Some(&replacement),
);
assert!(net.with_core(a, |c, _| c.accept_identity(&bob)).unwrap());
net.with_core(a, |c, _| c.set_verified(&bob, true));
assert_eq!(known(&net, a, b), (Some(replacement), true));
```

The adapted test passed, including delivery of the subsequent `"hello again"` message to the replacement app. The test models competing identities through app reinstalls; it does not simulate the Swift UI or radio interception. The stale displayed identity and node-only approval behavior were independently traced in the Swift source.

**Fix direction:** approval must carry the exact expected identity and atomically compare it with the pending identity before adoption. If it changed, fail approval and require comparison again. Marking an identity verified must likewise be conditional on the identity actually compared; preferably accept-and-verify in one operation. Do not continue to `set_verified` after a failed or stale approval. Add an integration regression with two pending identities and a stale approval, asserting that neither the newer identity is accepted nor any message is released to it.

### Remaining work

Resolve PR-07 before treating identity-change consent as secure.

1. **Correct or implement the guarantees used in ADR-0014.** The ADR still assumes attacker compliance with the EU duty-cycle allowance, claims 24-hour/4-MB session renewal without corresponding enforcement in the inspected core/session code, and says group sender keys change at every membership change even though additions distribute the existing key. These were described in the first recheck and remain unresolved. Physical radio limits can support an argument, but regulatory compliance is not an adversarial guarantee. The exact truncated-Poly1305 bound has not been independently validated by this review.
2. **Qualify the discovery uniqueness wording.** The original deterministic replay-triggered reuse is fixed. However, `r` is a random 64-bit value, so the spec's claim that replies “never” reuse a key is probabilistic, not absolute. For repeated responses to one fixed probe ID, the collision term depends on the number of replies under that ID (approximately `q(q-1) / 2^65` for independent uniform draws). Document the bound and allowed usage; a larger responder salt would provide more margin. This is a residual design qualification, not a reproduction of the old deterministic attack.

### Validation and limits

- Ran the **unfiltered** `cargo test -p lc-proto -p lc-core`: **76 passed, 3 ignored measurement tests, 0 failed, 0 filtered out**. The collision test no longer stalls.
- A separate temporary test reproduced PR-07: the displayed identity differed from the accepted/verified identity, and the replacement received a subsequent message.
- Inspected discovery serialization/derivation and the core's randomness generation, both identity-conflict branches, group verification/replay ordering, and the repaired test fixture.
- Inspected FFI acceptance persistence and the iOS conflict/safety-number action wiring. This was a source review, not a live UI test.
- The committed identity regression covers initiator-side conflict and acceptance. Rejection, restart, and responder-side conflict are not independently exercised by that particular test; those branches were inspected in source.
- This recheck did not rerun the full workspace, hardware checks, fuzzing, or the optional oxymesh simulation. The other author's claims in `docs/review-response-2026-11-06.md` about those checks are not substituted for results from this recheck.
- No implementation files were modified. Only this report was updated.

## Recheck — current status after Claude's changes

Rechecked on 2026-11-06 at HEAD `34e435b` plus the uncommitted group fixes, protocol wording change, and ADR-0014. This section supersedes the original status table below; the original findings remain as historical evidence. No implementation files were changed by this recheck.

**Assessment: improved, but not ready for a security sign-off. Two original high-priority issues remain.**

| ID | Recheck result | Evidence |
|---|---|---|
| PR-01 | **Still open** | The original repeated-keystream reproduction was rerun against the current library and passed. Reply key derivation and fixed nonce remain unchanged. |
| PR-02 | **Fixed for the reported attack** | Signature verification now precedes `ks.mark` and persistence. `group::tests::forgery_does_not_move_the_replay_window` passed. |
| PR-03 | **Still open; stronger reproduction obtained** | An adapted identity integration test explicitly verified the old identity, then confirmed that a replacement identity received the next message without any approval. |
| PR-04 | **Code corrected; regression test blocked** | Candidate replay rejection now uses `continue`. The new collision test never finishes because its seed search cycles through a set containing no match; see PR-06 below. |
| PR-05 | **Partially addressed in documentation** | The spec now acknowledges the custom variant and ADR-0014 records the decision. Its security justification still relies on assumptions that are either unenforced or contradicted by the current implementation. |

### PR-03 reproduction added during recheck

I compiled a temporary copy of `crates/lc-core/tests/identity.rs` against the current built libraries. In `changed_identity_is_reported`, immediately after `let old = identity(&net, b);`, I added:

```rust
net.with_core(a, |c, _| c.set_verified(&bob, true));
assert_eq!(known(&net, a, b), (Some(old), true));
```

The test passed, including its existing final assertions that Alice now stores the replacement identity as unverified and that the replacement app receives `"after"`. No user approval is supplied anywhere in that test. This establishes the absence of a core send barrier for a previously verified identity; the test models identity replacement, not an over-the-air interception/jamming attack.

The relevant code remains `learn_identity` (replaces the identity and clears verification) followed by `on_intro_ack` (installs the session, requeues the message, and calls `tick`).

### PR-05: ADR-0014 needs corrections before closing the concern

The naming correction is useful. It does not independently validate the truncated-tag construction. RFC 8439's full-tag requirement remains unchanged: [§4](https://www.rfc-editor.org/rfc/rfc8439.html#section-4).

Specific problems with the new rationale:

- **An attacker need not obey the regulatory duty cycle.** A security attempt budget must use physical receiver limits or enforced verification limits, not assume compliance with the EU transmit allowance. Multiple candidate keys and recipients also need to be accounted for when counting verification opportunities.
- **The advertised session expiry is not enforced in the reviewed core.** `Session` tracks the datagram tag counter, and the core renews on `SendError::Exhausted`; I found no session-age or cumulative-byte enforcement for the stated 24-hour/4-MB limits. Implement those limits or remove them as assumptions in the argument.
- **Group keys do not change at every membership change.** In `on_group_state`, additions share the existing sender key; removals rotate it. A stable group can retain a sender key for a long time. The ADR must describe actual key lifetimes.
- **Discovery key single-use is still false under replay**, as PR-01 demonstrates. Retries also mean that handshake/introduction keys should be described in terms of unique nonces and bounded use, rather than literally one encryption per key.

These observations do not establish that a practical forgery from tag truncation alone is feasible. They mean the documented acceptance argument is not yet supported by the implementation. The precise truncated-Poly1305 bound in ADR-0014 was not independently proven in this review.

### PR-06 — Medium: collision regression test loops forever

**Location:** `crates/lc-core/src/group.rs`, test helper `core_with_group`, used by `colliding_senders_are_both_heard`.

The helper starts with `[seed; 32]`, changes only `s[31]` with `wrapping_add(1)`, and loops until the public key starts with `0x42`. It can visit only 256 distinct seeds. Finite enumeration of all 256 variants gave:

| Initial repeated seed byte | Matching values of the last byte |
|---|---|
| 10 | 139, 252 |
| 20 | 148 |
| 30 | none |
| 40 | none |

The collision test requests seeds 30 and 40. It therefore loops forever on the first one. I observed the combined test run stall at this test and interrupted it after confirming the finite search has no solution.

Use fixed, known colliding keypairs, or a bounded search over a larger counter with a clear failure assertion. The already demonstrated seed-10/seed-20 pairs are distinct identities with the required common prefix. Keep the differing replay floors and candidate-order assertions.

### Recheck validation results

- The core now **compiles**; the original migration build errors are no longer current.
- Unfiltered `cargo test -p lc-proto -p lc-core` stalled in the collision test described above and was interrupted.
- `cargo test -p lc-proto -p lc-core -- --skip group::tests::colliding_senders_are_both_heard`: **73 passed, 3 ignored measurement tests, 1 filtered out**.
- Temporary repeated-keystream characterization: **passed**, confirming PR-01 remains.
- Temporary identity-change test with an explicitly verified old identity: **passed**, confirming the missing trust barrier in PR-03.
- Finite seed enumeration: completed, confirming PR-06. PR-04's production-code correction was inspected, but its intended regression test cannot be claimed as passing until the fixture is repaired.

Next steps: fix PR-01 and PR-03, repair the collision fixture and run the unfiltered suite, and revise ADR-0014 to distinguish enforced guarantees from intended future behavior.

## Purpose and review status

This report hands off a code and design review to Claude for independent verification. It is not a formal cryptographic audit. Validate each finding against the current tree, challenge the attack assumptions, and add regression tests before changing behavior.

**Assessment:** the protocol has good foundations, but the findings below prevent an unqualified security endorsement. Separate directional keys, authenticated headers, bounded fragment buffers, retry nonces, and loss/property tests are useful strengths.

The review covered `docs/spec/protocol.md`, relevant ADRs, `lc-proto`, and the discovery, handshake, session, and group integration in `lc-core`. It did not exhaustively audit firmware behavior, platform key storage, UI trust handling, media, or the future Internet bridge.

HEAD when this report was written: `2ac62934cf4e1e39279942022c15731aea243a75`. The review included uncommitted changes. Files were being edited concurrently, particularly the phone-identity migration, so HEAD alone does not identify the reviewed source. Function names below are more durable than line numbers.

## Findings at a glance

| ID | Priority | Finding | Evidence status |
|---|---|---|---|
| PR-01 | High | Replayed discovery probes can cause PATH_REPLY key/nonce reuse | Primitive-level reproduction executed; network trigger traced in code |
| PR-02 | High | Group replay state advances before signature verification | Confirmed code ordering; integration exploit test still needed |
| PR-03 | High | Automatic identity replacement can bypass a previously verified identity | Spec issue; replacement and immediate session activation also observed in evolving core |
| PR-04 | Medium | Replay rejection for one hash candidate discards other candidates' traffic | Confirmed code path; collision regression test still needed |
| PR-05 | Design concern | Eight-byte Poly1305 tags depart from RFC 8439 | Confirmed construction and standards mismatch; no practical forgery demonstrated |

## PR-01 — Discovery reply key/nonce reuse

**Locations:** `crates/lc-proto/src/discovery.rs`, `reply_key` and `seal_path_reply`; `crates/lc-core/src/core.rs`, `on_log_rx` and `seen_probes`; protocol §4.5.

`reply_key` derives the reply key from the static shared secret, probe ID, and endpoint identities. `seal_path_reply` fixes the control subtype, total, and attempt (`0`), and derives the header tag from the same probe ID. For unchanged endpoints, replaying an ID therefore recreates the same key and nonce.

The encrypted plaintext includes the path carried by the received flood. That path can change independently of the authenticated probe payload. The core remembers only 64 probe IDs in memory. After eviction or an app restart, it can answer the same authenticated probe again with a different path. The spec says `LOG_RX_DATA` is delivered before the firmware duplicate filter, so that filter is not a sufficient defense for the app's reply generation.

**Impact:** different plaintexts use the same ChaCha20 keystream and Poly1305 one-time key. The reproduced ciphertext XOR equals plaintext XOR. This violates the repository's nonce invariant and undermines discovery authentication. This finding does **not** demonstrate recovery of session keys or decryption of DATA traffic; discovery uses separate derived keys, and path data is already public routing metadata.

**Executed reproduction:** seal two replies with identical keying/endpoints/ID and different equal-length paths. The test below passed, observing ciphertext XOR `[00, 22, 66]`.

```rust
use lc_proto::{
    discovery::{self, Keying},
    header::Header,
};

#[test]
fn same_probe_id_reuses_reply_keystream() {
    let keying = Keying {
        s: &[7; 32],
        anonymous: false,
        prober: &[1; 32],
        target: &[2; 32],
    };
    let id = [3; 8];
    let mut a = [0; 184];
    let mut b = [0; 184];
    let na = discovery::seal_path_reply(
        &keying, 2, 1, &id, 2, &[0x11, 0x22], &mut a,
    ).unwrap();
    let nb = discovery::seal_path_reply(
        &keying, 2, 1, &id, 2, &[0x33, 0x44], &mut b,
    ).unwrap();
    assert_eq!(
        Header::decode(&a[..na]).unwrap().0.nonce(),
        Header::decode(&b[..nb]).unwrap().0.nonce(),
    );
    // Seven header bytes + eight clear probe-ID bytes precede ciphertext.
    let xor: Vec<u8> = a[15..18].iter()
        .zip(&b[15..18]).map(|(x, y)| x ^ y).collect();
    assert_eq!(xor, vec![0, 0x11 ^ 0x33, 0x22 ^ 0x44]);
}
```

This is a characterization of the current bug, not the desired post-fix assertion. It can be adapted into `crates/lc-proto/tests/discovery.rs` and run with `cargo test -p lc-proto --test discovery`.

**Suggested direction:** put fresh responder randomness in each newly generated reply and bind it into the reply key derivation and AAD. Alternatively, redesign the public path reply around an appropriate authentication construction. Do not rely on an expiring or volatile cache to enforce a lifetime cryptographic invariant.

**Claude verification:** reproduce through `LOG_RX_DATA`, first after cache eviction and then after restart; confirm the same recorded authenticated probe is accepted with a changed path. After repair, prove distinct reply encryption contexts even across these transitions and check the resulting packet-size limits.

## PR-02 — Group replay-window poisoning before signature verification

**Location:** `crates/lc-core/src/group.rs`, `Core::on_group_envelope`.

After AEAD verification and reassembly, the code calls `ks.mark(hdr.counter)` and marks the group dirty before checking message length and the Ed25519 signature.

Every group member receives the other members' symmetric sender keys. Possession of such a key is therefore not proof that a packet originated with the named sender: the signature provides that distinction. A malicious member can encrypt a packet under a victim's sender key, choose a counter near `u32::MAX`, and attach an invalid signature. The packet is rejected, but its counter has already advanced the victim's replay window and can be persisted.

**Impact:** subsequent legitimate messages from that sender are rejected as too old until its key changes. This is a targeted availability attack by a group insider, not an outsider forgery of the victim's signature.

**Suggested direction:** validate the complete signed message before advancing or persisting authenticated replay state. Also review partial-reassembly resource limits separately: fragment AEAD authenticates group-key possession, not individual authorship.

**Claude verification:** establish a three-member group; have one member create an AEAD-valid, signature-invalid packet naming another member with a high counter. Assert no replay-state change, then deliver a legitimate low-counter message successfully. Cover too-short signed plaintext as well as invalid signatures, and reload the group snapshot to check persistence behavior.

## PR-03 — Verified identity can be replaced without a mandatory send barrier

**Locations:** protocol §4.6, especially the identity-replacement and HELLO-fallback rules; `crates/lc-core/src/core.rs`, `learn_identity`, `on_intro_ack`, and `on_intro_fin`.

The specification allows reintroduction after failed HELLOs and says a newly learned identity replaces the old identity, unverified, with an app notification. It does not require approval before messages are released to that replacement identity.

In the implementation observed while writing this report, `learn_identity` replaces the stored identity and clears `verified`. `on_intro_ack` then installs the new session, requeues the in-flight message, and calls `tick`. An event alone does not establish a mandatory barrier before transmission.

**Attack assumptions:** an active attacker can prevent the existing authenticated handshake from completing and answer the resulting introduction while claiming the contact's public node hash. The attacker proves possession of its own new identity, which the introduction permits; it does not need the previously verified identity's private key.

**Impact:** queued confidential messages may be delivered to a substituted identity despite prior verification. The attack depends on the fallback path and absence of a mandatory trust gate; verify the full current application flow before treating an end-to-end exploit as demonstrated.

**Suggested direction:** retain the previously trusted identity, record a pending replacement, and block outbound application messages to the replacement until the user explicitly accepts it or an authenticated key-transition mechanism validates it. Distinguish first contact from changes to an existing identity, especially a verified one.

**Claude verification:** start with a verified contact and a queued message, force HELLO failure, complete INTRO with a different identity, and assert that no queued DATA is emitted under the replacement session before approval. Test both initiating and responding roles, persistence, and reconnects.

## PR-04 — Hash-collision candidates are rejected globally

**Location:** `crates/lc-core/src/group.rs`, `Core::on_group_envelope`, the `if !ks.accepts(h.counter) { return; }` branch inside candidate/key iteration.

Groups and senders are selected using truncated hashes. Multiple identities may share the same one-byte sender prefix. Before authenticating a packet against a candidate key, the receiver checks that candidate's replay window. A rejection returns from the whole handler instead of continuing to the next candidate.

For example, candidate A has already received counter 100, candidate B shares A's sender prefix and sends its first message at counter 0. If A is tried first with the same key-epoch value, A's replay window rejects counter 0 and the receiver never tries B's key.

**Impact:** legitimate group messages can be dropped because of another member's independent counter history. Colliding group hashes may expose a similar early-exit problem across group candidates.

**Suggested direction:** a replay rejection for an unauthenticated candidate must skip that candidate rather than terminate the packet search. Preserve normal duplicate suppression once the sender/key is established.

**Claude verification:** generate two actual identity keypairs with the same first public-key byte, install both as group members at the same key epoch, and give them different replay histories. Verify both delivery orders. Also test colliding group hashes and current/previous key candidates where applicable.

## PR-05 — Truncated Poly1305 tags need an explicit design decision

**Locations:** `crates/lc-proto/src/aead.rs`, `truncate` and `seal`; protocol §5 and packet size calculations throughout the spec.

The AEAD implementation retains only the first eight bytes of a Poly1305 tag. [RFC 8439 §4](https://www.rfc-editor.org/rfc/rfc8439.html#section-4) explicitly requires the complete tag and prohibits truncation.

**Assessment:** this is a custom variant, not the standard RFC 8439 AEAD. The mismatch is confirmed; this review did not demonstrate a feasible forgery solely from truncation. LoRa's transmission rate limits matter to a quantitative analysis, but do not by themselves provide a security argument for every future transport or verification interface.

**Suggested direction:** use the full 16-byte tag and a standard AEAD implementation. If retaining truncation, document and independently review the exact security bounds, maximum key usage, failed verification budget, and transport assumptions. Do not describe the variant as unmodified RFC 8439 ChaCha20-Poly1305.

**Claude verification:** assess the tradeoff explicitly. A switch to full tags requires revisiting DATA chunk sizes, ACK airtime/timers, discovery, INTRO sizes, group payload/padding limits, vectors, and interoperability/versioning.

## Validation already performed

- `cargo test -p lc-proto`: **34 passed, 1 ignored**. The ignored case is a measurement test.
- A standalone Rust reproduction linked against the built `lc-proto` confirmed PR-01's repeated keystream. Its complete logic is included above so the temporary artifact is not required.
- `cargo test -p lc-proto -p lc-core`: failed to compile during the active identity migration. The initial run had outdated handshake/discovery API callers; a later run had missing introduction methods/fields and outdated configuration/contact initializers. These are historical observations of a moving tree, not claims about the current build. Rerun after the migration settles.
- PR-02, PR-03, and PR-04 were traced in source; their full integration attack scenarios were not executed.
- No implementation files were modified by this review. This report is the handoff artifact.

## Requested follow-up

For each finding, return **confirmed**, **partially confirmed**, or **rejected**, with the current source evidence and any regression-test result. Separate protocol changes from implementation fixes. Prioritize nonce uniqueness, signature-before-state ordering, and identity continuity before broader cleanup. Recheck current source rather than relying on historical line numbers or the temporary build failures.

# Opus Direction Review #47

**Date:** 2026-05-21
**Scope:** PRs #163/#165/#166/#162/#161/#154/#157 + open PRs #164/#167. Ground
truth: direct file reads on `origin/master`; PR #164 branch inspected via
`git show origin/feat/chirp-avatar-image-fadein`.

---

## Q1 — Is the inert-seam streak broken?

**Verdict: structurally wired, not yet verified end-to-end.**

PR #163 is the best kind-of-wiring landed to date: `KernelModel.apply()` calls
`maybePublishDmRelayList()` (KernelModel.swift:450) on every snapshot tick,
which calls `kernel.publishDmRelayList(relays:)` (DmBridge.swift:117) when the
read-eligible relay set changes. The guard at KernelModel.swift:528–534 prevents
re-firing for the same account+URL set.

What is missing for a "verified working" verdict:

1. **No end-to-end test.** The 8 unit tests in `dm_relay_list.rs` cover the
   builder and validator in isolation. There is no Rust integration test that
   (a) signs in, (b) configures relay rows, (c) fires a snapshot tick, (d)
   asserts a kind:10050 event was published to an in-process relay. The
   `maybePublishDmRelayList` Swift-side logic also has no XCTest coverage.

2. **Thin-shell role-parsing in Swift.** `readEligibleRelayUrls(rows:)` at
   KernelModel.swift:548 re-implements relay-role token logic in Swift (tokens
   split on `,`/`+`/whitespace). A mismatch against `relay_roles::has_role` in
   Rust would silently publish the wrong relay set. The comment at line 513
   acknowledges this debt; it is not blocked on this PR but is an unverified
   assumption.

To call it "verified working": add one `nmp-testing` integration test that
round-trips `nmp_app_dispatch_action("nmp.dm.publish_relay_list", …)` and
asserts the emitted kind:10050 event reaches a mock relay.

---

## Q2 — Highest-ROI next feature

**NIP-17 DM receive-side (kind:1059 inbox view).**

The send path (`nmp.dm.send`) is live. The relay-list publish (kind:10050) is
wired via PR #163. The inbox projection (`DmInboxProjection`) is already built
(`crates/nmp-nip17/src/inbox.rs`). The iOS screens exist: `DmListView.swift`,
`DmConversationView.swift`, `DmInboxStore` (DmBridge.swift:143–212). The
registration FFI `nmp_app_chirp_register_dm_inbox` is wired and called from
`KernelModel.apply()` (line 427).

What is missing is plumbing rather than invention: `DmInboxStore.apply` is
called every tick but whether `update.dmInbox` is populated depends on whether
the `nip17.dm_inbox` snapshot projection is emitting non-nil payloads after a
real kind:1059 envelope arrives. A single integration test + one real DM send
from a second client would confirm the full loop. This is a verification PR, not
a build-from-scratch PR — the highest leverage per hour of engineering.

NIP-57 LNURL zap (PR #164) is a distant second: it ships the LNURL HTTP leg
using an in-process blocking thread, which is a D8 risk (actor-thread cannot be
blocked), and the correlation_id round-trip is explicitly incomplete
(`zap.rs:65`). Profile edit in Chirp is pure UI work with no new Rust seam
needed; it does not advance the architecture.

---

## Q3 — ADR-0026 and NIP-17 send via bunker

`send_gift_wrapped_dm` at dm.rs:87 hard-gates on `identity.active_local_keys()`
and toasts an explicit message for bunker users. ADR-0026's
`RemoteSignerHandle::nip44_encrypt` exists but is inert on both sides.

Minimum work for bunker-signed NIP-17 sends:

1. Add `nmp_nip59::gift_wrap_with_signer` that accepts a
   `RemoteSignerHandle::nip44_encrypt` closure for the kind:13 seal step (ECDH
   over the sender's bunker key) and generates a fresh ephemeral key locally for
   the outer kind:1059 wrap. This is a `nmp-nip59` crate change only.
2. Wire `send_gift_wrapped_dm` to call the new function when
   `active_local_keys()` is `None` but a remote signer is active.
3. The ECDH call on the seal requires one async round-trip to the bunker (one
   `nip44_encrypt` RPC). The actor must be async-capable or the call must
   off-load to a spawned thread (same pattern `nmp-app-chirp/src/zap.rs` uses
   for LNURL HTTP). This is the deepest blocker — not an API question, a
   threading model question.

**Staged approach that delivers value sooner:** surface "DMs are not available
with a remote signer" in the DM send UI _before_ dispatch (guard in
`DmConversationView.swift` checking `localPubkey` existence) so bunker users
see a clear message rather than a toast after a failed round-trip. That takes
one day and is honest to users without requiring any Rust change.

---

## Q4 — Highest structural risk

**PR #164's LNURL executor threading model.**

PR #164 (`feat/chirp-avatar-image-fadein`) contains two unrelated features in
one branch: the avatar fade-in (pure cosmetic) and a full NIP-57 LNURL executor
(`crates/nmp-nip57/src/lnurl.rs` + `apps/chirp/nmp-app-chirp/src/zap.rs`). The
LNURL executor is the risk:

1. **D8 violation risk.** `lnurl.rs:33` explicitly says `fetch_invoice` is
   "synchronous and blocking" and "the actor thread MUST NEVER call it directly."
   The host wires a spawned thread. If this reviewer pattern is copied elsewhere
   (or the call site changes), the actor thread stalls on a network call.

2. **`ShowToast` overloaded.** `zap.rs:68` routes the bolt11 invoice through
   `ActorCommand::ShowToast` — the error surface. A success ("invoice ready:
   lnbc…") populates `last_error_toast`, which iOS reads as a toast to dismiss.
   No UI exists to hand the invoice to a lightning wallet; the user sees a raw
   bolt11 string in an error-styled alert.

3. **Two signed kind:9734 events** (`zap.rs:44–50`): one signed by the actor for
   relay publish, one re-signed locally by the worker for the LNURL POST. The
   doc acknowledges they differ by wall-clock skew. This is an observable
   divergence between the on-relay record and the LN provider's embedded receipt.

4. **`nmp.zap` re-registration.** PR #154 deleted `nmp.zap`; PR #164 re-adds it
   (`ffi.rs:187–193`). Dormant-executor regression risk if PR #164 merges without
   a corresponding Swift caller wired before the zap button appears in a UI.

The correct fix before merging PR #164 Rust content: split the PR (cosmetic
fade-in merges today; zap executor waits until a zap button exists in Chirp that
calls `nmp.zap`, a `WalletPayInvoice` path exists or a NIP-60 wallet integration
is scoped, and the `ShowToast` overload is resolved).

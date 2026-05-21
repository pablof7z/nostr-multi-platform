# Opus Direction Review #32 — NMP Architecture

Date: 2026-05-21
Reviewer: Opus (architect advisor)
Worktree HEAD: `6ecec6c6` (branch `worktree-agent-ac965f615eda5bdc5`)
Scope: post-ViewModule-deletion substrate, NIP-57 zaps, `dispatch_action` coverage, iOS `action_results` adoption.

---

## 0. Scan-state note (read this first)

This review was written against the **worktree** checkout, not `master`.
`master` HEAD is `cfaa2116` (review #29). The PRs the task references —
#93–#97, the `ActionPlan` collapse (`7bf93366`), and the `ViewModule` deletion
(`487b35f3`) — are **committed on the worktree branch but NOT yet merged to
`master`**. Everything below is evidence-cited against worktree paths under
`.claude/worktrees/agent-ac965f615eda5bdc5/`. When these land on `master` the
findings hold; until then, `master` still carries `ViewModule` and `ActionPlan`.

Two numbers in the task framing are wrong and are corrected here with evidence:

- ActorCommand has **36 variants**, not 40 (`actor/mod.rs` enum, counted).
- NIP-57 infra is **~60–70 % built**, not 80 % (review #30's figure is stale —
  file inventory below).

---

## 1. Q1 — Post-`ViewModule` deletion: formalize a new trait, or keep static dispatch?

### Evidence

- `ViewModule` trait is gone. `substrate/view.rs:88-97` carries a tombstone
  comment; the file now exports only `ViewDependencies`, `KernelEvent`,
  `ProjectionChange`, `ViewContext`.
- The one live consumer drives view types by **static dispatch through inherent
  methods**: `nmp-nip01/src/timeline_projection.rs:72` calls
  `Nip10ModularTimelineView::open(...)`, `:87` `::snapshot(...)`, `:104`
  `::on_event_inserted(...)`. No `dyn ViewModule`, no `ViewRegistry`.
- `nmp-nip57/src/lib.rs` documents the same pattern explicitly: `ZapsView`'s
  `open`/`on_event_*`/`snapshot` "are reached via static dispatch" and notes
  the trait + `register(&mut ModuleRegistry)` were "both deleted because no
  kernel-side registry ever drove them."

### Verdict: **Do NOT reintroduce a trait. Static dispatch is strictly better here.**

The deleted `ViewModule` was a 20-impl, 0-driver abstraction (review #25, #27,
#31 all flagged it; #97 finally deleted it). A trait earns its keep only when
something stores `dyn Trait` and iterates heterogeneously — i.e. a
`ViewRegistry`. None ever shipped. Reintroducing a "lighter trait" now would
recreate the exact failure mode: a contract with no runtime consumer.

**The implicit protocol concern is real but does not need a trait to solve.**
The `open` / `on_event_inserted` / `on_event_removed` / `on_event_replaced` /
`on_projection_changed` / `on_tick` / `snapshot` method set is currently an
unenforced naming convention. The right enforcement mechanism is **a derive
macro or a doc-tested template**, not a trait object — both give signature
consistency without forcing dynamic dispatch or a registry that nothing reads.

### Long-term costs of static dispatch (name them honestly)

1. **No compile-time signature enforcement.** A new view crate can name its
   method `on_inserted` instead of `on_event_inserted` and nothing breaks until
   a human notices. Mitigation: a `#[derive(KernelView)]` macro that *generates*
   the inherent methods from a spec, or a `view_module! {}` declarative macro.
2. **No heterogeneous storage.** If NMP ever wants "register N views, fan one
   event to all of them" generically (a true `ViewRegistry`), static dispatch
   cannot do it — you would need the trait back. **This is the trigger
   condition**: reintroduce a trait *only* when a `ViewRegistry` has a concrete
   caller, not before. The 20:0 history is the evidence for "not before."
3. **Codegen duck-typing** (review #18 raised this for actions; it applies to
   views too): any UniFFI/codegen layer that wants to treat views uniformly has
   to pattern-match concrete types. Acceptable while there are ~3 view types;
   revisit at ~10.

`ViewDependencies` is correctly *kept* — it is a plain data struct bridging a
view's event needs to `LogicalInterest` (`view.rs:49-74`), load-bearing and
consumer-driven. That is the correct shape for a substrate type: data, not a
vtable.

---

## 2. Q2 — NIP-57 zaps: highest-risk gaps + the LNURL-HTTP ownership question

### File inventory (what exists today)

`nmp-nip57/src/`:

| File | Status |
|------|--------|
| `build.rs` | `ZapRequestBuilder` — kind:9734 builder. Complete, 8 tests. |
| `decode.rs` | kind:9735 receipt decoder. Present. |
| `domain.rs` | `ZapsDomain` reverse-index `(event → receipt_ids)`. Present. |
| `view.rs` | `ZapsView` reactive aggregate (total msats, zappers). Present. |
| `bolt11.rs` | HRP amount parser for the authoritative msat amount. Present. |
| **ActionModule impl** | **ABSENT** — `grep "impl ActionModule" nmp-nip57/` returns nothing. |
| **executor** | **ABSENT** — no `nip57`/`zap` entry in `default_registry` or any chirp ffi registration. |
| **LNURL HTTP call** | **ABSENT** — no HTTP client anywhere in the crate. |

So NIP-57 is **~60–70 % built**, not 80 %. The "infra 80 % built" line from
review #30 predates the honest file count above.

### Highest-risk gaps in the zap flow, ranked

The NIP-57 flow: client builds kind:9734 → resolves recipient LNURL from their
kind:0 `lud06`/`lud16` → GETs the LNURL pay endpoint → POSTs the kind:9734 to
the `callback` URL with `?amount=&nostr=` → LN provider returns a bolt11
invoice → user/wallet pays the invoice → LN provider mints + publishes the
kind:9735 receipt.

1. **The LNURL HTTP round-trip has no owner decided.** This is the #1 risk and
   it is *architectural*, not implementation. See the verdict below — it must be
   resolved BEFORE the `ZapModule` executor is written, because the answer
   changes the executor's signature.
2. **kind:9735 receipt verification is unspecified.** `decode.rs` decodes a
   receipt but NIP-57 §"Appendix F" requires verifying the receipt's
   `description` tag hashes to the original kind:9734, and that the `bolt11`
   amount matches. If `decode.rs` does not enforce this, a malicious relay can
   inject fake zap receipts and `ZapsView` will count them. **Verify
   `decode.rs` does the `description`-tag SHA-256 check** — if not, that is a
   silent integrity hole feeding the aggregate.
3. **Invoice payment is out of NMP's scope entirely** — it belongs to a wallet
   (NIP-47 NWC, or an external LN wallet). NMP must NOT try to own this. The
   executor's job ends at "obtained a bolt11 invoice"; handing it to a wallet
   is a separate action (`nmp.wallet.pay` / `WalletPayInvoice`). This boundary
   needs to be explicit in the `ZapModule` design or the executor will sprawl.
4. **`amount` tag vs invoice amount mismatch.** The kind:9734 carries an
   optional `amount` tag (msats); the LNURL provider returns an invoice whose
   amount is authoritative. `ZapsView` must aggregate the **bolt11** amount
   (`bolt11.rs` exists for exactly this) — confirm the view does not trust the
   request's `amount` tag.

### Verdict on LNURL HTTP ownership: **Rust must own the LNURL HTTP call. iOS owning it would repeat the `nmp_app_publish_note` mistake.**

The LNURL pay flow is **protocol-defined behavior**, not host glue:

- The endpoint is *named inside Nostr data* — the recipient's kind:0
  `lud16`/`lud06`. Resolving it is reading a Nostr event, which is squarely
  `nmp-core`'s job.
- The kind:9734 is signed by NMP's actor (D4) and POSTed verbatim — the HTTP
  body *is* a Nostr event. Splitting "build + sign in Rust" from "POST in
  Swift" puts a protocol step on the wrong side of the FFI boundary.
- This is structurally identical to the `nmp_app_publish_note` anti-pattern the
  project spent reviews #13–#19 unwinding: a per-app host owning a step the
  substrate should encode. Doing it again for zaps would be a regression.

**Concrete recommendation:** add a host-injected HTTP capability (the
`CapabilityModule` seam — review #21 confirmed `CapabilityModule` is live via
`KeyringCapability`). The host supplies an *HTTP transport* function pointer
(the host owns the actual socket — iOS uses `URLSession`); `nmp-core` /
`nmp-nip57` owns the LNURL *protocol logic* (URL construction, JSON parsing,
kind:9734 attachment, invoice extraction, receipt verification). This keeps D8
clean (the HTTP call is async, driven through the actor, never blocking the FFI
thread) and keeps protocol logic in Rust. **iOS supplies a dumb pipe; Rust
supplies the protocol.**

This decision is a **blocker** for the in-flight `ZapModule` work: if the
executor is written assuming iOS does the HTTP, its signature (does it return
an invoice? does it dispatch a follow-on `WalletPayInvoice`?) will be wrong and
have to be redone.

---

## 3. Q3 — `dispatch_action` coverage: 36 ActorCommands, 5 namespace paths

### Evidence

- **36** `ActorCommand` variants (`actor/mod.rs`, `pub enum ActorCommand`).
- **5** namespaces reachable through `dispatch_action` today:
  - `nmp.publish` — built-in, `action_registry.rs:401`. One namespace covers
    4 verbs (`Publish`, `PublishNote`, `PublishProfile`, `Cancel`).
  - `chirp.react`, `chirp.follow`, `chirp.unfollow` — `chirp/ffi.rs:248,264,277`.
  - `nip29.join_request` — `chirp/ffi.rs:324`.
- **15** `ActionModule` impls exist in `nmp-nip29` alone (8 explicit `impl
  ActionModule` + 7 from the `admin_action!` macro in `action/admin.rs`). Only
  `JoinRequestAction` is wired. **14 are dormant.** This is the same
  built-but-unconsumed pattern that killed `ViewModule`.

### Is the "two doors" model acceptable?

The 36 ActorCommand variants are **not all candidates for `dispatch_action`** —
that framing overstates the gap. Categorize them:

- **Lifecycle / engine control** (~12: `Start`, `Stop`, `Configure`, `Reset`,
  `Shutdown`, `LifecycleEvent`, `PushInterest`, `IngestPreVerifiedEvents`,
  `Kernel`, `ShowToast`, `OpenTimeline`, …) — these are *not* user actions.
  They are kernel control plane. They should **never** route through
  `dispatch_action`; a `correlation_id` for "Start" is meaningless.
- **Navigation** (`OpenAuthor`, `CloseAuthor`, `OpenThread`, `CloseThread`,
  `OpenFirehoseTag`) — view-scope changes, not protocol actions. Belong to a
  view/interest path, not `dispatch_action`.
- **Identity** (`SignInNsec`, `SignInBunker`, `CreateAccount`, `SwitchActive`,
  `RemoveAccount`, `AddRemoteSigner`, `RemoveRemoteSigner`,
  `BunkerHandshakeProgress`) — review #21 already judged `IdentityModule`
  marginal. These are bootstrap, mostly one-shot, and OK as bespoke symbols.
- **Genuine protocol actions that SHOULD route through `dispatch_action`**
  (~8): `PublishNote`, `PublishProfile`, `PublishSignedEvent`,
  `PublishUnsignedEvent`, `PublishUnsignedEventToRelays`, `React`, `Follow`,
  `Unfollow`, plus `CancelPublish`, `RetryPublish`, `WalletPayInvoice`,
  `WalletConnect`, `WalletDisconnect`, `ClaimProfile`/`ReleaseProfile`.
  Of these, `PublishNote`/`PublishProfile`/`React`/`Follow`/`Unfollow` already
  do (via `nmp.publish` and `chirp.*`). The **remaining bespoke door** is
  `CancelPublish`, `RetryPublish`, and the wallet verbs.

### Verdict: the "two doors" model is acceptable **as a permanent design**, BUT the bespoke door must be **scoped to control-plane commands**, not protocol actions.

The strategic seam does not need to carry `Start`/`Stop`/`Shutdown` — those are
not actions. What it *must* carry is every **protocol verb** (anything that
produces or cancels a Nostr event). Today it carries publish + 3 social verbs +
1 NIP-29 verb. The honest gap is:

1. `CancelPublish` / `RetryPublish` — publish-lifecycle verbs still bespoke
   (the `nmp.publish` `Cancel` arm is a no-op stub — `action_registry.rs:461`).
2. The 14 dormant `nmp-nip29` `ActionModule` impls.
3. NIP-25 reactions in `nmp-nip25` (if a typed `ActionModule` exists there, it
   is not wired — `chirp.react` is a chirp-local shim, not the NIP crate's).

**Closing the bespoke door for protocol actions is worth doing.** It is mostly
mechanical: each dormant `ActionModule` needs a ~10-line `register_*` pair like
`register_nip29_actions` (`chirp/ffi.rs:323`). The seam has *proven* it can
carry any action (NIP-29 `join_request` drives a real host-pinned
`ActorCommand` — `chirp/ffi.rs:337`). The risk now is not "can the seam carry
it" — it is "14 typed modules rot unconsumed" (the `ViewModule` failure
replaying). Wire them or delete them.

---

## 4. Q4 — iOS `action_results` adoption: the migration path

### Evidence — this is the most important finding in the review

- PR #94 added a **per-tick-drained `Vec` projection**:
  `update.rs:272` calls `take_action_results_projection()`, inserted as
  `projections["action_results"]`. Backed by
  `publish_engine.rs:460 take_action_results_projection()` which calls
  `take_pending_terminals()` — it **drains**, so every terminal that settled
  since the last tick is surfaced exactly once.
- The **old sticky scalar still ships alongside it**: `update.rs:262` inserts
  `projections["last_action_result"]` from
  `publish_engine.rs:437 last_action_result_projection()`, which reads
  `last_terminal()` — a sticky "most recent only" value.
- **iOS reads ONLY the sticky scalar.** `KernelBridge.swift:501,650,665` all
  reference `projections["last_action_result"]`. `grep -c action_results
  ios/Chirp/Chirp/Bridge/KernelBridge.swift` → **0**.
  `grep -rln action_results ios/` → **nothing**. iOS has never heard of
  `action_results`.

### What this means: PR #94's bug fix is invisible from the device

PR #94's stated purpose is to fix the **spinner-hang bug**: when two actions
settle in the same tick, the sticky scalar only reports the last one, so the
earlier action's spinner never clears. PR #94 fixed this **on the Rust side**.
But the fix only takes effect for a host that *reads `action_results`*. iOS
does not. **The spinner-hang bug PR #94 claims to fix is still live in
production**, because the consumer never migrated. The fix is real in the
library and absent on the device.

### Correct migration path

1. **iOS must read `projections["action_results"]` as an array** and resolve
   *every* `correlation_id` in it per snapshot tick. The shape is
   `[{"correlation_id","status","error"}]` (`publish_engine.rs:465-478`).
2. During the transition iOS can read **both** — `action_results` for the
   array, falling back to `last_action_result` only if `action_results` is
   absent — but the end state is `action_results`-only.
3. Once iOS reads the array, **delete `last_action_result`** from
   `update.rs:262` and `publish_engine.rs:437`. The scalar is a bug-shaped
   API; keeping it invites the next host to make the same mistake. Deleting it
   is a forcing function — exactly the pattern that worked for
   `nmp_app_publish_note`.

### What breaks if iOS never migrates

- The spinner-hang bug stays live forever for any burst of ≥2 actions per tick
  (rapid double-tap publish, publish-then-react). The user sees a spinner that
  never resolves.
- NMP carries two projections for one concern indefinitely — the exact
  "diff-signal asymmetry" debt review #25 flagged.
- Every future host copies the wrong one, because the wrong one is what the
  reference app (Chirp) uses.

This is a **small, contained migration** (one Swift file, `KernelBridge.swift`)
with an outsized correctness payoff. It is the #1 priority below.

---

## 5. Q5 — The single most dangerous architectural debt in NMP today

**The `action_results` consumer gap: `KernelBridge.swift` reads the sticky
`projections["last_action_result"]` scalar (`KernelBridge.swift:501`) while the
correct per-tick `projections["action_results"]` Vec
(`update.rs:272`) is never read by any iOS code.**

Why this is the *most* dangerous debt, above the dormant modules or the NIP-57
gaps:

- **It is a live production bug, not a structural smell.** The other debts
  (dormant `ActionModule` impls, two-doors model, missing `ZapModule`) are
  *latent* — code that is not wrong, just incomplete. This one is a *wrong
  observable behavior* shipping on the device: a spinner that hangs.
- **It is invisible.** PR #94 merged with passing tests and a convincing
  changelog ("fixes spinner-hang"). The tests exercise the Rust projection.
  Nothing tests that iOS *reads* it. A green CI and a merged "fix" PR mask a
  bug that is still 100 % reproducible on a real phone. That gap between
  "library is correct" and "device is correct" is the dangerous part —
  it means the project's verification does not cover the actual user-facing
  contract.
- **It is the cheapest of all the debts to close** — one Swift file — which
  makes leaving it open indefensible.

Runner-up: the **14 dormant `nmp-nip29` `ActionModule` impls** + absent
NIP-57 `ActionModule` — the `ViewModule` 20:0 failure pattern replaying in the
action layer. Dangerous because it is the *same* mistake the project just spent
PR #97 cleaning up, now accreting again one crate over.

---

## 6. Ranked priority list — next 5 items

### 1. Migrate iOS `KernelBridge` to read `projections["action_results"]`
**Why first:** closes a live production bug (spinner-hang) that PR #94's
Rust-side fix does not actually deliver to the device. One file
(`ios/Chirp/Chirp/Bridge/KernelBridge.swift`), smallest blast radius, highest
correctness payoff. Until this lands, PR #94 is a fix in name only.
**Done when:** iOS resolves every `correlation_id` in the `action_results`
array each tick; a double-publish burst clears both spinners.

### 2. Decide LNURL-HTTP ownership BEFORE writing the NIP-57 `ZapModule` executor
**Why second:** this is an architectural blocker for in-flight work. The
verdict (§2): Rust owns LNURL protocol logic; the host injects a dumb HTTP
transport capability. If the `ZapModule` executor is written assuming iOS does
the HTTP, its signature is wrong and gets redone. Decide, write it down in an
ADR, *then* build the executor.
**Done when:** an ADR records the HTTP-capability seam; `ZapModule` +
executor are designed against it.

### 3. Delete the sticky `last_action_result` scalar (after #1)
**Why third:** it is a bug-shaped API. As long as it exists, the next host
copies it (Chirp already did). Deleting it forces every consumer onto the
correct array. Forcing-function discipline — the same move that worked for
`nmp_app_publish_note`. Gated on #1 so iOS is not broken.
**Done when:** `update.rs:262` and `publish_engine.rs:437` are gone; only
`action_results` ships.

### 4. Wire or delete the 14 dormant `nmp-nip29` `ActionModule` impls
**Why fourth:** 15 typed `ActionModule` impls exist in `nmp-nip29`, 1 is wired.
This is the `ViewModule` 20:0 failure replaying. Either register them
(mechanical — copy `register_nip29_actions`, `chirp/ffi.rs:323`) or delete them.
"Built but unconsumed" is debt regardless of how typed it is.
**Done when:** every `nmp-nip29` `ActionModule` is either reachable via
`dispatch_action` or removed; no orphan typed module remains.

### 5. Audit `CancelPublish` / `RetryPublish` / wallet verbs — route protocol
   actions through `dispatch_action`
**Why fifth:** the `nmp.publish` `Cancel` arm is a no-op stub
(`action_registry.rs:461`). `CancelPublish` and `RetryPublish` are genuine
publish-lifecycle verbs still on the bespoke door. The "two doors" model is
fine for control-plane commands (`Start`/`Stop`/lifecycle) but every *protocol
verb* must go through the seam. Audit the 36 ActorCommands, confirm the ~8
protocol verbs route through `dispatch_action`, leave the ~28 control/nav/
identity commands bespoke by design.
**Done when:** a documented list classifies every ActorCommand as
"protocol-action → must route" vs "control-plane → bespoke OK"; the protocol
verbs all route.

---

## 7. Summary

The substrate is healthier than prior reviews — `ViewModule` (the longest-lived
0-driver abstraction) is finally deleted, `ActionPlan`'s three dead fields are
gone, and `dispatch_action` demonstrably carries a real NIP-crate action
(`nip29.join_request` drives a host-pinned `ActorCommand`). Static dispatch +
inherent methods is the correct replacement for `ViewModule` — do not
reintroduce a trait until a `ViewRegistry` has a concrete caller.

The dangerous debt is not structural any more — it is a **verification gap**:
PR #94 fixed a bug in the library that the iOS consumer never picked up, so the
bug still ships. Fix that first (one Swift file), then delete the sticky scalar
that caused it. For NIP-57, settle the LNURL-HTTP ownership question (Rust owns
the protocol, host injects the pipe) before the `ZapModule` executor is written
— otherwise it will be built wrong. And wire or delete the 14 dormant NIP-29
action modules before they become the next `ViewModule`.

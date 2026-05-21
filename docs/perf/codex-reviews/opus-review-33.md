# Opus Direction Review #33 — NMP Architecture

Date: 2026-05-21
Reviewer: Opus (architect advisor)
`master` HEAD: `1cdfe12b` (`fix(ios): expose action_results array in SnapshotProjections`)
Scope: post-NIP-57-zaps substrate, `dispatch_action` round-trip, `action_results`
iOS adoption, relay transport, WASM surface, NIP-57 LNURL ownership.

---

## 0. Scan-state — three task-framing errors corrected up front

The task prompt contains three factual errors. Correcting them is part of the
review, not a footnote — building on them would produce wrong direction.

1. **`crates/nmp-nip17/` does not exist.** `ls crates/` returns no `nmp-nip17`.
   `grep -rl nip17 crates/` matches only `nmp-nip59` (gift-wrap) and incidental
   comments in `nmp-core`. NIP-17 DMs have **not been started** — there is no
   partial crate. The NIP-59 gift-wrap *substrate* (`gift_wrap` /
   `unwrap_gift_wrap` free functions + `WelcomeUnwrapModule`) is built and is
   the only DM-adjacent code in tree.

2. **NmpHighlighter's 39K Swift LoC is not a D0 violation.** D0 governs the
   *shape of protocol-crate state* — "no app nouns as named `KernelSnapshot`
   fields." It says nothing about app-crate line count. NmpHighlighter is a
   separate app, not substrate. Calling its LoC a "797:1 D0 violation"
   conflates two unrelated doctrines. It is app-debt, real but off-doctrine;
   do not score it against D0.

3. **PR numbers #93–#98 are merged on `master`** (confirmed: `git log`
   shows `7a2c1c0c (#98)`, `661a878c (#97)`, `dc0e28a7 (#95)`,
   `7bf93366`, `85b9e358`, `231a45d8`). PR #99-equivalent (`1cdfe12b`) is also
   merged. Review #32 was written against a worktree; #33 is against `master`.

---

## 1. The headline correction: review #32's "#1 dangerous debt" was wrong

Review #32 named the iOS `action_results` consumer gap "the single most
dangerous architectural debt," asserting "a live production bug ships on the
device — a spinner that hangs." **That is false, and the evidence is now in.**

### Evidence

- `ios/Chirp/Chirp/Bridge/KernelModel.swift` is the *only* `KernelUpdate`
  consumer. Its `apply(result:)` (line 287) wires `publishQueue` (`:331`),
  `publishOutbox` (`:332`), `lastErrorToast` (`:333`) — and **nothing else
  action-related**.
- `grep -rn 'actionResults\|lastActionResult' ios/Chirp/` returns **only**
  declarations and comments in `KernelBridge.swift`. Zero call sites read
  either. `KernelModel` never references `update.actionResults` or
  `update.lastActionResult`.
- The per-publish UI surface that *does* exist is the **publish engine's own
  projection**: `NotificationsView.swift:91-92` reads
  `publishOutbox.filter { $0.status == "sending" }` /
  `{ $0.status == "retrying" }`; `HomeFeedView.swift:136` badges
  `publishOutbox.count`. `DiagnosticsView.swift:274` lists `publishQueue`.

### Verdict

Chirp has **no spinner keyed on `correlation_id` at all.** It never had one.
The "spinner-hang when two actions settle in one tick" bug review #32 said PR
#94 fixed-but-didn't-deliver **cannot occur**, because the consumer it requires
was never written. PR #94 (scalar→Vec) and PR #99 (`actionResults` decode in
`SnapshotProjections`) fixed and exposed a projection that **no host reads and
no host needs** — Chirp's publish UX is driven entirely by `publishOutbox`
status strings, which already handle multi-publish bursts correctly because
they are a per-event list, not a scalar.

This reframes everything below. `action_results` is not a live bug — it is
**dead weight** (see §3, Q2). Carrying #32's priority forward "because it was
written down" would have spent the next PR on a non-problem.

---

## 2. Q1 — What should NMP support that it doesn't

Ranked by ROI, "substrate already partially built" weighted highest.

### 2.1 — HIGH: an `HttpCapability` so NIP-57 zaps are not dead-on-arrival

`nmp-nip57` ships a `ZapModule` `ActionModule` + executor (PR #98), registered
into Chirp at `apps/chirp/nmp-app-chirp/src/ffi.rs:434`. **The feature does
not work.** The executor (`zap_request_command`, `action.rs:151`) publishes the
kind:9734 *to Nostr relays* — but NIP-57's actual money path is the LNURL HTTP
round-trip: GET the recipient's lnurl-pay endpoint, POST the signed kind:9734
to its `callback` URL, receive a bolt11 invoice. `action.rs:24-29` admits it:
"Leg 2 (the LNURL HTTP round-trip) has **no kernel capability yet**." So
`ZapModule` is a *registered, validated, tested action that produces nothing a
user can pay.* It is shipped-but-inert.

The substrate to fix this **already exists** — see §6, Q5. This is the highest
ROI item in the review: one capability seam turns a dead feature live.

### 2.2 — MEDIUM: a real backpressure story for the actor command channel

`crates/nmp-core/src/actor/mod.rs:119` — `use std::sync::mpsc::{...}` and
`:440 run_actor(command_rx: Receiver<ActorCommand>, ...)`. The command channel
is an **unbounded `mpsc::channel`**. The module header (`:6-12`) explicitly
records the decision: a bounded `SyncSender` "4096-slot bounded channel could
fill" so they chose unbounded "no merged SyncSender, no forwarder threads, no
drops" (`:542`). The cost of that choice: **a fast producer (FFI thread
spamming `dispatch_action`, or a relay flood feeding back) can grow the queue
without bound** — there is no admission control, no shed-load, no
"queue is full, reject this action" path. `actor_queue_depth` exists as a
metric field (`kernel/types.rs:473`) but is **hardcoded to `0`**
(`kernel/update.rs:104`) — it measures nothing. This is latent (no load test
has hit it) but it is the kind of thing that is invisible until a crowded relay
or a tight UI loop turns it into an OOM. Not urgent; name it so it is a known
risk, not a surprise.

### 2.3 — LOW for now: NIP-17 DMs

The task asks "NIP-17 DMs?" The honest answer: the crate does not exist, only
the NIP-59 gift-wrap *substrate* does. NIP-17 is a genuine feature gap, but it
is **greenfield**, not "substrate already partially built." It does not clear
the ROI bar against §2.1 (which is one seam from working) or §3 (deletions
that cost nothing). Defer NIP-17 until the zap path is closed.

### 2.4 — WASM coverage is a fiction, not a gap to fill

`grep -rl wasm_bindgen crates/` → **nothing.** `grep -rl wasm crates/*/Cargo.toml`
→ only `nmp-signers`. There is no `wasm_bindgen` surface anywhere in the
monorepo. The project description's "compiles to WebAssembly for browsers" is
**aspirational, not implemented.** This is not a "support more WASM" item — it
is a "stop claiming WASM is a target" item (see §3). Building a real WASM
surface is a multi-month effort that nothing currently justifies; the one live
proof app (Chirp) is iOS.

---

## 3. Q2 — What should NMP stop doing or delete

### 3.1 — DELETE: the sticky `last_action_result` scalar AND keep `action_results` only if a host ever needs it

Review #32 said "delete `last_action_result`, keep `action_results`." With the
new evidence (§1), the honest call is sharper: **both** the scalar and the Vec
are currently consumed by **zero hosts**. The publish UX runs on
`publish_outbox`. So:

- `last_action_result` (`publish_engine.rs` `last_action_result_projection`,
  inserted at `update.rs`): **delete now.** It is a bug-shaped API (scalar that
  drops terminals) with no consumer. Nothing breaks.
- `action_results` (`publish_engine.rs` `take_action_results_projection`):
  this is the *correctly-shaped* version, but it is also unconsumed. Do **not**
  delete it reflexively — but do **stop treating its iOS adoption as a
  priority.** It is a correct, dormant seam. If a future host wants
  per-`correlation_id` action feedback (e.g. a generic SDK consumer that is not
  Chirp), `action_results` is the right thing to read. Keep it as a documented,
  tested, *optional* output; remove every comment and review note that calls
  iOS adoption "the #1 fix." It is not.

Net: one deletion (`last_action_result`), one reclassification (`action_results`
from "migration debt" to "dormant-but-correct optional seam").

### 3.2 — STOP claiming WASM is a build target

Until a `wasm_bindgen` surface exists and a browser app consumes it, every doc
that says "compiles to WASM" is unverified. Either build a minimal WASM smoke
target (a real, CI-gated artifact) or strike the claim from `aim.md` / the
README. A capability the project asserts but cannot demonstrate is worse than
an admitted gap — it misleads direction reviews (this one included, until the
grep).

### 3.3 — WIRE-OR-DELETE: the 15 dormant `nmp-nip29` ActionModule impls

`grep -rn 'impl ActionModule for\|admin_action!' crates/nmp-nip29/src/` → **16
matches** across `membership.rs`, `admin.rs`, `content.rs`, `composed.rs`.
Exactly **one** (`JoinRequestAction`) is wired —
`apps/chirp/nmp-app-chirp/src/ffi.rs:334` registers `nip29.join_request`. The
other ~15 have validators and no executor registration. This is the **exact
`ViewModule` failure pattern** (20 impls, 0 drivers) that PR #97 just spent
effort deleting — now re-accreting one crate over. Review #32 flagged it; it is
still true. Per-`ActionModule` the fix is mechanical (~10 lines:
`register_action_module` + `register_action_executor`, copy the
`register_nip29_actions` shape). If Chirp has no surface that needs them, that
is the answer: **delete them.** "Typed but unconsumed" is debt no matter how
clean the type.

### 3.4 — `ZapsView` / `ZapsDomain` are tests-only — wire to ingest or mark explicitly deferred

`nmp-nip57/src/domain.rs:14 ZapsDomain` declares `ingest_kinds = &[9735]`;
`view.rs:79 ZapsView` is a reactive aggregate. `grep` shows **no kernel ever
registers `ZapsDomain`** — every `ZapsView::open`/`on_event_inserted` call is
inside `#[cfg(test)]`. So the zap-receipt *display* half is also dormant. This
is not necessarily wrong (it is deferred), but it must be **labelled** deferred,
not left looking wired. It also downgrades §4.2's integrity concern from
"exploitable" to "latent" — see there.

---

## 4. Q3 — Most fragile / riskiest active design

### 4.1 — #1 RISK: shipped-but-inert features create a false sense of coverage

The single most dangerous *pattern* in NMP right now is not a data structure —
it is a **process failure that the codebase makes visible in three places at
once**:

- `ZapModule` (PR #98): registered, validated, 14 passing tests,
  wired into Chirp's `register_nip57_actions` — **and cannot complete a zap**
  because leg 2 is unbuilt. A reader of `git log` sees "feat(nip57): zaps,
  wired into Chirp" and reasonably believes zaps work.
- `action_results` (PR #94 + #99): "fix(ios): expose action_results array,"
  "fixed spinner-hang" — **and no host reads it.** The fix is real in the
  library, absent in every app.
- The 15 `nmp-nip29` `ActionModule` impls: typed, tested, **0–1 reachable.**

Each lands green, with a convincing changelog, and each is *inert*. The danger
is not any one of them — it is that **the project's verification (CI, tests)
confirms the library half and never the consumed-by-a-real-app half.** Reviews
#25, #27, #31, #32 all chased this and it keeps recurring because the test
suite cannot see it. The structural fix is a discipline rule, not a refactor:
**a feature is not "done" until one real app's UI exercises it end to end, and
that path is in CI.** Until then it is a scaffold and the commit message should
say "scaffold," not "feat … wired into Chirp."

This is a worse risk than the per-app-projection / bespoke-FFI concern prior
reviews led with, because that concern is *visible* (everyone sees the FFI) —
this one is *camouflaged by green CI.*

### 4.2 — Runner-up: NIP-57 receipt integrity hole (latent)

`nmp-nip57/src/decode.rs` decodes a kind:9735 receipt and extracts
`amount_msats` from the `bolt11` HRP (`:85-88`). NIP-57 Appendix F requires
verifying that the receipt's `description` tag (the embedded kind:9734)
actually hashes (SHA-256) to the value committed in the bolt11 invoice's
`description_hash`, and that the embedded request is internally consistent.
`grep -n 'sha256\|hash\|verify\|digest' decode.rs` → **nothing.** A malicious
relay can therefore inject fabricated kind:9735 receipts and any `ZapsView`
consuming them counts fake msats.

**Why this is runner-up, not #1:** `ZapsView`/`ZapsDomain` are tests-only
(§3.4) — no kernel ingests kind:9735 today. The hole is **latent, not
exploitable.** It must be closed **before** `ZapsDomain` is wired to a real
kernel, not after. Track it as a hard precondition on the zap-display work.

### 4.3 — The bespoke-FFI / per-app-projection concern: downgraded, correctly

`apps/chirp/nmp-app-chirp/src/ffi.rs` is 693 lines and still per-app, but it is
now **mostly composition, not logic**: it registers `ChirpModularTimeline`,
`register_chirp_actions`, `register_nip29_actions`, `register_nip57_actions`,
and exposes 4 thin C symbols. The action-registry seam (`register_action_module`
/ `register_action_executor`) is doing its job — NIP-29 and NIP-57 modules
reach the kernel without `nmp-core` learning their nouns (D0 holds). The
per-app FFI is no longer the #1 risk; §4.1 is.

---

## 5. Q4 — Is the `dispatch_action` seam working

**Yes, the seam itself is sound.** The round-trip and the executor pattern are
correct; the gaps are coverage, not design.

### Evidence the seam works

- `kernel/action_registry.rs`: `ActionRegistry::start` validates + mints a
  `correlation_id` (`:268`); `execute` (`:297`) runs the registered executor
  inside `catch_unwind` (`:311`) — D6-clean, a panicking host executor becomes
  `Err`, never an FFI unwind.
- The `correlation_id` round-trip is **closed for the publish family**: the
  `nmp.publish` executor threads the minted id onto `ActorCommand::PublishNote`
  / `PublishProfile` (`action_registry.rs:438,455`), and
  `preferred_action_id` binds the pre-signed `Publish` path to `event.id`.
  Test `publish_note_executor_threads_correlation_id_onto_actor_command`
  (`:629`) proves it.
- NIP-29 `join_request` and NIP-57 `nmp.zap` both drive real host-pinned
  `ActorCommand::PublishUnsignedEventToRelays` — `ffi.rs:332,434`. Tests
  `nip29_join_request_dispatches_through_action_registry` and the zap
  command tests pass.

### Race conditions: none in the registry

`execute` only does a non-blocking channel `send` (D8). The registry holds no
mutable kernel state. The `result_observer` slot is `Arc<Mutex<…>>` with `&self`
methods — registration and delivery never need `&mut`. No race.

### The real gaps (coverage, not design)

1. **`Cancel` is a no-op stub** — `action_registry.rs:461`,
   `PublishAction::Cancel { .. } => Ok(())`. The comment admits it: "No
   publish-engine cancel command yet." Meanwhile `nmp_app_cancel_publish` /
   `nmp_app_retry_publish` still exist as **bespoke C symbols**
   (`KernelBridge.swift:229,233`). Publish-lifecycle verbs are split across two
   doors. Either route `CancelPublish`/`RetryPublish` through `dispatch_action`
   *with a real executor*, or delete the dead `Cancel` arm so it does not
   masquerade as wired.
2. **The `action_results` push/pull outputs are unconsumed** (§1). The seam's
   *return value* (`correlation_id` from `dispatch_action`) is also dropped by
   every Chirp call site — `KernelBridge.swift:222-226,251-255` free the
   returned JSON and ignore it. That is *fine* for Chirp's fire-and-forget UX,
   but it means the seam's feedback half has **never been exercised by a real
   consumer.** The seam *can* carry feedback; nothing proves it *does*.

Verdict: the `dispatch_action` design is the healthiest part of the substrate.
Stop adding modules to it and start either consuming or deleting the ones
already registered.

---

## 6. Q5 — NIP-57 LNURL HTTP ownership: DECIDED

**Option A — host-injected `HttpCapability` via the `CapabilityModule` seam.**
This is not a close call. The other two options are wrong for concrete reasons.

### Why A, with evidence the substrate already exists

The `CapabilityModule` seam is **live and proven**:

- `crates/nmp-core/src/substrate/capability.rs` — `CapabilityModule` trait:
  `NAMESPACE`, `type Request`, `type Result`, `callback_interface_name()`.
- `crates/nmp-core/src/capability_socket.rs` — the FFI plumbing: the host
  registers **one** `extern "C"` callback (`CapabilityCallback`,
  `:15`); `dispatch_capability` (`:32`) routes a JSON `CapabilityRequest` to it
  and gets a `CapabilityEnvelope` back. Failures are **data, never panics**
  (`:58 capability_error_envelope`) — D6-clean.
- `crates/nmp-core/src/substrate/keyring.rs:30` — `KeyringCapability` is a
  real, shipping `CapabilityModule` impl. `KernelBridge.swift:70-76`
  (`registerCapabilityHandler`) is the iOS side. **The exact seam needed for
  LNURL HTTP is already carrying production keyring traffic.**

So an `HttpCapability` is not new architecture — it is a **second
`CapabilityModule`** alongside `KeyringCapability`. iOS supplies the
`URLSession` call behind the same `nmp_app_set_capability_callback` socket;
`nmp-nip57` owns the LNURL *protocol logic* (URL construction, the `?amount=&
nostr=` query, JSON parse of the lnurl-pay response, invoice extraction).

### Why not B (iOS does the HTTP, passes the invoice back)

B puts a protocol step on the wrong side of FFI. The kind:9734 POSTed to the
callback **is a signed Nostr event**; the lnurl endpoint is **named inside a
Nostr event** (the recipient's kind:0 `lud16`/`lud06`). Resolving and POSTing
it is protocol logic. B is structurally the `nmp_app_publish_note`
anti-pattern reviews #13–#19 spent six cycles unwinding — a per-app host owning
a substrate step. Doing it again for zaps is a regression, and §4.1 shows the
project cannot afford another inert-but-"wired" feature.

### Why not C (blocking Rust thread)

C violates D8 if the HTTP call stalls the actor. The `CapabilityModule` seam
already solves this correctly: the request goes out async through the socket,
the host's `URLSession` does the blocking I/O off the actor thread, the
envelope comes back as data. C reinvents — badly — what A already has.

### The one design note for A

`ZapModule`'s executor signature **will change** and that is fine: it currently
ends at `PublishUnsignedEventToRelays` (publish the kind:9734 to relays). With
`HttpCapability` it must instead (a) build+sign the kind:9734, (b) issue an
`HttpCapability` request to the lnurl callback, (c) on the returned bolt11,
hand off to the wallet (`WalletPayInvoice` — a *separate* action; the executor
must NOT try to own payment). This is a multi-step async action — exactly what
the `correlation_id` + capability-envelope correlation machinery is for. Write
the ADR before touching `ZapModule` so the executor is built once, against the
right seam.

---

## 7. Q6 — The single highest-ROI next PR

**Build `HttpCapability` as a `CapabilityModule` and wire `ZapModule`'s
executor through it so a zap actually completes.**

It wins on every axis a freeze-deadline PR should:

1. **Substrate exists** — `CapabilityModule` + `capability_socket.rs` are
   proven by `KeyringCapability`. This is a second impl of a live pattern, not
   new architecture. Low risk.
2. **It turns a shipped-but-inert feature live.** PR #98 registered
   `ZapModule` into Chirp; today it produces nothing payable. One capability
   closes the gap between "feat: zaps, wired into Chirp" and zaps that work —
   directly attacking the §4.1 #1 risk (camouflaged-inert features).
3. **Wrong answer is expensive later.** If the `ZapModule` executor is left
   assuming iOS does the HTTP (Option B), its signature is wrong and gets
   redone. Deciding A *now* and building the executor against it once is the
   cheap path.
4. **It exercises the feedback half of the seam.** A zap is a genuinely
   multi-step async action (build → lnurl HTTP → invoice → wallet). Wiring it
   forces the `correlation_id` + capability-envelope round-trip to carry a real
   workload — the first time anything has.

**Done when:** an ADR records the `HttpCapability` `CapabilityModule`; an
`HttpCapability` trait + FFI registration exists alongside `KeyringCapability`;
`ZapModule`'s executor issues the lnurl-pay GET+POST through it and yields a
bolt11 invoice; an end-to-end test (host-side HTTP stub) proves a zap request
produces a payable invoice. **Precondition for the *display* follow-up:**
close the `decode.rs` description-hash verification gap (§4.2) before
`ZapsDomain` is ever registered with a real kernel.

Runner-up (if zaps are deemed out of scope for the freeze): **delete
`last_action_result` and the 15 dormant `nmp-nip29` `ActionModule` impls** —
pure subtraction, zero consumer breaks, removes the §3.3/§4.1 debt. Lower
upside than the zap PR but zero risk.

---

## 8. Summary

The substrate is structurally sound — `dispatch_action` is the healthiest seam
in the codebase, D0 holds at the `KernelSnapshot` boundary, and the
`CapabilityModule` socket is live and proven. The danger has shifted from
*structure* to *process*: three separate features (`ZapModule`,
`action_results`, the 15 NIP-29 modules) are **shipped, green-CI, "wired" —
and inert**, because the test suite verifies the library half and never the
consumed-by-a-real-app half. Review #32's "spinner-hang" #1 priority was
wrong: Chirp has no `correlation_id` spinner at all; its publish UX runs on
`publish_outbox`, so `action_results` is dormant-correct, not buggy. The
highest-ROI move is to build the `HttpCapability` `CapabilityModule` (second
impl of the proven keyring seam) and route `ZapModule`'s executor through it —
turning a dead feature live, against the right seam, before the executor
signature ossifies. Close the `decode.rs` receipt-hash verification gap before
`ZapsDomain` is ever wired. And stop calling WASM a build target until a
`wasm_bindgen` surface actually exists.

# Opus Direction Review #30

Date: 2026-05-21
Scope: review #30 in the running series. Code read, not docs. Five questions.

## TL;DR

The substrate seam is structurally sound and has live consumers, but it
carries a large amount of dead type machinery that the in-flight cleanup PRs
correctly target. Two findings worth acting on beyond what's already queued:

1. `ActionPlan` is not just `initial_step`-dead — for the built-in dispatch
   path **all three fields** (`initial_step`, `initial_status`, `deadline_ms`)
   are computed and then discarded. The host-validator return type should
   collapse to `Result<(), ActionRejection>` once `reduce()`/`type Step` go.
2. The review #29 #1 priority (per-tick `Vec` for `last_action_result`)
   requires a **coordinated Swift change**. The iOS shell currently decodes
   `lastActionResult` as a scalar `LastActionResult?`. The Rust PR alone
   leaves the host on the old shape.

NIP-59 is clean — no D0 violation, no MLS leakage into the kernel snapshot.
Chirp's "thin-shell" LoC concern from earlier reviews is malformed and should
be retired (see Q3).

---

## Q1 — ActionPlan simplification

`ActionModule::start` returns `ActionPlan<Step>` with three fields
(`crates/nmp-core/src/substrate/action.rs:44-49`). Tracing every field:

- **`initial_step`** — review #29 confirmed dead. Verified again: the only
  production read is the adapter re-pack at
  `action_registry.rs:95` (`serde_json::to_value(&plan.initial_step)`), which
  feeds the erased `ActionPlan<Value>` that `dispatch_action_json` binds to
  `_plan` and drops (`ffi/action.rs:373`, `383-385`).
- **`initial_status`** — same fate. Repacked at `action_registry.rs:96`,
  then dropped with the rest of `_plan` at `ffi/action.rs:373`. It is **never
  surfaced in the dispatch response**. (See Q4.)
- **`deadline_ms`** — same fate. Repacked at `action_registry.rs:97`, dropped
  at `ffi/action.rs:373`.

The one place that *writes* a meaningful `deadline_ms` is
`nmp-nip77/src/run_sync.rs:70` (`deadline_ms: action.deadline_ms`). That is a
**write into the plan**, not a read of it — the value is set by `RunSync::start`
and then thrown away one frame later by `dispatch_action_json`. nip77 does not
read the plan back. There is no deadline enforcement anywhere.

**Where the fields are *not* dead:** `fixture-todo-core/src/lib.rs:121-124`
and `ffi/action.rs::default_pending_plan()` both *construct* an `ActionPlan`
to satisfy the host-validator return shape (`ValidatorFn` →
`Result<ActionPlan<Value>, ActionRejection>`, `action_registry.rs:121-122`).
But those constructed values follow the exact same path: into `_plan`, then
discarded. So the fields are "consumed" only in the sense that a return type
forces every validator to produce them.

**Conclusion.** This is sharper than "`initial_step` is dead." For the
built-in dispatch path, `ActionPlan` carries **zero** information that ever
reaches the host or the actor. The type survives only because it is the
declared return shape of `ActionModule::start` and the host-validator closure.
Once the in-flight `delete dead ActionModule::reduce` PR removes `type Step`,
the next PR should:

- Collapse `ActionModule::start` to `Result<(), ActionRejection>` (accept /
  reject is the entire signal it actually produces).
- Collapse `ValidatorFn` to `Box<dyn Fn(&str) -> Result<(), ActionRejection>>`.
- Delete `ActionPlan`, `ActionStatus` (the enum is otherwise unused once
  `reduce` is gone — `ActionStatus` appears only inside `ActionPlan`,
  `ActionInput`, `ActionTransition`), and `default_pending_plan()`.

If a future M6 action ledger needs a status, it should be reintroduced as a
real persisted record, not as a return value that is born dead.

---

## Q2 — nmp-nip59 MLS coupling

**No D0 violation. No MLS state in the kernel snapshot.** The review prompt
named a `WelcomeWrapModule`; the actual type is `WelcomeUnwrapModule`
(`crates/nmp-nip59/src/domain/welcome_unwrap.rs:45`).

What `nmp-nip59` contains:

- `gift_wrap` / `unwrap_gift_wrap` (`wrap.rs`) — thin synchronous wrappers
  over `nostr::EventBuilder::gift_wrap` and
  `nostr::nips::nip59::UnwrappedGift::from_gift_wrap`. This correctly follows
  the "use rust-nostr, not scratch crypto" memory rule.
- `WelcomeUnwrapModule` — a `DomainModule` (not an `ActionModule`) declaring
  `ingest_kinds() = &[1059]` and a `WelcomeRecord` shape. It is a **declaration
  only**: its `migrations()` and `indexes()` both return empty `Vec`s. The
  docstring is explicit that the actual decrypt + MDK dispatch happens in the
  actor layer, not here.

Is it wired as an action module? **No** — and correctly so. The crate's own
`lib.rs:23-24` states there is "no Marmot-specific ActionModule here"; the
Marmot Welcome path consumes the free functions directly from
`nmp-marmot::service`.

Does any `KernelSnapshot` field carry MLS/nip59 state? **No.** Verified the
full `KernelSnapshot` struct (`crates/nmp-core/src/kernel/types.rs:506-608`):
the typed fields are `rev`, `schema_version`, `last_tick_ms`, `update_kind`,
`running`, `metrics`, `relay_status(es)`, `logical_interests`,
`wire_subscriptions`, `logs`, `last_error_toast`, `last_error_category`,
`last_planner_error`, and the `projections` map. No `mls`, `marmot`,
`welcome`, or `nip59` typed field, and no built-in projection key for them
(the reserved keys are the publish cluster, identity pair, views cluster).

Marmot/MLS state reaches the iOS host through an **entirely separate** C-ABI
surface — `nmp_app_chirp_marmot_*` — consumed by `MarmotStore` in
`ios/Chirp/Chirp/Bridge/MarmotBridge.swift`. That is a deliberate, clean
separation: MLS group state is not multiplexed into the protocol-neutral
kernel snapshot.

`WelcomeUnwrapModule`'s residual concern: it is a `DomainModule` whose only
job today is to *declare* `ingest_kinds = &[1059]`. If the kernel dispatch
table genuinely routes kind:1059 by reading that declaration, it is live; if
the routing is hardcoded elsewhere, this is another dormant `*Module` impl in
the family review #19/#27 flagged. That is a follow-up to verify, not a
blocker — it is small and contained.

---

## Q3 — Chirp LoC audit

`find ios/Chirp -name "*.swift" | xargs wc -l` → **8,816 total across 37
files** (includes `ChirpTests` 270 + `ChirpUITests` 426 = 696 LoC of tests).
Production Swift is ~8,120 LoC.

Top 5 production files:

| File | LoC | Character |
|---|---|---|
| `Bridge/KernelBridge.swift` | 825 | Thin FFI: C-symbol wrappers + snapshot DTO decode. No protocol logic. |
| `Features/DiagnosticsView.swift` | 485 | Pure SwiftUI render — `List`/`Section`s reading `model.*`. |
| `Bridge/MarmotBridge.swift` | 434 | Thin FFI: DTO decode + op-envelope dispatch. No protocol logic. |
| `Features/RelayDetailView.swift` | 428 | SwiftUI render. |
| `Bridge/KernelModel.swift` | 404 | `@Observable` store applying snapshots. |

**The "~300 LoC thin shell" target from earlier reviews is malformed and
should be retired.** A shell with a real UI — timeline, profile, relay
settings, diagnostics, wallet, Marmot group chat — cannot be 300 LoC of
SwiftUI. The thin-shell rule's *substance* (ZERO protocol logic, only C-ABI
delegation) is the right test, and on that test Chirp passes:

- `KernelBridge.swift` and `MarmotBridge.swift` are delegation: every method
  is `withCString` → `nmp_app_*` → free the returned pointer → decode JSON.
  The only branching is D6 resilience (nil pointer → empty state). No NIP
  encoding, no event construction, no signing.
- `DiagnosticsView.swift` (sampled in full) is `List { Section { HStack {
  Text } } }` reading `model.isRunning`, `model.rev`, etc. Zero logic.
- `publishProfile` (`KernelBridge.swift:183-196`) builds a kind:0 unsigned
  event dictionary in Swift. This is the **one** mild smell — the shell knows
  "kind 0 = profile" and assembles the tag/content envelope. It is borderline:
  arguably the kernel should expose a `publishProfile(json)` action so the
  shell never names a kind. Not urgent, but it is the single place Chirp
  encodes protocol knowledge.

**Recommendation:** stop measuring Chirp by raw LoC. Measure it by "does any
file construct a Nostr event, name a kind, or make a protocol decision." Today
that count is one (`publishProfile`). Convert it to a `dispatch_action`
namespace and the shell is fully clean.

---

## Q4 — dispatch_action JSON binding

`dispatch_action_json` (`ffi/action.rs:366-414`) does exactly this with the
`ActionPlan` from `ActionRegistry::start()`:

```rust
match app.action_registry.start(&mut ctx, namespace, action_json) {
    Ok((correlation_id, _plan)) => {   // line 373 — plan bound to _plan
        // `_plan` (the `ActionPlan`) is intentionally dropped
        ...
        match execute_action(app, namespace, action_json, &correlation_id) {
            Ok(()) => {
                ...
                format!(r#"{{"correlation_id":{}}}"#, json_string(&correlation_id))
```

**The `ActionPlan` is bound to `_plan` and dropped on line 373.** It never
reaches `execute_action` and never reaches the response.

Is `initial_status` surfaced in the dispatch response? **No.** The success
response is exactly `{"correlation_id":"<hex>"}` — one field. For a
`PublishNote` dispatch the host sees only the 32-hex correlation id; for a
pre-signed `Publish` it sees the event id as the correlation id
(`preferred_action_id`, PR #86). The host learns nothing about whether the
action is `Pending` vs `Running` vs `Cancelled` from the dispatch return —
even though `start()` computed exactly that and threw it away.

This is mostly benign (the snapshot's `last_action_result` projection carries
the eventual terminal verdict), but it confirms the Q1 conclusion: the
`initial_status` the modules carefully set — `Cancelled` for
`PublishAction::Cancel`, `Pending` for everything else — is pure dead
computation on the built-in path.

The result-observer push (`deliver_result`, `ffi/action.rs:400-403`) also
carries only `correlation_id` + `result_json: null` — not the status. So
**no path** out of `dispatch_action` surfaces `initial_status`.

---

## Q5 — what's genuinely missing from NMP

NIP crates present: `nip01, nip22, nip23, nip29, nip42, nip42-types, nip57,
nip59, nip77`. Plus `nmp-nwc` (NIP-47) and `nmp-reactions` (NIP-25/18).

Mapped against the candidate list:

| NIP | Status |
|---|---|
| NIP-42 (relay AUTH) | **Live.** `nmp-nip42` is wired deep into `nmp-core` — `subs/auth_gate.rs`, `kernel/auth.rs`, `kernel/requests/auth_gate.rs`, fail-closed tests. This is a finished feature. |
| NIP-57 (zaps) | **Partial infrastructure, NOT surfaced.** `nmp-nip57` has `bolt11.rs`, `decode.rs`, `build.rs`, `domain.rs`, `view.rs`, `kinds.rs` — substantial code — but **no `ActionModule`, no executor registration, no Chirp surface.** Outside its own crate it is imported only by `nmp-reactions` and `nmp-testing`. It is a built-but-unwired crate. |
| NIP-17 (encrypted DMs) | **No crate.** But the hard part — NIP-59 gift-wrap — already exists and is wired (kind:1059 ingest, `gift_wrap`/`unwrap_gift_wrap`). NIP-17 is gift-wrapped kind:14 chat. Highest leverage-to-effort ratio. |
| NIP-28 (public channels) | **No crate, no infrastructure.** |
| NIP-96 (HTTP file upload) | **No crate, no infrastructure.** Purely additive; no protocol-state coupling. |

**Direct answer to "which has partial infrastructure not surfaced":**
`nmp-nip57`. It is the cleanest example in the codebase of a crate that was
built and then never connected to a dispatch namespace or a host.

**Highest-ROI addition:** **NIP-17 encrypted DMs.** Rationale:

1. A social app without DMs is incomplete in a way a power user notices
   immediately — this is the most user-visible gap.
2. The cryptographic substrate (NIP-59 gift-wrap, kind:1059 ingest pipeline,
   `WelcomeUnwrapModule`'s routing pattern) is **already built and wired**.
   NIP-17 is "gift-wrap a kind:14, route inbound kind:1059 to a DM domain
   module instead of / alongside the Marmot one."
3. It exercises the substrate seams the project has spent reviews #13–#28
   building — a new `ActionModule` for "send DM," a new snapshot projection
   for "conversations" — giving the seam a *second non-Marmot consumer* and
   answering the recurring "the substrate is consumer-starved" finding.

NIP-57 (zaps) is the second pick: the crate exists, so wiring it is an
afternoon of `ActionModule` + projection work, and it would prove the
"built-crate → live-feature" path the project keeps half-finishing.

NIP-96 is a distant third — useful, but it is an HTTP concern with no Nostr
protocol-state coupling, so it teaches the architecture nothing.

---

## Cross-cutting note: the in-flight `last_action_result` Vec PR

Review #29's #1 priority — make `last_action_result` a per-tick `Vec` so two
actions settling in one tick don't shadow each other — is correct and the
in-flight `pending_terminals: Vec<LastTerminal>` PR addresses the Rust side.

**Flag for the orchestrator:** the iOS shell is not ready for the new shape.
`ios/Chirp/Chirp/Bridge/KernelBridge.swift` decodes
`projections["last_action_result"]` as a **scalar** `LastActionResult?`
(`KernelBridge.swift:508`, `571`, `666-670`). If the Rust PR changes the
projection to `projections["action_results"]: [...]`, the Swift `Decodable`
will silently get `nil` (key renamed) and the spinner bug the PR fixes will
*persist on iOS* — the symptom just moves from "wrong result shown" to "no
result ever shown." This needs a coordinated `KernelBridge.swift` change in
the same merge, decoding an array and folding it into per-correlation-id
state. Land them together or the fix is invisible on the only real host.

---

## Recommendations, ranked

1. **Coordinate the `last_action_result` Vec PR with a Swift change.** The
   Rust-only PR regresses iOS. Highest priority — it is an in-flight PR about
   to break the only shipping host.
2. **After `delete reduce()` lands, collapse `ActionModule::start` to
   `Result<(), ActionRejection>` and delete `ActionPlan`/`ActionStatus`.**
   Q1+Q4 prove every byte of `ActionPlan` is dead on the dispatch path. Don't
   leave a born-dead return type.
3. **Wire NIP-17 DMs.** The substrate seam needs a second non-Marmot
   consumer; the crypto substrate (NIP-59) is already built and wired; DMs are
   the most user-visible missing feature. This is the highest-ROI feature
   work.
4. **Retire the "~300 LoC Chirp" target.** Replace it with a real test:
   "no Chirp file constructs a Nostr event or names a kind." Today the count
   is one (`publishProfile`); convert it to a `dispatch_action` namespace.
5. **Verify `WelcomeUnwrapModule`'s `ingest_kinds` is actually read** by the
   kernel dispatch table. If routing is hardcoded, it joins the dormant
   `*Module` family; if read, it is live. Small, contained follow-up.

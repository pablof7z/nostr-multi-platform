# Opus Direction Review #29 — NMP architecture

Date: 2026-05-21
Scope: Post-PR-#91; ongoing FSM/ViewModule/D0 audit

Method note: this review reads the code as it stands on `master` at PR #92.
The two PRs the spawning brief described as "currently open" do NOT exist in
that state. `gh pr list --state all` shows PR #92 is `docs(perf): architectural
polish scan` — a scan, not the `ActionTransition`/`reduce()` deletion. There is
no open or closed PR deleting `ActionModule::reduce()` / `type Output` /
`ActionInput` / `ActionTransition`. Treat that deletion as **not yet started**,
not "merged" — `crates/nmp-core/src/substrate/action.rs` still defines all four.

## Q1 — chirp.react/follow/unfollow registration (false positive correction)

**Confirmed: review #28's correction was right. The three social verbs ARE
registered.** Review #28's *original* claim (that they were unregistered) was
the false positive; #28 then corrected itself. This review re-verifies the
correction holds.

Evidence in `apps/chirp/nmp-app-chirp/src/ffi.rs`:

- `nmp_app_chirp_register` (line 98) calls `register_chirp_actions(&mut *app)`
  at line 115 — before it takes the shared `&NmpApp` borrow, exactly as a
  `&mut NmpApp` registration must.
- `register_chirp_actions` (line 261) registers **both halves** for each of the
  three namespaces:
  - `chirp.react` — `register_action_module` (line 263) + `register_action_executor`
    (line 268). Executor sends `ActorCommand::React`.
  - `chirp.follow` — `register_action_module` (line 279) + `register_action_executor`
    (line 284). Executor sends `ActorCommand::Follow`.
  - `chirp.unfollow` — `register_action_module` (line 292) + `register_action_executor`
    (line 297). Executor sends `ActorCommand::Unfollow`.
- The registration is exercised end-to-end by the test
  `social_verbs_dispatch_through_action_registry` (line 507): it dispatches all
  three through `nmp_app_dispatch_action` and asserts each returns a 32-hex
  `correlation_id`, proving both the module validator (`start()`) and the
  executor (`execute()`) are wired.

Note the registration uses `register_action_module` (the host-validator seam,
which routes to `ActionRegistry::register_with_validator` → `ClosureModule`),
NOT `ActionRegistry::register::<M>()`. So Chirp's verbs are *host-closure*
modules, not compile-time `ActionModule` types. That is the intended D0-clean
path: the social verbs live in the app crate, never in `nmp-core`.

The root cause of #28's original false positive: the Opus agent searched
`crates/` and missed `apps/chirp/nmp-app-chirp/`. Lesson for future reviews —
**`apps/` is a first-class source root; always grep it.**

## Q2 — ViewModule: dead or live?

`ViewModule` (`crates/nmp-core/src/substrate/view.rs`, line 89) defines **8
methods**: `key`, `dependencies`, `open`, `on_event_inserted`,
`on_event_removed`, `on_event_replaced`, `on_projection_changed`, `on_tick`
(default `None`), and `snapshot`. Plus the `NAMESPACE` const and 5 associated
types.

The honest answer is **not "dead" and not "live" — it is a half-used
abstraction**. Three distinct facts:

1. **No `ViewRegistry` exists anywhere in the workspace.** `grep -rn
   "ViewRegistry"` returns zero hits. There is no `dyn ViewModule`, no erased
   facade, no dispatch table. Contrast with `ActionModule`, which has a real
   runtime (`ActionRegistry` + `ErasedActionModule`). `ViewModule` has no
   runtime at all.

2. **Exactly ONE of the ~20 `ViewModule` impls is driven by production code:
   `Nip10ModularTimelineView`.** It is consumed by `ModularTimelineProjection`
   in `crates/nmp-nip01/src/timeline_projection.rs` — a hand-rolled wrapper
   that is registered as a `KernelEventObserver` by Chirp's `ffi.rs` (line
   143). The wrapper calls:
   - `Nip10ModularTimelineView::open(...)` — `new()`, line 72 (non-test)
   - `Nip10ModularTimelineView::snapshot(...)` — `snapshot()`, line 87 (non-test)
   - `Nip10ModularTimelineView::on_event_inserted(...)` — `on_kernel_event()`,
     line 104 (non-test)

   This is **concrete-type dispatch**: the wrapper names the type directly. It
   does not use the `ViewModule` trait as a trait — it could call the same
   three functions if they were inherent methods on a plain struct. The trait
   bound buys nothing here.

3. **The other ~19 `ViewModule` impls are test-only.** Every other
   `impl ViewModule for …` (in `nmp-reactions`, `nmp-nip29` ×5, `nmp-nip57`,
   `nmp-marmot`, `nmp-content`, `nmp-nip23` ×2, `nmp-nip22` ×2, `nmp-nip01`
   `RepliesView`/`ThreadView`, `nmp-core/publish`) is only ever called from
   `#[cfg(test)]` modules. They drive zero runtime behavior. They are not even
   wrapped in a bespoke projection like `Nip10ModularTimelineView` is — they
   are pure compile-checked specifications with test coverage and no consumer.

**Conclusion:** the `ViewModule` *trait abstraction* is dead — no registry, no
trait-object dispatch, no polymorphic consumer. Three of its 8 methods
(`on_event_removed`, `on_event_replaced`, `on_projection_changed`, `on_tick`)
are not called by *any* production code, including the one live wrapper —
`ModularTimelineProjection` only ever calls `open`/`snapshot`/`on_event_inserted`.
The trait survives as a shared *shape contract* that 20 impls conform to, of
which one impl's three methods are reachable at runtime via direct calls.

This is the same pathology review #19 named (dormant substrate traits) — but
narrower than "20 implementations drive zero runtime behavior." One impl drives
behavior; the *trait* drives none.

## Q3 — ActionPlan.initial_step: is it ever read?

**No. `ActionPlan.initial_step` is dead at the only FFI call site, and the
whole `ActionPlan` is dropped there.**

Trace:

1. `ErasedActionModule::start` (`action_registry.rs` line 65) returns
   `(Option<ActionId>, ActionPlan<Value>)`. `ActionModuleAdapter::start` (line
   84) builds the `ActionPlan` by `serde_json::to_value(&plan.initial_step)` —
   so `initial_step` *is* erased and carried this far.
2. `ActionRegistry::start` (line 274) returns `(ActionId, ActionPlan<Value>)`.
3. The **only** caller of `ActionRegistry::start` outside tests is
   `dispatch_action_json` in `crates/nmp-core/src/ffi/action.rs` line 373:
   ```rust
   Ok((correlation_id, _plan)) => {
       // `_plan` (the `ActionPlan`) is intentionally dropped: plan
       // persistence is the M6 action ledger's job (a follow-up).
   ```
   The plan is bound to `_plan` and never read. `initial_step`,
   `initial_status`, and `deadline_ms` are all discarded. Only
   `correlation_id` is used.

So the full path is: `ActionModule::start` produces a typed
`ActionPlan<Step>` → adapter erases `Step` to `Value` → registry returns it →
FFI drops it on the floor. **`initial_step` is computed, JSON-serialized, and
immediately discarded on every dispatch.** The `Step` associated type, the
`ActionPlan` struct's `initial_step` field, and the per-module
`fire_and_forget_plan()` / `default_pending_plan()` helpers that populate it
are all dead weight from the consumer's point of view.

`initial_status` and `deadline_ms` are *also* dropped here — equally dead at
runtime. The only field of `ActionPlan` that matters is none of them; the
registry's separate `correlation_id` minting is what the host acts on.

This corroborates the standing concern: `type Step` is "semi-live" only in the
sense that it is *typed and serialized*; its *value* is never observed by
anything. `ActionTransition` / `reduce()` are worse — they have no caller at
all (`ErasedActionModule` exposes only `start()`, never `reduce()`).

## Q4 — nmp-reactions D0 violation: real or overstated?

**Overstated — and now moot. The `SocialRecord` / `SocialKind` types do not
exist.** `grep -rn "SocialRecord\|SocialKind"` across the whole workspace
returns zero hits.

The crate `nmp-reactions` (`crates/nmp-reactions/src/lib.rs`, internally named
`nmp-relations`) is structured around **protocol-accurate nouns**, not app
nouns:

- `ReactionRecord` / `ReactionKind` / `ReactionTarget` — NIP-25 (kind:7).
- `Repost` / `GenericRepost` — NIP-18 (kind:6 / kind:16).
- `ReactionsDomain` — composite reverse indexes keyed by event id; `kinds`
  `[7, 6, 16]`.
- `ReactionSummaryView` / `RepostsView` — `ViewModule` impls (test-only per Q2).

These are NIP-protocol record types, exactly what a protocol crate *should*
own. "Reaction" is a NIP-25 noun, not an app noun — kind:7 is in the NIP. A D0
violation would be `nmp-reactions` naming a *Chirp* concept ("timeline card",
"feed", "follow button"). It does not. The `lib.rs` header note even records
that the old `register(&mut ModuleRegistry)` entry point was deleted because
`ModuleRegistry` was a dead string-collector — the crate was already cleaned of
the one structural seam that was wrong.

The prior-review concern (`project_d0_structural_violations.md`: "nmp-reactions
(SocialRecord/SocialKind) … D0 violations needing cross-crate structural
moves") refers to types that have since been renamed/removed. **This concern is
resolved. It should be struck from the standing-concerns list.** The remaining
half of that memo — `nmp-nip59`'s `WelcomeWrapModule`/MLS coupling — was not
re-examined here and may still stand; it needs its own check.

## Q5 — Next highest-ROI architectural move

**Fix `last_action_result` from a single sticky slot to a per-tick `Vec`.**
File: `crates/nmp-core/src/kernel/publish_engine.rs`,
`last_action_result_projection()` (line 437), backed by
`publish_engine.last_terminal()`.

Why this beats the alternatives:

- It is a **real, user-visible correctness bug**, not dead-code cleanup. The
  projection calls `publish_engine.last_terminal()` — singular. The kernel test
  `last_action_result_is_overwritten_by_the_most_recent_terminal`
  (`publish_terminal_status_tests.rs` line 602) *documents the bug as intended
  behavior*: when two actions settle between snapshot ticks, the first
  terminal verdict is silently overwritten and lost. A host that dispatched two
  publishes and is spinning on both correlation_ids will see only one clear —
  the other spinner hangs forever. Reviews #24 and #25 both flagged this;
  #25 was explicit: "last_action_result scalar is BROKEN (must be Vec per
  tick)." It is still scalar.
- The fix is **small and contained**: `publish_engine` already has
  `take_completed()` (line 467, `apply_engine_completions`) which drains *all*
  completions since the last drain. The terminal-result path just needs to
  accumulate those into a `Vec<ActionResult>` drained per snapshot tick,
  instead of keeping `last_terminal()` as a single overwritten field. The wire
  shape changes from `last_action_result: {…}|null` to
  `action_results: [{…}, …]` (empty array when nothing settled). Blast radius
  is one projection function, one engine accessor, and the iOS host's
  result-consumption code — all already touching this exact key.
- The two cleanup candidates are lower ROI:
  - Deleting `ActionModule::reduce()` / `type Output` / `ActionInput` /
    `ActionTransition` (Q3 confirms `reduce` has zero callable surface and
    `ActionTransition` is never constructed by runtime code) is correct and
    should still happen — but it changes **no runtime behavior**. It removes
    confusion, not a bug. Do it second.
  - Deleting the 19 dormant `ViewModule` impls + collapsing the trait (Q2) is
    also correct eventually, but it is a 20-file churn for zero behavior
    change, and it requires first deciding whether `Nip10ModularTimelineView`
    keeps the trait or becomes a plain struct. Do it third, as deliberate
    scope, not now.

`last_action_result → Vec` is the only candidate of the three that fixes a
behavior a user can observe (a stuck spinner). Ship that next.

---

### Standing-concerns ledger update

- **Resolved:** nmp-reactions `SocialRecord`/`SocialKind` D0 violation — the
  types do not exist; the crate owns NIP nouns correctly (Q4).
- **Still open:** `ActionTransition` FSM has no callable surface (Q3) — the
  deletion PR described in the brief was never opened; recommend opening it
  *after* Q5.
- **Still open:** `ViewModule` trait abstraction is unused — no `ViewRegistry`,
  no `dyn ViewModule`; 1/20 impls live via concrete dispatch (Q2).
- **Still open and now ranked #1:** `last_action_result` is a single
  overwritten slot; concurrent terminal verdicts are lost (Q5).
- **Not re-examined:** `nmp-nip59` `WelcomeWrapModule`/MLS coupling — needs its
  own audit; do not assume resolved.

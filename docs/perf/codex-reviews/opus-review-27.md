# Opus Direction Review #27 — 2026-05-21

Reviewer: Opus 4.7. Scope: architecture state after PRs #85–#88 merged to master,
#89 still in CI. Verified against the tree, not the summary.

## TL;DR

The substrate seam (`dispatch_action` + projections + ActionResult) is now
*mechanically complete* but **consumer-starved**: after 27 reviews and ~89 PRs,
the live consumers of the action seam are exactly **two** — the built-in
`PublishModule` and the `fixture-todo-core` test double. Every other
`ActionModule` impl (NIP-29 ×4, Marmot ×1, NIP-77 ×1) is dormant — compiled,
tested, never registered. The "first NIP-29 action wired" claimed in reviews
#22–#23 is **not on master**: `nmp-app-chirp` does not exist as a crate, and
no live caller of `register_action_module`/`register_executor` for a NIP-29
namespace exists outside `fixture-todo-core` and unit tests.

The riskiest unresolved bet is unchanged from #19/#25 and is now overdue for a
verdict: **the project keeps building extensibility surface and never retires
any.** Each review finds the same three problems (Mutex sprawl, dormant traits,
Chirp LOC) and recommends the same shape ("wire one real consumer / delete one
dead trait"). That is not an architecture problem anymore — it is an execution
discipline problem. This review recommends a **hard moratorium on new seams**
and a single large delete sweep.

## What actually landed (verified)

- **PR #85** — confirmed. `ActionRegistry.modules`/`executors` are
  `HashMap<String, …>`; no `Box::leak`. `SnapshotRegistry` replace-by-key.
  `nmp.*` namespace guard present in `nmp_app_register_action_executor` /
  `nmp_app_register_action_module` (action.rs:158, :244).
- **PR #86** — confirmed. `ActionModule::preferred_action_id()` exists
  (action.rs:33); `PublishModule` returns `event.id` for pre-signed `Publish`.
  Verified by `dispatch_publish_action_returns_event_id_as_correlation_id`.
- **PR #87** — confirmed. `relay_url`/`test_npub` gone from `KernelSnapshot`.
- **PR #88** — confirmed. Views cluster (`profile`, `timeline`, `author_view`,
  `thread_view`, `inserted`/`updated`/`removed`) is in `projections`, no typed
  fields. `KernelSnapshot` is now down to ~12 typed fields + the projections map.
- **PR #89** — **NOT on master.** Branch `358d7c34` / `d4d2c7c0` exists but is
  unmerged. Master's `ActorCommand::PublishNote { content, reply_to_id }`
  (actor/mod.rs:219) has **no `correlation_id` field**. The round-trip gap for
  `PublishNote` is fully open — see below.

## Direction assessment

### Riskiest unresolved bet

**The substrate is a one-app framework wearing two-app clothing.** The seam
exists, but its only non-test exercise is `nmp.publish` — a *built-in*, wired in
`default_registry()`, not through the host-registration path. `fixture-todo-core`
exercises `register_action_module` + `register_executor` + `register_snapshot_projection`
end-to-end, which is genuinely valuable — but it is a **hollow test double**: its
executor is a `Vec<TodoRecord>` mutation behind an `Arc<Mutex>`, it issues no
`ActorCommand` (`_send` is unused), it has no relay traffic, no persistence, no
async step, no capability await. It proves the *registration plumbing* compiles
and dispatches; it proves **nothing about whether the seam can carry a real
protocol action** (signing, relay-pinned publish, multi-step reduce, capability
await). The 14-variant `ActionTransition` enum (`AwaitCapability`,
`AwaitUserApproval`, `Continue`, `ResumedAfterRestart`) has **zero live
exercise** — `reduce()` is never called by any runtime path. The registry only
ever calls `start()`. The entire step-machine half of `ActionModule` is dead.

So the bet is: *the action seam generalizes to real protocol actions.* It has
not been tested against one. The NIP-29 join request — the smallest real
candidate — was claimed wired and is not.

### Is the substrate converging or fragmenting?

**Converging in shape, fragmenting in semantics.** Three specific fractures:

1. **Two correlation-id namespaces that cannot be unified.** `dispatch_action`
   returns a minted 32-hex id for `PublishNote` (event.id unknown at `start()`),
   but `last_action_result.correlation_id` is the publish engine's
   `PublishHandle == event_id` (publish/engine.rs:96). For `PublishNote` these
   are **different strings forever** — a host that keys a spinner on the
   `dispatch_action` return value will *never* see a matching
   `last_action_result`. PR #86 fixed this for pre-signed `Publish` only.
   PR #89 (unmerged) threads the minted id through `ActorCommand::PublishNote`
   so the engine adopts it — that is the correct fix and **must land**.

2. **`last_action_result` is scalar and lossy — review #25's concern is
   real and unfixed.** `publish_engine.rs:420` reads `last_terminal()`, a
   single `Option<LastTerminal>` that is *overwritten* on every terminal
   (engine.rs:350, in a loop over `take_completed() -> Vec<TerminalOutcome>`).
   When two actions settle inside one snapshot tick, the projection reports
   only the last; the other verdict is **silently lost**. The push
   `ActionResult` observer does not cover this either — it fires at
   *enqueue* time, not *terminal* time (it carries `result_json: null`
   always). So there is **no reliable terminal-result path for the second of
   two same-tick actions** through either the pull or the push channel. This
   must become a `Vec` drained per tick, or `last_terminal` a queue.

3. **Push vs pull never reconciled.** `ActionResult` (push) means "accepted
   and enqueued." `last_action_result` (pull) means "most recent terminal."
   These are *different events about different moments* sharing the word
   "result." A host wiring both gets one signal at enqueue (null payload) and
   a sticky scalar at terminal. There is no single "this action's lifecycle"
   stream. This is fine for `nmp.publish` (one consumer, knows the quirk) but
   will not survive a second real action type without a host writing bespoke
   reconciliation — which is the D0 violation (app logic) the seam was built
   to prevent.

### The pattern NMP should stop doing entirely

**Stop landing extensibility seams and dormant `*Module` traits.** The
substrate now has five module traits — `ViewModule`, `ActionModule`,
`DomainModule`, `CapabilityModule`, `IdentityModule`. Live-consumer count:

- `ActionModule` — 1 live (`PublishModule`), 6 dormant impls.
- `ViewModule` — **0 live.** ~14 impls across nip01/22/23/29/57/reactions/
  marmot/threading/content. **No `ViewRegistry` exists** — nothing in
  `nmp-core` opens, drives, or reads a `ViewModule`. Every impl is
  tests-only. This has been true since review #19, #20, #25, #26.
- `IdentityModule` — 0 live runtime consumers; `KeyringCapability` is the
  identity path. The trait is marginal at best (review #21, #25).
- `DomainModule` / `CapabilityModule` — exercised only by `fixture-todo-core`.

Every review since #19 has said "delete `ViewModule`." It is still here. The
trait is not neutral: it inflates the "module count" metric, it makes
`fixture-todo-core` carry a 70-line `TodoViewModule` impl that does nothing,
and it sends a false signal that NMP has a view-extensibility story when it
has none — the views cluster ships as **hardcoded `make_update` projections**
(`update.rs:286-317`), not via `ViewModule` at all. The trait actively
contradicts the implementation.

## Top 3 highest-leverage next moves

### 1. The delete sweep — one PR, removes ~1,500 LOC of dead substrate

Delete in a single PR: the `ViewModule` trait + all ~14 impls; the
`IdentityModule` trait + impls; the 6 dormant `ActionModule` impls (NIP-29 ×4,
Marmot, NIP-77) **unless** move #2 wires one of them this cycle — if so, keep
that one. Also delete the dead `reduce()`/`ActionTransition` step-machine half
of `ActionModule` *unless* a real multi-step action is imminent (it is not).
This is pure subtraction: smaller surface, honest module count, `fixture-todo-core`
shrinks to just the action + snapshot proof. **Highest leverage because it is
the only move that reduces the thing every review keeps re-discovering.**
Pair it with a **moratorium**: no new `register_*` / `dispatch_*` FFI symbol
lands until an existing seam has ≥2 real (non-fixture) consumers.

### 2. Wire ONE real protocol action through the seam — NIP-25 reaction

Pick the smallest real action: a NIP-25 kind:7 reaction (or NIP-22 comment).
It needs: a builder (protocol crate), a host `register_action_module` +
`register_executor` that issues `ActorCommand::PublishUnsignedEvent`, and a
terminal result. This is the **only** move that tests the bet in §"riskiest."
NIP-29 join was the prior candidate and was claimed-then-lost — a reaction has
even smaller blast radius (no group-relay pinning). If this cannot be wired in
one cycle, that is itself the finding: the seam does not generalize and the
`ActionModule` trait should be replaced with a thinner contract.

### 3. Land PR #89 and make `last_action_result` a drained `Vec`

Two coupled fixes: (a) merge #89 so `PublishNote`'s minted id reaches the
publish engine and the correlation round-trip closes; (b) change
`last_action_result_projection()` to drain a `Vec<LastTerminal>` per tick so
two same-tick terminals are both reported. Without (b) the projection is a
known-lossy channel and review #25/#27's finding stays open. Cheap, bounded,
closes the last semantic hole in the publish path before move #2 adds a second
action type that would hit the same bug.

## Doctrine violations flagged since #25

- **D0 (latent).** `ActorCommand::PublishNote { content, reply_to_id }` is a
  kind:1-shaped command living in `nmp-core` — a protocol noun. It is the
  inverse of the D0-correct `PublishUnsignedEvent(UnsignedEvent)` sibling that
  already exists. `PublishNote` should be deleted and routed through an
  `ActionModule` executor that builds the `UnsignedEvent` and sends
  `PublishUnsignedEvent`. Its own doc comment admits it is a "stepping stone…
  deprecates kind-by-kind." It has not been deprecated.
- **D0 (cosmetic, not a violation but misleading).** `KernelSnapshot` is now
  clean — views/identity/publish clusters are all in `projections`. Good.
  But the *built-in* projections are inserted by hardcoded `make_update` calls
  (`update.rs:239-317`), not via `SnapshotRegistry`. That is acceptable
  (kernel-owned state) but means the `ViewModule`/projection-registry story is
  even more dormant than it looks — see move #1.
- **No new D6/D8 violations.** Panic isolation (`catch_unwind` in `execute` /
  `ClosureModule::start`) is present and tested. The 60 Hz path stays O(1).
  Mutex *lock-call* count in `nmp-core/src` is **147** (`grep '\.lock()'`,
  non-test) — note review #19/#25's "108 lock sites" was a different count;
  the trend is up, not down. `pending_mls_autopublish` is still
  `Arc<Mutex<bool>>` (ffi/mod.rs:255), still the cheapest single Mutex→
  `ActorCommand` conversion available, still not done since review #20.

## The meta-finding

Reviews #19 through #27 have recommended, in some form: delete `ViewModule`,
delete `IdentityModule`, wire a real action, fix `pending_mls_autopublish`,
shrink Chirp. None are done. The architecture is not the bottleneck — the
substrate seam is sound enough. The bottleneck is that **new surface keeps
landing faster than dead surface is removed.** The single most valuable change
this cycle is not a feature or a fix; it is the **delete sweep + moratorium**.
If review #28 still finds `ViewModule` alive, the recommendation will be to
stop reviewing direction entirely and spend that effort deleting code.

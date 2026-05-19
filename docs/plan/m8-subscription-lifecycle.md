# M8-subs — Subscription Lifecycle (the seam between planner and wire)

> Part of the [Build & Validation Plan](../plan.md). Arc 1 — Kernel substrate.
> Owner alias: `m8-subs-impl` (Task #46). Sequenced **after** M2 (planner) and
> **before** M4 (negentropy reconciler), M5 (NIP-42 auth), M7 (publishing).
> NOT to be confused with [M8 Multi-account](m8-multi-account.md) — that is a
> session-scope concern; this is a connection-pool / trigger-fan-in concern.

## 1. Purpose

The M2 planner (already on master) produces `CompiledPlan` objects: a
deterministic per-relay mapping of `SubShape` (one wire `REQ` per shape) plus
plan-id for stable diagnostic continuity.

What is **missing** between `CompiledPlan` and the wire is exactly four things:

1. A **logical-interest registry** that view modules push interests into and
   that the planner reads from on every recompile.
2. A **trigger inbox** (FIFO) that fans in the ten triggers enumerated in
   [`subscription-compilation/recompilation.md`](../design/subscription-compilation/recompilation.md)
   §4.0–§4.2 and coalesces them per actor tick (one compile per tick max).
3. A **wire-emitter** that diffs an incoming `CompiledPlan` against the
   currently-live wire-subscription set and emits `REQ` / `CLOSE` frames
   for the delta.
4. A **connection pool** that exposes a uniform send-path used by REQ
   emission (M2), NIP-77 reconciliation (M4), NIP-42 auth (M5), and event
   publishing (M7). Shared discipline: defer when disconnected, drop on
   close, never re-route across roles.

These four pieces together form the **subscription lifecycle**. With them
in place, M4 / M5 / M7 each plug into a clean seam instead of growing
parallel connection-pool plumbing.

## 2. Scope discriminator (what this is NOT)

| Concern | Lives here? | Owner |
|---|---|---|
| Connection pool + send path + RelayReconnected trigger emission | **Yes** | this task |
| Trigger inbox + per-tick coalescing | **Yes** | this task |
| Wire-emitter (CompiledPlan → REQ/CLOSE diff) | **Yes** | this task |
| Logical-interest registry (push from view modules, read from planner) | **Yes** | this task |
| NIP-77 negentropy reconciler (`Negentropy::reconcile`, `NEG-MSG` envelopes) | No | M4 / nmp-nip77 |
| NIP-42 challenge-response (kind:22242 builder, signer routing) | No | M5 / nmp-nip42 |
| Event publishing retry + OK acknowledgement parsing | No | M7 / nmp-core action ledger |
| Active-account scope binding (rebuild on switch) | No | M8 multi-account |

The discipline this task enforces is the *seam shape*. M4 emits its own
`Trigger::RelayReconnected` calls into the inbox we ship; we do not implement
the reconciler. M5 emits `Trigger::RelayAuthStateChanged { url, state }` into
the same inbox; we ship the enum variant and the pause-gate on `RelayPlan`,
not the AUTH handshake. M7 uses the shared `ConnectionPool::send_publish`
API we ship; we do not implement the OK parser.

## 3. Coordination with in-flight work (T39/T40/T43)

T39 (`nmp-nip77`), T40 (`nmp-nip42`), T43 (`nmp-signers`) are dispatched in
HB27 in parallel with this task. The contract:

- **T39 wins on `Negentropy` type, watermark shape, capability cache.** This
  task only emits `Trigger::RelayReconnected` into the inbox; we do not
  import `nmp-nip77` types.
- **T40 wins on `RelayAuthState` enum and the kind:22242 builder.** This task
  defines `Trigger::RelayAuthStateChanged { url, state: RelayAuthState }` as a
  variant carrying an opaque type alias declared in this crate; T40 substitutes
  the canonical type on its merge.
- **T43 wins on `Signer` trait and `SessionState`.** This task models
  `Trigger::SignerAvailable { account, signer_id }` as a no-op variant with
  opaque `AccountId` / `SignerId` type aliases.
- **T44 wins on `kernel/mod.rs` and `kernel/ingest.rs` HARD-cap splits.** This
  task lands in a **new** module `crates/nmp-core/src/subs/` so we do not
  touch the HARD-capped kernel files.

If T40 lands first, we re-export their `RelayAuthState`. If we land first, T40
substitutes the type alias. Either order is conflict-free.

## 4. Module layout

```
crates/nmp-core/src/subs/
├── mod.rs           — public API surface; SubscriptionLifecycle struct
├── registry.rs      — logical-interest registry (push/withdraw + iter_active)
├── trigger.rs       — CompileTrigger enum + InvalidateReason
├── inbox.rs         — FIFO + per-tick coalescing
├── wire.rs          — CompiledPlan → Vec<WireFrame> diff
└── pool.rs          — ConnectionPool trait + InMemoryPool test-support impl
```

Existing `crates/nmp-core/src/relay_worker.rs` stays as the actual WebSocket
worker. `ConnectionPool` is the abstraction the actor speaks to; `relay_worker`
is one implementation. M4/M5/M7 use the pool, not the worker.

Existing `crates/nmp-core/src/kernel/requests/mod.rs::req` stays for the
hand-rolled startup-REQ path that M1 relies on. The new `subs::wire` module
is consumed by future view modules that produce `LogicalInterest` rather than
named `OutboundMessage`s. The two paths coexist until M11 begins migrating
podcast view modules onto `LogicalInterest`.

## 5. Public API surface (D0 boundary)

`subs::` is a substrate-public module in the same tier as `planner::` — no
FFI exposure, but reachable by integration tests and future view modules
without a feature flag. The exported items mirror the four seam concerns:

- `subs::{SubscriptionLifecycle, plan_diff, WireFrame}` — wire-emitter API.
- `subs::{CompileTrigger, InvalidateReason, RelayAuthState, TriggerInbox}` —
  trigger model + per-tick coalescing inbox.
- `subs::InterestRegistry` — push/withdraw the active-interest set.
- `subs::{ConnectionPool, InMemoryPool, PoolSendOutcome}` — send-path trait
  + in-memory impl for tests / harnesses; production wraps `relay_worker`.
- `subs::{AccountId, SignerId}` — opaque newtypes for M6/M8 coordination;
  these are protocol-level (NIP-42 / NIP-65 account references), not
  app-level domain nouns. When M6 (signer trait) and M8 (session machine)
  land, they substitute their canonical types into the same names.

The actor will reach into `subs::` directly (not via `ActorCommand`) on the
M2-phase-2 wiring task — that task replaces the kernel's hand-rolled
`req` / `defer_outbound` calls with `SubscriptionLifecycle::drain_tick` +
`ConnectionPool::send`. No FFI surface is added in this task or that one;
all FFI exposure stays gated behind the existing `test-support` feature.

**T40 substitution contract:** when M5 / T40 lands the canonical
`RelayAuthState` enum (with `Failed { reason }` and friends), it replaces
the seam type in `subs::trigger`; the inbox / auth-gate plumbing stays
intact because all consumers match on `Authenticated` / `Paused-ish`
buckets, not on exhaustive enum exhaustion.

## 6. The eight integration tests

All in `crates/nmp-testing/tests/m8_subscription_lifecycle.rs`:

| # | Test name | What it pins |
|---|---|---|
| 1 | `compile_plan_to_wire_frames_emits_one_req_per_sub_shape` | wire-emitter ground truth: one `SubShape` → one `["REQ", sub_id, filter]` frame |
| 2 | `plan_diff_closes_removed_subs_and_opens_added_subs` | diff semantics: removed shapes → `CLOSE`, added shapes → `REQ`, unchanged → no frame |
| 3 | `reconnect_replays_current_plan_without_recompile` | A5 trigger semantics from recompilation.md §4.2: on `RelayReconnected`, wire-emitter re-issues current plan to that relay; planner is not invoked |
| 4 | `trigger_inbox_coalesces_within_one_tick` | §4.3 idempotence + per-tick coalescing: 50 enqueued triggers → 1 compile pass; subsequent ticks see no further compiles |
| 5 | `oneshot_lifecycle_closes_on_eose` | wire-emitter lifecycle observer: `InterestLifecycle::OneShot` shape emits CLOSE when the corresponding EOSE is reported |
| 6 | `bounded_time_lifecycle_closes_at_deadline` | wire-emitter time gate: `BoundedTime { until_ms }` emits CLOSE when wall-clock crosses the deadline, EOSE or not |
| 7 | `send_path_defers_outbound_when_pool_disconnected` | connection-pool discipline: send on disconnected role returns `PoolSendOutcome::Deferred`; reconnect drains deferred queue in FIFO order |
| 8 | `auth_paused_relay_holds_reqs_until_authenticated` | A9 trigger seam from recompilation.md §4.2: `RelayAuthStateChanged { state: ChallengeReceived }` marks `RelayPlan` as paused; `Authenticated` resumes pending REQs |

Tests 5, 6, 8 use the `test-support` feature to inject synthetic events,
deadlines, and auth-state transitions without a real relay. Test 7 uses the
`InMemoryPool` impl to drive send-path behaviour deterministically.

## 7. Exit gate

- All 8 integration tests pass: `cargo test -p nmp-testing --test m8_subscription_lifecycle`.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo test --workspace` baseline (37+ existing tests) unaffected.
- No file in `crates/nmp-core/src/subs/` exceeds 300 LOC (AGENTS.md soft cap).
- `crates/nmp-core/src/kernel/mod.rs` and `kernel/ingest.rs` line counts
  **unchanged** (we do not touch these HARD-capped files in this task; T44
  owns their split).

## 8. Doctrine compliance

- **D0** — no app nouns in `nmp-core::subs`. The registry is keyed by opaque
  `InterestId(u64)` from the planner; consumers (view modules) live above.
- **D3** — `wire::plan_diff` consumes the planner's per-relay mapping
  verbatim; we never re-route or override.
- **D4** — `InterestRegistry` is the single writer of the active-interest set.
  Snapshots are derived via `iter_active()`.
- **D6** — `WireFrameError` and `PoolSendError` are internal `Result` types,
  never re-exported through FFI.
- **D7** — `ConnectionPool` reports; the actor decides. The pool never
  spawns reconnect logic; that lives in the worker (which the pool may wrap
  but does not own policy for).
- **D8** — the trigger inbox folds N enqueued triggers into one compile per
  tick (≤60 Hz/view budget per ADR-0002). The wire-emitter's diff allocates
  only on actual REQ/CLOSE deltas, not on no-op recompiles.

## 9. Out-of-scope deferrals

- **NIP-77 reconciler logic** — M4 / T39. We only ship the
  `Trigger::RelayReconnected` inbox variant the reconciler will hook.
- **NIP-42 challenge-response** — M5 / T40. We only ship the
  `Trigger::RelayAuthStateChanged` variant + `RelayPlan` pause gate.
- **Publishing OK parser + retry** — M7 / future task. We only ship
  `ConnectionPool::send_publish` with the deferred-when-disconnected discipline.
- **Multi-account scope binding** — M8 multi-account doc. We ship the
  `Trigger::ActiveAccountChanged` variant; the session-state machine wires it.
- **LMDB-backed interest registry** — M3 / future task. v1 in-memory only.

# ADR-0029 — Bounded actor command channel + shed-load policy

- **Status:** Accepted
- **Date:** 2026-05-22
- **Supersedes:** the dual-channel "unbounded mpsc, drops cannot happen" claim wired into `Kernel::dispatch_drops` (`crates/nmp-core/src/kernel/mod.rs:427-434`) and `retention_tests.rs:42-55`.

## Context

The single-actor design routes every host-issued command through a `std::sync::mpsc::channel()` constructed in `nmp_app_new` at `crates/nmp-core/src/ffi/mod.rs:386`. The channel is unbounded.

This was called out as the #1 risk in the very first Opus direction review (2026-05-20):

> single-actor + hand-rolled sync transport, no backpressure

It has since aged badly:

- Mutex count grew from ~108 (review #19) to **~125** today — every newly-registered observer slot becomes another `Arc<Mutex<…>>` the actor visits per tick. The actor cannot get faster than its slowest tick, but it _can_ get arbitrarily backed up.
- A pathological producer loop (a Marmot key-package retry fanning out per peer, a NIP-46 broker re-stamping `BunkerHandshakeProgress` after a network glitch, or a host calling `nmp_app_push_interest` from an iOS scroll handler at 120 Hz) currently grows the queue without bound. The `actor_queue_depth` AtomicU64 is the _observation_ surface (live since G-S4) but no _enforcement_ surface exists.
- `Kernel::dispatch_drops` (`Arc<AtomicU64>`) is already plumbed end-to-end through `actor/mod.rs:645,671` → `kernel/types.rs:641` → snapshot `Metrics::dispatch_drops_total` — but the increment is dead code: the field doc-comment literally says _"under the current dual-channel design this is always zero (unbounded command channel cannot drop). Retained for API compatibility."_ The slot is waiting for this PR.
- `retention_tests.rs:42` documents the historical precedent: `BOUNDED_ACTOR_CMD_CAPACITY=4096`, drop-newest, `dispatch_drops_total` accounting (commit `44cbfd2`, T114 part 1). The same scheme was removed when the dual-channel split landed; this ADR restores it.

Doctrine constraints (verbatim from `AGENTS.md`):

- **Doctrine #1** — substrate is the single seam. _All_ command paths into the actor must enforce the same bound; a `pub fn actor_sender()` raw clone that bypasses backpressure (the present G-S4 caveat at `ffi/mod.rs:806`) lets a producer-loop in `nmp-signer-broker` or `nmp-marmot` defeat the gate.
- **Doctrine #8 — NO POLLING.** The bound must use OS-primitive blocking only; no sleep-then-retry, no spin loop.
- **D7 — actor-death visibility.** Existing `send_cmd` semantics: a dead-actor send is silently dropped; the actor's panic frame on the update channel is the host's terminal signal. The new shed path must coexist with that — a `Full` is NOT actor death and must surface differently.

## Decision

### 1. Bound size

**`BOUNDED_ACTOR_CMD_CAPACITY = 4096`**, restored from the T114-part-1 precedent.

Justification:

- The historical bound came from production-load measurement (commit `44cbfd2`) — a value already validated against real Apple-relay event-rate traces.
- A rough sanity check against today's instrumentation: `actor_queue_depth` is sampled inside `ffi/action.rs` (lines 600/602/617/620/636) — depth-before/depth-after of a single `dispatch_action`. Steady-state observation is single-digit. The bound is therefore ~500-1000× steady state — comfortable headroom against an honest burst, narrow enough to catch a producer-loop blow-up before iOS triggers an OOM jetsam kill.
- We do _not_ have a long-window p99 depth histogram. The constant is annotated `// ADR-0029 — revisit when a depth p99 metric stabilizes` so the next iteration can tune from data rather than precedent.

A `BOUNDED_ACTOR_CMD_CAPACITY` constant in `ffi/mod.rs` (not magic-numbered) is the canonical name — it makes the term in `retention_tests.rs:42` searchable again.

### 2. Shed policy — **drop-newest with counter increment**

Three options were considered; the hybrid wins on doctrine grounds.

| Policy | Rejected because |
|---|---|
| (a) Sender blocks until space (`SyncSender::send`) | Blocks the FFI caller. iOS shells call `send_cmd` from the main thread (every `nmp_app_*` is invoked from the JNI/Obj-C bridge on the calling thread). Blocking the iOS main thread on actor backlog is a UI hang and violates the synchronous-thread doctrine in `aim.md`. |
| (b) Sender gets `Result<(), ActorBusy>` everywhere | Touches every one of the ~25 `send_cmd` call sites and forces each FFI verb to define what failure means. The cost is the entire FFI surface for a counter that the host should treat as a diagnostic, not a contract. |
| (c) **Drop-newest, atomic increment of `dispatch_drops_total`** | Matches the historical `BOUNDED_ACTOR_CMD_CAPACITY=4096` precedent. Zero churn at call sites. Backed by an existing snapshot metric the host already decodes. Failure is _visible_ via the metric, not silent. |
| (d) Drop-oldest | Requires a reader-thread to manually pop, which the actor's `try_recv` model does not give us; would also drop an in-flight `SignInNsec { secret: Zeroizing<String> }` ahead of a less-critical refresh. Worst-payload-first wrong. |

**Chosen policy: (c) drop-newest.** When the bounded channel is full, `send_cmd` records one drop in `dispatch_drops_total` and returns. The dropped `ActorCommand` is moved through `TrySendError::Full(T)` and explicitly dropped at the `send_cmd` site so destructors (including `Zeroizing<String>` for nsecs) fire deterministically.

**Why drop-newest is acceptable:** Under genuine sustained overload, every command in the queue _ahead_ of the newest one is a snapshot of a slightly-fresher producer intent. Dropping the newest preserves causal order (the host saw the older command "happen" first) and gives the actor the oldest workload — the one that has been waiting longest — first. A user who hit "publish" three times because the UI hung still has the first publish in flight; dropping the third is the right outcome.

**Drop visibility in the UI:** `dispatch_drops_total` is already on the snapshot — every host that already renders `actor_queue_depth` (e.g. the Chirp diagnostic surface) sees the drop count automatically. A future enhancement can promote a non-zero drop to a `ShowToast` envelope; that is _not_ in this ADR's scope (the toast itself would need to traverse the channel, which is the thing we are flow-controlling).

### 3. Metric alignment — reuse `dispatch_drops_total`

The historical T114b metric (`kernel/types.rs:641`, plumbed via `Kernel::dispatch_drops` / `set_dispatch_drops_handle` / snapshot `Metrics::dispatch_drops_total`) is _exactly_ the shed counter. We do **not** add a parallel `actor_shed_total`:

- A dormant field staring at a live field with the same meaning is the kind of substrate sprawl reviews #18-#27 keep flagging.
- The doc-comments in `kernel/types.rs:638-641` and `kernel/mod.rs:427-433` are updated to drop the "always zero" claim — they now read as the live shed counter for ADR-0029.
- `retention_tests.rs:42-55` is updated to match (the line documents the bounded-channel design verbatim).

`actor_queue_depth` remains the live gauge. Under steady state it tracks pre-bound depth; if the bound bites it pegs at `BOUNDED_ACTOR_CMD_CAPACITY` and `dispatch_drops_total` advances.

### 4. Single seam — kill `actor_sender()` as a raw clone

The G-S4 caveat at `ffi/mod.rs:806` documents the existing bypass: `actor_sender()` hands out a raw `Sender<ActorCommand>` clone, used by:

- `nmp-signer-broker/src/broker.rs:329, 344` (`BunkerBroker::actor_tx: Sender<ActorCommand>`)
- `nmp-marmot/src/projection/state.rs:458` (key-package fanout loop — _the_ pathological producer pattern this ADR was written for)

A raw-clone bypass defeats the bound and is rejected by Doctrine #1. The fix:

- Change the channel constructor from `mpsc::channel()` to `mpsc::sync_channel(BOUNDED_ACTOR_CMD_CAPACITY)`. The natural primitive — `std::sync::mpsc::SyncSender` — is already in `std`; no new dep, no `crossbeam_channel` (which is not a transitive dep of `nmp-core` per `cargo tree`, so adding it is unjustified for this PR).
- Introduce a public, `Clone`able typed sink: `pub struct ActorCommandSink { tx: SyncSender<ActorCommand>, drops: Arc<AtomicU64> }`. All sends go through `ActorCommandSink::try_send`, which is the single point that increments the drop counter on `TrySendError::Full`.
- `NmpApp::actor_sender()` is retyped to return `ActorCommandSink` (not `SyncSender<ActorCommand>` raw). Same name, same semantics for the caller (`.try_send(cmd)`), one shed-policy implementation.
- Broker and marmot import `ActorCommandSink` and replace their `Sender<ActorCommand>` fields. This is a small, mechanical edit (two crates, ~5 lines each).

### 5. `send_cmd` compatibility

`send_cmd` is `pub(crate) fn` — only `nmp-core` crates it. Auditing the call sites (28 total):

| File | Site | Thread | New behaviour |
|---|---|---|---|
| `ffi/mod.rs:370` (Drop) | `Shutdown` | process teardown | shed is harmless (we're dying anyway) |
| `ffi/mod.rs:783` | test-only `execute()` closure | actor thread | shed → drop counter |
| `ffi/mod.rs:817, 850, 1000, 1126` | identity, push-interest, publish | FFI (main thread) | shed → drop counter (matches doctrine — no UI block) |
| `ffi/mod.rs:1231, 1248, 1259, 1267` | `Start`/`Configure`/`Stop`/`Reset` | FFI | shed → drop counter (Start is one-shot so shedding under flood means the actor is unhealthy; the host already debounces) |
| `ffi/identity.rs:*`, `ffi/timeline.rs:*`, `ffi/publish.rs:*`, `ffi/wallet.rs:*` | all FFI verbs | FFI | shed → drop counter |
| `ffi/action.rs:136, 472` | ack action stage; executor cmd-send closure | FFI / actor | shed → drop counter (action executors run on actor thread; blocking would self-deadlock) |
| `ffi/testing.rs:91, 139` | test injectors | test | shed → drop counter |
| `nmp-signer-broker/src/broker.rs:329, 344` | broker thread (NIP-46 response) | broker thread | uses `ActorCommandSink::try_send`; shed → drop counter |
| `nmp-marmot/src/projection/state.rs:458` | key-package fanout in projection | actor thread | **critical** — must `try_send`, never blocking, or the actor self-deadlocks |

No call site changes infallibility semantics for the host (`send_cmd` was already infallible from the caller's POV — `let _ = self.tx.send(cmd)`). The compatibility contract is preserved: a returned `Err(TrySendError::Full(_))` is consumed inside `send_cmd` (the cmd is dropped, the counter is incremented). The result type of `send_cmd` stays unit; callers do not change.

The `Zeroizing<String>` payload inside `ActorCommand::SignInNsec` is recovered from `TrySendError::Full(cmd)` and then immediately dropped at the `send_cmd` site, so the `Zeroizing` `Drop` impl fires and zeroes the buffer just like the actor-thread path would have on a healthy dequeue.

### 6. Actor self-deadlock guard

Action executors run on the actor thread (per `ActionRegistry::execute`'s docs). Marmot's projection runs on the actor thread. If those used `SyncSender::send` (blocking) they would block the actor — _which is the consumer_ — waiting for itself to drain. Guaranteed deadlock under load.

`try_send` is therefore the only sound primitive for any send issued from the actor thread, _and_ it is sound for all external sends since the shed cost is at most "the newest command is dropped under sustained overload." We adopt `try_send` everywhere — there is no second policy.

## Consequences

### Positive

- The #1 substrate risk from the very first direction review (2026-05-20) is closed by enforcement, not by documentation.
- Memory blow-up under producer-loop pathology is now impossible: queue is bounded at 4096 `ActorCommand` values (estimated <2 MiB resident in the worst case).
- The existing `dispatch_drops_total` snapshot field becomes live — the host can render an alarm panel against a real signal.
- `actor_sender()` no longer hands out a raw clone; both broker and marmot traverse the same backpressure gate as FFI. The G-S4 caveat is _resolved_ rather than _accepted_.
- The bound is constitutionally enforced — adding a new external sender in the future is a type-level error (you need an `ActorCommandSink`, you get one from `app.actor_sender()`, you cannot construct it any other way).

### Negative

- Under genuine sustained overload, host commands are silently dropped (visible only via `dispatch_drops_total`). This is the explicit trade-off vs. blocking the iOS main thread, and is the right one — see the table in §2.
- 4096 may not be the right number forever; the constant carries a `// revisit` comment.
- A drop of a `SignInNsec` under flood is racier than under the unbounded design (the user's nsec import may silently no-op if the actor is overloaded at the exact moment of import). Mitigation: the user can retry; the counter records the drop. A future ADR can promote "drop while sign-in pending" to a `ShowToast` once the toast path is itself out-of-band.

### Compatibility

- No FFI ABI change (`send_cmd` returns unit before and after).
- `actor_sender()` return type changes from `Sender<ActorCommand>` to `ActorCommandSink`. This is a Rust-only API change; two known consumers (`nmp-signer-broker`, `nmp-marmot`) get a one-line edit.
- Snapshot schema unchanged: `actor_queue_depth` and `dispatch_drops_total` were already in `Metrics`.

## Validation

A new `bounded_channel_floods_shed` test in `crates/nmp-core` floods `2 * BOUNDED_ACTOR_CMD_CAPACITY` commands at a paused actor (held at the `command_rx.recv()` boundary in `run_actor_with_observers`) and asserts `dispatch_drops_total >= BOUNDED_ACTOR_CMD_CAPACITY`. This is the load test demanded by the PR-G deliverable.

Doctrine-lint stays green: no new banned tokens, no new polling sites, no D0 violations (`ActorCommandSink` lives in `nmp-core` alongside `ActorCommand`).

## References

- T114 part 1 (commit `44cbfd2`) — the historical `BOUNDED_ACTOR_CMD_CAPACITY=4096` precedent
- G-S4 (`ffi/mod.rs:346-360`) — the `actor_queue_depth` instrumentation this ADR makes enforceable
- Direction reviews #19 (Mutex proliferation), #20-#22 (broker bypass), #34 (`actor_queue_depth` legitimate)

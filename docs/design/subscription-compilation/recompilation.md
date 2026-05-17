# Subscription Compilation §4 — Recompilation Triggers

> Parent: `docs/design/subscription-compilation.md`.
> Read first: [compiler.md](compiler.md) §3.4 for plan-id semantics.

The compiler is idempotent and pure given `(interest_set, mailbox_snapshot, indexer_set, user_configured_relays)`. Recompilation is therefore safe to run on every trigger; the wire-emitter's diff turns no-op recompilations into zero wire effect.

This section enumerates **every trigger that may cause recompilation** and the actor message shape each one carries. Triggers fan in from three sources: relay ingest, view registry mutations, and operator/user state changes. All of them route through the same actor mailbox (`docs/design/reactivity/loop-and-reverse-index.md`).

## 4.0 Internal vs external triggers

Two trigger classes exist:

- **Internal** triggers are emitted by the actor itself in response to an `InternalEvent`. They are `Trigger::*` enum variants; the planner consumes them off its own internal queue. They have no public dispatch surface.
- **External** triggers are `AppAction` variants the platform may dispatch directly. There is exactly one — `AppAction::InvalidateCompile { reason }` — to keep the public surface minimal per `docs/aim.md` §6 doctrine 5.

The full list:

| ID | Source | Trigger | Carries |
|---|---|---|---|
| A1 | ingest | `Trigger::Nip65Arrived { pubkey, created_at }` | kind:10002 just landed |
| A2 | view registry | `Trigger::ViewOpened { interest_ids }` | one or more interests just registered |
| A3 | view registry | `Trigger::ViewClosed { interest_ids }` | warmth grace expired; interests dropped |
| A4 | session | `Trigger::ActiveAccountChanged { from, to }` | account switch (M8) |
| A5 | relay worker | `Trigger::RelayReconnected { url }` | socket re-established after backoff |
| A6 | operator | `AppAction::InvalidateCompile { reason }` | external force-recompile |
| A7 | config | `Trigger::UserConfiguredRelaysChanged { generation }` | added/removed relay in local config |
| A8 | config | `Trigger::IndexerSetChanged { generation }` | indexer relay list edited |
| A9 | auth | `Trigger::RelayAuthStateChanged { url, state }` | NIP-42 transition (M5+) |
| A10 | session | `Trigger::SignerAvailable { account, signer_id }` | signer-loss-then-recovery (M6+) |

A1–A3 are M2 scope; A4–A10 are interface seams that M2 establishes so later milestones do not have to re-plumb. The compiler treats unknown triggers as `Trigger::Generic`.

## 4.1 Actor message shapes

```rust
// crates/nmp-core/src/kernel/planner/trigger.rs (proposed)

#[derive(Clone, Debug)]
pub enum CompileTrigger {
    Nip65Arrived {
        pubkey: Pubkey,
        created_at: UnixSeconds,    // for replay-window skew detection
    },
    ViewOpened {
        interest_ids: Vec<InterestId>,
    },
    ViewClosed {
        interest_ids: Vec<InterestId>,
        warmth_expired_at_ms: u64,
    },
    ActiveAccountChanged {
        from: Option<AccountId>,
        to: Option<AccountId>,
    },
    RelayReconnected {
        url: RelayUrl,
        prior_state: RelayConnectionState,  // for diagnostics
    },
    InvalidateCompile {
        reason: InvalidateReason,
    },
    UserConfiguredRelaysChanged {
        generation: u64,                    // monotonic config rev
    },
    IndexerSetChanged {
        generation: u64,
    },
    RelayAuthStateChanged {
        url: RelayUrl,
        state: RelayAuthState,              // re-exported from ADR-0007
    },
    SignerAvailable {
        account: AccountId,
        signer_id: SignerId,
    },
}

#[derive(Clone, Debug)]
pub enum InvalidateReason {
    DiagnosticsManualRefresh,               // operator UI button
    TestForceRecompile,                     // nmp-testing harness
    External(String),                       // catch-all with diagnostic string
}
```

`InvalidateReason::TestForceRecompile` is the seam the wire-frame audit gate (§9) drives so the test does not have to fake a kind:10002 arrival to exercise the recompile path.

## 4.2 Trigger semantics

### A1 — Nip65Arrived

Emitted from `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`) when **and only when** the parser decides to replace the prior mailbox entry (the `should_replace` branch at line 218–222). Stale arrivals do not trigger recompilation.

Compiler effect: re-runs Stages 1–4 for every interest that touches `pubkey` either as a member of `shape.authors` or as a member of `shape.tags[#p]`. Other interests stay assigned to their current plan-id slot; the merged plan-id changes (because the mailbox snapshot's `created_at` for `pubkey` advanced) but its `per_relay` content may be identical.

Outbox routing implication: if `pubkey` was previously routed to the indexer fallback set (Stage 2 read-fallback), the compiler now reassigns to the author's declared write relays. The wire-emitter closes the indexer REQ for that author's slice and opens a new REQ on the declared relays. ADR-0007 diagnostics reflect the route source flipping from `Indexer` to `Nip65`.

### A2 — ViewOpened

Emitted by the view registry when a `ViewModule::open` returns a non-empty `Vec<LogicalInterest>` (per [intro.md](intro.md) §2.2). May fire in batches when a screen opens many rows at once.

Batching contract: the actor's planner inbox coalesces consecutive `ViewOpened` triggers within one actor tick into a single recompile pass. This is the existing reactivity batching (`docs/design/reactivity/scheduling-and-data-model.md`) extended to the planner; the M2 implementation respects the same `≤60Hz/view` budget from ADR-0002 by capping recompiles at one per tick regardless of trigger fan-in.

### A3 — ViewClosed

Emitted by the view registry after the warmth grace expires for an interest with refcount = 0. The warmth window is configurable (`AppConfig.view_warmth_ms`, default 30,000 — matching the doctrine in `docs/product-spec/subsystems.md` §7.6 "View warmth"). Closing an interest mid-warmth (e.g. account switch invalidates the prior account's interests) is a separate `ActiveAccountChanged` trigger, not this one.

### A4 — ActiveAccountChanged

M2 establishes the trigger; M8 wires the multi-account state machine that actually emits it. For M2, the trigger fires once at startup with `from: None, to: Some(active)` so the test surface can exercise account-scope binding without waiting for M8.

Compiler effect: every `InterestScope::ActiveAccount` interest is re-resolved as if newly opened. `InterestScope::Account(specific)` interests are untouched. `InterestScope::Global` interests are untouched.

### A5 — RelayReconnected

Emitted by the relay worker (`crates/nmp-core/src/relay_worker.rs`) after a successful re-handshake. Compiler effect: the wire-emitter re-issues the relay's `SubShape` set as REQs to restore tail subscriptions; the compiler does *not* re-merge or re-resolve. This is a pure "replay current plan to one relay" operation, not a real recompilation, but it routes through the same trigger queue so the diagnostic stream sees it.

Per `docs/product-spec/subsystems.md` §7.2 "Reconnect": "the planner restores live REQs and schedules a coverage-aware gap fill. View payloads do not reset." The gap-fill schedule is M4 (NIP-77); for M2 the gap is implicit (live tail resumes without backfill).

### A6 — InvalidateCompile

The single external `AppAction` variant. Useful for:

- Operator diagnostics screens — "Force re-route now."
- Test harnesses — see §9.
- Future debugging tools that change runtime config.

Compiler effect: full recompile from scratch, ignoring incremental caches. Plan-id will change iff any input changed since last compile.

### A7 / A8 — User/indexer config changes

Both bump a `generation: u64` so the plan-id picks up the change (per [compiler.md](compiler.md) §3.4). M2 binds the generation counters but does not yet implement a settings UI to mutate them; v1 ships the seams.

### A9 — RelayAuthStateChanged

M5 wires this fully. M2 only models the trigger so the compiler's data-flow shape does not need to change in M5. Compiler effect: marks the relay as "auth-paused" in its `RelayPlan` so the wire-emitter knows to hold REQs until `RelayAuthState::Authenticated`. Open question 6 in the parent index covers where the gate physically lives.

### A10 — SignerAvailable

M6+ trigger. Some interests (private DMs in M9, NIP-42 challenge response in M5) only become routable once a signer is loaded for their account. M2 records the trigger shape; behaviour is no-op pre-M6.

## 4.3 Trigger ordering and idempotence

The actor's planner inbox is a FIFO queue. Order matters only at the granularity of a tick: within a tick, all queued triggers are folded into the compile inputs and one compile runs. Across ticks, recompiles happen in order received.

Idempotence: running the compiler twice in a row with the same inputs yields identical outputs (same `plan_id`, same `per_relay`). The wire-emitter's diff of two identical plans is empty. This is the contract the audit gate in §9 leans on.

## 4.4 What does *not* trigger recompilation

Explicit non-triggers (so future code does not accidentally over-couple):

- **An EVENT arrival on an existing REQ.** The compiler does not care; the view-modules' projections do.
- **An EOSE on a one-shot interest.** The interest closes via lifecycle; that flows through `ViewClosed`-equivalent path (the registry drops the interest, fires `ViewClosed`).
- **A profile-claim refcount delta that does not cross 0↔1.** Going from refcount 5 → 4 is invisible to the compiler.
- **A relay's RTT or bytes-rx counter ticking.** Diagnostics-only.
- **A new event id surfacing inside a `ThreadView`'s reduce.** The view module re-invokes `interests()` and returns the augmented set; that emits `ViewOpened` for the *new* `InterestId`s, not a full thread-view recompile. The compiler sees only the additive delta.

These non-triggers keep the recompile cadence aligned with material routing changes, not with event throughput. That is what protects against the "subscription churn under firehose load" failure mode the NDK/Applesauce lessons explicitly warn against (`docs/design/ndk-applesauce-lessons.md` §7 "should recompile" paragraph, lines 92–94).

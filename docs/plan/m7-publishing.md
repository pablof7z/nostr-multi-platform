# M7 — Publishing pipeline + retry queue (substrate)

> Part of the [Build & Validation Plan](../plan.md). Companion to
> [m7-interaction-loop.md](m7-interaction-loop.md) — the interaction-loop
> milestone (reactions, threads, replies) is the user-visible product; this
> document describes the publishing substrate that the interaction loop
> publishes through.

**Demo product:** any action that produces a signed event (kind:1, kind:7,
kind:30023, kind:6, …) hits a single substrate-level `PublishEngine` that
routes per NIP-65, retries transient failures, surfaces persistent failures to
a snapshot, and never loses a queued publish across kernel restart.

**Scope.** A new `crates/nmp-core/src/publish/` module (kept in-tree rather
than a separate crate per `AGENTS.md` file-size discipline; promote
to `crates/nmp-publish/` when the engine acquires its own dependency tree):

- `PublishAction { Publish { handle, event, target: Auto | Explicit } }` +
  `PublishOutcome { Accepted | Mixed | FailedAfterRetries | NoTargets |
  Cancelled }` on the action ledger surface.
- Per-(event, relay) state machine (`Pending → InFlight → Ok |
  RelayError | TimedOut → FailedAfterRetries`) with deterministic retry
  policy: AUTH-REQUIRED → reauth + 1 retry; transient → up to 3 total
  attempts (initial + 2 retries) at 1s / 4s.
- `PublishStatusView` view module with bounded snapshot: `in_flight`,
  `recent_ok` (cap 32), `recent_errors` (cap 32), `rev` (change marker for
  D8 projection coalescing).
- Trait shims for dependencies that have not yet landed: `Signer` (M6 #43),
  `RelayDispatcher` (M8 RelayManager #46), `OutboxResolver` (M2 NIP-65),
  `PublishStore` (M3 LMDB). Each shim has an `InMemory*` / `Static*` /
  `Noop*` impl for tests and bootstrap.

**Doctrine map.**
- D3 (outbox automatic): `PublishTarget::Auto` → `OutboxResolver` resolves
  author writes ∪ `#p`-tagged inbox reads; explicit relays are the named
  opt-out.
- D4 (single writer per fact): per-(event, relay) state is owned by the
  engine; the snapshot is derived.
- D5 (snapshots bounded by what's open): `PublishStatusSnapshot` carries
  in-flight rows + bounded recent windows; no event-store payload crosses
  FFI through this view.
- D6 (errors never cross FFI): operational publish failures surface as
  `RecentFailure` rows on the snapshot; `PublishEngine` returns
  `Result<_, PublishEngineError>` internally, and actor/FFI wiring must map
  those before they cross the boundary.
- D7 (capabilities report, never decide): `RelayDispatcher` returns raw
  `RelayAck`; classification (`AckClass::AuthRequired | Transient |
  Permanent`) is the engine's; retry-or-give-up is the engine's.
- D8 (≤60 Hz/view, working set bounded): `rev` marks publish-status changes;
  the projection bridge must coalesce/rate-limit emitted deltas.

**Wiring (deferred to dependency milestones).** This milestone delivers the
engine + traits + tests. The actor / ledger wiring is bundled with the
dependencies it requires:

| Wiring | Lands with |
|---|---|
| `IdentityModule`-backed `Signer` implementation | M6 (#43) |
| `RelayManager`-backed `RelayDispatcher` over real websocket subs | M8 (#46) |
| NIP-65 `OutboxResolver` reading kind:10002 from the event store | M2 |
| LMDB-backed `PublishStore` | M3 |
| Bridge `ActionLedger::reduce` → `PublishEngine::on_ack` | M6 ledger |

Each is a thin adapter (≤100 LOC each) — the substrate carries the policy,
the milestones drop in the I/O.

**Exit gate (this milestone).**

- All 6 state-machine transitions covered by unit tests in
  `crates/nmp-core/src/publish/tests.rs`.
- 9 integration scenarios in `crates/nmp-core/tests/publish_engine.rs`:
  - `publish_auto_resolves_outbox` — only relays in the author's
    kind:10002 set receive the EVENT.
  - `publish_p_tag_inbox_routing` — `#p:bob` adds bob's read relays.
  - `publish_retry_on_connection_drop` — transient → retry → OK.
  - `publish_giveup_after_three_attempts` — three transient attempts → FailedAfterRetries.
  - `publish_durable_across_restart` — engine instance 1 queues + dies;
    instance 2 resumes from the same `PublishStore` and completes.
  - `publish_dedup_on_same_event_multi_relay_single_rev_per_batch` — 5 relay
    fan-out, bounded replay-harness rev churn, one `recent_ok` entry.
  - `publish_outcome_classification_matches_per_relay_states` — coarse
    outcome derived from per-relay map.
  - `publish_store_persists_event_for_resume_round_trip` — store contract.
  - `publish_store_error_does_not_panic_engine` — D6.
- `cargo test --workspace` green, `cargo clippy --workspace --all-targets
  -- -D warnings` green, firehose-bench replay quick gates PASS.

**Exit gate (downstream milestones).**

- When M2, M3, M6, M8 all land, integration tests in
  `crates/nmp-testing/tests/` exercise the wired pipeline against MockRelay
  with real kind:10002 events, real LMDB persistence, real signer reauth.
  Those tests are owned by their respective milestones; this milestone
  does not block on them.

**Runnable artifact.** `cargo test -p nmp-core publish`. The compose flow in
iOS that exercises the wired pipeline ships with M6.

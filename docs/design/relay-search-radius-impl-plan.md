# Relay-Search-Radius Impl Plan

> Workstream plan for the spec at [`relay-search-radius.md`](relay-search-radius.md).
> Anchors PR for issue [#632](https://github.com/pablof7z/nostr-multi-platform/issues/632).
> Phase 3 implements the workstreams below; Agent B reviews this plan against
> the spec and the code anchors.

This document is the *how*. The spec is the *what*. Doctrines D0/D4/D6/D8
apply throughout; specific guards are called out per workstream.

---

## 0. Spec §10 — resolution table

Every open question in spec §10 is resolved below with file:line justification.
Items marked **deferred** require Agent B to break a tie.

| # | Question | Resolution | Justification |
|---|---|---|---|
| 1 | Score scheme A/B/C | **A — paired counters + decay multiplier** | LMDB key/value is fixed 32-byte payload (`u32 successes + u32 failures + u64 last_used_unix_s + u64 reserved`); restart-trivial. The cold-start ambiguity the spec calls out (§7) is moot because the WARM_THRESHOLD test in W4 already requires at least one prior success. Candidates B/C add complexity (`f32` serialization stability for B, hand-rolled Wilson math for C) that buys nothing the Phase 1 decision actually consumes. |
| 2 | `WARM_THRESHOLD` | **0.40** under scheme A's `weight = successes/(successes+failures+1) * exp(-0.0495*age_days)` | One-hit-zero-miss cell evaluates to `1/2 ≈ 0.50` fresh; one-hit-one-miss to `1/3 ≈ 0.33`. 0.40 admits a single hit but excludes a hit paired with a miss — i.e. requires the relay to be more right than wrong on Gigi. Cited in W1 constant doc. |
| 3 | Wall-clock budgets | Confirm spec §6: `PHASE_1_BUDGET_MS=1500`, `PER_RELAY_REQ_TIMEOUT_MS=5000`, `PER_CLAIM_TOTAL_BUDGET_MS=8000`, `MAX_EXPANSION_CONCURRENCY=3`, `MAX_RELAYS_TRIED_PER_CLAIM=12` | The keepalive idle (`crates/nmp-network/src/relay_worker/mod.rs:170` — `KeepaliveState::new(_, keepalive_idle, _)`) is 30 s in production. 1.5 s sits inside one keepalive window; 8 s sits inside one reconnect backoff (`RELAY_RECONNECT_DELAY_INITIAL`). No measured-data refinement is required for v1; revisit when wire-log telemetry from W8 lands. |
| 4 | Decay half-life | **14 days** (`k = ln(2)/14 ≈ 0.0495`) | Matches spec recommendation; aligns with typical user re-engagement cadence. Lower (7 d) would churn warm cells across weekly usage gaps; higher (30 d) would keep a since-dead relay warm long enough that A5 fails. |
| 5 | Persistence target | **LMDB sub-db `relay_author_scores_v1`** | The store opens with reserved `additional_dbs` slots (`crates/nmp-nostr-lmdb/src/store/lmdb/mod.rs:131-146` — `open_env` takes `additional_dbs: u32`). Schema-bump = name bump (§5 E6); no migration code. JSON-on-idle is rejected — it forfeits atomic consistency with other actor writes and loses the schema-versioning ergonomics. |
| 6 | Phase 2 ordering tiebreaker | **lex-DESC URL** (matches selector) | `crates/nmp-planner/src/selection.rs:243-250` uses `b.0.cmp(a.0)` for URL ties. Reusing the same total order means a relay's selection-time and expansion-time ranking are consistent for an operator reading wire logs. NIP-11 intersection is out — no NIP-11 cache exists in-tree (grep `nip11` returns docs only). |
| 7 | Background-complete Phase 2 after user budget | **Yes** | Without it, A5's failed-relay learning is starved on long-tail relays that EOSE just past the 8 s budget. Implementation cost is zero — the per-candidate LogicalInterest (W5) is a normal `OneShot` whose EOSE handler (`kernel/ingest/mod.rs:200-207`) already routes to `complete_unknown_oneshot`; we simply do NOT CLOSE the interest at budget elapse. |
| 8 | New module vs extend `discovery.rs` | **New module `crates/nmp-core/src/kernel/claim_expansion.rs`** | `discovery.rs` is 251 LOC and scoped to the unknown-id drain seam (a different lifecycle: per-tick batch, no per-claim state). Mixing per-claim phase-state into it would push past the 500 LOC ceiling (D-V12) and conflate two concerns. The new module mirrors `discovery.rs`'s shape (typed entry-point methods on `impl Kernel`) but owns the per-claim state machine. |
| 9 | Phase 2 wire mechanics — one REQ per relay vs batched | **One new `LogicalInterest` per Phase 2 candidate**, each carrying a single `RelayHint` pinning it to that candidate | Per-relay outcome attribution (the score writeback in §4.3) requires per-relay EOSE/EVENT visibility, which requires distinct `sub_id`s, which requires distinct `InterestId`s. The existing `OneshotApi.request` (`crates/nmp-core/src/subs/oneshot.rs:97-137`) dedups identical `(scope, shape)` — so distinct shapes (the hint set differs) produce distinct interests naturally. See W7 for the hint-consumption seam that makes this routing actually land. |
| 10 | Wire-log lines | **New env-gated tracer `NMP_WIRE_LOG`** emitting structured lines through the existing `Kernel::log` seam | `Kernel::log` already exists (called from `kernel/ingest/mod.rs:146, 239, 251`). Adding a typed `log_claim_event` helper that writes to the same sink when `std::env::var("NMP_WIRE_LOG").is_ok()` is non-invasive. Format defined in W8. |

**Implicit prerequisites discovered (raised in §F):**
- The FFI symbol `nmp_app_claim_event` does not exist (only `nmp_app_claim_profile` at `crates/nmp-ffi/src/timeline.rs:75-98`); the existing event-URI entry point is `nmp_app_open_uri` (`crates/nmp-ffi/src/timeline.rs:62-73`) routed via `KernelAction::OpenUri` (`crates/nmp-core/src/kernel_action.rs:62-103`).
- `LogicalInterest.hints` (`crates/nmp-planner/src/interest.rs:280`) is defined but **not consumed** by partition — see W7.
- `NMP_WIRE_LOG` does not exist — see W8.

---

## 1. Workstream summary table

| WS | Name | Crate(s) | Approx LOC | Depends on |
|---|---|---|---|---|
| W1 | `RelayAuthorScore` substrate type + in-memory map | `nmp-core` | +200 | — |
| W2 | LMDB sub-db + load/flush wiring | `nmp-store`, `nmp-core` | +250 | W1 |
| W3 | Score-update seam on EVENT / EOSE / FailedAfterRetries | `nmp-core` | +180 | W1 |
| W4 | Phase 1 warm-relay preference in planner | `nmp-planner`, `nmp-core` (adapter) | +180 | W1 |
| W5 | `claim_expansion.rs` controller (Phase 1/2/3 state machine) | `nmp-core` | +450 | W1, W3, W7 |
| W6 | Edge-triggered deadline polling on actor idle tick | `nmp-core` | +120 | W5 |
| W7 | Hint consumption in `compiler/partition/case_a*.rs` | `nmp-planner` | +120 | — |
| W8 | `NMP_WIRE_LOG` telemetry seam | `nmp-core` | +90 | W3, W5 |
| W9 | Acceptance integration tests A1–A6 | `nmp-testing` | +600 | W1–W8 |

Workstreams W1, W7 are **independent** and can land in parallel. W2 needs W1.
W3 needs W1. W4 needs W1. W5 needs W1+W3+W7. W6 needs W5. W8 cuts across W3/W5
but is small. W9 is the final consolidation.

---

## Workstream W1 — `RelayAuthorScore` substrate type + in-memory map

### Summary

Introduce the substrate-pure `RelayAuthorScore` value type and a single
in-memory `BTreeMap<(Pubkey, RelayUrl), RelayAuthorScore>` slot on `Kernel`.
Pure helpers (`weight`, `decay_now`, `record_hit`, `record_miss`,
`record_failure`) live in the same module — no kernel mutation, no I/O, no
protocol nouns. The map is the single source of truth queried by W4 and
W5; W2 hydrates it from LMDB on construct and snapshots it on flush.

### Files touched (full paths, LOC delta)

- **NEW** `crates/nmp-core/src/kernel/relay_score.rs` — ~180 LOC.
  Defines `RelayAuthorScore { successes: u32, failures: u32, last_used_unix_s: u64 }`,
  constants `WARM_THRESHOLD: f32 = 0.40`, `DECAY_HALFLIFE_DAYS: f32 = 14.0`,
  `MAX_RELAYS_TRIED_PER_CLAIM: usize = 12`, `MAX_EXPANSION_CONCURRENCY: usize = 3`,
  `PHASE_1_BUDGET_MS: u64 = 1500`, `PER_RELAY_REQ_TIMEOUT_MS: u64 = 5000`,
  `PER_CLAIM_TOTAL_BUDGET_MS: u64 = 8000`. Pure helpers.
- **MODIFY** `crates/nmp-core/src/kernel/mod.rs` — +1 mod line at ~115 (alongside
  existing `mod relay_diagnostics;`), +1 struct field on `Kernel` at the
  field-block adjacent to `oneshot_subs` (search for `oneshot: OneshotApi,`
  declaration), +1 init line in `Kernel::new`. Net +6 LOC. **No method impls
  here** — those live in claim_expansion.rs (W5) and score-update seams (W3).
- **NEW** `crates/nmp-core/src/kernel/relay_score_tests.rs` — ~80 LOC; unit
  tests behind `#[cfg(test)]` mod from `kernel/mod.rs`.

### Doctrine guards

- **D0** — `RelayAuthorScore` is keyed `(Pubkey, RelayUrl)` from
  `crates/nmp-planner/src/interest.rs` (substrate types). No `nip65_*` /
  `gigi_*` / `oneshot_*` naming. Doctrine-lint smoke
  (`crates/nmp-testing/tests/framework_magic_contract.rs`) already greps
  for banned tokens.
- **D6** — All helpers return owned values, never `Result`. `weight()` is
  total: a saturated 100-year-old cell with `now = 0` returns `f32` in
  `[0.0, 1.0]` (the `exp` term saturates toward 0).
- **D8** — `BTreeMap` insertion is O(log N) per update — only on EVENT /
  EOSE / Failed seams (edge-triggered). No polling. The map is NOT part
  of any `AppUpdate` snapshot (spec §8.3), so update-equality is preserved.

### Test plan — failing-first

Write **before** the implementation:

1. `relay_score_tests.rs::weight_zero_for_unknown_cell` — a cell with
   `successes=0, failures=0` weighs 0 regardless of age.
2. `relay_score_tests.rs::weight_drops_below_threshold_after_paired_miss`
   — `successes=1, failures=1` evaluates to `1/3 ≈ 0.33 < WARM_THRESHOLD`.
3. `relay_score_tests.rs::weight_above_threshold_after_clean_hit` —
   `successes=1, failures=0, age=0` evaluates to `1/2 = 0.50 >
   WARM_THRESHOLD`.
4. `relay_score_tests.rs::decay_halves_weight_at_14_days` — fixed `now`
   set 14 d after `last_used_unix_s`; weight scales by `~0.5`.
5. `relay_score_tests.rs::record_hit_is_idempotent_in_signature` —
   `record_hit(&mut score, now)` mutates `successes += 1`,
   `last_used_unix_s = now`.
6. `relay_score_tests.rs::kernel_has_empty_score_map_after_new` —
   integration test against `Kernel::new` proving the new field
   initializes empty.

Test scope: `cargo test -p nmp-core --test relay_score_tests` + the
always-on `cargo test -p nmp-testing --test doctrine_lint_smoke`.

### Dependencies

None. Lands first.

---

## Workstream W2 — LMDB sub-db + load/flush wiring

### Summary

Add a fixed-width-record sub-db `relay_author_scores_v1` to the NMP-side
LMDB env, hydrate the W1 map on `Kernel` construction, batch dirty cells
on actor idle, and flush them through a new substrate trait
`RelayAuthorScoreStore`. Schema-bump = sub-db-name bump (no migration
code, per §5 E6).

### Files touched

- **MODIFY** `crates/nmp-nostr-lmdb/src/store/lmdb/mod.rs` —
  `additional_dbs` reserve raise by +1 at line 140; new `Database<Bytes,
  Bytes>` field `relay_author_scores: Database<Bytes, Bytes>` on `Lmdb`
  struct at lines 76-100; open at `open_databases_on_env` body
  (lines 151-220) — +20 LOC. Schema name = `b"relay-author-scores-v1"`.
- **NEW** `crates/nmp-store/src/lmdb/relay_scores.rs` — ~150 LOC. Pure
  encode/decode (fixed 24-byte record: `[u32 successes][u32 failures][u64
  last_used_unix_s][u64 _reserved]`, key `[32-byte pubkey hex bytes][url
  bytes]` — variable-length but pubkey-prefix sortable). Iterator helper
  `load_all()`; transactional `put_batch(&[(Pubkey, RelayUrl,
  RelayAuthorScore)])`.
- **NEW** `crates/nmp-core/src/substrate/relay_score_store.rs` — ~60
  LOC. Trait `RelayAuthorScoreStore { fn load_all(&self) -> Vec<(Pubkey,
  RelayUrl, RelayAuthorScore)>; fn put_batch(&mut self, deltas: &[...]) ;
  }`. `NoopRelayAuthorScoreStore` (D6: tests inject the noop; production
  injects the LMDB-backed impl from `nmp-nostr-lmdb`).
- **MODIFY** `crates/nmp-core/src/kernel/mod.rs` — `Kernel::new` adds the
  store as an injected dependency through the existing app construction
  path (track the `set_clock` precedent at lines around
  `crate::kernel::clock::SystemClock`). +25 LOC.
- **NEW** `crates/nmp-core/src/kernel/relay_score_flush.rs` — ~40 LOC.
  `Kernel::flush_relay_scores_if_dirty()` called once per actor idle
  tick. Keeps mod.rs unchanged for D-V12.

### Doctrine guards

- **D0** — `RelayAuthorScoreStore` is a substrate trait in `nmp-core`;
  the LMDB impl is in `nmp-nostr-lmdb` (substrate-pure boundary).
- **D4** — Single writer (the kernel actor) calls `put_batch`. Loading
  happens once at construct (before the actor loop starts).
- **D6** — Trait methods return owned `Vec` / take `&mut self`, no
  `Result` across FFI; LMDB errors map to a panic-free `log!` line plus
  graceful skip (matches `nmp-nostr-lmdb/src/lib.rs:160`-style precedent).
- **D8** — Flush is gated by a `score_map_dirty: bool` flag set on each
  `record_*` call in W3; clean ticks are zero-allocation.

### Test plan — failing-first

1. `crates/nmp-store/src/lmdb/relay_scores_tests.rs::roundtrip_persists_one_cell`
   — write one cell, reopen the env, assert `load_all()` returns it
   unchanged.
2. `crates/nmp-store/src/lmdb/relay_scores_tests.rs::schema_bump_resets_table`
   — write to `relay-author-scores-v1`, reopen with the hypothetical
   `_v2` name (test-injected constant), assert `load_all()` is empty.
3. `crates/nmp-core/src/kernel/relay_score_flush.rs#[cfg(test)]::flush_is_noop_when_clean`
   — call flush without any prior `record_*`; assert the trait's
   `put_batch` was not called (count via a test double).
4. **Integration** —
   `crates/nmp-testing/tests/relay_score_persistence.rs::scores_survive_kernel_restart`
   — drive a `record_hit` through the score-update seam, run flush, drop
   the kernel, re-construct it pointing at the same env, assert weight ≥
   WARM_THRESHOLD on the same `(pubkey, url)` cell. Underwrites A3.

Test scope: `cargo test -p nmp-store`, `cargo test -p nmp-core --test
relay_score_flush`, `cargo test -p nmp-testing --test
relay_score_persistence`.

### Dependencies

W1 (types).

---

## Workstream W3 — Score-update seam on EVENT / EOSE / FailedAfterRetries

### Summary

Three edge-triggered hooks that translate wire-frame outcomes into score
deltas, all flowing through `Kernel` methods so D4 (single writer) is
trivially preserved. A new `Kernel::record_claim_outcome(author,
relay_url, outcome)` helper centralises the delta logic; the three
ingest seams call it.

### Files touched

- **MODIFY** `crates/nmp-core/src/kernel/ingest/mod.rs` —
  - `handle_event` (called from line 163) — on a matched EVENT for an
    expansion-tracked sub_id, call
    `self.record_claim_outcome(author, relay_url, ClaimOutcome::Hit)`.
    The author is on the event; the relay is the delivering URL.
  - EOSE arm (lines 166-240) — when `is_discovery_oneshot(sub_id)` (or
    the new `is_claim_expansion_oneshot(sub_id)` from W5) AND no EVENT
    was seen for this `sub_id`, call `record_claim_outcome(author,
    relay_url, ClaimOutcome::EoseNoMatch)`. "Author" comes from
    looking up the originating interest on `sub_id` via the new
    `Kernel::lookup_sub_author(sub_id)` helper.
  - Net +35 LOC; under the 500-LOC pressure on ingest/mod.rs (currently
    ~500-ish — verify with `wc -l`) the EOSE arm extraction may need to
    move to `kernel/ingest/eose.rs` if it pushes over. **Defer this
    extraction decision to Agent B's review** if the count is borderline.
- **MODIFY** `crates/nmp-core/src/kernel/relay_transport.rs` —
  `FailedAfterRetries` handler (grep `FailedAfterRetries` in this file —
  it owns the transport-failure dispatch). Call
  `record_claim_outcome(_, relay_url, ClaimOutcome::Failed)` for every
  expansion-tracked author whose route includes the failed relay. +25
  LOC.
- **NEW** `crates/nmp-core/src/kernel/relay_score_record.rs` — ~90 LOC.
  `enum ClaimOutcome { Hit, EoseNoMatch, Failed }`,
  `Kernel::record_claim_outcome` body. Delta table:
  - `Hit` → `successes += 1`, `last_used_unix_s = clock.now_unix_s()`.
  - `EoseNoMatch` → `failures += 1` (small decrement),
    `last_used_unix_s = now`.
  - `Failed` → `failures += 3` (large decrement),
    `last_used_unix_s = now`.
  Each call sets `score_map_dirty = true`.

### Doctrine guards

- **D0** — All deltas operate on `(Pubkey, RelayUrl)`; no protocol noun.
- **D4** — Every call site is `&mut self` on `Kernel`; the registry is
  not touched here (W5 owns interest registration).
- **D6** — `record_claim_outcome` is total; unknown authors / relays
  insert a fresh cell.
- **D8** — The hooks are on already-edge-triggered seams (frame ingest,
  transport-failure callback). No new polling.

### Test plan — failing-first

1. `kernel/relay_score_record.rs#[cfg(test)]::hit_increments_successes_and_sets_now`.
2. `kernel/relay_score_record.rs#[cfg(test)]::eose_no_match_increments_failures_by_one`.
3. `kernel/relay_score_record.rs#[cfg(test)]::failed_after_retries_increments_failures_by_three`.
4. `kernel/relay_score_record.rs#[cfg(test)]::dirty_flag_set_after_any_record`.
5. **Wire-shaped test** —
   `crates/nmp-core/src/kernel/discovery_tests.rs` analogue
   `claim_expansion_event_hit_records_score` — uses the same test_router
   precedent as `discovery_tests.rs:140` (`kernel.complete_unknown_oneshot(sub_id)`)
   but drives an EVENT first.

Test scope: `cargo test -p nmp-core`.

### Dependencies

W1.

---

## Workstream W4 — Phase 1 warm-relay preference in planner

### Summary

The planner gets an injectable `RelayAuthorScoreLookup` (substrate
trait) and consults it for `OneShot` interests with a non-empty
`authors` set. Effect: within the existing `apply_selection`
(`crates/nmp-planner/src/selection.rs:118`), the per-author NIP-65
outbox is filtered to "warm OR connected" before the greedy step. This
is the smallest viable Phase 1 surface — operator-pinned relays still
bypass selection (commit `680666a0`), and the cap stays `max_per_user=2`
on the residual cold set.

### Files touched

- **NEW** `crates/nmp-core/src/substrate/relay_score_lookup.rs` — ~50
  LOC. Trait `RelayAuthorScoreLookup { fn weight(&self, author: &Pubkey,
  relay: &RelayUrl) -> f32; fn is_warm(&self, author: &Pubkey, relay:
  &RelayUrl) -> bool; }`. The `is_warm` default impl is `weight ≥
  WARM_THRESHOLD`. Default impl `NoopRelayAuthorScoreLookup` returns
  `0.0` / `false`.
- **MODIFY** `crates/nmp-core/src/kernel/mailboxes.rs` —
  `KernelMailboxes` gains a fifth Arc (currently NIP-65 cache and
  DM-inbox lookup; see `lifecycle_drain.rs:37-41`) — an
  `Arc<RelayAuthorScoreLookup>` view onto the W1 in-memory map.
  +20 LOC, behind an `impl RelayAuthorScoreLookup for Kernel` block.
- **MODIFY** `crates/nmp-planner/src/selection.rs` — new optional
  parameter `score_lookup: Option<&dyn RelayAuthorScoreLookup>` on
  `apply_selection` (line 118); when `Some`, Stage 1 filters
  `per_relay_authors` to drop `(relay, author)` entries where the relay
  is neither operator-pinned (already exempt at line 117-130) nor warm
  for that author. +60 LOC; the trait import is a new
  `super::interest::RelayAuthorScoreLookup` re-export hop.
- **MODIFY** `crates/nmp-planner/src/lib.rs` — re-export the trait from
  `interest::` so `nmp-core`'s adapter doesn't reach into substrate
  through a back-edge.
- **MODIFY** `crates/nmp-core/src/subs/lifecycle.rs` — call site of
  `apply_selection` (grep for it) — pass `Some(&self.score_lookup)` once
  the field exists on `SubscriptionLifecycle`. +12 LOC.

### Doctrine guards

- **D0** — Trait lives in `nmp-core/substrate/`, impl uses substrate
  types only.
- **D3** — Outbox routing still derives from NIP-65 (lane 1); the score
  acts as a *filter*, never adds a new lane. Selection remains the only
  planner pruning point.
- **D8** — `is_warm` is an O(log N) `BTreeMap` lookup; no allocation per
  call. Planner already runs only on `CompileTrigger` events
  (edge-triggered).

### Test plan — failing-first

1. `crates/nmp-planner/src/selection/tests.rs::warm_lookup_filters_cold_outbox_before_greedy`
   — author has three outbox relays; one is warm; assert the other two
   are eligible only if `max_per_user > 1` and the warm one is picked
   first.
2. `crates/nmp-planner/src/selection/tests.rs::operator_pinned_bypasses_warm_filter`
   — regression: an `AppRelay`-tagged URL that is cold for the author
   still survives.
3. `crates/nmp-planner/src/selection/tests.rs::noop_lookup_preserves_existing_behaviour`
   — proves the new optional parameter doesn't regress today's
   `selection/tests.rs::app_relay_survives_*`.
4. **Adapter** —
   `crates/nmp-core/src/kernel/mailboxes_tests.rs::kernel_mailboxes_exposes_score_lookup`
   (new) — `KernelMailboxes` returns the live score-map view.

Test scope: `cargo test -p nmp-planner`, `cargo test -p nmp-core --test
mailboxes_tests`.

### Dependencies

W1 (the score type the trait returns).

---

## Workstream W5 — `claim_expansion.rs` controller

### Summary

The per-claim Phase 1/2/3 state machine. Owns
`pending_claims: Vec<PendingClaim>` on `Kernel`. Each `PendingClaim`
tracks the originating interest's `(scope, shape)`, author, claim
deadline, attempted relay set, and Phase-2 candidate queue (descending
score). Three public entry points on `impl Kernel`:

- `Kernel::register_claim_expansion(uri_routing) -> ()` — called from
  `crates/nmp-core/src/kernel_action.rs::open_uri` (line 62) right after
  the existing `ensure_sub` call at line 100. Allocates a `PendingClaim`
  and starts Phase 1 (the existing OneShot registration already covers
  this; the controller only stores bookkeeping).
- `Kernel::poll_claim_expansion(now: Instant) -> Vec<OutboundMessage>`
  — called from the actor idle tick after `drain_lifecycle_tick`
  (`lifecycle_drain.rs:36`). Advances any claim past its Phase-1 budget
  to Phase 2 by registering one new `LogicalInterest` per Phase-2
  candidate (up to `MAX_EXPANSION_CONCURRENCY`), each carrying a single
  `RelayHint` (W7).
- `Kernel::on_claim_outcome(sub_id, outcome)` — called from the W3
  hooks. Advances the per-claim state on Hit (terminate claim, drop
  pending entry) / EoseNoMatch (record score; free Phase-2 slot;
  enqueue next candidate) / Failed (same, large decrement).

### Files touched

- **NEW** `crates/nmp-core/src/kernel/claim_expansion.rs` — ~450 LOC.
  - `struct PendingClaim { interest_id: InterestId, author: Pubkey,
    shape: InterestShape, started_at: Instant, phase: Phase, attempted:
    BTreeSet<RelayUrl>, candidate_queue: VecDeque<RelayUrl>,
    in_flight_subs: BTreeMap<String /*sub_id*/, RelayUrl>, }`.
  - `enum Phase { Phase1, Phase2, Terminal(ClaimTermination) }`.
  - `enum ClaimTermination { Hit, Exhausted, Budget }`.
  - Three impl methods above + private helpers
    (`candidate_queue_for_author`, `enqueue_next_phase2_attempt`,
    `is_claim_expansion_oneshot`).
- **NEW** `crates/nmp-core/src/kernel/claim_expansion_tests.rs` — ~250
  LOC.
- **MODIFY** `crates/nmp-core/src/kernel/mod.rs` —
  - `mod claim_expansion;` + `#[cfg(test)] mod claim_expansion_tests;`
    at the mod-block (currently ends around line 155).
  - `pending_claims: Vec<PendingClaim>` field on `Kernel` (one line).
  - `Kernel::new` initializes it.
  Net +6 LOC.
- **MODIFY** `crates/nmp-core/src/kernel_action.rs:62-103` — after the
  `ensure_sub` call at line 100, call
  `kernel.register_claim_expansion(...)`. +5 LOC.

### Doctrine guards

- **D0** — `PendingClaim` is substrate-typed only.
- **D4** — Every mutation goes through `Kernel::*` methods; the registry
  is reached only via the existing `self.lifecycle.registry_mut()` path
  the OneshotApi uses (`subs/oneshot.rs:127`). No back-door.
- **D6** — No `Result` returns; unknown `sub_id` → no-op; the
  expansion-relays cap (`MAX_RELAYS_TRIED_PER_CLAIM`) terminates the
  claim deterministically rather than looping.
- **D8** — `poll_claim_expansion` is O(active_claims); idle tick with
  zero pending claims is a length-0 vec check. Wall-clock checks against
  `Instant::now()` are the same pattern as
  `crates/nmp-core/src/actor/pending_sign.rs:130` (`Instant::now() >=
  self.deadline`) — established no-polling-doctrine-compliant idiom.

### Test plan — failing-first

1. `claim_expansion_tests::phase1_hit_terminates_without_phase2`.
2. `claim_expansion_tests::phase1_eose_advances_to_phase2_after_budget`
   — uses the `set_clock` / `FixedClock` precedent
   (`crates/nmp-core/src/kernel/clock.rs:45`) to fast-forward 1500 ms.
3. `claim_expansion_tests::phase2_concurrency_capped_at_3`.
4. `claim_expansion_tests::phase2_candidates_ordered_by_score_desc_then_lex_desc`
   — covers spec §10 Q6.
5. `claim_expansion_tests::phase2_exhausts_then_terminates`.
6. `claim_expansion_tests::phase2_per_claim_total_budget_terminates_user_visible`
   — Hit-after-budget still updates scores (covers spec §10 Q7 in code).
7. `claim_expansion_tests::concurrent_claims_for_same_author_share_score_writeback`
   — covers A6: two registrations, A's hit updates the map, B's
   subsequent recompile picks the warm relay in Phase 1.
8. `claim_expansion_tests::max_relays_tried_per_claim_capped_at_12`.

Test scope: `cargo test -p nmp-core --test claim_expansion_tests`.

### Dependencies

W1 (types), W3 (outcome seam), W7 (hint consumption to actually route
Phase-2 REQs).

### File-size discipline (D-V12)

`kernel/mod.rs` is already 1877 LOC — well over the 500 LOC ceiling
(pre-existing violation; see V-12 backlog memory). W5 adds 6 LOC to
mod.rs (only the field and mod declarations) and ~450 LOC to the new
`claim_expansion.rs`, which stays under the ceiling.
`claim_expansion_tests.rs` is `#[cfg(test)]` and is exempt by the
existing convention (`identity.rs` test extraction precedent — memory
note e79f7a90). **Do not** allow any new impl methods to land in
`mod.rs` proper; the natural seam is the claim_expansion module
itself.

---

## Workstream W6 — Edge-triggered deadline polling on actor idle tick

### Summary

`Kernel::poll_claim_expansion` (W5) needs to be called from the actor
idle tick. The pattern already exists for `pending_sign` (see
`crates/nmp-core/src/actor/pending_sign.rs` — `deadline` field, idle
tick `retain_mut` precedent at lines 161-191). We extend that pattern:
add a call site in the actor's idle section that drains
`Kernel::poll_claim_expansion`, then converts any returned
`OutboundMessage`s back through the existing
`wire_frames_to_outbound`-style path.

### Files touched

- **MODIFY** `crates/nmp-core/src/actor/loop.rs` (or wherever the idle
  branch lives — grep `pending_sign` in `crates/nmp-core/src/actor/` to
  find the precedent call site). Add a sibling call:
  ```
  outbound.extend(kernel.poll_claim_expansion(Instant::now()));
  ```
  +5 LOC.
- **NO new infrastructure.** Confirm during W5 implementation that the
  idle-tick frequency (existing `emit_hz` at
  `crates/nmp-core/src/actor/dispatch.rs:206`) gives sub-100ms polling
  resolution. Default `DEFAULT_EMIT_HZ` is 4 Hz (250 ms) — that's
  coarser than the spec's 1500 ms budget by a factor of 6, comfortably
  inside the budget's resolution requirement.

### Doctrine guards

- **D8** — No new polling. The actor idle tick is the existing
  wall-clock-gated observer the spec §4.1 references (driving
  `drain_lifecycle_tick`). We are adding **one more callee**, not a new
  loop.
- **D6** — `poll_claim_expansion` is total and side-effect-free for an
  empty `pending_claims` vec.

### Test plan — failing-first

1. `crates/nmp-testing/tests/t142_drain_tick_actor_idle_loop.rs` — extend
   the existing test to also assert that `poll_claim_expansion` is
   called once per drain cycle (use a counter probe on a test-injected
   kernel).

Test scope: `cargo test -p nmp-testing --test
t142_drain_tick_actor_idle_loop`.

### Dependencies

W5.

---

## Workstream W7 — Hint consumption in `compiler/partition/case_a*.rs`

### Summary

`LogicalInterest.hints` (defined at
`crates/nmp-planner/src/interest.rs:280`) is currently parsed but never
routed. All four partition cases set `hints: Vec::new()` on the inner
interest (`case_b_addresses.rs:104`, `case_d_no_author.rs:158,257`). W5
depends on the planner *actually* honouring `RelayHint` entries on
Phase-2 expansion interests; this workstream wires that consumption.
Scope is narrow: case_a (the only case that fires on `authors`-shape
oneshots, which is the spec's claim path) AND case_b for addressable
events (Gigi's article is `kind:30023`, routed via case_b).

### Files touched

- **MODIFY** `crates/nmp-planner/src/compiler/partition/case_a_authors.rs`
  — after the existing mailbox-resolved route emission, walk
  `interest.hints` and emit one additional `RelayEntry` per hint with
  `RoutingSource::Hint`. Skip a hint whose URL is already in the route
  set for this interest (dedupe). +40 LOC.
- **MODIFY** `crates/nmp-planner/src/compiler/partition/case_b_addresses.rs`
  — same change; line 104 is where `hints: Vec::new()` is set today
  (note: that line is the *sub-interest*'s hint vec, not the parent —
  verify before patching). +40 LOC.
- **NEW** test module `crates/nmp-planner/src/compiler/partition/hint_consumption_tests.rs`
  — ~80 LOC.

### Doctrine guards

- **D3** — Hints become a new lane (`RoutingSource::Hint`, already
  defined at `crates/nmp-planner/src/plan.rs:103`); the four-lane
  diagnostic discipline is preserved. **Indexer fallback** is unchanged
  — hints do not bypass `unroutable_authors`.
- **D6** — Malformed hints (non-canonical URL) are dropped silently;
  `canonical_relay_url` returns `None` → skip.
- **D8** — Hint walk is O(hints.len()) per interest; oneshot interests
  carry ≤1 hint by construction in W5 (one candidate per expansion
  attempt), so this is constant-time in practice.

### Test plan — failing-first

1. `hint_consumption_tests::single_user_configured_hint_routes_to_that_relay_in_case_a`.
2. `hint_consumption_tests::hint_routes_independently_of_nip65_outbox`
   — author with a known mailbox AND a hint: assert both relays appear
   with their respective `RoutingSource` lanes.
3. `hint_consumption_tests::hint_dedup_against_existing_route` — a hint
   matching the author's existing NIP-65 outbox produces one `RelayEntry`
   with both `Nip65` and `Hint` in `role_tags`, not two.
4. `hint_consumption_tests::case_b_addressable_with_hint_routes_per_hint`
   — same shape, for kind:30023 / addressable cases.
5. `hint_consumption_tests::malformed_hint_url_silently_dropped`.

Test scope: `cargo test -p nmp-planner`.

### Dependencies

None — independent of W1–W6. Can land in parallel with W1.

---

## Workstream W8 — `NMP_WIRE_LOG` telemetry seam

### Summary

A1 requires reading wire-log output that does not exist today
(`grep NMP_WIRE_LOG` returns only the spec doc). This workstream adds an
env-gated structured emitter through the existing `Kernel::log` seam
(used at `kernel/ingest/mod.rs:146, 239, 251`). Output is plain stderr
when `NMP_WIRE_LOG` is set; otherwise no allocation, no I/O.

### Files touched

- **NEW** `crates/nmp-core/src/kernel/wire_log.rs` — ~70 LOC.
  ```
  pub(crate) enum WireLogEvent<'a> {
      ReqEmit { sub_id: &'a str, relay_url: &'a str, phase: &'a str, ... },
      EoseRx  { sub_id: &'a str, relay_url: &'a str, matched: bool },
      EventRx { sub_id: &'a str, relay_url: &'a str, event_id: &'a str, author: &'a str },
      ClaimPhaseAdvance { author: &'a str, from: &'a str, to: &'a str, reason: &'a str },
      ScoreUpdate { author: &'a str, relay_url: &'a str, delta: &'a str, new_weight: f32 },
  }
  pub(crate) fn log_wire(event: WireLogEvent<'_>) {
      if std::env::var_os("NMP_WIRE_LOG").is_none() { return; }
      eprintln!("nmp.wire {}", serde_json::to_string(&event).unwrap_or_default());
  }
  ```
- **MODIFY** call sites in `kernel/ingest/mod.rs` (EVENT line 160-165;
  EOSE line 166-240), `kernel/relay_transport.rs` (FailedAfterRetries),
  `kernel/claim_expansion.rs` (W5 — at every phase transition), and
  `kernel/relay_score_record.rs` (W3 — every `record_*` call). +1
  one-liner per call site, ~10 sites, ~20 LOC.

### Doctrine guards

- **D6** — `unwrap_or_default()` on the JSON encode means an
  unrenderable event silently produces `""` — never a panic.
- **D8** — `env::var_os` is the early-bailout (an OS syscall per call —
  measure during W9 if hot; if measurable, cache in a `OnceLock<bool>`
  at module load).

### Test plan — failing-first

1. `kernel/wire_log_tests.rs::env_unset_silences_output` — set `unset
   NMP_WIRE_LOG`, capture stderr, assert empty.
2. `kernel/wire_log_tests.rs::env_set_emits_one_line_per_event`.
3. `kernel/wire_log_tests.rs::output_line_starts_with_nmp_wire`.

Test scope: `cargo test -p nmp-core --test wire_log_tests`.

### Dependencies

W3, W5 (the events to emit are defined there).

---

## Workstream W9 — Acceptance integration tests A1–A6

### Summary

The end-to-end harness against a *real* relay. Each acceptance criterion
gets one integration test in `crates/nmp-testing/tests/`, modelled on
`real_relay_outbox.rs` (the existing real-relay precedent — same
crate). Tests are gated behind the `real-relay` cargo feature so they
stay out of the default scoped-test path.

### Files touched

- **NEW** `crates/nmp-testing/tests/relay_search_radius_a1_cold_claim.rs`
  — A1: Gigi article cold-claim against `app_relays =
  [purplepag.es]`. Captures stderr (`NMP_WIRE_LOG=1`) and asserts:
  1. `ReqEmit phase=phase1 relay_url=purplepag.es` present.
  2. `ReqEmit phase=phase2 relay_url=<dergigi or other outbox>` present.
  3. `EventRx ... author=<gigi_pk>` present.
  4. Claim resolves in `< 5500 ms` wall-clock.
- **NEW** `crates/nmp-testing/tests/relay_search_radius_a2_warm_path.rs`
  — A2: prime the score map by replaying A1, then issue a second claim
  for a *different* Gigi event; assert the delivering relay from A1
  appears in the Phase-1 ReqEmit set (`phase=phase1`).
- **NEW** `crates/nmp-testing/tests/relay_search_radius_a3_restart_persistence.rs`
  — A3: prime, drop the kernel, re-open against the same store, claim
  → assert Phase 1 hit, no `phase=phase2` line.
- **NEW** `crates/nmp-testing/tests/relay_search_radius_a4_doctrine_lint.rs`
  — A4: just runs `cargo test -p nmp-testing --test
  doctrine_lint_smoke` in-process or as a build-time guard. (May be a
  no-op if the smoke test catches all banned tokens; verify during
  W9.)
- **NEW** `crates/nmp-testing/tests/relay_search_radius_a5_mid_claim_unreachable.rs`
  — A5: spawn a stub relay that drops the connection mid-claim; assert
  the search advances within the wall-clock budget and the
  `FailedAfterRetries` outcome records a `failures += 3` delta.
- **NEW** `crates/nmp-testing/tests/relay_search_radius_a6_concurrent_claims.rs`
  — A6: register two distinct claims for events authored by the same
  author; assert claim B's compile pass sees claim A's score delta if it
  registers strictly after A's first scoring frame.

Each test ships its own `_relay_log_capture` helper (or a shared one in
`crates/nmp-testing/tests/common/wire_log.rs`).

### Doctrine guards

- A4 closes the loop.
- A1–A3 + A5–A6 use real-relay sockets; gated under
  `--features real-relay` so they don't break the scoped-test cadence in
  CLAUDE.md.

### Test plan

Tests ARE the deliverable here. No further test plan.

### Dependencies

W1–W8 all green.

---

## 2. Sequencing diagram

```text
        ┌─────┐     ┌─────┐     ┌─────┐
        │ W1  │     │ W7  │     │ A1? │   (Phase-3 entrypoints; not blocking)
        └──┬──┘     └──┬──┘     └─────┘
           │           │
   ┌───────┼───────┐   │
   │       │       │   │
   ▼       ▼       ▼   │
  W2      W3      W4   │
   │       │       │   │
   │       └───┬───┘   │
   │           │       │
   │           ▼       │
   │           W5 ◄────┘
   │           │
   │           ▼
   │           W6
   │           │
   └────►─W8◄──┘
              │
              ▼
              W9
```

**Critical path:** W1 → W3 → W5 → W6 → W9 (≈ 5 serial steps).
**Parallel slack:** W2 (LMDB) and W7 (hints) and W4 (planner) can all run
alongside W3/W5/W6 once W1 lands.

---

## 3. Risk ledger (top 5)

| # | Risk | Mitigation |
|---|---|---|
| **R1** | **D4 single-writer race** — A6 requires claim B's compile pass to read claim A's score delta from the same actor tick. If the planner reads LMDB (which only flushes on idle, NOT per-frame), the read is stale. | The planner reads **only** the in-memory `BTreeMap` via `RelayAuthorScoreLookup` (W4). LMDB is the *durability* layer (load at construct, batched put on idle), never the *read* layer during a live tick. W2 keeps the load-path strictly at `Kernel::new`. W3 marks `score_map_dirty = true` synchronously inside the same actor mutation that handles the inbound frame, so the next compile pass in the same idle window sees the update. **Tests A6 + `concurrent_claims_for_same_author_share_score_writeback` (W5)** are the regression. |
| R2 | **`mod.rs` file-size violation drift** — `kernel/mod.rs` is already 1877 LOC. Even a small addition per workstream compounds. | Strict rule: only field declarations + mod-block lines in mod.rs. Every method body lives in its own file (W3 → `relay_score_record.rs`, W5 → `claim_expansion.rs`, W2 → `relay_score_flush.rs`, W1 → `relay_score.rs`). Net additions to mod.rs ≤ 20 LOC across all workstreams. Agent B should flag any drift. |
| R3 | **EOSE-without-author** — the EOSE arm in `kernel/ingest/mod.rs:166` does not have the originating author at hand; it has the sub_id. The W3 score-update needs `(author, relay)`. | Introduce `Kernel::lookup_sub_author(sub_id) -> Option<Pubkey>` in W5 (the `PendingClaim.in_flight_subs` map already keys `sub_id → relay_url`; extending it to `sub_id → (relay_url, author)` is the same allocation). For unknown `sub_id` (non-claim oneshot), return `None` and skip scoring. Documented in `claim_expansion.rs`. |
| R4 | **Hint consumption in partition (W7) is a planner-correctness change** — could regress existing routing in unexpected cases. | All today's call sites set `hints: Vec::new()` (verified at `partition/case_b_addresses.rs:104`, `case_d_no_author.rs:158,257`). The new hint-walk is a *no-op* when hints is empty. `hint_consumption_tests::noop_when_hints_empty` is a required regression. |
| R5 | **`NMP_WIRE_LOG` env-var hot-path overhead** — emitted from EVENT/EOSE seams; `env::var_os` is a syscall. | Cache the bool in a `OnceLock<bool>` at module load — if `NMP_WIRE_LOG` is set after startup it won't take effect, but that matches the convention for other env-gated flags. The cost-when-unset is one atomic load. Measure during W9 to confirm. |

---

## 4. Acceptance test plan — A1–A6

| Ai | Test file | Wire-log assertions | Lives in |
|---|---|---|---|
| **A1** | `crates/nmp-testing/tests/relay_search_radius_a1_cold_claim.rs` | `ReqEmit phase=phase1 relay_url=purplepag.es` present; ≥1 `ReqEmit phase=phase2 relay_url=<gigi-outbox-url>` line; `EventRx author=<gigi_pk>`; wall-clock resolution < 5500 ms. | `nmp-testing` (real-relay) |
| **A2** | `crates/nmp-testing/tests/relay_search_radius_a2_warm_path.rs` | After A1 priming + 2nd claim: assert the delivering URL from A1 appears in `phase=phase1` ReqEmit set for the 2nd claim; no `phase=phase2` ReqEmit until/unless P1 EOSEs. | `nmp-testing` (real-relay) |
| **A3** | `crates/nmp-testing/tests/relay_search_radius_a3_restart_persistence.rs` | Same as A2 across a kernel-drop-and-reopen against the same store path; assert `phase=phase1` hit and **no** `phase=phase2` lines. | `nmp-testing` (real-relay) |
| **A4** | `crates/nmp-testing/tests/relay_search_radius_a4_doctrine_lint.rs` (thin wrapper) | Invokes `doctrine_lint_smoke`; asserts no banned-token regression. | `nmp-testing` |
| **A5** | `crates/nmp-testing/tests/relay_search_radius_a5_mid_claim_unreachable.rs` | Stub relay drops connection after CONNECT, before EOSE; assert `ScoreUpdate ... delta=failed_after_retries new_weight=<lower>` line; assert claim resolves to a different relay within PER_CLAIM_TOTAL_BUDGET_MS. | `nmp-testing` (stub-relay) |
| **A6** | `crates/nmp-testing/tests/relay_search_radius_a6_concurrent_claims.rs` | Register claim A, drive its EVENT (assert `ScoreUpdate`); register claim B; assert claim B's first `ReqEmit phase=phase1` set contains the relay that just scored. | `nmp-testing` (real-relay) |

---

## 5. Out-of-scope reaffirmation (spec §11)

Implicit scope additions discovered during planning — **all flagged as
follow-ups, none silently absorbed**:

1. **`nmp_app_claim_event` FFI symbol is absent.** Only
   `nmp_app_claim_profile` exists (`crates/nmp-ffi/src/timeline.rs:75`).
   The spec calls the path "`nmp_app_claim_event(uri)`"; the actual entry
   point today is `nmp_app_open_uri` → `KernelAction::OpenUri`
   (`crates/nmp-core/src/kernel_action.rs:62`). **This plan wires the
   feature against `OpenUri` for `naddr`/`nevent` URIs** (the existing
   path), NOT against a new FFI symbol. If product wants the explicit
   `nmp_app_claim_event` symbol shape, that's a separate follow-up PR.
2. **`NMP_WIRE_LOG` does not exist.** Workstream W8 introduces it
   because A1 depends on it — but the spec did not list it as a
   deliverable, so I'm calling it out as scope-creep-because-necessary.
   Agent B should confirm.
3. **`LogicalInterest.hints` is unconsumed by partition (W7).** A
   prerequisite the spec assumes is free. It is not. W7 makes it work.
4. **`mod.rs` file-size discipline.** This plan does not attempt to
   split `kernel/mod.rs` to comply with D-V12 — that's the V-12 backlog
   item, separate from this feature.
5. **A4 narrowly checks `doctrine_lint_smoke`.** If the smoke test does
   not yet cover the new banned tokens (e.g., a hypothetical
   `claim_expansion_*` token introduced by typo), Agent B should add the
   token to the smoke test's banned list.

---

## 6. Deferred to Agent B's review

- **Spec §10 Q6 — NIP-11 tiebreaker.** I chose lex-DESC URL. Agent B
  should confirm there is no in-tree NIP-11 cache I missed; if there
  is, it might be a better tiebreaker.
- **Risk R3 — `sub_id → author` lookup placement.** I propose extending
  `PendingClaim.in_flight_subs`. Alternative: a separate
  `Kernel::sub_to_author: BTreeMap<String, Pubkey>` on `Kernel` directly.
  The first is local; the second is general-purpose. Agent B picks.
- **Workstream W6 — exact actor idle-tick call-site file.** I wrote
  "`actor/loop.rs`" but the actual file may be elsewhere (the actor
  module is laid out across `dispatch.rs`, `pending_sign.rs`, etc.).
  The implementer will grep `pending_sign.rs::poll` to find the precedent
  call site.
- **Whether to split `kernel/ingest/mod.rs`** if W3's EOSE arm changes
  push it past 500 LOC. Recommendation: extract `kernel/ingest/eose.rs`
  in a separate small refactor PR *before* W3 lands.

---

## 7. Post-#599 retarget (2026-05-27)

PR #599 merged after Agent A authored §0–§6. It shifted the baseline this
plan was written against. This section is the **authoritative delta**;
where it conflicts with §0–§6, this section wins.

### 7.1 What PR #599 already landed

Three items §5 listed as out-of-scope **now exist on master**:

1. **`nmp_app_claim_event` FFI symbol** —
   `crates/nmp-ffi/src/timeline.rs:133`. Re-exported in
   `crates/nmp-ffi/src/lib.rs:107`. Symmetric with `nmp_app_claim_profile`
   at `timeline.rs:76`.
2. **Kernel `claim_event` / `release_event`** —
   `crates/nmp-core/src/kernel/requests/event.rs:56` and `:215`. Parses
   `nostr:` URI → `InterestShape` (NIP-01 §3.7 `{kinds, authors, "#d"}`
   for naddr, `{ids}` for nevent) → `OneshotApi::request` →
   `pending_discovery_oneshots.insert(interest_id, token)` →
   `event_claim_requested.insert(primary_id)` →
   `CompileTrigger::ViewOpened`.
3. **Operator-pinned `AppRelay` selection bypass** —
   `crates/nmp-planner/src/selection.rs:84-95` (`is_app_relay`) and
   `:117-146` (the bypass in `apply_selection`). This is the planner half
   of issue #632; the spec already references this as commit `680666a0`
   (now merged on master as part of #599).

`NMP_WIRE_LOG` **also exists on master** but with different semantics
than W8 proposed — see §7.4 below.

### 7.2 §5 out-of-scope ledger — supersedes items 1 and 3

- **Item §5.1 (claim_event FFI absent)** — superseded. The feature
  hooks `claim_event` at `requests/event.rs:197` directly, NOT
  `kernel_action.rs::open_uri`. See §7.3.
- **Item §5.3 (`LogicalInterest.hints` unconsumed)** — still applies.
  Verified at `interest.rs:280`; the four partition cases still set
  `hints: Vec::new()`. W7 is unchanged.
- **Items §5.2, §5.4, §5.5** — unchanged.

### 7.3 W5 entry point — supersedes §W5 "Files touched" first bullet

§W5 says: "called from `crates/nmp-core/src/kernel_action.rs::open_uri`
(line 62) right after the existing `ensure_sub` call at line 100."

**Retarget.** The claim-expansion controller now hooks
`crates/nmp-core/src/kernel/requests/event.rs::claim_event` immediately
after the existing line `:199`:

```
self.pending_discovery_oneshots.insert(interest_id, token);
self.event_claim_requested.insert(primary_id);
// ←  W5 INSERTION POINT
//    self.register_claim_expansion(interest_id, primary_id.clone(), shape.clone(), now)
```

Rationale:
- This is the single registration site for event claims on master.
  `kernel_action.rs::open_uri` is the *generic* URI dispatcher; for
  `nevent`/`naddr` it now delegates routing through the same
  `OneshotApi::request` path via `resolve_open_uri` (verify in
  `kernel/app/uri.rs` if implementing).
- The new hook point already has `interest_id`, `primary_id`, and
  `shape` in scope — no extra plumbing needed.
- `claim_profile` (`requests/profile.rs`) is the **out-of-scope twin**.
  Profile expansion follows the indexer lane (NIP-65 inbox), not author
  outbox; the spec is event-centric.

`PendingClaim.interest_id` now stores the `InterestId` returned by
`oneshot.request`. The author for `EventRx`/`EoseNoMatch` scoring comes
from the `InterestShape.authors` set for naddr claims (single-author by
construction at `requests/event.rs:125`) and from the **EVENT pubkey**
for nevent claims (the URI carries no author — the score row is created
lazily on the first EVENT arrival).

### 7.4 W8 env-var rename — supersedes §W8 entirely on the env var

§W8 proposes `NMP_WIRE_LOG` as an env-gated **stderr structured
emitter** of `WireLogEvent::{ReqEmit, EoseRx, EventRx,
ClaimPhaseAdvance, ScoreUpdate}` JSON lines.

**Conflict.** `NMP_WIRE_LOG` is already taken at
`crates/nmp-network/src/relay_worker/socket_io.rs:13-40` as a
**file-path-based raw-frame logger** (`NMP_WIRE_LOG=/tmp/wire.log`,
appends `[ts] <relay> → <text>\n` per write/read frame). The two
semantics are not compatible — one is "wire frames to a file", the
other is "semantic claim events to stderr". They log different things
at different layers.

**Retarget.** W8 uses a distinct env var:

- **New name**: `NMP_CLAIM_LOG` (env-gated, stderr, structured JSON).
- **Rationale**: matches "claim expansion" (the feature) rather than
  "wire frames" (the lower transport seam). The two can coexist; A1's
  acceptance test reads `NMP_CLAIM_LOG`, A5 can read both.
- **Alternative considered**: piggyback on `NMP_WIRE_LOG` by detecting
  a non-path value (e.g. `NMP_WIRE_LOG=1`). Rejected: the existing
  impl unconditionally calls `PathBuf::from` on the value and tries to
  open it — adding a sentinel would require a second seam in
  `socket_io.rs`, breaking D0 layering (semantic claim events belong
  in `nmp-core`, not in `nmp-network`).

All other W8 internals (event enum, JSON encoding,
`unwrap_or_default`, `OnceLock<bool>` early-bailout cache) stand.

A1/A2/A3/A5/A6 acceptance tests in W9 read `NMP_CLAIM_LOG` for
semantic-event assertions; they may **additionally** set
`NMP_WIRE_LOG=<tmpfile>` to capture raw frames for cross-checking
(belt-and-braces — useful for debugging A5's mid-claim disconnect).

### 7.5 §1 workstream table — adjusted LOC

W5's "+450 LOC" budget previously included a private `lookup_sub_author`
helper. With `pending_discovery_oneshots: HashMap<InterestId, String>`
(`kernel/mod.rs:672`) already mapping `interest_id → token`, the lookup
becomes a `PendingClaim.author: Pubkey` lookup keyed off the same
`interest_id` — no kernel-wide helper needed. Net delta: −20 LOC on
W5, no W3 change.

### 7.6 §1 file-size drift

§W5 says `kernel/mod.rs` is 1877 LOC. Current count: **1952 LOC**
(`wc -l crates/nmp-core/src/kernel/mod.rs`). The R2 risk (mod.rs LOC
drift) is now sharper: every field/mod-line addition is on top of an
already-larger pre-existing violation. Strict no-impls-in-mod.rs rule
stands; budget the entire feature at ≤25 mod.rs additions.

§W3's caution about `kernel/ingest/mod.rs` exceeding 500 LOC is now
**already realised**: `wc -l` reports **645 LOC**. The extraction to
`kernel/ingest/eose.rs` recommended in §6 is no longer optional — W3
must land it as a prerequisite refactor (own commit) to avoid further
inflating the violation.

### 7.7 §1 line-number drift catalog

Non-load-bearing but worth recording for the implementer's grep:

| Plan ref | Plan line | Master line | Notes |
|---|---|---|---|
| `selection.rs::apply_selection` | `:118` | `:133` | unchanged signature |
| `selection.rs` operator-bypass | (commit `680666a0`) | `:117-146` | now landed on master |
| `oneshot.rs::request` | `:97-137` | `:97-…` | unchanged |
| `interest.rs::hints` | `:280` | `:280` | unchanged |
| `kernel_action.rs::open_uri` | `:62-103` | `:62-…` | unchanged but no longer the hook (see §7.3) |
| `kernel/mod.rs` | 1877 LOC | 1952 LOC | drifted up |
| `kernel/ingest/mod.rs` | "~500" | 645 LOC | over ceiling — pre-extract |
| `kernel/discovery.rs` | 251 LOC | 251 LOC | unchanged |
| `lmdb/mod.rs additional_dbs` | `:131-146` | `:114, :135, :140` | API stable, lines shifted |

### 7.8 Reviewer note for Agent B

The four items §6 deferred to Agent B remain on the table:

1. NIP-11 tiebreaker — confirm no NIP-11 cache exists; if found, choose
   intersection over lex-DESC.
2. `sub_id → author` lookup placement — §7.3 now proposes `PendingClaim`
   carries the author directly (interest_id keys both maps). Confirm or
   suggest the kernel-wide `BTreeMap<String, Pubkey>` alternative.
3. Actor idle-tick call-site — implementer greps
   `pending_sign.rs::poll`; reviewer confirms the right grep target.
4. Ingest/mod.rs split — §7.6 elevates from "if it overflows" to
   "required pre-refactor"; reviewer confirms scope.

Plus three new items raised by this retarget:

5. **§7.3 author lookup for nevent claims** — the URI carries no
   author; the score row gets created on the first EVENT. Is the
   "create-on-first-EVENT" semantics OK, or should nevent claims skip
   scoring entirely (they have no author signal until they resolve)?
6. **§7.4 env-var rename** — `NMP_CLAIM_LOG` vs. piggyback vs. some
   third option (e.g. `NMP_TRACE=claim,wire,...` comma-list). Pick.
7. **§7.6 ingest/mod.rs pre-extraction** — should W3 land the
   `kernel/ingest/eose.rs` extraction as its first commit, or should
   a separate refactor PR land first? (Recommendation: land it as W3
   commit 1; small and self-contained.)

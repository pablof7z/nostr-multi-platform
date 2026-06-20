# ADR-0056 — K3 coverage ledger: staged migration from presence-floor to per-(filter, relay) coverage

- Status: Accepted / Implemented (Stages A–E all landed)
- Date: 2026-06-14
- Keystone: K3 (read-path soundness) of the nmp-multi-platform excellence program
- Doctrine: `doctrine:d2` (negentropy-first), `doctrine:d8` (bounded work)
- Related: ADR-0045 (store→projection cache-serve, the single-mechanism replay this
  ADR's invariant rides on), #1090 / #1327 (floor-coherent eviction —
  `kernel/ram_eviction_floor.rs`, the eviction⇄watermark coherence rule that this
  ADR no longer needs to introduce), #1087 / #1091 / V-118 (author-aware watermark
  min/abort rule — the partial H1 fix already in master).

## 1. Problem — "presence is not coverage"

The live `since`-floor for a subscription is **content-derived**: the kernel's
`watermark_fn` (`crates/nmp-core/src/kernel/mod.rs`, the closure installed at
`set_watermark_fn`, currently around lines 1772–1888) floors each non-ephemeral
REQ's `since` to "newest STORED event matching the shape + 1" so a freshly-opened
REQ does not re-fetch events already on disk (T129 / NDK `addSinceFromCache`
heritage). `apply_watermark_rewrite`
(`crates/nmp-core/src/subs/recompile.rs:316`) applies that floor to each
sub-shape's filter.

The floor answers the question *"what is the newest thing I have that looks like
this shape?"* — **presence**. Read-path soundness needs the answer to a different
question: *"through what timestamp have I actually COMPLETED a sync for this
(filter, relay)?"* — **coverage**. Presence ≠ coverage, and the gap between them
is a class of permanent backfill holes.

### 1.1 The two consequences the 16-journey review named

- **H1 — cross-shape floor poisoning.** A single stored event authored by a user
  (regardless of which shape fetched it — a thread reply seen before you followed
  them is still a `kind:1` event by that author, and so lands in the
  `idx_author_kind` index) floors that author's follow-feed shape above the event,
  permanently suppressing their older history from backfill.

- **H2 — NEG-OPEN inherits the floor.** NIP-77 negentropy set-reconciliation
  inherits the watermark-floored `since` through `ReqFrameContext.filter_json`
  (`crates/nmp-core/src/actor/outbound.rs:66`, then
  `crates/nmp-nip77/src/runtime.rs::intercept_req` →
  `crates/nmp-nip77/src/filter.rs::local_items`). Reconciliation therefore only
  covers `[floor, ∞)` — exactly the window the floor already declared boring — so
  it **cannot repair below-floor gaps**, defeating the one mechanism that could
  have self-healed H1.

## 2. Recon — what is true in master as of 2026-06-14

This ADR was written against current code, not the review snapshot. Several review
premises have moved:

1. **`WatermarkRow` / `crates/nmp-store/.../types/watermark.rs` NO LONGER EXIST.**
   The persisted-watermark machinery (`nmp-watermarks` LMDB sub-db) was deleted as
   dead, zero-caller code in **#1090 Stage 3** (see
   `crates/nmp-store/src/lmdb/open.rs:34-35`). The review's plan to "wire the
   dormant `WatermarkRow`" is therefore obsolete: **Stage D must RE-CREATE the
   ledger type and its sub-db**, not re-activate a dormant one. Any doc still
   claiming the ledger "[LANDED M4]" is false and is corrected in Stage E.

2. **H1 is PARTIALLY fixed.** The author branch of `watermark_fn` now computes
   `min(per-author newest)` and returns `None` (no floor, full backfill) if ANY
   author in the shape has zero stored events (V-118,
   `kernel/mod.rs` author loop ~lines 1866–1887). So the "newly-followed author"
   sub-case is already sound. H1 still bites in the narrower case where every
   author in the shape has ≥1 stored event but some have a below-newest gap
   (single-author thread feeds; partial-knowledge follow feeds).

3. **H2 STILL HOLDS** exactly as described — verified in
   `runtime.rs::intercept_req` and `filter.rs::local_items` (both consume the
   floored `since`).

4. **The eviction⇄watermark coherence rule already landed** (#1090 Stage 2 +
   #1327): `crates/nmp-core/src/kernel/ram_eviction_floor.rs::shape_floor` +
   `pin_shape_events_below_floor` pin every stored event at or below the floor so
   LRU eviction cannot punch a hole the floor would then suppress. This means the
   review's "co-land eviction coherence in Stage D" item is **already satisfied**;
   Stage D only needs to keep the ledger and the pin-set reading the same floor.

5. **`shape_floor` is a hand-synced SECOND copy of the predicate.** The floor is
   now computed in two places that "MUST stay in lockstep" by comment and by the
   `cache_serve_budget_tests` floored⇒served guard: `watermark_fn`
   (`kernel/mod.rs`) and `shape_floor` (`ram_eviction_floor.rs`). A third mapping,
   `shape_to_store_queries` in `kernel/cache_serve/queries.rs`, drives the
   store-serve. Three hand-synced copies of "which events match this shape" is the
   migration hazard Stage C removes.

6. **ADR-0045 cache-serve has landed** (`crates/nmp-core/src/kernel/cache_serve/`)
   and its §6 invariant — *"no watermark floor without replay coverage for the
   same shape"* — holds by construction. Crucially this guarantees the floored
   events you ALREADY HAVE are replayed from the store; it does **not** fetch
   events you never had. The below-floor *gap* (never-fetched events) is precisely
   what cache-serve cannot help with, and what the coverage ledger fixes.

## 3. Decision — a per-(filter_hash, relay) coverage ledger, migrated in five de-risked stages

Replace the presence-derived `since`-floor with a coverage ledger: a row per
`(filter_hash, relay)` recording the timestamp through which a sync has actually
**completed** (EOSE for a plain REQ, NEG-DONE for negentropy). The floor is then
read from the ledger, not inferred from stored presence. A coverage gate refuses
to floor a `(filter, relay)` that has no completed-coverage row.

The ledger swap (Stage D) is **the single riskiest item in the entire excellence
program** — it changes how every read subscription decides what to fetch. The
sequencing below is the risk control: each stage is independently shippable and
reviewable, the cheap self-healer lands first, and the ledger lands LAST, after
the predicate has been unified so there is one mapping to migrate, not three.

### Stage A — un-floor NEG-OPEN (de-risker, LANDED this unit)

Strip `since` from the filter used for NIP-77 `local_items` and the NEG-OPEN
frame, so set reconciliation covers `[0, ∞)`. The floor is kept ONLY on frames
that go out as plain REQs (the NEG-unsupported fallback REQ and the Tailing
live-only REQ). Because NIP-77 transfers exactly the id-set symmetric difference,
an un-floored reconciliation is **self-healing**: any below-floor gap (H1/H2)
surfaces as a `need` id and is fetched.

- Implementation: `EligibleFilter::unfloored()` in `crates/nmp-nip77/src/filter.rs`;
  wired in `runtime.rs::intercept_req`.
- Oracle: `crates/nmp-nip77/src/runtime_tests_k3.rs::neg_open_unfloors_since_and_repairs_below_floor_gap`
  — a stray below-floor event is stored, the REQ is floored above it, and a NEG
  round-trip against a spec-compliant relay mock (one that scopes its set to the
  NEG-OPEN filter window) repairs the gap. Companion guard asserts the fallback
  plain REQ still carries the floor.
- Cost / blast radius: contained to `nmp-nip77`; touches NO watermark code; makes
  H1/H2 self-healing for every NEG-eligible shape (≥50 author×kind fanout — the
  follow feed, exactly the H1/H2 shape).

### Stage B — soundness patches on the heuristic while it lives

Harden the floor where it still applies (plain REQs, sub-threshold shapes) before
it is replaced:

- **B1 — align the address-pointer branch with the authors min/abort rule.** The
  authors branch returns `None` if any author lacks stored events; the
  address-pointer (`KindDtag`) and Etag/Ptag branches of `watermark_fn` do not
  apply an equivalent abort, so a partially-known multi-coord shape can be floored
  unsafely. Make every multi-target branch use the same "any target with zero
  stored matches ⇒ no floor" rule the authors branch uses.
- **B2 — NEG-OPEN liveness deadline.** A relay that silently ignores NEG-OPEN
  (neither NEG-MSG, NEG-ERR, nor NOTICE) leaves the interest stuck in `Probing`
  forever and never falls back. Add a per-session liveness deadline that falls
  back to the floored plain REQ if no terminal NEG response arrives, so the
  interest is never starved.
- **B3 — Etag/Ptag truncated-serve floor refusal.** When the floor-coherent pin
  scan truncates (`PinScanOutcome::Truncated`, `ram_eviction_floor.rs`), the floor
  for that shape is not provably coherent with what eviction may have removed;
  refuse to floor that shape for the tick rather than risk a hole.

### Stage C — unify the predicate (precondition for the ledger swap) — LANDED

Implement `watermark_fn` OVER `shape_to_store_queries` so the floor computation
and the store-serve read the SAME shape→query mapping. Collapse the three
hand-synced copies (`watermark_fn`, `shape_floor`, `shape_to_store_queries`) to
one. This is the precondition that makes Stage D reviewable: one mapping to
migrate from presence to ledger, not three copies to keep in lockstep across the
swap.

**Disposition (2026-06-14).** The first copy was already collapsed: the ADR-0045
§6 / #1119 refactor routed the live `watermark_fn` (`kernel/mod.rs`) through
`cache_serve::watermark_from_queries`, which folds over `shape_to_store_queries`
— "one mapping read two ways". Stage C verified that and removed the **two
residual copies** in the floor-coherent eviction helpers
(`kernel/ram_eviction_floor.rs`):

- `shape_floor` carried a hand-rolled shape→`StoreQuery` match that had ALREADY
  DRIFTED — its addressable (`KindDtag`) branch folded multi-coord with
  max-ignoring-empties (the pre-B1 unsafe policy) while `watermark_from_queries`
  uses the Stage B1 min/abort rule. It now routes through
  `watermark_from_queries`, gaining a `truncated: &HashSet<u64>` parameter wired
  to the live `Kernel::etag_ptag_truncated_serves` set so it refuses exactly the
  Stage B3 cursor-less shapes the installed floor refuses. `shape_floor` is now
  byte-identical to the `watermark_fn` floor.
- `pin_shape_events_below_floor` (which mirrored `shape_floor`'s mapping to pin
  below-floor events) now derives its queries from `shape_to_store_queries` and
  applies the `<= floor` bound, rather than re-deriving the mapping by hand.

The unification is **locked** by `gc_floor_unification_tests.rs`:
`shape_floor_matches_unified_floor_for_partial_addressable_shape` and
`shape_floor_equals_unified_floor_for_shape_battery` assert that for a battery of
representative `InterestShape`s the floor-coherent `shape_floor` equals
`watermark_from_queries(shape, …)` exactly (same kinds/authors/tags/coords fold,
same min/abort and truncation policy). Both were verified RED against the
reintroduced drift (the addressable branch produced `Some(stored newest)` vs the
unified `None`) before passing GREEN. There is now a SINGLE
`shape_to_store_queries` mapping, so Stage D migrates one mapping from presence to
ledger, not three.

### Stage D — wire the coverage ledger as the sole since-floor source (behind a flag)

Stage D is the riskiest item in the program (it flips the read-path floor from
presence-based to coverage-based), so it is split into three independently
shippable, separately-gated units. D1 is additive and safe to land immediately;
D2 is the read surgery (fixture-relay-gated); D3 is the eviction coherence leg.

#### Stage D1 — additive ledger WRITE path, off-by-default flag (LANDED)

- Re-create the ledger type (`CoverageRow { filter_hash, relay, covered_through }`)
  and its store sub-db (the #1090-deleted `nmp-watermarks` machinery, rebuilt with
  real readers/writers this time): `nmp-store/src/types/coverage.rs`,
  `EventStore::record_coverage`/`get_coverage` on BOTH backends (`MemEventStore`
  HashMap + LMDB `nmp-coverage` sub-db).
- **Written** at EOSE (`kernel/ingest/eose.rs::handle_eose`) and NEG-DONE
  (`nmp-nip77/runtime.rs` → `Kernel::record_neg_done_coverage`), per
  `(filter_hash, relay)` where `filter_hash` is the SAME `canonical_filter_hash`
  the `sub-<hash>` wire id and the recompile floor already key on.
- **Honest coverage (the invariant that makes D2 sound where presence is not):**
  `covered_through` is downward-closed — a row asserts coverage of `[0, T]` and
  nothing weaker. NEG-DONE records `now` (un-floored `[0, ∞)` per Stage A); an
  EOSE records `now` ONLY for an un-floored REQ, and `0` (no-op) for a
  `since`-floored REQ whose EOSE proves only `[floor, now]` — it must NOT
  over-claim `[0, floor)`. The REQ's `since` rides on `WireSub.since_floor`.
- **No read change:** `recompile`/`apply_watermark_rewrite` is untouched; the
  since-floor stays presence-derived. The write path is gated behind
  `Kernel::set_coverage_ledger_enabled` (DEFAULT **false**), so D1 is a pure
  additive no-op until D2.

#### Stage D2 — the read swap (fixture-relay-gated) — LANDED

- `recompile`'s floor reads the ledger entry for `(filter_hash, relay)` instead of
  computing presence. NOTE: the floor read site
  (`apply_watermark_rewrite` → `watermark_fn(&shape)`) was per-shape, not
  per-relay — D2 threaded the relay into the floor computation so the ledger is
  read by `(filter_hash, relay)`.
- The coverage gate consults ledger **staleness** (no completed-coverage row ⇒
  refuse to floor) instead of fanout-only.
- **MERGE GATE:** a fixture-relay journey test proving
  **follow-a-user-AFTER-a-thread-reply backfills the author's FULL history** (the
  H1 headline — presence-floor would suppress it; ledger-floor must not). Unit
  tests on the ledger are necessary but not sufficient.
- The flag flips **default-on for exactly ONE release cut** (a later
  release-cut PR, owner's call) so git-rev-pinning external consumers
  (podcast-player, hl, win-the-day) can pin across the change.

**Disposition (2026-06-14).** Landed flag-gated, **DEFAULT-OFF** (the swap is
proven under test with the flag on, ships dormant, and is flipped deliberately
in a later release-cut PR — production is byte-identical to pre-D2 with the flag
off). Three legs:

1. **Per-relay threading.** `WatermarkFn` became
   `Fn(&InterestShape, &str /*relay*/) -> Option<u64>`. BOTH floor-application
   sites — `apply_watermark_rewrite` (`subs/recompile.rs`) and `handle_reconnect`
   (`subs/handlers.rs`, the reconnect-replay path) — thread the target relay
   through, so the floor is per-`(filter_hash, relay)`. The presence heuristic
   ignores the relay (presence is relay-agnostic), so the swap is
   behaviour-preserving with the flag off.
2. **Read-with-fallback, single decision table.**
   `kernel/coverage_ledger.rs::coverage_floor_with_fallback` is the SOLE decision
   table, called by both the installed `WatermarkFn` closure (`build_watermark_fn`)
   and the `&self` wrapper `Kernel::coverage_floor_for` (the unit-test surface) —
   one mapping read two ways, no drift (the Stage-C discipline). The flag is a
   shared `Arc<AtomicBool>` read live per recompile.
3. **No-row REFUSES the floor (clarification of "presence fallback").** The §3.D2
   draft said presence remains "a fallback when the ledger has no row." Read
   against the **merge gate** (§4) and item 2 above ("no completed-coverage row
   ⇒ refuse to floor"), the operative semantics are: **flag ON + no row ⇒ refuse
   the floor (full `[0, ∞)` window)**, NOT a presence fallback. The H1 headline is
   precisely the case where presence is unsound — a stray kind:1 by A makes the
   presence floor for A's follow-feed `Some(stray_ts)`, suppressing A's history
   below the stray, even though that shape never completed a sync against the
   relay. A presence fallback would re-inherit that poisoned floor and FAIL the
   gate. Refusing the floor (full backfill) is the soundness fix and is "never a
   worse one" (a full re-fetch can only fetch MORE, never suppress). The presence
   computation survives ONLY behind the default-off flag, to be deleted in Stage
   E. Cost: a one-time full re-fetch per shape the first time the flag is enabled,
   after which the EOSE/NEG-DONE-recorded `covered_through` floors normally.

**Merge gate (the H1 journey test).** `kernel/coverage_ledger_d2_journey_tests.rs`
drives a REAL in-process WebSocket relay (tungstenite, `native`-gated, NOT
`#[ignore]`, so it gates the default `cargo test --workspace` PR lane — a
`nak serve`-dependent test would `#[ignore]` and run only in the nightly lane,
and so could not be the gate; §4 sanctions the in-process pattern as the
alternative). It stores a stray kind:1 by A (t=300), follows A, recompiles
through the REAL production watermark closure, forwards the compiled REQ over the
socket, ingests the relay's reply through the REAL Schnorr-verify + store gate,
and asserts: flag ON ⇒ un-floored REQ ⇒ A's FULL history (t=100/200/300)
backfills; flag OFF ⇒ `since=301` floored REQ ⇒ only the stray survives
(suppressed — the bug, intact under the off flag).

#### Stage D3 — eviction⇄ledger coherence

- **Durable LRU eviction remains supported only as an explicit finite-retention
  policy** (`GcBudget::with_durable_event_ceiling(n)`). Production default GC
  now keeps valid fetched rows (`GcBudget::production().max_total_events =
  usize::MAX`, #1480) and bounds RAM through the kernel RAM-cache pass instead.
  D3 remains the required backstop for any explicit durable quota path.
- Today the floor-coherent pin set (#1090 Stage 2 / #1327,
  `ram_eviction_floor.rs::pin_shape_events_below_floor`) pins every event at/below
  the PRESENCE floor so eviction never strands it. After D2 makes the ledger the
  floor source, the same pin set keeps `shape_floor`/pin == the ledger floor for
  covered shapes. **Rule:** if eviction can delete events below an active
  `covered_through`, it MUST lower that `covered_through` in the **same
  transaction** (or it punches the permanent hole the memory review flagged).
  Specify both legs precisely (pin-below-ledger-floor + eviction-lowers-ledger) at
  D3 start; re-verify eviction enablement and the pin-set wiring then.

**Disposition (LANDED).** Both legs shipped behind the default-off
`coverage_ledger_enabled`, so D3 is dormant in production like D1/D2. #1480
changed production GC to keep durable rows by default; the same D3 machinery
continues to guard explicit finite durable-retention budgets.

- **Leg 1 — pin below the ledger floor.** `Kernel::pin_floor_for_shape`
  (`kernel/ram_eviction_coverage.rs`) feeds the floor-coherent pin set
  (`add_floor_coherent_pins`, relocated to the same file) the floor a REQ will
  actually carry: flag OFF ⇒ presence (`floor::shape_floor`, unchanged); flag ON
  + ≥1 coverage row ⇒ the MAX `covered_through` across the shape's relays
  (`EventStore::coverage_max_for_filter_hash` — the event store is relay-agnostic
  and over-pinning is always safe); flag ON + no row ⇒ `None` (D2 refuses the
  floor, so the relay re-sends the full history and no pin is needed). This is
  the SAME decision `coverage_floor_with_fallback` (D2) makes — no third floor
  computation (Stage C single-source discipline preserved).
- **Leg 2 — eviction lowers the ledger.** New
  `EventStore::gc_step_with_pins_and_coverage(budget, now, pins, &[CoverageGuard])`
  (default delegates to `gc_step_with_pins`; real impls on both backends).
  `Kernel::derive_coverage_guards` emits one guard per active covered
  `(filter_hash, relay)` carrying the kernel-owned
  `InterestShape::matches_event_with_id` predicate (D0: the shape match never
  leaks into the store). When Phase-2 LRU deletes a matched event with
  `created_at <= covered_through`, the store lowers that row to
  `oldest_evicted - 1` (or clears it on `0`) **in the same transaction/lock as
  the delete** — mem under the held `MemState` lock, LMDB inside the Phase-2
  write txn (`coverage::lower_guards_in_txn`). Lowering to `oldest_evicted - 1`
  (not "just below the oldest surviving covered event") is the hole-free choice:
  it forces a re-fetch from the oldest evicted hole even under non-contiguous
  eviction, so the ledger never over-claims a range it no longer holds.
- **Oracles.** Store-layer `for_each_backend!`
  (`nmp-testing/tests/store_coverage_eviction_backstop.rs`) proves the backstop
  atomically on Mem + LMDB (RED-by-sabotage when the lowering is neutered);
  kernel-layer `gc_coverage_coherent_d3_tests` proves leg 1 (the ledger floor
  governs over presence) and leg 2 (the guard set) including an integrated
  production `run_gc_step` pass. The flag stays default-off; the default-on flip
  is the deliberate Stage-E release-cut PR.

### Stage E — delete the presence heuristic; correct the docs (IMPLEMENTED)

**Disposition (IMPLEMENTED).** The default-on flip landed as the K3 release-cut
PR, then Stage E deleted the presence heuristic entirely:

- `watermark_fn`'s presence-floor computation and the flag-off fallback are
  gone; `coverage_ledger::coverage_floor` (ledger `covered_through`, or `None` ⇒
  refuse the floor / full window) is the SOLE since-floor source.
- The off-by-default `coverage_ledger_enabled` flag and all its plumbing are
  removed (the ledger is unconditionally the floor authority).
- The presence-only support machinery deleted with it: `shape_floor`
  (`ram_eviction_floor.rs`), `watermark_from_queries` + `cursor_less_query_key`
  (`cache_serve/queries.rs`), and the entire Stage-B3 truncated-serve tracking
  (`etag_ptag_truncated_serves` / `etag_ptag_truncated_query_keys` /
  `recompute_truncated_query_keys` / `truncated_serve_snapshot`), whose only
  consumer was the presence-floor refusal.
- `shape_to_store_queries` (the single serve/pin query mapping) is preserved —
  cache-serve and the floor-coherent pin scan still ride on it.
- The D3 pin floor (`pin_floor_for_shape`) and backstop guards
  (`derive_coverage_guards`) read the ledger unconditionally.

The H1 journey test (`coverage_ledger_d2_journey_tests.rs`) and the D3 coherence
tests (`gc_coverage_coherent_d3_tests.rs`) pass with presence fully removed. The
stale "[LANDED M4]" doc claims are corrected (the watermark was deleted in #1090
and superseded by this ledger, not "landed").

## 4. Gate — Stage D must not land without a fixture-relay journey test

Stage D MUST be gated by a fixture-relay journey test (using `nak serve` in-memory
or the existing real-relay harness pattern in `crates/nmp-testing`) proving the
canonical scenario end-to-end: **follow a user AFTER a thread reply from them is
already stored, and confirm the author's full history backfills** (i.e. the floor
no longer suppresses below-the-stray-reply history). A unit test on the ledger is
necessary but not sufficient; the swap changes real fetch behaviour and must be
proven against a relay.

## 5. Consequences

- Stage A makes the highest-value case (the follow feed, the H1/H2 shape)
  self-healing immediately, at near-zero risk, buying time for the careful Stages
  B–D.
- The single-mechanism cache-serve principle (ADR-0045 Rev 2) is preserved: the
  ledger is the floor SOURCE; cache-serve remains the one event-acquisition path.
  The ledger does not introduce a second replay stage or per-domain special-casing.
- Until Stage D lands, sub-threshold shapes (plain REQs that never qualify for
  negentropy) still rely on the presence floor and are protected only by Stage B's
  hardening — this is acceptable because the floor-coherent pin set already
  prevents eviction holes, and the residual gap is "never-fetched below-floor
  history for small non-NEG shapes," the lowest-impact slice.

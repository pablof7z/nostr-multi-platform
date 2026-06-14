# ADR-0056 — K3 coverage ledger: staged migration from presence-floor to per-(filter, relay) coverage

- Status: Accepted (Stage A landed; Stages B–E queued)
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

### Stage C — unify the predicate (precondition for the ledger swap)

Implement `watermark_fn` OVER `shape_to_store_queries` so the floor computation
and the store-serve read the SAME shape→query mapping. Collapse the three
hand-synced copies (`watermark_fn`, `shape_floor`, `shape_to_store_queries`) to
one. This is the precondition that makes Stage D reviewable: one mapping to
migrate from presence to ledger, not three copies to keep in lockstep across the
swap.

### Stage D — wire the coverage ledger as the sole since-floor source (behind a flag)

- Re-create the ledger type (`CoverageRow { filter_hash, relay, covered_through }`)
  and its store sub-db (the #1090-deleted machinery, rebuilt with real
  readers/writers this time).
- **Written** by the sync engine at EOSE (plain REQ) and NEG-DONE (negentropy),
  per `(filter_hash, relay)`.
- **Read** by `recompile` (the floor source) and by the coverage gate (refuse to
  floor an uncovered `(filter, relay)`).
- **Eviction⇄ledger coherence:** LRU eviction MUST lower `covered_through` for any
  evicted range, or it punches a permanent hole (a separate High finding). The
  existing floor-coherent pin set (#1090/#1327) already keeps below-floor events
  pinned; Stage D extends the same coherence to the ledger by having eviction of a
  range below `covered_through` lower the ledger rather than strand the floor.
- Ship behind a **default-on flag for exactly ONE release cut**, so git-rev-pinning
  external consumers (podcast-player, hl, win-the-day) can pin across the change.
  The presence-derived floor remains as a fallback during migration ONLY.

### Stage E — delete the presence heuristic; correct the docs

Once Stage D is proven in a release, delete `watermark_fn`'s presence computation
and the fallback, leaving the ledger as the sole floor source. Correct any doc
still claiming the ledger "[LANDED M4]".

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

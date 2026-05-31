# ADR-XXXX: Kernel-Owned `PaginationController` for Backwards Timeline Backfill

**Status:** DRAFT (seeking feedback)  
**Date:** 2026-05-31  
**Decision Maker:** @pablof7z  
**Related:** ADR-0033 (nmp-feed FFI), ADR-0035 (root-indexed engine), docs/design/ndk-applesauce-lessons.md §8, docs/design/subscription-compilation/intro.md §2.1

---

## Problem Statement

Today, when a user scrolls to the end of locally-cached events (500 event ceiling in `nmp-feed`), the app **hits a wall**. The feed's `load_older()` function pages over in-memory blocks only; it has no seam to signal "fetch older events from relays." The two halves exist separately:

1. **View window** (`nmp-feed::FeedWindowState::load_older`) advances over local blocks; `has_more` means "more cached blocks exist," not "older events on relays."
2. **Network backfill** (planner + kernel subscriptions) can support `until`-bounded requests in principle, but `InterestShape.until` is never mutated at runtime. The only production time-bound mutators move `since` *forward* to reduce relay load.

**Consequences today:**
- Home feed scrolls to 500 events then stops (Chirp-TUI observed; web M15 pre-gate before claim-driven profiles).
- Profile timelines at 500 event ceiling.
- Search results cannot page backwards.
- No integration with NIP-77 (NegentropySyncRuntime) coverage checking, so even if an older event exists on disk the planner might re-fetch if the relay hasn't been explicitly sync-marked as authoritative for older times.

This blocks the infinite-scroll UX described in aim.md §4.8 and is the missing final piece of the feed-architecture vision (ndk-applesauce-lessons.md §8: "A user scrolling is not asking the app to send a relay filter; they are asking the app to extend a view window").

---

## Design Rationale

### Architectural Foundation

Per ndk-applesauce-lessons.md §8 and subscription-compilation/intro.md §2.1:

> "The intended bridge is that **extending the window is a planner intent** that the compiler may satisfy from cache, NIP-77 coverage, a bounded backwards REQ (`until = oldest − 1`, per applesauce G4), or fallback — with the shell never owning cursor policy and never naming relays."

This ADR realizes that vision by introducing a **kernel-owned `PaginationController`** that:
- Lives alongside the `InterestRegistry` in `SubscriptionLifecycle` (D4 single-registration point).
- Is driven by `nmp-feed`'s `load_older` through a generic capability closure (D7 "engine asks, wiring decides").
- Advances `until` in bounded `OneShot` interests, dedups with the planner's merge lattice (so repeated scroll-at-same-boundary coalesces), and gates against `Coverage::CompleteAsOf` watermarks (so fully-synced relays stop issuing REQs).
- Owns the sliding-window eviction policy (drop oldest blocks as new ones arrive) to keep D8 memory bounds.

### Doctrine Alignment

- **D0 (no app nouns in nmp-feed):** `PaginationController` lives in `nmp-core`, not the feed. The feed's only new seam is a protocol-neutral closure.
- **D2 (all reads through the store):** Backfilled events land in LMDB; projections ingest them via the existing observer path.
- **D4 (single registration):** All backwards pagination interests route through `InterestRegistry`'s dedup seam, not FFI-direct claims.
- **D5 (outbox routing automatic):** The controller never names relays; the planner routes to the author's write relays (per existing Nostr conventions).
- **D6 (auto-group, auto-close, auto-dedup, auto-buffer):** Planner dedup by `(filter_hash, relay)` combines overlapping backfill REQs; EOSE + coverage gating close them automatically.
- **D7 (engine asks, wiring decides):** The feed emits a `BackfillRequest` via a closure sink; `nmp-nip01` wiring translates it to `ActorCommand::BackfillFeed`.
- **D8 (reactivity bounded, memory grows with views not history):** Sliding window keeps the in-memory event list constant-size.

---

## Proposed Solution

### 1. Add `BackfillCapability` to `nmp-feed`

In `crates/nmp-feed/src/root_indexed/engine/mod.rs`:

```rust
/// Request to backfill older events from relays.
#[derive(Clone, Debug)]
pub struct BackfillRequest {
    /// Oldest locally-cached event in the feed (the lower boundary).
    /// The planner will issue `until = oldest.created_at - 1` (applesauce G4 fix).
    pub oldest: FeedCursor,
    
    /// Unique ID for this feed (home, profile, search, etc.).
    /// Used by the wiring to map back to the correct feed's controller.
    pub feed_key: String,
    
    /// Unique consumer ID (assigned by the engine).
    /// Used for refcounting and dedup in the planner.
    pub consumer_id: String,
}

/// Sink the engine pushes `BackfillRequest`s through.
pub type BackfillSink = Arc<dyn Fn(BackfillRequest) + Send + Sync>;
```

Modify `RootIndexedFeed::new` to accept an optional `BackfillSink`:

```rust
pub fn new(
    viewer: &str,
    follow: Arc<dyn FollowPredicate + 'static>,
    lookup: Arc<dyn EventLookup + 'static>,
    claim_sink: ClaimSink,
    backfill_sink: Option<BackfillSink>,  // NEW
    consumer_id: String,
) -> Self { ... }
```

In `RootIndexedFeed::load_older`, when local blocks are exhausted:

```rust
fn load_older(&self) -> bool {
    // ... existing window-advance logic ...
    if self.window_state.load_older(blocks, cards) {
        return true;
    }
    
    // Local blocks exhausted. Signal backfill if a sink is installed.
    if let Some(sink) = &self.backfill_sink {
        if let Some(oldest) = blocks.last().map(|block| block_cursor(block, cards)) {
            (sink)(BackfillRequest {
                oldest,
                feed_key: self.consumer_id.clone(),  // reuse as feed key
                consumer_id: format!("{}-backfill", self.consumer_id),
            });
            return false;  // No local change, but backfill is in-flight.
        }
    }
    false
}
```

### 2. Add `PaginationController` to `nmp-core`

In `crates/nmp-core/src/subs/pagination.rs` (new file):

```rust
/// Kernel-side pagination state machine. Owns the backwards-fetch cursor,
/// EOSE acknowledgement, and coverage-complete gating per (view_key, filter).
pub struct PaginationController {
    /// Per-view backfill state: cursor, interest_id, in-flight, EOSE ack, coverage complete.
    backfills: BTreeMap<String, BackfillState>,
}

struct BackfillState {
    feed_key: String,
    oldest: FeedCursor,
    interest_id: Option<InterestId>,  // None before interest is registered, Some(id) after.
    coverage_complete: bool,  // True if Coverage::CompleteAsOf reached (relay fully synced to this depth).
    eose_seen: bool,  // True once EOSE received; used to detect exhaustion.
}

impl PaginationController {
    pub fn new() -> Self { Self { backfills: BTreeMap::new() } }
    
    /// Called by the wiring when a feed's `BackfillRequest` arrives.
    /// Returns an `InterestLifecycle::BoundedBackfill { until }` that should
    /// be registered with the planner (or None if already in-flight).
    pub fn request_backfill(
        &mut self,
        feed_key: &str,
        oldest: FeedCursor,
        authors: Vec<Pubkey>,  // From the view context
        kinds: Vec<u32>,
        current_watermark_fn: &WatermarkFn,  // For coverage gating
    ) -> Option<InterestShape> {
        // Dedup: if we're already backfilling this view, skip.
        if self.backfills.contains_key(feed_key) {
            return None;
        }
        
        // Coverage gating: if the relay is fully synced to older than `oldest`, suppress REQ.
        let until = oldest.created_at - 1;  // Applesauce G4 fix: NIP-01 until is inclusive
        let watermark = current_watermark_fn(&InterestShape {
            authors: authors.clone().into_iter().collect(),
            kinds: kinds.clone().into_iter().collect(),
            until: Some(until),
            ..Default::default()
        });
        if let Some(w) = watermark {
            if w >= until {
                // Watermark is at or past `until`; relay already synced to this depth.
                self.backfills.insert(feed_key.to_string(), BackfillState {
                    feed_key: feed_key.to_string(),
                    oldest,
                    interest_id: None,
                    coverage_complete: true,
                    eose_seen: true,
                });
                return None;  // Don't issue REQ; backfill is complete.
            }
        }
        
        // Register the backfill interest.
        self.backfills.insert(feed_key.to_string(), BackfillState {
            feed_key: feed_key.to_string(),
            oldest,
            interest_id: None,  // Will be filled by kernel after registration.
            coverage_complete: false,
            eose_seen: false,
        });
        
        Some(InterestShape {
            authors: authors.into_iter().collect(),
            kinds: kinds.into_iter().collect(),
            until: Some(until),
            limit: Some(200),  // Configurable backfill page size
            ..Default::default()
        })
    }
    
    /// Called by the kernel after an interest is registered to record its ID.
    pub fn record_interest_id(&mut self, feed_key: &str, interest_id: InterestId) {
        if let Some(state) = self.backfills.get_mut(feed_key) {
            state.interest_id = Some(interest_id);
        }
    }
    
    /// Called by the planner after EOSE on a backfill interest.
    pub fn on_eose(&mut self, interest_id: InterestId) {
        for state in self.backfills.values_mut() {
            if state.interest_id == Some(interest_id) {
                state.eose_seen = true;
                // Don't auto-clean here; leave for explicit release or recompile.
            }
        }
    }
    
    /// Cleanup: called when a view closes or scrolling stops.
    pub fn release_backfill(&mut self, feed_key: &str) -> Option<InterestId> {
        self.backfills.remove(feed_key).and_then(|state| state.interest_id)
    }
}
```

### 3. Wire in `SubscriptionLifecycle`

In `crates/nmp-core/src/subs/lifecycle.rs`:

```rust
pub struct SubscriptionLifecycle {
    registry: InterestRegistry,
    oneshot: OneshotApi,
    pagination: PaginationController,  // NEW
    // ... existing fields ...
}

impl SubscriptionLifecycle {
    pub fn new(...) -> Self {
        Self {
            registry,
            oneshot,
            pagination: PaginationController::new(),  // NEW
            // ...
        }
    }
    
    pub fn pagination_mut(&mut self) -> &mut PaginationController {
        &mut self.pagination
    }
}
```

### 4. Kernel-side dispatch

In `crates/nmp-core/src/actor/dispatch.rs`, add:

```rust
ActorCommand::BackfillFeed {
    feed_key,
    oldest,
    authors,
    kinds,
} => {
    // Call the pagination controller to check coverage and build an interest.
    if let Some(shape) = kernel.lifecycle
        .pagination_mut()
        .request_backfill(&feed_key, oldest, authors, kinds, &kernel.watermark_fn)
    {
        // Register as a bounded OneShot (dedups by shape; same boundary = same interest).
        let interest = LogicalInterest {
            shape,
            lifecycle: InterestLifecycle::OneShot,
            is_indexer_discovery: false,
            scope: InterestScope::Feed,
        };
        let interest_id = kernel.lifecycle.registry_mut().ensure_sub(
            (&feed_key, "backfill", &Interest),
            interest,
        );
        kernel.lifecycle.pagination_mut().record_interest_id(&feed_key, interest_id);
        kernel.enqueue_trigger(CompileTrigger::ViewOpened);
    }
    kernel.changed_since_emit = true;  // Re-emit snapshot (maybe coverage-gated, no REQ).
}
```

### 5. Sliding window in `nmp-feed`

Modify `FeedWindowState` to track a "eviction watermark":

```rust
pub struct FeedWindowState {
    pub(crate) oldest_visible: Option<FeedCursor>,
    pub(crate) max_window_size: usize,  // NEW: e.g., 500 for memory bound
    pub(crate) total_ingested: usize,   // NEW: count of events ever added
}

impl FeedWindowState {
    /// Returns (visible_blocks, dropped_blocks) for a snapshot.
    /// Visible window is bounded to `max_window_size` newest events.
    pub fn snapshot_blocks_windowed<B, C, S>(
        &self,
        all_blocks: &[B],
        cards: &S,
    ) -> (Vec<B>, Vec<B>, FeedPage)
    where
        B: FeedBlock,
        C: FeedCard,
        S: FeedCardStore<C>,
    {
        let total = all_blocks.len();
        let window_start = total.saturating_sub(self.max_window_size);  // Newest 500
        let visible = &all_blocks[window_start..];
        let dropped = &all_blocks[..window_start];
        
        // ... existing page calculation over `visible` ...
    }
}
```

The projection's card cache and block list both use `BoundedMessageMap` / `deque` structures, so old cards and blocks are automatically evicted when new ones arrive (D8 bounded reactive state).

### 6. `nmp-nip01` wiring

In `crates/nmp-nip01/src/op_feed/wiring.rs`, add the backfill sink builder:

```rust
pub fn build_backfill_sink(app_ptr: Arc<NmpApp>) -> impl Fn(BackfillRequest) + Send + Sync {
    move |req: BackfillRequest| {
        let feed_ctx = /* look up feed controller by req.feed_key to get authors, kinds */;
        app_ptr.send_cmd(ActorCommand::BackfillFeed {
            feed_key: req.feed_key.clone(),
            oldest: req.oldest.clone(),
            authors: feed_ctx.authors.clone(),
            kinds: feed_ctx.kinds.clone(),
        });
    }
}
```

Register the sink when constructing the home-feed `RootIndexedFeed`:

```rust
let backfill_sink = Some(Arc::new(build_backfill_sink(app_ptr)));
let home_feed = RootIndexedFeed::new(
    ...,
    claim_sink,
    backfill_sink,  // NEW
    "home-feed".to_string(),
);
```

---

## Sub-Decisions

### 1. Backfill page size
- **Value:** `limit: 200` per backwards REQ.
- **Rationale:** applesauce loaders use ~50–100; larger reduces round-trips but increases EOSE latency. 200 is a sensible middle ground for Nostr's typical relay response times (100–500ms).
- **Knob location:** `PaginationController::request_backfill` or the wiring module.

### 2. Applesauce G4 boundary fix
- **Value:** `until = oldest.created_at - 1`.
- **Rationale:** NIP-01 specifies `until` as **inclusive**; without the −1, the oldest event is returned again on the next page, causing an infinite loop on singleton blocks or stalled pagination.
- **Test:** unit test in `crates/nmp-store/tests/pagination_boundary.rs` that loads a single-event page, then backfills, and asserts no duplicate at the boundary.

### 3. Coverage gating
- **Value:** Consult `WatermarkFn` before issuing a backwards REQ. If `watermark >= until`, suppress the REQ.
- **Rationale:** If the relay is fully synced to (or past) the requested depth, a cache-miss is authoritative "doesn't exist." Suppression avoids redundant network round-trips.
- **Fallback:** If `WatermarkFn` is not installed (rare; kernel-only path), issue the REQ unconditionally.
- **Future:** Wire `Coverage::CompleteAsOf` from `nmp-store` to the planner's `apply_coverage_filter` gate (M4 follow-up). For now, `WatermarkFn` (newest-stored-per-shape) is the bar.

### 4. Sliding window eviction
- **Value:** `FeedWindowState::snapshot_blocks_windowed` keeps the newest `max_window_size` (500) events visible; older blocks are dropped from the UI (not the store).
- **Rationale:** Keeps in-memory projection state constant-size (D8 bounded); users never scroll back to the 1st event of a 10k-event home feed, so dropping out-of-view blocks is safe. The store always has the full history; a re-open of the same feed reloads from cache.
- **Caveat:** Profile/author timelines may be shorter (often <500 total); the window naturally settles to the full set.

### 5. Backwards interest lifecycle
- **Value:** `InterestLifecycle::OneShot` (not a new lifecycle variant).
- **Rationale:** A backwards `until`-bounded REQ is explicitly one-shot; it closes on EOSE. Merging two overlapping backfill REQs (same feed, overlapping `until` bounds) is handled by the planner's lattice: `until = max(a.until, b.until)`, so a new scroll merges with in-flight older pagination into a single request to the relay.
- **Dedup key:** `(feed_key, "backfill", shape)` in `ensure_sub` — identical feed+bounds = same interest.

---

## Implementation Plan (High-Level)

1. **Phase 1: Add seams and types** (1–2 days)
   - `BackfillRequest` + `BackfillSink` in `nmp-feed` (src/root_indexed/engine/mod.rs).
   - `PaginationController` in `nmp-core/subs/pagination.rs`.
   - `InterestLifecycle::OneShot` wiring for backfill interests.
   - Tests: unit tests for pagination dedup, boundary fix, coverage gating.

2. **Phase 2: Wire the feed** (1 day)
   - `RootIndexedFeed::load_older` → emit `BackfillRequest`.
   - `nmp-nip01` wiring (build sink, dispatch to `ActorCommand::BackfillFeed`).
   - E2E test: home-feed scroll to 500, load_older, assert new blocks arrive.

3. **Phase 3: Sliding window** (1 day)
   - `FeedWindowState::snapshot_blocks_windowed`.
   - Projection truncation; verify D8 memory bound in a large-scroll test.

4. **Phase 4: Coverage gating** (½ day)
   - Consult `WatermarkFn` in `PaginationController::request_backfill`.
   - Unit test: watermark >= until → no REQ emitted.

5. **Integration & testing** (1–2 days)
   - Chirp-TUI scrolling to 500+ events on home feed.
   - Profile timelines with pagination.
   - Web builds (post-M15; may be gated).

---

## Testing Strategy

1. **Unit tests** (nmp-core/subs):
   - `pagination_dedup.rs`: two backfill requests at same boundary → one interest registered.
   - `pagination_boundary.rs`: oldest event on page N, backfill page N+1, assert no duplicate.
   - `pagination_coverage_gating.rs`: watermark >= until → no REQ; watermark < until → REQ issued.

2. **Integration tests** (nmp-app-template or Chirp):
   - Feed scroll to 500 local blocks, call `load_older`, wait for events to ingest, scroll again. Repeat to 2–3 backfill rounds; assert no duplicates, no gaps.

3. **E2E (Chirp-TUI):
   - Home timeline scroll to end, backfill, scroll again. Profile scroll. Search scroll.

---

## Risks & Mitigations

| Risk | Mitigation |
|------|-----------|
| Duplicate events at boundary (G4 bug) | Unit test + assertion in `request_backfill` that `until = oldest - 1`. |
| Planner latency on large backfill (limit:200 slow) | Monitor EOSE latency; adjust `limit` down to 100 if needed. Lazy-load (Prop 2 of future: paginate within a page). |
| Memory growth if sliding window not implemented | Phase 3 is mandatory; block implementation until done. |
| Coverage gating suppresses a REQ when relay *is* incomplete | `WatermarkFn` only rises on successful ingest; a crash/partial EOSE won't poison it (it never decays). Worst case: one extra REQ per session per feed. Acceptable for M1. Phase 2 (wire `CompleteAsOf`) hardens this. |
| Multiple feeds (home + profile + search) all backfilling concurrently | Planner merge lattice dedupes by `(filter_hash, relay)` — different `authors` = different hashes, so home + profile don't collide. Acceptable (feeds have different scopes anyway). |

---

## Alternatives Considered

### Alt 1: `BackfillSink` only (P1)
Add the seam to `nmp-feed`, wire via `nmp-nip01` to `ActorCommand::BackfillFeed`, register a OneShot interest directly in the kernel dispatch. No `PaginationController`; cursor/dedup lives in the wiring.
- **Pros:** Smaller, faster.
- **Cons:** Each feed needs a seam; cursor dedup is implicit in the interest shape (fragile if feeds diverge); no coverage gating in the controller (would live in dispatch, harder to test).
- **Verdict:** Chosen to be deferred. Implement the full version now; backport to P1 only if performance becomes a problem.

### Alt 2: Extend `InterestShape` to include a "pagination mode"
Add `mode: PaginationMode { Backwards { until }, Forwards { since }, ... }` to the shape, let the planner decide how to emit.
- **Pros:** Unified in the planner; simpler than a separate controller.
- **Cons:** More planner changes; harder to gate on coverage in a clean way; `until` becomes planner-mutable, which is a bigger semantic shift.
- **Verdict:** Not chosen. The dedicated controller is more modular and respects the existing `until: Option<UnixSeconds>` field.

### Alt 3: Extend `FeedWindowState` to emit demands directly
Let the feed itself register interests in the kernel (coupling feed to core).
- **Pros:** One less indirection.
- **Cons:** Violates D0 and D7; `nmp-feed` would depend on `nmp-core` and know about `ActorCommand` / `InterestShape`.
- **Verdict:** Not chosen; violates doctrine.

---

## Success Criteria

- [ ] Home feed scrolls to 500+ events without hitting a local wall.
- [ ] Profile timelines backfill on scroll.
- [ ] No duplicate events at page boundaries.
- [ ] Memory stays constant-size (D8 bounded).
- [ ] Planner dedupes overlapping backfill REQs (same boundary = one REQ to relay).
- [ ] Coverage gating suppresses redundant REQs when relay is fully synced.
- [ ] All existing tests pass; new unit + E2E tests added.

---

## References

- ADR-0033: nmp-feed viewport FFI
- ADR-0035: generic root-indexed feed engine (D7 closure sinks)
- docs/design/ndk-applesauce-lessons.md §8: "pagination as window intent"
- docs/design/subscription-compilation/intro.md §2.1: "window vs wire intent"
- docs/research/applesauce/gotchas.md §G4: "NIP-01 until inclusive boundary fix"
- docs/aim.md §4.8: "live views should not wait for full reconciliation"
- nmp-feed/src/window.rs: `FeedWindowState::load_older` (local-only today)
- nmp-store/src/types/watermark.rs: `Coverage` enum + `WatermarkFn`
- nmp-core/src/subs: `InterestRegistry`, `OneshotApi`, `SubscriptionLifecycle`


# LMDB sub-design: GC working-set policy

> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Formalises the hot-resident / cold-on-disk split required by ADR-0003 (`docs/decisions/0003-working-set-memory.md`).

## 1. Definitions

```
stored_events = every event currently in `events` (primary), not tombstoned

claim_pinned  = ⋃ { ids | ids ∈ claims[claimer] for each registered claimer }
                where each `claimer` is an open ViewHandle / open ActionHandle

open_view_cover = ⋃ { dependency_target_ids(spec)
                       | (view_id, spec) ∈ open_views }
                  computed from the composite reverse-index per ADR-0001

recently_touched = top-N by `last_touched_ms` (default N = 10,000)

hot_resident = claim_pinned ∪ open_view_cover ∪ recently_touched
cold         = stored_events \ hot_resident
```

`last_touched_ms` is bumped on every `get_by_id`, on every secondary scan that *materialises* the event body, and on `insert` for a fresh row. Scans that only return ids/timestamps (e.g., the early-filter pass in a view's planner) do **not** bump it — only the construction of a `Delta` payload that needs the body does.

`hot_resident` is stored in memory; `cold` lives only on disk. The store still **knows** about every cold event via secondaries — the reverse index covers both per ADR-0003: "The reverse index indexes both hot and cold events. Lookup returns view ids immediately; event bodies for delta construction load lazily and synchronously via the storage backend."

## 2. Hot data structure

```rust
pub(crate) struct HotSet {
    // LRU bounded by `target_hot_size` (default 10,000), evicts non-pinned.
    lru: lru::LruCache<EventId, Arc<nostr::Event>>,
    // Strong-pin overlay; refcounted by ClaimerId.
    pinned: HashMap<EventId, u32>,                   // event_id → refcount
    // Reverse map for cheap release(); BTreeSet ensures claim() is idempotent per claimer.
    by_claimer: HashMap<ClaimerId, BTreeSet<EventId>>,
    // Per-view ceiling registered by register_view_cover().
    view_budgets: HashMap<ClaimerId, usize>,
    target_hot_size: usize,
    // Ceilings (enforced on every claim() call — D8 / ADR-0001..0004).
    max_claim_per_view: usize,   // default 1_000; callers may lower via register_view_cover
    max_pinned_total: usize,     // default 20_000; hard cap on pinned.len()
}

impl HotSet {
    /// Record the budget for a view before its first claim. If not called, the
    /// default `max_claim_per_view` applies. Calling it again with a lower budget
    /// after claims have already been issued does *not* retroactively reject them;
    /// the lower ceiling applies to future claim() calls.
    pub fn register_view_cover(&mut self, c: ClaimerId, budget: usize) {
        self.view_budgets.insert(c, budget);
    }

    /// Pin `ids` for `c`. Idempotent: re-claiming an id already in the claimer's set
    /// is a no-op (refcount not double-incremented). Budget checks count only genuinely
    /// new ids. Returns `StoreError::OverPinned` if limits would be exceeded.
    /// On rejection, the state is unchanged (all-or-nothing per call).
    pub fn claim(&mut self, c: ClaimerId, ids: &[EventId]) -> Result<(), StoreError> {
        let existing = self.by_claimer.get(&c);
        // Collect into a BTreeSet to dedup both intra-call and against already-claimed ids.
        let new_ids: BTreeSet<EventId> = ids.iter()
            .filter(|id| existing.map_or(true, |s| !s.contains(*id)))
            .copied()
            .collect();
        let per_view_ceiling = self.view_budgets
            .get(&c)
            .copied()
            .unwrap_or(self.max_claim_per_view);
        let current_for_claimer = existing.map_or(0, |s| s.len());
        if current_for_claimer + new_ids.len() > per_view_ceiling {
            return Err(StoreError::OverPinned {
                claimer: c,
                requested: current_for_claimer + new_ids.len(),
                ceiling: per_view_ceiling,
            });
        }
        let new_global = self.pinned.len() + new_ids.iter()
            .filter(|id| !self.pinned.contains_key(id))
            .count();
        if new_global > self.max_pinned_total {
            return Err(StoreError::OverPinned {
                claimer: c,
                requested: new_global,
                ceiling: self.max_pinned_total,
            });
        }
        let set = self.by_claimer.entry(c).or_default();
        for id in &new_ids {
            set.insert(*id);
            *self.pinned.entry(*id).or_insert(0) += 1;
        }
        Ok(())
    }

    pub fn release(&mut self, c: ClaimerId) {
        if let Some(ids) = self.by_claimer.remove(&c) {
            for id in ids {
                if let Some(rc) = self.pinned.get_mut(&id) {
                    *rc = rc.saturating_sub(1);
                    if *rc == 0 { self.pinned.remove(&id); }
                }
            }
        }
        self.view_budgets.remove(&c);
    }

    pub fn touch(&mut self, id: EventId, e: Arc<nostr::Event>) {
        self.lru.put(id, e);                          // bumps LRU
        self.trim();
    }

    fn trim(&mut self) {
        while self.lru.len() > self.target_hot_size {
            // pop_lru returns oldest; skip pinned ones until we find an evictable.
            // (LruCache::pop_lru doesn't take a predicate; we rotate.)
            let mut skipped: SmallVec<[(EventId, Arc<nostr::Event>); 8]> = SmallVec::new();
            let evicted = loop {
                match self.lru.pop_lru() {
                    Some((id, e)) if self.pinned.contains_key(&id) => skipped.push((id, e)),
                    Some(pair) => break Some(pair),
                    None => break None,
                }
            };
            for (id, e) in skipped.drain(..) { self.lru.put(id, e); }
            // If every LRU entry is pinned, the overflow will not be resolved by
            // trim() alone. The working-set budget enforcement in claim() is the
            // primary defence; trim() stopping here is intentional, not a silent
            // acceptance of unbounded growth.
            if evicted.is_none() { break; }
        }
    }
}
```

`target_hot_size` is set from `AppConfig::hot_event_ceiling` (default 10,000) and may be lowered by `MemoryWarningCapability` events (iOS app suspend or low-memory warning → halve the ceiling, run `gc_step()` once, restore after the warning clears).

**Ceiling defaults** (see `StoreError::OverPinned` in [`trait/types.md`](trait/types.md)):
- `max_claim_per_view`: 1 000 events per claimer. A view that tries to pin more returns `OverPinned`; the actor surfaces this as `Effect::ViewOverPinned` and releases the claim.
- `max_pinned_total`: 20 000 events globally. Prevents many moderate-sized views from collectively overwhelming the working set (D8 / ADR-0003 gate).

These defaults allow 100 active views × 200 pins each = 20 000 globally, within the ADR-0003 §5 memory accounting (10k LRU + 20k pinned overlay ≈ 90 MB, under the 100 MB gate).

## 3. `gc_step()` algorithm

```rust
pub fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError> {
    let start = Instant::now();
    let now_s = unix_now();
    let mut report = GcReport::default();

    // 3.1 — NIP-40 expired reaper.
    let to_reap = self.scan_expiring_before(now_s, budget.max_events_per_step)?
        .collect::<Result<Vec<_>, _>>()?;
    for ev in to_reap {
        if start.elapsed().as_millis() as u32 >= budget.max_duration_ms { break; }
        self.reap_one(ev.raw.id.into(), TombstoneOrigin::NIP40Expiry, now_s)?;
        report.expired_reaped += 1;
    }

    // 3.2 — Trim LRU back to target.
    let lru_before = self.hot.lock().lru.len();
    self.hot.lock().trim();
    report.lru_evicted = lru_before.saturating_sub(self.hot.lock().lru.len());

    // 3.3 — Purge old tombstones whose target event is absent.
    let cutoff = now_s.saturating_sub(self.cfg.tombstone_retention_secs);
    report.tombstones_purged = self.purge_old_tombstones(cutoff,
        budget.max_events_per_step.saturating_sub(report.expired_reaped))?;

    report.duration_ms = start.elapsed().as_millis() as u32;
    Ok(report)
}
```

Single `gc_step()` is bounded by `GcBudget { max_events_per_step, max_duration_ms, max_total_events }`. The budget constructors are the single source of truth for these numbers (`crates/nmp-store/src/types/gc.rs`):

- `GcBudget::default()` — the doc-schedule scan bounds `max_events_per_step = 2000` (`GC_MAX_EVENTS_PER_STEP`), `max_duration_ms = 50` (`GC_MAX_DURATION_MS`), with `max_total_events = usize::MAX` (LRU eviction disabled; backward-compatible).
- `GcBudget::production()` — the on-device budget: the same scan bounds **plus** `max_total_events = HOT_EVENT_CEILING` (10,000) so Phase-2 LRU eviction actually runs. The finite ceiling is load-bearing: with `usize::MAX`, `st.events.len() > max_total_events` is never true and LRU growth stays unbounded.

The actor calls `gc_step()` (via `Kernel::run_gc_step`, which threads the kernel clock's `now_secs` into the store — D7/D9):

- **Every 60 seconds (shipped, #1069)** — cooperative; runs on the actor thread between mailbox messages, gated by an `Instant`-based `last_gc` in `run_actor_with_observers` (`GC_TICK_INTERVAL`). Piggy-backs the existing ≤250 ms `compute_wait` loop wake — no sleep loop, no timer thread (D8 / "no polling").
- On `MemoryWarningCapability::Pressure` (iOS / Android low-memory signals) — **staged, not yet wired**: `MemoryWarningCapability` does not yet exist; building the capability seam (iOS `didReceiveMemoryWarning` → typed event → actor) is a follow-up.
- On any single `insert()` that observes `hot.lru.len() > 2 * target_hot_size` (safety net) — **staged, not yet wired** (internal to the store).

`gc_step()` is **never** invoked from an FFI call path — it runs on the actor's own schedule so any latency it introduces is invisible to the platform. The last run's `GcReport` and its wall-clock time are recorded on the kernel (`Kernel::last_gc` / `last_gc_at_ms`) so the schedule is observable rather than a silent ending — see §7.

## 4. Pin-set wiring (derived, ephemeral — #1090 Stage 1)

The earlier design persisted per-`ClaimerId` claim sets in dedicated LMDB
sub-dbs (`nmp-claims` / `nmp-claims-budget`) and exposed
`register_view_cover` / `claim` / `release` on `EventStore`. That machinery had
**zero production callers** (V-117) — no actor path ever issued a `claim`, so
the persisted state was dead weight and the protected-set was always empty.
#1090 Stage 1 **deletes** it: the sub-dbs, the `ClaimerId` type, the
`StoreError::OverPinned` variant, the `ClaimGuard` RAII helper, and the four
trait methods are gone (a hard break — downstream pins by git rev, no compat
shims).

In its place, the pin set is **derived from live kernel state on every GC pass
and passed straight into the store** — never persisted:

```rust
fn gc_step_with_pins(
    &self,
    budget: GcBudget,
    now_secs: u64,
    pins: &HashSet<EventId>,   // ← caller-derived, ephemeral
) -> Result<GcReport, StoreError>;
```

`gc_step(budget, now_secs)` remains as a convenience wrapper that calls
`gc_step_with_pins` with an empty set — used by tests and any context that
protects no events.

### How the kernel derives the pin set

`Kernel::run_gc_step` calls `Kernel::derive_store_pin_set()` immediately before
`gc_step_with_pins`. The derivation (`crates/nmp-core/src/kernel/ram_eviction.rs`)
unions three live sources, exactly mirroring the RAM-tier `events` pin set, and
converts each hex id to a 32-byte store `EventId`:

1. **`self.timeline`** — the bounded visible feed (≤ `TIMELINE_CACHE_LIMIT`).
2. **`self.event_claims`** — keys for which a UI component currently holds a
   claim. (These keys are `kind:pubkey:d_tag` coordinates for naddr URIs and
   hex64 ids for nevent/note URIs; only the hex64 keys parse to a store
   `EventId` — coordinate keys have no store row and are skipped.)
3. **Active open interest** — every event matching the wire-filter shape of any
   active `LogicalInterest` in the planner registry
   (`self.lifecycle.registry().iter_active()` + `shape.matches_event_with_id`),
   the same predicate `should_store_event`'s `matches_active_open_interest`
   clause uses for admission.

Because the set is recomputed each pass, there is **no restart-recovery
problem** and **no persisted-claims drift**: the cold-start path naturally
re-derives the pin set from whatever timeline / claims / interests are live at
the first GC tick after restart. There is nothing to reconcile on disk.

> **Ceiling still disabled.** `GcBudget::production()` keeps
> `max_total_events = usize::MAX` (Phase-2 eviction off) through Stage 1.
> Re-enabling the finite `HOT_EVENT_CEILING` is Stage 3, gated on the Stage-2
> watermark-coherence decision: a finite ceiling is only safe once we are
> confident the derived pin set fully covers the live snapshot working set.

## 5. Memory accounting (the ADR-0003 gate)

The relevant figure for the M3 exit gate is **working-set RSS at the configuration described in ADR-0003 §Decision**: 100 active views, 10k hot events, 1M cached on disk, ≤ 100 MB.

Components measured:

| Source | Approx bytes | Notes |
|---|---|---|
| Hot LRU (10k × Arc<Event>) | ~30 MB | average kind:1 event with content ~800 B, profile/contacts can be 4–8 KB each; mix-weighted average ~3 KB; the `Arc` is shared with view module payloads so the same body isn't duplicated |
| Claim refcount maps (≤20k pinned + 10k LRU entries) | ~1 MB | `HashMap<EventId, u32>` + reverse `by_claimer: HashMap<ClaimerId, BTreeSet<EventId>>` + `view_budgets`; global ceiling 20k pins keeps this bounded |
| Reverse index in-memory (composite keys for 100 views) | ~5 MB | from ADR-0001 — bounded by `~broad_axes_guardrail` per ADR-0001 |
| Projection caches (author display, reaction counts) | ~10 MB | LRU-bounded by referenced-view count per ADR-0003 |
| LMDB page cache (kernel-owned, *not* counted toward RSS budget) | 0 | OS-paged, evicted under pressure; counts against system memory but not app working set |
| Watermarks (loaded as `HashMap` for hot lookups) | ~2 MB | M4 — assuming O(10k) watermarks (one per `(filter, relay)` pair) |
| Tombstone bloom filter (if added — see open questions) | ~1 MB | accelerates the `tombstones.contains_key()` check on insert |
| Action ledger in-flight rows | ~1 MB | bounded by spec §7.5 |
| Slack / Rust allocator overhead | ~20 MB | empirical from reactivity-bench |
| **Total target** | **~70 MB** | leaves ~30 MB headroom against the 100 MB gate |

The 1M-events-on-disk dimension does **not** appear in the budget because LMDB does not page them into our heap; they exist in mmap'd pages the OS may evict at will. This is the design intent of ADR-0003.

## 6. Failure modes and degraded behavior

| Failure | Detection | Response |
|---|---|---|
| LMDB env out of space | LMDB `MDB_MAP_FULL` on a write | Run an emergency `gc_step()` with relaxed budget; if still full, surface `Effect::StoreOutOfSpace`, refuse new inserts, allow reads + deletes |
| LRU evicted a still-pinned event (bug) | `trim()` would have skipped it; if observed, log + invariant violation | Pin reinstated from `claims_meta`; fire `tracing::error!`; flagged as critical bug class to investigate |
| `gc_step()` over-budget | `start.elapsed() > max_duration_ms` mid-loop | Break out of current loop early; remaining work picked up next call (no state corruption — every reaped event is its own transaction) |
| `release()` called for unknown `ClaimerId` | `by_claimer.remove` returns None | Silent no-op; logged at debug; not a bug (idempotent close) |
| `claim()` exceeds per-view or global ceiling | Per-view: `by_claimer[c].len() + new_ids.len() > view_budgets[c]`; global: `pinned.len() + new_unique > max_pinned_total` (both counts deduplicated) | Return `StoreError::OverPinned`; state unchanged; actor surfaces `Effect::ViewOverPinned` and calls `release(claimer_id)` |
| Memory warning during heavy insert burst | iOS `didReceiveMemoryWarning` → `MemoryWarningCapability` event | Actor lowers `target_hot_size` to 5k, runs `gc_step({max_events_per_step:5000, max_duration_ms:200})` once; restored after the warning clears |

## 7. Diagnostics integration (ADR-0007)

The store exposes a `StoreHealth` snapshot for the diagnostics bridge:

```rust
pub struct StoreHealth {
    pub primary_event_count: u64,
    pub tombstone_count: u64,
    pub hot_lru_size: usize,
    pub claim_pinned_count: usize,
    pub watermark_count: usize,
    pub on_disk_bytes: u64,
    pub last_gc: Option<GcReport>,
    pub last_gc_at_ms: Option<u128>,
}
```

Surfaced in the diagnostics screen alongside relay status (ADR-0007 §1). The Phase 1a.7 proof app already has the rendering scaffold (`ios/NmpStress/NmpStress/DiagnosticsView.swift`); M3 adds the StoreHealth row to it.

## 8. Why not a periodic full sweep?

A full sweep is `O(stored_events)`. With 1M events on disk the LMDB scan alone is 100–500 ms wall-time on iPhone 12 NAND — well outside the actor's single-message budget. The bounded `gc_step()` with explicit budget is therefore the only correct shape; it composes with LMDB's natural mmap eviction model and never blocks the mailbox for long.

A periodic vacuum/compact pass (LMDB's equivalent of `VACUUM`) **is** scheduled — once per app launch, at idle, after the first 30 seconds of quiescence. It is *not* part of `gc_step`'s budget envelope and runs as a separate low-priority actor message that yields between LMDB page boundaries.

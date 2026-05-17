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
    // Reverse map for cheap release().
    by_claimer: HashMap<ClaimerId, SmallVec<[EventId; 8]>>,
    target_hot_size: usize,
}

impl HotSet {
    pub fn claim(&mut self, c: ClaimerId, ids: &[EventId]) {
        for id in ids {
            *self.pinned.entry(*id).or_insert(0) += 1;
        }
        self.by_claimer.entry(c).or_default().extend_from_slice(ids);
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
            if evicted.is_none() { break; }           // every entry is pinned
        }
    }
}
```

`target_hot_size` is set from `AppConfig::hot_event_ceiling` (default 10,000) and may be lowered by `MemoryWarningCapability` events (iOS app suspend or low-memory warning → halve the ceiling, run `gc_step()` once, restore after the warning clears).

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

Single `gc_step()` is bounded by `GcBudget { max_events_per_step, max_duration_ms }`. Defaults: `max_events_per_step = 2000`, `max_duration_ms = 50`. The actor calls `gc_step()`:

- Every 60 seconds (cooperative; runs on the actor thread between mailbox messages).
- On `MemoryWarningCapability::Pressure` (iOS / Android low-memory signals).
- On any single `insert()` that observes `hot.lru.len() > 2 * target_hot_size` (safety net).

`gc_step()` is **never** invoked from an FFI call path — it runs on the actor's own schedule so any latency it introduces is invisible to the platform.

## 4. Claim / release wiring

The kernel actor holds `view_claims: HashMap<ViewId, ClaimerId>`. On `open_view(spec)`:

1. The view module's `dependencies(spec)` is consulted (per `kernel-substrate.md` §3).
2. The composite reverse-index resolves the dependency set to a (small, bounded) set of currently-known event ids — the *view cover*.
3. `store.claim(claimer_id, &cover_ids)` pins those events in hot.
4. As events arrive matching the dependency, the actor calls `store.claim(claimer_id, &[new_id])` incrementally (claim is idempotent under increment).

On `close_view(view_id)`:

1. `store.release(claimer_id)` drops every pin in one call.
2. The view module's `state` is dropped; its claim refcounts decay; the next `gc_step()` evicts any newly-unpinned cold from LRU.

Restart recovery: `claims_meta` sub-db ([`keys.md`](keys.md) §1) holds the persisted per-`ClaimerId` pin set. On startup the actor rebuilds active views first (per the diagnostics replay sequence), then re-claims; entries in `claims_meta` whose `ClaimerId` is not associated with a re-opened view are dropped from the persisted map. This means the cold-start path always re-derives claims from open-view state, but the persistence is what lets the store survive an actor restart without losing hot-set protection mid-shutdown.

## 5. Memory accounting (the ADR-0003 gate)

The relevant figure for the M3 exit gate is **working-set RSS at the configuration described in ADR-0003 §Decision**: 100 active views, 10k hot events, 1M cached on disk, ≤ 100 MB.

Components measured:

| Source | Approx bytes | Notes |
|---|---|---|
| Hot LRU (10k × Arc<Event>) | ~30 MB | average kind:1 event with content ~800 B, profile/contacts can be 4–8 KB each; mix-weighted average ~3 KB; the `Arc` is shared with view module payloads so the same body isn't duplicated |
| Claim refcount maps (10k entries) | ~0.5 MB | `HashMap<EventId, u32>` + reverse `by_claimer` |
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

# Reactivity: Scheduling And Data Model

[Back to Design: Reactivity](../reactivity.md)

## 7. Scheduling and batching

### 7.1 Synchronous fan-out

All `on_event_inserted` / `on_event_removed` / `on_projection_changed` calls happen **synchronously on the actor thread**, inline with the triggering `CoreMsg`. This preserves the single-writer invariant and the actor's deterministic message ordering.

No view code spawns tokio tasks. No view code awaits. If a view needs async work (e.g., fetching a NIP-05 record to verify a domain), it returns immediately and the actor schedules a separate `CoreMsg` to handle the async completion.

### 7.2 Delta buffer with within-view coalescing (ADR-0002)

```rust
pub struct DeltaBuffer {
    deltas: Vec<ViewDelta>,
    pending_full_state: bool,
    last_flush: Instant,
}
```

After processing each `CoreMsg`, the actor checks if a flush is due:

- **Time-based:** `now - last_flush >= flush_interval` (default 16ms = ~60Hz).
- **Size-based:** `deltas.len() >= max_buffered_deltas` (default 256 pre-coalesce).
- **Forced:** certain messages (account switch, view open) force an immediate flush.

On flush, the actor **coalesces by view id, applying per-view-kind merge rules**, then emits one `AppUpdate::ViewBatch { rev, views: coalesced_deltas }`. If `pending_full_state` is true, emit `AppUpdate::FullState` instead and discard the buffered deltas.

```rust
fn flush(&mut self) -> AppUpdate {
    self.deltas.sort_by_key(|d| d.view_id());
    let mut out = Vec::new();
    for (view_id, group) in self.deltas.drain(..).chunk_by(|a, b| a.view_id() == b.view_id()) {
        out.extend(coalesce(view_id, group.collect()));
    }
    self.last_flush = Instant::now();
    AppUpdate::ViewBatch { rev: ..., views: out }
}
```

Per-view-kind coalescing rules (full enumeration per kind in `view-catalog.md`):

| View kind | Rule |
|---|---|
| Timeline | Consecutive `Inserted { at, items }` at adjacent positions merge. `Updated { id, item }` for different ids with shared author projection collapse to one `UpdatedMany { ids, patch: AuthorPatch }`. `Removed { ids }` accumulate. |
| Reactions | N `EmojiAdjusted { emoji, delta }` for same emoji → one with summed delta. Different emojis stay separate. |
| Conversation | Consecutive `Appended { messages }` merge. |
| Profile | `Replaced { payload }` later supersedes earlier within tick. |
| Thread | Multiple `NodeInserted` accumulate; `RootUpdated` later supersedes earlier. |

Per-view delta budget: ≤ 60 deltas/sec/view (matches the 60Hz flush). Total `ViewBatch` size scales with active view count; no absolute ceiling.

### 7.3 Backpressure

If the reconciler callback latency (measured via metrics) exceeds 100ms p99, the actor switches to **catch-up mode**:

- Stop emitting deltas.
- On next tick, emit a single `FullState` snapshot.
- Resume delta emission once latency drops below threshold.

This is lossless: the `FullState` snapshot includes every open view's current payload, so the platform's shadow ends up in the same state it would have via deltas.

---

## 7.4 The three-tier data model (ADR-0005 codifies the boundary)

Data lives in three derived tiers; only the bottom is source of truth.

```
┌──────────────────────────────────────────────────────────┐
│   Tier 3: Platform shadow (TTL, domain-keyed)            │
│   - profiles: [PubKey: ProfileView]                      │
│   - reactionSummaries: [EventId: ReactionSummary]        │
│   - timelines: [SpecHash: TimelineView]                  │
│   - Read by components via wrappers (useProfile, etc.)   │
│   - Evicts on refcount=0 after 30s grace                 │
│   - Not persisted; rebuilt from Rust on cold start       │
└──────────────────────▲───────────────────────────────────┘
                       │ ViewBatch / FullState via FFI
┌──────────────────────┴───────────────────────────────────┐
│   Tier 2: Rust working set + projections                 │
│   - Hot events resident in memory (LRU-bounded)          │
│   - Projection caches (author_display, reaction_summary) │
│   - Reverse index (composite-keyed)                      │
│   - View payloads in actor's view registry               │
└──────────────────────▲───────────────────────────────────┘
                       │ EventStore reads
┌──────────────────────┴───────────────────────────────────┐
│   Tier 1: Rust durable storage (source of truth)         │
│   - LMDB / SQLite / IndexedDB / nostrdb                  │
│   - All events ever cached, replaceable supersession     │
│     enforced, NIP-40 expiration scheduled, kind:5        │
│     tombstones, sync watermarks                          │
└──────────────────────────────────────────────────────────┘
```

Properties:

- **Tier 1** is the only persisted layer. Tier 2 is rebuilt from Tier 1 on actor restart. Tier 3 is rebuilt from Tier 2 on app restart.
- **Tier 2 is bounded** (working-set policy, ADR-0003). The reverse index keys on attributes, not bodies, so it can cover unbounded Tier-1 events.
- **Tier 3 is TTL-bounded** (ADR-0005). The platform shadow holds only domain entries with active component interest.
- **Reads flow up; updates flow down.** Component reads happen entirely in Tier 3 — no FFI on the read path. Updates from relays land in Tier 1, propagate to Tier 2 working set + projections, then to Tier 3 via `ViewBatch`.
- **Subscription lifecycle is refcounted per tier.** Component refcount in Tier 3 drives `OpenView`/`CloseView` to Rust; Rust's claim count in Tier 2 drives planner REQ subscriptions to relays; relay REQs on the wire are the bottom of the stack.

## 7.5 Working-set discipline (ADR-0003)

The `EventStore` holds a **bounded hot working set** in memory; cold events live in the durable storage backend. The reverse index covers both.

Working-set policy:

- **Hot:** events referenced by any open view's claim set, plus a configurable recency window (default: most recent 5,000 events globally).
- **Cold:** everything else, on disk only.
- **Eviction:** LRU among hot events not currently claimed. Claimed events are never evicted.

The reverse index keys on attributes, not bodies, so it can cover unbounded cached events. Lookup returns view ids immediately. When a delta needs to be constructed from a cold event, the body is loaded synchronously from the storage backend.

Projection caches (`author_display`, `reaction_summary`, etc.) are LRU-bounded by **referenced-view count** — only pubkeys/events referenced by some open view stay in cache. A profile rendered once and dismissed evicts; reappearance re-hydrates from the store.

The working-set memory budget (≤ 100 MB at 100 active views, 10k hot events) is what reactivity-bench gates against. Total cached events on disk is unbounded.

## 7.6 Allocation discipline (ADR-0004)

The harness binary uses a counting allocator (custom `GlobalAlloc` wrapper or `dhat`) to verify the zero-allocation-per-event steady-state invariant.

Steady-state is defined as: after a 1,000-event warmup window. First-time path allocations (view registration, projection-cache miss) are exempt.

The harness reports per-scenario:

- Total allocations.
- Allocations attributable to insert → lookup → recompute → buffer-push path.
- Peak heap.

A scenario fails the gate if any per-event allocation appears post-warmup.

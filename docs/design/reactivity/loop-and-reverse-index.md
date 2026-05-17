# Reactivity: Loop And Reverse Index

[Back to Design: Reactivity](../reactivity.md)

# Design: Reactivity (internal mechanism)

> **Audience:** Framework contributors (not app developers). App developers see only the public surface in `product-spec.md` §6.4, §6.6, §7.6.

> **Status:** rev 1 — incorporating findings from reactivity-bench run 001 (2026-05-17). See `docs/perf/reactivity-bench/2026-05-17-run-001.md` for the measurement report. Decisions: ADR-0001 (composite keys), ADR-0002 (per-view delta budget), ADR-0003 (working-set memory), ADR-0004 (allocation measurement).

> **Prerequisites:** `product-spec.md` §6.2 (`AppState`), §6.4 (`AppUpdate`), §6.6 (Subscriptions/views), §7.2 (planner), §7.6 (Views), Appendix A1 (FFI architecture).

---

## 1. What this doc covers

The internal Rust-side machinery the actor uses to keep view payloads in sync with the event store. Specifically:

- How the actor decides which views care about a newly-inserted event.
- How each view recomputes (or incrementally updates) its payload.
- The `ViewDelta` enum that the recompute produces.
- How shared projections (author display names, reaction counts) are reused across views.
- How deltas are batched into the `ViewBatch` that crosses FFI.

What this doc does **not** cover: the public API for opening/closing views, the cross-FFI delivery mechanism, the per-view-kind payload shapes (those are in `view-catalog.md`).

---

## 2. The actor's reactive loop, end-to-end

```
                ┌──────────────────────────────────────────┐
                │           Actor message loop             │
                │                                          │
relay/sync ───▶ │  CoreMsg::EventInserted(event)           │
                │     │                                    │
                │     ▼                                    │
                │  EventStore::insert(event)               │
                │     │  (replaceable handling, GC, etc.)  │
                │     ▼                                    │
                │  ReverseIndex::lookup(event) ─▶ Vec<ViewId>│
                │     │                                    │
                │     ▼                                    │
                │  For each ViewId:                        │
                │     view.on_event_inserted(event)        │
                │     ─▶ Option<ViewDelta>                 │
                │     │                                    │
                │     ▼                                    │
                │  DeltaBuffer::push(delta)                │
                │                                          │
                │  ── on tick (≤60Hz) ──                   │
                │  DeltaBuffer::flush()                    │
                │  ─▶ AppUpdate::ViewBatch                 │
                │  ─▶ update_tx.send()                     │
                │                                          │
                └──────────────────────────────────────────┘
                              │
                              ▼
                    Reconciler callback (background thread)
                              │
                              ▼
                       Platform UI thread
```

Three subsystems collaborate:

- **`EventStore`** owns the actual events and the reverse index. Inserting goes through it.
- **`ViewRegistry`** owns the open views (`HashMap<ViewId, View>`). Each entry is one of the view-kind structs.
- **`DeltaBuffer`** accumulates per-tick deltas and emits `ViewBatch` at the planner's batching interval.

All three live on the single actor thread. No locks, no atomics; just sequential message processing.

---

## 3. Dependency tracking: the reverse index

### 3.1 The problem

When an event arrives, the actor needs to know which views care about it. Naive answer: run every view's filter against every event. That's O(views × inserts), unworkable at firehose scale.

### 3.2 The decision: composite-keyed reverse index (ADR-0001)

The store maintains a reverse index keyed primarily by **composite** event attributes, not by independent axes. Independent-axis buckets exist only for views with genuinely broad filters; using them produces a debug-build guardrail warning.

```rust
pub struct ReverseIndex {
    // Primary (composite) keys — preferred for almost all views
    by_kind_author: HashMap<(u16, PubKey), HashSet<ViewId>>,
    by_kind_e_tag: HashMap<(u16, EventId), HashSet<ViewId>>,
    by_kind_p_tag: HashMap<(u16, PubKey), HashSet<ViewId>>,
    by_kind_author_d: HashMap<(u16, PubKey, String), HashSet<ViewId>>,
    by_kind_d_tag: HashMap<(u16, String), HashSet<ViewId>>,

    // Broad (single-axis) keys — guardrailed; for search / hashtag-scan only
    by_kind: HashMap<u16, HashSet<ViewId>>,
    by_author: HashMap<PubKey, HashSet<ViewId>>,
    by_e_tag: HashMap<EventId, HashSet<ViewId>>,
    by_p_tag: HashMap<PubKey, HashSet<ViewId>>,
    by_d_tag: HashMap<String, HashSet<ViewId>>,

    catch_all: HashSet<ViewId>,
}
```

When a view opens, the registry picks the **most specific** index that covers its dependencies:

| View shape (from `Dependencies`) | Indexes used |
|---|---|
| kinds + authors | `by_kind_author[(k, a)]` for each k×a |
| kinds + e-tag refs | `by_kind_e_tag[(k, e)]` |
| kinds + p-tag refs | `by_kind_p_tag[(k, p)]` |
| kinds + authors + d-tag refs | `by_kind_author_d[(k, a, d)]` |
| kinds + d-tag refs only | `by_kind_d_tag[(k, d)]` |
| kinds only | `by_kind[k]` — broad-cost flag |
| authors only | `by_author[a]` — broad-cost flag |
| no constraint | `catch_all` — guardrail warning |

Each view, when opened, registers a `Dependencies` declaration:

```rust
pub struct Dependencies {
    pub kinds: Vec<u16>,
    pub authors: Vec<PubKey>,
    pub e_tag_refs: Vec<EventId>,
    pub p_tag_refs: Vec<PubKey>,
    pub d_tag_refs: Vec<String>,
    pub catch_all_filter: Option<Filter>,                // expensive — see §3.4
}
```

The registry computes the most-specific composite registration internally; the view doesn't enumerate cartesian products itself.

**Why composite-first:** reactivity-bench run 001 measured 98% false-wakeup rate in quiet_idle and 49% in following_timeline_scroll under the v0 design (which unioned independent axis buckets). Conjunctive composite keys eliminate the false wakes. The cost is registration-size growth (kinds × authors cartesian product), bounded by working-set memory budget.

### 3.3 Lookup on insert

When an event arrives, compute its **tuple signature** — every composite-key tuple this event implies — and look up each. Union the small resulting sets.

```rust
fn lookup(&self, event: &Event) -> HashSet<ViewId> {
    let mut hits = HashSet::new();

    // Composite (primary) lookups
    hits.extend(self.by_kind_author.get(&(event.kind, event.pubkey)).into_iter().flatten().copied());
    if let Some(d) = event.d_tag() {
        hits.extend(self.by_kind_author_d.get(&(event.kind, event.pubkey, d.clone())).into_iter().flatten().copied());
        hits.extend(self.by_kind_d_tag.get(&(event.kind, d)).into_iter().flatten().copied());
    }
    for e_ref in event.e_tags() {
        hits.extend(self.by_kind_e_tag.get(&(event.kind, e_ref)).into_iter().flatten().copied());
    }
    for p_ref in event.p_tags() {
        hits.extend(self.by_kind_p_tag.get(&(event.kind, p_ref)).into_iter().flatten().copied());
    }

    // Broad (guardrailed) lookups — empty for well-shaped apps
    hits.extend(self.by_kind.get(&event.kind).into_iter().flatten().copied());
    hits.extend(self.by_author.get(&event.pubkey).into_iter().flatten().copied());
    hits.extend(&self.catch_all);

    hits
}
```

Cost: O(K + P) composite lookups plus O(|broad indexes used|) plus O(|catch_all|). For an event with K e-tags and P p-tags in a well-shaped app, that's a handful of HashMap probes. Reactivity-bench run 001 measured p99 lookup at 84 ns to 1,083 ns — far below the 100 µs gate.

### 3.4 The catch-all slow path

Some views have filters that don't naturally key into the index — full-text search on event content, time-windowed scans across many authors, regex over tags. For those, `catch_all_filter` causes the view to be considered for every insert; the view's `on_event_inserted` evaluates the filter and decides.

This is the expensive path. The guardrails (`nmp-guardrails`) emit a warning in debug builds when a view declares `catch_all_filter`, so framework users notice when they're paying the cost. Most built-in view kinds will never use it.

### 3.5 What ruled out alternatives

| Alternative | Why not |
|---|---|
| Per-event run all view filters | O(views × inserts); breaks at firehose scale. |
| Event-bus pub/sub (e.g. `tokio::broadcast` keyed by filter hash) | Doesn't compose for views whose filter overlaps multiple categories; adds a dependency without removing complexity. |
| External observables library (`futures-signals`, etc.) | We don't need composable operators; we have a closed set of recompute call sites. Adds abstraction overhead and unclear scheduling guarantees inside the actor loop. |
| Trie/B-tree by tag prefix | Premature; HashMap is fine until benchmarks show otherwise. |

---

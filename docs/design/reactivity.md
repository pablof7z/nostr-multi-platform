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

## 4. The per-view-kind contract

Each view kind is a Rust module with a `State` struct and a set of free functions. **No trait.** Reasoning: the framework owns the closed set of view kinds for v1 (spec §13 open question 6 defers consumer-defined view kinds), and enum-based dispatch in the actor is simpler than a trait object to read and debug.

```rust
// crates/nmp-views/src/timeline.rs

pub struct State {
    spec: TimelineSpec,
    items: Vec<TimelineItem>,
    cursor: Cursor,
    by_event_id: HashMap<EventId, usize>,  // index into items
    by_author: HashMap<PubKey, Vec<usize>>, // for kind:0 refresh
}

pub fn open(spec: TimelineSpec, store: &EventStore) -> (State, Dependencies, TimelineView) {
    let deps = compute_dependencies(&spec);
    let mut state = State { spec, items: vec![], cursor: Cursor::Empty, by_event_id: HashMap::new(), by_author: HashMap::new() };
    state.recompute_full(store);
    let payload = state.snapshot();
    (state, deps, payload)
}

pub fn on_event_inserted(state: &mut State, event: &Event, store: &EventStore) -> Option<TimelineDelta> {
    if !matches_spec(&state.spec, event) { return None; }
    let item = build_item(event, store);
    let at = state.items.iter().position(|i| i.created_at < item.created_at).unwrap_or(state.items.len());
    state.items.insert(at, item.clone());
    reindex_after_insert(state, at);
    Some(TimelineDelta::Inserted { at, items: vec![item] })
}

pub fn on_event_removed(state: &mut State, id: &EventId) -> Option<TimelineDelta> { ... }
pub fn on_event_replaced(state: &mut State, old_id: &EventId, new_event: &Event, store: &EventStore) -> Option<TimelineDelta> { ... }
pub fn on_projection_changed(state: &mut State, change: &ProjectionChange) -> Option<TimelineDelta> { ... }
```

The actor's dispatch is a `match` on the enum that wraps each view kind:

```rust
enum View {
    Profile(profile::State),
    Timeline(timeline::State),
    Thread(thread::State),
    // ... 12 more
}

impl View {
    fn on_event_inserted(&mut self, event: &Event, store: &EventStore) -> Option<ViewDelta> {
        match self {
            View::Profile(s) => profile::on_event_inserted(s, event, store).map(ViewDelta::Profile),
            View::Timeline(s) => timeline::on_event_inserted(s, event, store).map(ViewDelta::Timeline),
            View::Thread(s) => thread::on_event_inserted(s, event, store).map(ViewDelta::Thread),
            // ...
        }
    }
}
```

This is verbose but explicit. A future v2 can revisit the trait-vs-enum question if consumer-defined view kinds are added.

---

## 5. `ViewDelta` catalog (high-level)

`ViewDelta` is a per-view-kind enum-of-enums:

```rust
#[derive(Clone, uniffi::Enum)]
pub enum ViewDelta {
    Profile { id: ViewId, delta: ProfileDelta },
    Timeline { id: ViewId, delta: TimelineDelta },
    Thread { id: ViewId, delta: ThreadDelta },
    Reactions { id: ViewId, delta: ReactionsDelta },
    Conversation { id: ViewId, delta: ConversationDelta },
    ConversationList { id: ViewId, delta: ConversationListDelta },
    // ... one variant per view kind
    FullReplace { id: ViewId, payload: ViewPayload },  // escape hatch
}
```

Per-view-kind delta shapes are designed for each view's natural update pattern. See `view-catalog.md` for the full enumeration. Examples:

```rust
pub enum TimelineDelta {
    Inserted { at: usize, items: Vec<TimelineItem> },
    Removed { ids: Vec<String> },
    Updated { id: String, item: TimelineItem },
    CursorAdvanced { cursor: Cursor },
}

pub enum ProfileDelta {
    Replaced { payload: ProfilePayload },
}

pub enum ReactionsDelta {
    Adjusted { emoji: String, delta: i32 },
    NewEmoji { emoji: String, count: u32 },
}
```

The `FullReplace` variant is the per-view-kind escape hatch: when in-place delta computation would be more expensive than just shipping the whole payload (rare; threshold TBD), emit it.

---

## 6. Shared projections, not view-on-view

### 6.1 The problem

A `TimelineView` payload contains 200 `TimelineItem`s. Each item has an `author_display: String`. When a new kind:0 arrives for one of those authors, **every item in every view by that author needs its display updated**. Naively, that's a fan-out problem: each view scans every item.

### 6.2 The decision: shared projection cache + targeted wake

The store maintains projection caches:

```rust
pub struct Projections {
    author_display: HashMap<PubKey, AuthorDisplay>,
    author_picture: HashMap<PubKey, String>,
    author_nip05: HashMap<PubKey, Option<String>>,
    reaction_summary: HashMap<EventId, ReactionSummary>,
    zap_total: HashMap<EventId, u64>,
    reply_count: HashMap<EventId, u32>,
}
```

When a kind:0 arrives, the store:

1. Inserts the event (replaceable supersession in the store proper).
2. Recomputes `author_display`, `author_picture`, `author_nip05` for that pubkey from the new kind:0.
3. If any field actually changed, emits a `ProjectionChange::AuthorDisplay { pubkey, new: AuthorDisplay }` to the registry.
4. Registry looks up views indexed by that pubkey via the reverse index (`by_author`), calls each view's `on_projection_changed`.

The view's `on_projection_changed` for a timeline:

```rust
pub fn on_projection_changed(state: &mut State, change: &ProjectionChange) -> Option<TimelineDelta> {
    match change {
        ProjectionChange::AuthorDisplay { pubkey, new } => {
            let idxs = state.by_author.get(pubkey)?.clone();
            for idx in idxs {
                state.items[idx].author_display = new.display.clone();
                state.items[idx].author_picture = new.picture.clone();
                state.items[idx].author_nip05_domain = new.nip05_domain.clone();
            }
            // Emit a coalesced delta. For multiple items, a single Updated would lose info;
            // we use a typed `UpdatedMany` variant (see §5).
            Some(TimelineDelta::UpdatedMany { ids: idxs.iter().map(|i| state.items[*i].id.clone()).collect(), patch: AuthorPatch { ... } })
        }
        ProjectionChange::ReactionSummary { event_id, new } => { ... }
        // ...
    }
}
```

This is cheap: O(items by that author in this view), which is typically tiny (1–5).

### 6.3 Why not view-on-view subscriptions?

You could model this as "TimelineView subscribes to ProfileView for each author." That creates a dependency graph that's hard to reason about, hard to GC, and hard to debug. Shared projections in the store let us keep view internals flat and the dispatch story simple.

### 6.4 What goes in `Projections` vs what doesn't

Projections are for **frequently-read derived facts about events and pubkeys** that many views consume. Heuristic:

- Read by more than one view kind → projection.
- Cheap to compute from the underlying events → projection.
- Expensive to compute but stable (e.g., NIP-05 verification) → projection with background refresh.
- One-off display logic specific to a single view → not a projection; live in the view's own state.

---

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

## 8. What this design rules out

Listed so we notice when we accidentally violate them:

- **Async view recompute.** Views are synchronous. Async data flow goes through a separate `CoreMsg` and lands as a fresh insert/projection change.
- **Cross-view dependencies.** Views read from the store and from projections, never from other views.
- **Mutation of `EventStore` from within a view.** Views observe; they don't write. Only the actor's top-level handlers and actions write.
- **Hidden allocations on hot paths.** The reverse-index lookup, the view dispatch, and the delta buffer must not allocate in steady state (we'll use `SmallVec`, `IndexSet`, etc. where appropriate).
- **Per-event FFI calls.** All FFI emission happens at the batch boundary, never per-event.

---

## 9. Open questions to settle empirically

These questions can be answered with measurement, not argument. They block locking in the design but not starting to build (the design can absorb them).

1. **Reverse-index hit rate.** For realistic Nostr apps, what fraction of inserts have ≥ 1 view interested? Affects whether we should optimize the no-hit path.
2. **Recompute cost per view kind.** Is incremental insert into a 200-item timeline genuinely cheaper than full rebuild from a 10k-event store? Probably yes, but at what threshold does it flip?
3. **Projection cache scope.** Should `author_display` cache **every** pubkey we've ever seen, or only those currently referenced by an open view? The latter saves memory; the former saves recompute on view open.
4. **Delta buffer thresholds.** Is 16ms / 256 deltas the right default flush trigger, or should it adapt to platform-measured callback latency?
5. **`UpdatedMany` vs N × `Updated`.** When a projection change affects 50 items in a timeline, is one fat delta cheaper or 50 small ones? Wire cost says fat; per-platform application cost says depends.
6. **`catch_all_filter` cost.** What's the per-event overhead of filter matching for views that need it? Is it tolerable for hashtag search?
7. **Backpressure threshold.** Is 100ms p99 callback latency the right trigger for switching to `FullState` catch-up, or too aggressive / too lax?

---

## 10. Next step: build the stress harness *before* committing to this design

This design has assumptions that need to be measured, not argued. Before locking it in (i.e., before Phase 1 of the build plan goes very far), build a **standalone stress harness** in `nmp-testing`:

### 10.1 Harness scope

A headless Rust binary (`nmp-testing/bin/reactivity-bench`) that:

- Spawns a configurable `EventStore` (in-memory, LMDB, or SQLite backend).
- Pre-populates it with N synthetic events (configurable: 1k, 10k, 100k, 1M).
- Opens M views with configurable filter mixes (timelines, threads, profiles, hashtag catch-alls).
- Replays a configurable event stream (steady 100/sec, burst, firehose 500/sec, hashtag firehose 2000/sec).
- Reports: per-event lookup time, per-view recompute time, delta buffer fill rate, `ViewBatch` emission rate, memory footprint, allocation counts.

### 10.2 Scenarios to run

1. **Quiet idle.** 10k events in store, 10 views open, 1 event/sec.
2. **Following timeline scroll.** 100k events in store, 1 timeline view over 1k authors, scroll triggers (cursor advances) every 500ms.
3. **Hashtag firehose.** 1M events in store, 1 catch-all view over hashtag `#nostr`, 200 events/sec inbound.
4. **Profile fan-out.** 10k events, 50 timeline views over overlapping author sets, kind:0 for shared author arrives — measure how many views update and how fast.
5. **Thread blow-up.** 1 thread view, the root event has 500 replies + 5000 reactions; measure incremental vs full rebuild.
6. **Account switch.** 10 accounts, each with active views; switch between them; measure teardown + setup time.

### 10.3 Gates on harness results (rev 1, post run 001)

Refined per ADR-0001 through ADR-0004:

- **Reverse-index lookup p99 ≤ 100µs** at 100k events / 50 views. (Run 001: validated at 84 ns – 1,083 ns.)
- **Per-view incremental recompute p99 ≤ 1ms.** (Run 001: validated at ≤ 9,625 ns.)
- **Delta emission ≤ 60 deltas/sec/view** under all scenarios. (Per-view, not absolute. ADR-0002.)
- **False-wakeup rate ≤ 0.10** across all scenarios. (ADR-0001 gate. Run 001: 98%/49% under v0; expected near-zero under composite-key model.)
- **Candidates per delta ≤ 1.25.** (Sister metric to false-wake rate.)
- **Working-set memory ≤ 100 MB** at 100 active views / 10k hot events / 1M cached on disk. (ADR-0003.)
- **Zero per-event allocations on the steady-state path** after 1,000-event warmup, verified by counting allocator. (ADR-0004.)

If any gate fails, the design choices in §3–§7 get revisited before Phase 1 proceeds further. Each gate failure surfaces a write-up in `docs/perf/reactivity-bench/<date>-run-<n>.md` plus an ADR when a design change is adopted.

### 10.4 Where the harness lives

```
crates/nmp-testing/
├── src/
│   └── ...
└── bin/
    └── reactivity-bench/
        ├── main.rs
        ├── scenarios/
        └── reports/
```

Output is JSON, archived per run in `docs/perf/reactivity-bench/<date>.json`, with a Markdown summary in `docs/perf/reactivity-bench/<date>.md`. The proof app's performance overlay (Phase 8) reuses the same metric definitions.

### 10.5 Why this gate matters

The reverse-index + projection-cache + delta-buffer architecture is the load-bearing performance story for the entire framework. If it doesn't measure up, snapshots+ViewBatch falls back to "ship `FullState` everywhere" which we already know doesn't scale for Nostr timelines (Appendix A1 of the spec). The harness is how we know we don't have to fall back to the SQLite-shared-store hybrid prematurely (Appendix A2 of the spec).

Build the harness first. Measure. Then commit to Phase 1 of the build plan in earnest.

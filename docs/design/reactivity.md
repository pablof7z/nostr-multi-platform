# Design: Reactivity (internal mechanism)

> **Audience:** Framework contributors (not app developers). App developers see only the public surface in `product-spec.md` §6.4, §6.6, §7.6.

> **Status:** Draft. Decisions here are proposals; the open questions in the last section are gated on the stress harness (next step at the bottom).

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

### 3.2 The decision: reverse index by event attributes

The store maintains a reverse index from event attributes to interested views:

```rust
pub struct ReverseIndex {
    by_kind: HashMap<u16, HashSet<ViewId>>,
    by_author: HashMap<PubKey, HashSet<ViewId>>,
    by_e_tag: HashMap<EventId, HashSet<ViewId>>,
    by_p_tag: HashMap<PubKey, HashSet<ViewId>>,
    by_d_tag: HashMap<String, HashSet<ViewId>>,
    by_kind_author: HashMap<(u16, PubKey), HashSet<ViewId>>,
    by_kind_author_d: HashMap<(u16, PubKey, String), HashSet<ViewId>>,
    catch_all: HashSet<ViewId>,
}
```

Each view, when opened, registers a `Dependencies` declaration:

```rust
pub struct Dependencies {
    pub kinds: Vec<u16>,
    pub authors: Vec<PubKey>,
    pub e_tag_refs: Vec<EventId>,
    pub p_tag_refs: Vec<PubKey>,
    pub d_tag_refs: Vec<String>,
    pub kind_author_pairs: Vec<(u16, PubKey)>,         // for replaceable supersession
    pub kind_author_d_triples: Vec<(u16, PubKey, String)>, // for parameterized replaceable
    pub catch_all_filter: Option<Filter>,                // expensive — see §3.4
}
```

The registry calls `index.register(view_id, deps)` on open, `index.deregister(view_id)` on close.

### 3.3 Lookup on insert

When an event arrives:

```rust
fn lookup(&self, event: &Event) -> HashSet<ViewId> {
    let mut hits = HashSet::new();
    hits.extend(self.by_kind.get(&event.kind).into_iter().flatten().copied());
    hits.extend(self.by_author.get(&event.pubkey).into_iter().flatten().copied());
    hits.extend(self.by_kind_author.get(&(event.kind, event.pubkey)).into_iter().flatten().copied());
    if let Some(d) = event.d_tag() {
        hits.extend(self.by_kind_author_d.get(&(event.kind, event.pubkey, d)).into_iter().flatten().copied());
        hits.extend(self.by_d_tag.get(&d).into_iter().flatten().copied());
    }
    for e_ref in event.e_tags() {
        hits.extend(self.by_e_tag.get(e_ref).into_iter().flatten().copied());
    }
    for p_ref in event.p_tags() {
        hits.extend(self.by_p_tag.get(p_ref).into_iter().flatten().copied());
    }
    hits.extend(&self.catch_all);
    hits
}
```

Cost: O(1) per attribute lookup plus O(catch_all). For an event with K e-tags and P p-tags, that's O(K + P + |catch_all|) which is typically tiny (1–5 lookups, near-empty catch_all in well-designed apps).

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

### 7.2 Delta buffer

```rust
pub struct DeltaBuffer {
    deltas: Vec<ViewDelta>,
    pending_full_state: bool,
    last_flush: Instant,
}
```

After processing each `CoreMsg`, the actor checks if a flush is due:

- **Time-based:** `now - last_flush >= flush_interval` (default 16ms = ~60Hz).
- **Size-based:** `deltas.len() >= max_buffered_deltas` (default 256).
- **Forced:** certain messages (account switch, view open) force an immediate flush.

On flush, the actor emits one `AppUpdate::ViewBatch { rev, views: deltas.drain() }`. If `pending_full_state` is true, emit `AppUpdate::FullState` instead and discard the buffered deltas.

### 7.3 Backpressure

If the reconciler callback latency (measured via metrics) exceeds 100ms p99, the actor switches to **catch-up mode**:

- Stop emitting deltas.
- On next tick, emit a single `FullState` snapshot.
- Resume delta emission once latency drops below threshold.

This is lossless: the `FullState` snapshot includes every open view's current payload, so the platform's shadow ends up in the same state it would have via deltas.

---

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

### 10.3 Gates on harness results

The Phase 1 exit gate (per `plan.md`) should be extended:

- Reverse-index lookup p99 ≤ 100µs at 100k events / 50 views.
- Per-view incremental recompute p99 ≤ 1ms.
- `ViewBatch` emission rate stays ≤ 60Hz under hashtag firehose with cumulative delta count ≤ 1000/sec.
- Memory footprint of reverse index + projection caches ≤ 100MB at 100k events / 100 views.
- No allocations per-event on the steady-state path (verified by `dhat` or similar).

If any gate fails, the design choices in §3–§6 get revisited before Phase 1 proceeds further.

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

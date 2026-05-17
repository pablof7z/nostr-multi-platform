# Reactivity: View Deltas And Projections

[Back to Design: Reactivity](../reactivity.md)

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

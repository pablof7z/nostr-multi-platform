# View Catalog: Profile, Timeline, Thread, Reactions

[Back to Design: View Catalog](../view-catalog.md)

## 2. View kinds — full enumeration

| # | Kind | Detailed in this doc? | Phase |
|---|---|---|---|
| 1 | Profile | yes (§3) | 1 |
| 2 | Contacts | stub (§9) | 1 |
| 3 | Mailboxes | stub (§9) | 1 |
| 4 | Mutes | stub (§9) | 2 |
| 5 | Blossom servers | stub (§9) | 6 |
| 6 | Timeline | yes (§4) | 1 |
| 7 | Thread | yes (§5) | 1 |
| 8 | Replies | stub (§9) | 1 |
| 9 | Reactions | yes (§6) | 1 |
| 10 | Conversation list | stub (§9) | 5 |
| 11 | Conversation | yes (§7) | 5 |
| 12 | Zap history | stub (§9) | 6 |
| 13 | Wallet balance | stub (§9) | 6 |
| 14 | WoT rank | stub (§9) | 6 |
| 15 | Search | stub (§9) — heavy `catch_all_filter` | 1 |

The five detailed entries cover the structural patterns that the stubbed kinds will follow.

---

## 3. View: Profile

The simplest view. A pure projection of the latest kind:0 for a pubkey, with NIP-05 verification and bech32 encoding pre-computed.

### Spec

```rust
pub struct ProfileSpec {
    pub pubkey: PubKey,
    pub verify_nip05: bool,    // default true
}
```

### Payload

```rust
#[derive(Clone, uniffi::Record)]
pub struct ProfileView {
    pub pubkey: String,             // hex
    pub npub: String,               // bech32
    pub display_name: String,       // never empty (placeholder: shortened npub)
    pub name: String,
    pub about: String,
    pub picture: String,            // never empty (placeholder: identicon URI)
    pub banner: String,
    pub nip05: String,
    pub nip05_verified: bool,
    pub lud16: String,
    pub website: String,
    pub event_id: Option<String>,   // None if no kind:0 yet
    pub freshness: FreshnessHint,
}
```

### Delta

```rust
#[derive(Clone, uniffi::Enum)]
pub enum ProfileDelta {
    Replaced { payload: ProfileView },
}
```

Profiles are replaceable (kind:0). A new kind:0 means a fresh payload. No incremental — full replacement is the natural granularity and the payload is small.

### Dependencies

```rust
Dependencies {
    kinds: vec![0, 10002],                      // kind:0 (profile) + mailbox-tagged refresh
    authors: vec![spec.pubkey],
    kind_author_pairs: vec![(0, spec.pubkey)],
    ..Default::default()
}
```

`kind_author_pairs` is what fires on replaceable supersession.

### Recompute strategy

Full rebuild on every kind:0 arrival; cheap, payload is small.

### Best-effort placeholders

| Field | Placeholder |
|---|---|
| `display_name` | shortened npub (`npub1abc…xyz`) |
| `picture` | `data:image/svg+xml;...` identicon derived from pubkey |
| `name`, `about`, `nip05`, `lud16`, `website`, `banner` | empty string |
| `nip05_verified` | `false` |
| `freshness` | `Unknown` until kind:0 arrives, then `Fresh` / `HoursOld` / `DaysOld` based on age |

### Subtleties

- **Bare pubkey path.** If no kind:0 exists in the store yet, `open()` returns a placeholder-filled payload and the reverse-index dependency triggers `on_event_inserted` when one arrives. Doctrine D1 prohibits returning `None` here.
- **NIP-05 verification is async** but does not block view emission. The placeholder payload ships `nip05_verified = false`; a background task verifies and emits a `ProjectionChange::Nip05Verified { pubkey, verified }` later, which the view handles via `on_projection_changed`.
- **No deletion handling.** kind:0 cannot be deleted in any meaningful sense (a kind:5 event would only remove the specific event from store; the projection rebuilds from whatever kind:0 is now latest, which could be the previous one or empty). Profile gracefully degrades to placeholder.

---

## 4. View: Timeline

The workhorse. Paginated list of events matching a filter, with author display pre-formatted, sorted by `created_at` descending.

### Spec

```rust
pub struct TimelineSpec {
    pub kinds: Vec<u16>,                  // e.g., [1, 6, 7] for notes/reposts/reactions
    pub authors: Option<Vec<PubKey>>,     // None = any
    pub hashtags: Option<Vec<String>>,
    pub since: Option<Timestamp>,
    pub until: Option<Timestamp>,
    pub limit: usize,                     // soft cap on items held in payload
    pub include_replies: bool,            // if false, filter out events with e-tag references
    pub include_reposts: bool,            // if false, filter out kind:6
}
```

### Payload

```rust
#[derive(Clone, uniffi::Record)]
pub struct TimelineView {
    pub items: Vec<TimelineItem>,
    pub cursor: Cursor,
    pub has_more_below: bool,
}

#[derive(Clone, uniffi::Record)]
pub struct TimelineItem { /* per product-spec.md §7.6 */ }

#[derive(Clone, uniffi::Enum)]
pub enum Cursor {
    Empty,
    AtHead,
    AtTimestamp { ts: Timestamp, event_id: String },
}
```

### Delta

```rust
#[derive(Clone, uniffi::Enum)]
pub enum TimelineDelta {
    Inserted { at: usize, items: Vec<TimelineItem> },
    Removed { ids: Vec<String> },
    Updated { id: String, item: TimelineItem },
    UpdatedMany { ids: Vec<String>, patch: AuthorPatch },  // shared-projection fan-out
    CursorAdvanced { cursor: Cursor, appended: Vec<TimelineItem> },
    Cleared,
}

#[derive(Clone, uniffi::Record)]
pub struct AuthorPatch {
    pub author_display: String,
    pub author_picture: String,
    pub author_nip05_domain: String,
}
```

### Dependencies

```rust
Dependencies {
    kinds: spec.kinds.clone(),
    authors: spec.authors.clone().unwrap_or_default(),
    kind_author_pairs: spec.authors
        .as_ref()
        .map(|a| spec.kinds.iter().flat_map(|k| a.iter().map(|p| (*k, *p))).collect())
        .unwrap_or_default(),
    catch_all_filter: if spec.authors.is_none() && spec.hashtags.is_some() {
        Some(Filter::from_hashtags(&spec.hashtags.as_ref().unwrap()))
    } else {
        None
    },
    ..Default::default()
}
```

### Recompute strategy

Incremental. The `State` keeps `items: Vec<TimelineItem>` sorted by `created_at` descending plus a secondary `by_event_id: HashMap<EventId, usize>` index. On insert:

1. Build the `TimelineItem` from the event + projections.
2. Binary-search for insertion position by `created_at` (with `id` as tiebreaker).
3. Insert; shift indices in `by_event_id`.
4. If `items.len() > spec.limit + slack`, drop the oldest items past the cap.
5. Emit `Inserted { at, items: vec![item] }`.

On replace (e.g., parameterized replaceable kind 30023 article):

1. Look up the old item by event id.
2. Build the new item.
3. If `created_at` unchanged, in-place update — emit `Updated`.
4. If `created_at` changed, remove + re-insert at new position — emit `Removed` then `Inserted`.

On projection change (e.g., author kind:0 arrived):

1. Look up all items by that pubkey via `by_author`.
2. Update each item's `author_display`, `author_picture`, `author_nip05_domain` from the new projection.
3. Emit `UpdatedMany { ids, patch }`.

### Pagination

`AppAction::AdvanceCursor { view_id }` triggers the actor to query the store for events older than the current cursor, append to `items`, emit `CursorAdvanced`. The planner concurrently issues a negentropy sync for the requested time window if not already covered (per spec §7.8). The action is non-blocking; if more events are available, they stream in via `Inserted` deltas after the action completes.

### Best-effort placeholders

| Field | Placeholder |
|---|---|
| `author_display` | shortened npub |
| `author_picture` | identicon URI |
| `author_nip05_domain` | empty string |
| `content_preview` | empty if event content is empty; otherwise truncated event content |
| `created_at_display` | pre-formatted ("3m ago", "yesterday", etc.) |
| `reaction_summary` | `ReactionSummary::default()` (all zeros) |
| `zap_sats_total` | 0 |
| `reply_count` | 0 |

### Subtleties

- **Hashtag filters need `catch_all_filter`.** Hashtag membership is per-event, not per-author. The view registers `catch_all_filter` and the reverse-index slow path applies. Document this cost in `nmp-guardrails`.
- **Repost handling.** Kind:6 events embed an `e`-tagged reference to the reposted event. The timeline item displays the reposted event's content with a "reposted by" header. If the reposted event isn't in the store, render the placeholder (npub + "loading post"); the dependency on the e-tag fires `on_event_inserted` when the original arrives.
- **Reply-only filter.** If `spec.include_replies = false`, filter out events with an `e` tag in the `mark = "reply"` or root position. This filter runs in `matches_spec()` before adding to `items`.
- **Author kind:0 not yet loaded.** Per doctrine D1, render the item with placeholders. The `by_author` reindex on `on_projection_changed` updates the item in place. **The post is rendered immediately.**
- **Item dropped past cap.** When `spec.limit + slack` is exceeded, the oldest items fall out. Drop their reverse-index entries and emit `Removed`. If the user scrolls down later, they'll be re-fetched.
- **Sorting stability.** Tiebreak by `id` lex for deterministic order. This matters for cross-platform consistency tests.

---

## 5. View: Thread

A tree structure of replies rooted at one event. The view exposes the tree flat (depth-first traversal) with per-node depth metadata, so platforms can render with indentation without doing tree manipulation themselves.

### Spec

```rust
pub struct ThreadSpec {
    pub root_event: EventCoord,           // id + author + relay hint
    pub max_depth: u8,                    // default 6
    pub include_orphans: bool,            // events tagging root but missing parent
}
```

### Payload

```rust
#[derive(Clone, uniffi::Record)]
pub struct ThreadView {
    pub root: ThreadNode,                 // the root event, fully built
    pub flat: Vec<ThreadNode>,            // depth-first; includes root at index 0
    pub orphans: Vec<ThreadNode>,         // events tagging root but whose parent is missing
}

#[derive(Clone, uniffi::Record)]
pub struct ThreadNode {
    pub item: TimelineItem,               // reuses TimelineItem for display fields
    pub depth: u8,
    pub parent_id: Option<String>,
    pub child_count: u32,
    pub flat_index: usize,
}
```

### Delta

```rust
#[derive(Clone, uniffi::Enum)]
pub enum ThreadDelta {
    NodeInserted { at: usize, node: ThreadNode },
    NodeUpdated { id: String, node: ThreadNode },
    NodeRemoved { id: String },
    RootUpdated { root: ThreadNode },
    OrphanPromoted { id: String, new_position: usize },  // parent arrived; orphan becomes a real node
    OrphanInserted { node: ThreadNode },
    Rebuilt { flat: Vec<ThreadNode>, orphans: Vec<ThreadNode> },  // escape hatch
}
```

### Dependencies

```rust
Dependencies {
    kinds: vec![1, 7],                            // notes + reactions
    e_tag_refs: vec![spec.root_event.id],         // primary
    ..Default::default()
}
```

We don't preemptively register every reply's `id` as an `e_tag_ref`; we expand the set lazily as replies arrive (each reply may have its own replies pointing at it).

### Recompute strategy

Incremental. The `State` keeps:

```rust
struct State {
    spec: ThreadSpec,
    nodes: HashMap<EventId, ThreadNode>,
    children: HashMap<EventId, Vec<EventId>>,
    orphans: HashMap<EventId, ThreadNode>,        // keyed by orphan's own id
    flat_cache: Vec<ThreadNode>,                  // rebuilt on structure change
}
```

On insert:

1. Determine parent: scan `e` tags for `mark = "reply"`, fall back to last `e` tag, fall back to root.
2. If parent is in `nodes`, add child to `nodes` + `children[parent]`. Rebuild `flat_cache` from root depth-first. Emit `NodeInserted`.
3. If parent is missing and `spec.include_orphans`, add to `orphans`. Emit `OrphanInserted`.
4. If the inserted event's id matches an orphan's claimed parent, promote that orphan into the tree. Emit `OrphanPromoted`.

On reaction (kind:7) targeting any node: don't add to tree; update `nodes[target].item.reaction_summary` and emit `NodeUpdated`. Reactions on the same target are batched via projection cache (`reaction_summary` projection in `Projections`).

### Pagination

Threads are usually fully loaded — they're bounded by depth and reply count. If `flat.len() > 500` (heuristic), refuse to expand further and surface as a "thread truncated" hint in the UI. Phase-9 measurement decides the actual limit.

### Best-effort placeholders

- Root event missing → render placeholder root with `content_preview = ""`, depth-0 spinner-free placeholder.
- Reply with missing author → reuse `TimelineItem` placeholders.
- Orphans render with `parent_id = Some(unknown_id)`; UI can flag them visually.

### Subtleties

- **Reply marker ambiguity.** NIP-10 reply marking is messy (`#e` tags with `marker = "reply" | "root" | "mention"`, plus legacy positional convention). Use NIP-10 markers when present; fall back to positional (last `#e` tag is parent, all `#e` tags include root). Get this wrong and threads display inverted.
- **Deletion of root.** If kind:5 removes the root, the entire view emits `Cleared`. The store removes; the view tears down.
- **Cycles.** Malicious clients can publish a "reply" tagging itself. Detect via parent traversal capped at `max_depth`; drop self-referential edges.
- **Orphan storm.** A popular event can attract thousands of replies arriving before their parents. `orphans` is bounded by the same `limit` heuristic.

---

## 6. View: Reactions

Aggregate counts and per-pubkey reaction list for a specific target event. Backed by the `reaction_summary` projection in the store.

### Spec

```rust
pub struct ReactionsSpec {
    pub target: EventCoord,
    pub include_pubkey_list: bool,        // if true, payload includes who reacted (capped at 100)
}
```

### Payload

```rust
#[derive(Clone, uniffi::Record)]
pub struct ReactionsView {
    pub target_id: String,
    pub total: u32,
    pub by_emoji: Vec<EmojiCount>,        // sorted desc by count
    pub my_reactions: Vec<String>,        // emoji this account reacted with
    pub reactors: Vec<ReactorEntry>,      // empty if !include_pubkey_list
}

#[derive(Clone, uniffi::Record)]
pub struct EmojiCount {
    pub emoji: String,
    pub count: u32,
}

#[derive(Clone, uniffi::Record)]
pub struct ReactorEntry {
    pub pubkey: String,
    pub author_display: String,
    pub emoji: String,
}
```

### Delta

```rust
#[derive(Clone, uniffi::Enum)]
pub enum ReactionsDelta {
    EmojiAdjusted { emoji: String, delta: i32 },
    MyReactionsChanged { reactions: Vec<String> },
    ReactorAdded { entry: ReactorEntry },
    ReactorRemoved { pubkey: String, emoji: String },
}
```

### Dependencies

```rust
Dependencies {
    kinds: vec![7],
    e_tag_refs: vec![spec.target.id],
    ..Default::default()
}
```

### Recompute strategy

Incremental, backed by the `Projections::reaction_summary` cache. The view reads from the cache on open and applies deltas as the cache changes.

- On a new kind:7 event referring to the target: increment count for the emoji; if `include_pubkey_list` and the reactor isn't already present, emit `ReactorAdded`.
- On a kind:5 delete of a kind:7 event: decrement count; emit `ReactorRemoved`.
- On the active account publishing a kind:7: include in `my_reactions`; emit `MyReactionsChanged`.

### Pagination

`reactors` is capped at 100 (most-recent). Beyond that, a separate `RunSync` can be triggered to backfill, but for UI purposes the cap is enough.

### Best-effort placeholders

- Empty reactions → empty `by_emoji`, `total = 0`, `my_reactions = []`. View is valid; UI renders "0 reactions" or hides.
- Reactor with no kind:0 → `author_display` is shortened npub.

### Subtleties

- **Emoji normalization.** kind:7 content can be `"+"`, `"-"`, a custom emoji name (`":heart_pulse:"`), or a raw emoji (`"❤️"`). Per NIP-25, `"+"` and missing content default to "like". Normalize on insert to canonical emoji form.
- **Reactions by deleted accounts.** If the reactor publishes kind:5 deleting their own reaction, decrement and remove. If a third party publishes kind:5 attempting to delete someone else's reaction, the store ignores the delete (per kind:5 spec, only self-deletes are honored).
- **Reaction spam.** Aggregate by `(pubkey, emoji)`: a single pubkey reacting with the same emoji 50 times counts as 1. The projection cache enforces this.

---

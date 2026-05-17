# Design: View Catalog

> **Audience:** Framework contributors building view kinds. Each view kind in this catalog is a Rust module in `crates/nmp-views/`.

> **Status:** Draft. Five view kinds detailed here as templates; remaining 10 stubbed for completion during Phase 4 of the build plan.

> **Prerequisites:** `product-spec.md` §7.6 (Views, with the `TimelineItem` example and best-effort field contract), `reactivity.md` (the per-view-kind contract in §4).

---

## 1. Per-view-kind template

Every view kind in `crates/nmp-views/` follows this shape:

```
crates/nmp-views/src/<kind>.rs
```

with these public items:

```rust
// 1. The Spec — the input the consumer supplies when opening the view.
pub struct <Kind>Spec { ... }

// 2. The Payload — the serializable shape that crosses FFI.
#[derive(Clone, uniffi::Record)]
pub struct <Kind>View { ... }

// 3. The Delta enum — incremental update variants.
#[derive(Clone, uniffi::Enum)]
pub enum <Kind>Delta { ... }

// 4. Internal State — actor-side; never crosses FFI.
pub(crate) struct State { ... }

// 5. Lifecycle functions.
pub fn open(spec: <Kind>Spec, store: &EventStore) -> (State, Dependencies, <Kind>View);
pub fn on_event_inserted(state: &mut State, event: &Event, store: &EventStore) -> Option<<Kind>Delta>;
pub fn on_event_removed(state: &mut State, id: &EventId) -> Option<<Kind>Delta>;
pub fn on_event_replaced(state: &mut State, old_id: &EventId, new_event: &Event, store: &EventStore) -> Option<<Kind>Delta>;
pub fn on_projection_changed(state: &mut State, change: &ProjectionChange) -> Option<<Kind>Delta>;
pub fn snapshot(state: &State) -> <Kind>View;
```

For each kind below, the catalog documents:

- **Spec:** what the consumer supplies.
- **Payload:** what crosses FFI.
- **Delta variants:** incremental updates.
- **Dependencies:** what the reverse index registers.
- **Recompute strategy:** incremental, full, or hybrid.
- **Pagination:** how scrolling extends the window (if applicable).
- **Best-effort placeholders:** the per-field default when underlying data is missing.
- **Subtleties:** edge cases that bit applesauce / NDK / TS-land clients and that this kind must get right.

---

## 1.1 Platform cache key (per ADR-0005)

The per-platform wrapper layer organizes the shadow as typed domain-keyed dictionaries, not as a flat `[ViewId: ViewPayload]` map. Each view kind below declares a **platform cache key** — either a single domain identifier (pubkey, event id) or a spec hash for view kinds with richer parameters.

| View kind | Platform cache key | Wrapper API (illustrative) |
|---|---|---|
| Profile | pubkey | `useProfile(pubkey)` / `@Profile` |
| Contacts | pubkey | `useContacts(pubkey)` |
| Mailboxes | pubkey | `useMailboxes(pubkey)` |
| Mutes | active account pubkey | `useMutes()` |
| Blossom servers | pubkey | `useBlossomServers(pubkey)` |
| Timeline | spec hash | `useTimeline(spec)` |
| Thread | root event id | `useThread(rootEventId)` |
| Replies | target event id | `useReplies(targetEventId)` |
| Reactions | target event coord | `useReactions(target)` |
| Conversation list | active account pubkey | `useConversationList()` |
| Conversation | peer pubkey or group id | `useConversation(peer)` |
| Zap history | active account pubkey | `useZapHistory()` |
| Wallet balance | wallet id | `useWallet()` |
| WoT rank | pubkey | `useWotRank(pubkey)` |
| Search | spec hash | `useSearch(query)` |

`ViewId` is an internal FFI token; component code never sees it. Wrappers refcount per key, dispatch `OpenView`/`CloseView` to Rust, and enforce a 30s eviction grace period matching Rust-side view warmth.

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

## 7. View: Conversation

A paginated message list for a single NIP-17 DM peer (1:1 or a single group). Messages are decrypted server-side in the actor; plaintext crosses FFI only as fields of `ConversationMessage`.

### Spec

```rust
pub struct ConversationSpec {
    pub peer: PeerRef,                    // single pubkey for 1:1, group id for group
    pub limit: usize,                     // soft cap on messages held in payload
}

pub enum PeerRef {
    Direct(PubKey),
    Group(GroupId),
}
```

### Payload

```rust
#[derive(Clone, uniffi::Record)]
pub struct ConversationView {
    pub peer_display: ProfileView,        // for direct; placeholder for group
    pub messages: Vec<ConversationMessage>,
    pub cursor: Cursor,
    pub typing_indicators: Vec<PubKey>,   // future: NIP-XX typing
    pub unread_count: u32,
    pub last_read_ms: u64,
}

#[derive(Clone, uniffi::Record)]
pub struct ConversationMessage {
    pub id: String,
    pub author_pubkey: String,
    pub author_display: String,           // pre-formatted
    pub body: String,                     // plaintext, never crosses unencrypted
    pub created_at_ms: u64,
    pub created_at_display: String,
    pub attachments: Vec<MediaRef>,
    pub reply_to: Option<String>,
    pub reactions: ReactionSummary,
    pub delivery: DeliveryState,          // Sent | Delivered | Read | Failed
}
```

### Delta

```rust
#[derive(Clone, uniffi::Enum)]
pub enum ConversationDelta {
    Appended { messages: Vec<ConversationMessage> },
    Prepended { messages: Vec<ConversationMessage> },  // pagination
    Updated { id: String, message: ConversationMessage },
    Removed { ids: Vec<String> },
    UnreadCountChanged { count: u32 },
    DeliveryUpdated { id: String, state: DeliveryState },
}
```

### Dependencies

```rust
Dependencies {
    kinds: vec![1059],                    // NIP-59 gift wraps
    p_tag_refs: vec![active_account.pubkey],  // wraps addressed to us
    ..Default::default()
}
```

Plus peer-side: when the active account sends a message, the store also receives the gift-wrapped copy addressed to the peer and stores it locally for the conversation history.

### Recompute strategy

Incremental. On gift-wrap arrival:

1. Decrypt via NIP-44 in the actor.
2. Unwrap the inner rumor; check `p` tags include our pubkey and identify the sender.
3. Build `ConversationMessage`; insert into `messages` sorted by `created_at_ms`.
4. Emit `Appended` (if at tail) or `Updated`/structural delta (if out-of-order, rare).

Sending: the action publishes the gift wrap and atomically inserts the message into the conversation with `delivery: Sent`. Receipt of the relay's `OK` updates to `Delivered`. NIP-25-style read receipts (if we support them) update to `Read`.

### Pagination

Older messages: `AppAction::AdvanceCursor` triggers a sync against the peer's inbox relays (or the active account's inbox relays for received messages) for kind:1059 events older than the current cursor. Decrypt + insert via `Prepended`.

### Best-effort placeholders

- Peer with no kind:0 → `peer_display` is placeholder profile.
- Decryption failure → don't add to view; record in `DebugDiagnostics`. Doctrine: never expose ciphertext as a message.
- Send-failure (no inbox relay reachable) → `delivery: Failed`, message stays in view with a retry affordance.

### Subtleties

- **Decryption is expensive but synchronous to the actor.** A long batch of incoming wraps could block other actor work. Offload decryption to the tokio runtime as a `CoreMsg::DecryptedRumor`, then handle insert synchronously.
- **Sender spoofing.** The inner rumor's `pubkey` is the claimed sender. Verify against the wrap's NIP-59 sealed signature.
- **Read receipts.** NIP-17 doesn't define read receipts directly. If we implement them, decide whether they're per-platform (encryption-extension) or out-of-scope for v1. Currently out-of-scope.
- **Background decryption.** When the app is backgrounded and a push notification arrives, the NSE crate (`nmp-nse`, spec §7.14) decrypts and emits a notification. On next foreground, the conversation view rebuilds from store, picking up what the NSE already inserted. Plaintext in conversation view fields is the same in both paths.

---

## 8. Cross-cutting concerns

### 8.1 The `Projections` cache, in detail

Projections are derived from events but cached for cross-view reuse. v1 catalog:

| Projection | Source events | Updated on |
|---|---|---|
| `author_display` | kind:0 | kind:0 insert; NIP-05 verification completion |
| `author_picture` | kind:0 | same |
| `author_nip05` | kind:0 + DNS verification | kind:0 insert; NIP-05 async result |
| `reaction_summary` | kind:7 referring to target | kind:7 insert; kind:5 delete |
| `zap_total` | kind:9735 referring to target | kind:9735 insert |
| `reply_count` | kind:1 with `e`-tag to target | kind:1 insert; kind:5 delete |
| `follow_status` | kind:3 of active account | kind:3 of active account |
| `mute_status` | kind:10000 of active account | kind:10000 of active account |
| `relay_list` | kind:10002 per pubkey | kind:10002 insert |

Each projection emits a `ProjectionChange::<Kind> { key, new_value }` when it changes. Views indexed by the relevant key (`by_author`, `by_e_tag`, etc.) receive `on_projection_changed`.

### 8.2 The `Cursor` enum

Used by paginated views (Timeline, Conversation, Search, Zap history). Variants:

```rust
pub enum Cursor {
    Empty,
    AtHead,
    AtTimestamp { ts: Timestamp, event_id: String },
}
```

Cursor advancement is an action; views handle it via `on_cursor_advance` (added to the per-kind contract for paginated kinds only).

### 8.3 `EventCoord`

```rust
#[derive(Clone, uniffi::Record)]
pub struct EventCoord {
    pub id: String,
    pub author: String,
    pub kind: u16,
    pub relay_hint: Option<String>,
    pub d_tag: Option<String>,            // for parameterized replaceable
}
```

Used wherever a view refers to a specific event, especially for replaceable/parameterized-replaceable references where the (author, kind, d_tag) tuple is the actual identity.

---

## 9. Stubs — view kinds to be filled in during Phase 4

For each stub: spec name, scope, deferred-to-Phase note. Full per-template detail follows the §3–§7 pattern during Phase 4 implementation.

### Contacts

Parsed kind:3 follow list for one pubkey. Payload: `ContactsView { pubkey, follows: Vec<ContactEntry>, raw_event_id }`. ContactEntry has pubkey + (resolved via projection) display name + relay hint. Phase 1.

### Mailboxes

Parsed kind:10002 for one pubkey. Payload: `MailboxesView { pubkey, inbox_relays: Vec<String>, outbox_relays: Vec<String>, raw_event_id }`. Phase 1.

### Mutes

Parsed kind:10000 for the active account. Payload: `MutesView { muted_pubkeys: Vec<PubKey>, muted_hashtags: Vec<String>, muted_events: Vec<EventId>, muted_words: Vec<String> }`. Affects projection of "should this event be filtered out of timeline?" Phase 2.

### Blossom servers

Parsed kind:10063 for one pubkey. Payload: `BlossomServersView { pubkey, servers: Vec<String> }`. Used by media upload action. Phase 6.

### Replies

Flat list of replies to one event (subset of Thread without tree structure). Payload: `RepliesView { target, replies: Vec<TimelineItem>, has_more }`. Cheaper than Thread when UI doesn't need the tree. Phase 1.

### Conversation list

List of DM conversations for the active account, with last-message preview + unread count. Payload: `ConversationListView { conversations: Vec<ConversationSummary>, total_unread: u32 }`. Phase 5.

### Zap history

Bidirectional list of zaps sent/received for the active account. Payload: `ZapHistoryView { sent: Vec<ZapEntry>, received: Vec<ZapEntry>, cursor }`. Phase 6.

### Wallet balance

Reactive balance + pending transactions for the active wallet. Payload: `WalletBalanceView { sats, pending: Vec<PendingTx>, last_synced_at_ms }`. Backed by the wallet subsystem rather than the event store directly. Phase 6.

### WoT rank

Per-pubkey trust score + reasoning. Payload: `WotRankView { pubkey, score, depth_paths: Vec<DepthPath> }`. Backed by the WoT subsystem. Phase 6.

### Search

Full-text or filter-based search over the local store. Payload: `SearchView { query, results: Vec<TimelineItem>, has_more }`. **Heavy `catch_all_filter` use** — every insert evaluates against the query. Phase 1 with explicit guardrail warning.

---

## 10. What this catalog rules out

- **Consumer-defined view kinds.** v1 owns the enum. Per spec §13 open question 6, this may relax in v2 via either enum extension (awkward) or string-keyed payloads (consumer-decoded). Decide post-v1.
- **Mutable view payloads.** View payloads are immutable snapshots. Deltas describe the difference between snapshots. No "patch this field in place from native" path exists.
- **View-on-view subscriptions.** Per `reactivity.md` §6.3.
- **Computed-from-native fields.** All formatting, all derivations live in Rust per doctrine D5.

---

## 11. Validation: scenarios that must run before view-catalog ships

Five scenarios that must pass in the `reactivity-bench` harness (per `reactivity.md` §10) before Phase 4 of the build plan exits:

1. **Profile fan-out under kind:0 arrival.** 50 timeline views, each over 1k authors, with 50% author overlap; a single kind:0 arrives for a shared author. Measure: how many views are woken (should be ≈25); per-view recompute time; total `ViewBatch` latency. Gate: p99 ≤ 5ms end-to-end.
2. **Hashtag firehose.** 1 timeline view with `catch_all_filter` on `#nostr`; 200 events/sec. Measure: per-event filter eval time; ViewBatch frequency; ViewBatch size. Gate: ≤ 60Hz, ≤ 1000 deltas/sec cumulative.
3. **Thread orphan storm.** Thread view; 1000 replies arrive in random order; 50% of parents arrive after their children. Measure: orphan promotion rate; final tree correctness; tree-build time. Gate: tree is identical to a known-good single-pass build; build time ≤ 50ms.
4. **Reactions aggregation.** 1 reactions view; 10k kind:7 events arrive in 30 seconds. Measure: `EmojiAdjusted` delta count vs ideal coalesced count. Gate: deltas/sec ≤ 60.
5. **Conversation paging.** 1 conversation view; 100 historical decryptions triggered by `Prepended`; 5 incoming decryptions interleaved. Measure: decrypt throughput; ordering correctness; UI thread stays unblocked. Gate: no actor-thread starvation observed (other view updates continue to land during decrypt batch).

Each scenario is a `tests/` integration test in `nmp-testing` that asserts on the metric. Failures block Phase 4 exit.

---

## 12. Next step: run the five scenarios above against the reactivity harness

The `reactivity-bench` harness from `reactivity.md` §10 covers the framework-wide reactivity assumptions. This view-catalog adds **five view-kind-specific scenarios** (§11 above) that must pass before declaring the catalog ready. They reuse the same harness infrastructure with view-specific scenario configs.

Order of operations, concretely:

1. Build the harness (`reactivity.md` §10) at start of Phase 1.
2. Lock in the reverse-index + projection design based on harness measurements.
3. Implement view kinds 1–9 marked "Phase 1" in §2 of this doc (Profile, Contacts, Mailboxes, Timeline, Thread, Replies, Reactions, Search; Conversation/list deferred to Phase 5).
4. Run the five scenarios in §11 against the Phase 1 views.
5. If gates pass, Phase 1 exits. If not, fix the offending view kind's `recompute` strategy and re-run.

The catalog is a contract; the harness is how we know the contract holds.

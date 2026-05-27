# Product Spec: Subsystems

[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)

## 7. Subsystem specifications

### 7.1 EventStore

Single instance per `FfiApp`, owned by the actor. Public to the framework (not to native).

Behaviors guaranteed at insert time:

| Concern | Behavior |
|---|---|
| Insert API | Every event enters through one actor-owned insert path returning `InsertOutcome`; no caller mutates indexes or storage directly. |
| Signature/delegation validity | Verified before any tombstone, provenance, replaceable-index, or durable-storage mutation. |
| Duplicate id | Merge relay provenance set; keep earliest `received_at`; do not overwrite. |
| Replaceable kinds (0, 3, 10000-19999) | Compare `(pubkey, kind)` against existing; keep newest `created_at`; tie-break by lexicographically smallest `id`. |
| Parameterized replaceable (30000-39999) | Compare `(pubkey, kind, d-tag)`; same supersession rule. |
| Kind 5 (delete) | After verification, scan referenced `e` and `a` tags and remove matching events authored by the deleter. Persisted as tombstone so later re-insertion is suppressed. Tombstone timestamp is the maximum delete timestamp observed for that target. |
| NIP-40 expiration | Schedule a tokio timer to remove the event at the expiration timestamp; on actor restart, scan and re-schedule. |
| NIP-26 delegation | Validate delegation tag at insert; reject malformed. |
| Ephemeral events | Delivered to live consumers but not durably stored. |
| Provenance | Every event records typed sidecar provenance: relay URL, first seen, last seen, source, and deterministic primary relay. |
| Query matching | Storage backends may return candidates; every result is re-run through the canonical matcher before it affects state or views. |

Storage backend is configurable via `AppConfig.storage_backend` (LMDB or SQLite-style native backend, IndexedDB/OPFS strategy for web, final choice resolved before v1). The store wraps the Rust Nostr SDK protocol types, but NMP owns the application-kernel storage traits because the app kernel needs typed provenance, action ledger rows, relay metadata, domain records, and bounded-view indexes in addition to raw events.

GC: a claim-based collector tracks `view_id → Vec<event_id>` references. View close drops claims. A periodic `prune()` removes events with zero claims that are also absent from declared "pinned" sets (sessions' contact-list events, sessions' relay-list events).

**Sync watermarks.** The store maintains a per-`(filter_signature, relay_url)` table:

```
watermarks {
  filter_sig: Hash,            // canonicalized filter
  relay_url: String,
  synced_up_to: u64,           // unix seconds; "we have everything matching this filter on this relay up to T"
  last_sync_method: SyncMethod, // Negentropy | ReqScan | Manual
  bytes_saved_vs_req: u64,     // cumulative, for diagnostics
  updated_at: u64,
}
```

Watermarks are durable. On startup they are loaded into the actor; they survive app restarts. The planner (§7.2) consults them before issuing any backfill, and the sync engine (§7.8) updates them after every reconciliation.

A cache-miss query against a fully-synced `(filter, relay)` pair is **authoritative**: the answer is "this event does not exist on that relay." A cache-miss against an unsynced pair triggers either a sync (if NIP-77 supported) or a fallback fetch.

Fallback loading is split by need:

- Pointer/address misses: cache-first lookup for event id or replaceable address, batched and deduped across waiting views, then relay hints, then configured fallback sources.
- Tag-value and timeline-window misses: bounded historical window loads that record what range is still unknown.
- Authoritative absence: only a complete coverage record/watermark can turn a miss into "not found." A non-empty cache result is not proof that a query is complete.

The default loader queries open relays and configured sources. Users can add custom sources (CDN cache, local mirror, etc.) through app-kernel extension points, but loaded events still enter through the same verified insert path.

### 7.2 Subscription planner

Owns the mapping from `ViewSpec` → `Vec<Filter>` → `Vec<RelayUrl>` → on-the-wire REQ.

Behaviors:

- **Live tail first.** Live subscriptions register their local handler and start REQ tailing immediately. Historical backfill runs beside it, not before it.
- **Coverage-aware backfill.** Before issuing historical traffic, the planner consults cache coverage/watermarks (§7.1). Complete coverage serves from cache; partial coverage schedules a gap fill; unknown coverage triggers bounded fetch/REQ or NIP-77 if supported. A non-empty cache result is never treated as complete by itself.
- **Logical vs. wire subscriptions.** A logical subscription belongs to a view/action/monitor. A wire subscription belongs to a relay. Many logical consumers may share one wire REQ, and each consumer still receives only events matching its canonical filter.
- **Coalescing.** Filters that are equal or safely subsumable into a single broader filter share one REQ per relay. The planner maintains a formal merge lattice for `limit`, `since`, `until`, multi-filter arrays, and tag operators.
- **Loader integration.** Pointer/address/tag/timeline misses go through the pointer loader registry with cache-first batching, dedupe, relay hints, cancellation on view close, and explicit missing-window state.
- **Auto-close.** REQs without consumers are CLOSE'd. One-shot filters (those with no live subscribers, only an `until` upper bound) are CLOSE'd on EOSE.
- **Buffering.** Inbound events are batched to ≤ 60Hz per view (configurable). Batches turn into one `ViewBatch` per tick.
- **Backpressure.** If platform-side rendering falls behind, the planner drops `ViewBatch` updates in favor of a single `FullState` catch-up. View payload semantics make this lossless.
- **Reconnect.** On relay reconnect, the planner restores live REQs and schedules a coverage-aware gap fill. View payloads do not reset.

### 7.3 Outbox routing

Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.

**Resolution algorithm.**

| Operation | Relay set |
|---|---|
| Subscription with `authors` filter | Union of each pubkey's write relays (kind-10002), deduplicated. Pubkeys without known mailboxes trigger an opportunistic kind-10002 fetch from indexer relays. |
| Subscription with `p` tag filter or notifications | Union of each tagged pubkey's inbox relays. |
| Subscription with neither | Active session's read relays. |
| Publish of any signed event | Author's write relays. |
| Publish of discovery events (kind:0, 3, 1xxxx) | Also fan out to user's configured indexer relays (in addition to write relays above). |
| Publish with `p` tags (DMs, mentions, reactions) | Author's write relays **plus** each tagged pubkey's inbox relays. |
| DM (NIP-17 gift-wrapped) | **Only** resolved recipient inbox relays. Never the author's write relays. Never the active session's "default" relays. Missing recipient inbox relays fail closed. |
| Discovery (kind-10002 fetch for unknown pubkeys) | Configurable indexer relay set (default: a curated list of high-coverage relays). |

**Why this prevents specific failure modes.**

- "Publish leaked to wrong relays" → ruled out by the safe API. The developer cannot supply a relay list to `SendNote`. Explicit overrides are named, one-shot, and debug-flagged in logs.
- "DM accidentally public" → ruled out by the safe API. The DM publish path consults only resolved inbox relays; there is no fallback-to-all-relays path for gift wraps.
- "Reads missing an author's actual relays" → bounded and surfaced. If the author's kind-10002 is reachable it is opportunistically fetched on first contact; if not, coverage and diagnostic state expose the miss risk and configured fallback policy.
- "Hand-rolled fan-out logic" → no API surface for it.

**Per-pubkey relay-list lifecycle.**

- First contact with an unknown pubkey → enqueue kind-10002 fetch from indexer relays.
- Fresher kind-10002 arrives → invalidate dependent subscriptions, recompute relay sets, re-issue REQs as needed.
- Kind-10002 missing for a pubkey after N seconds → fall back to indexer set for reads only; do not publish content events to indexers.

The gossip cache is the `nostr-gossip` crate; backend selection (in-memory vs SQLite) follows the storage backend choice. Watermarks (§7.1) intersect with outbox: a sync watermark is keyed by `(filter, relay)` and naturally tracks per-author per-relay coverage.

### 7.4 Sessions

`SessionState` holds:

```rust
pub struct SessionState {
    pub accounts: Vec<Account>,
    pub active: Option<String>,             // pubkey
    pub status: SessionStatus,              // Loading / Syncing / Online / Offline
    pub last_activity_ms: u64,
}

pub struct Account {
    pub pubkey: String,
    pub display: AccountDisplay,            // pre-formatted name + npub
    pub signer_kind: SignerKind,
    pub profile_view_id: ViewId,            // points into ViewSnapshots
    pub contacts_view_id: ViewId,
    pub mailboxes_view_id: ViewId,
    pub mutes_view_id: ViewId,
    pub status: AccountStatus,
}
```

Signers are managed entirely in `nmp-core`. The initial product signer catalog is:

- Local key (raw nsec, stored encrypted via `KeyringCapability`)
- NIP-49 (password-encrypted private key)
- NIP-46 bunker / Nostr Connect
- NIP-07 (web only)
- External — Android Amber (NIP-55) bridged via `ExternalSignerCapability`

The signer abstraction inside `nmp-core` is a Rust trait with `sign(unsigned_event) -> Future<signed_event>`. Adding a signer kind is an internal task; external developers do not implement signers.

### 7.5 Actions catalog

Actions live in `nmp-actions`. Each action is a Rust async fn taking an action context (`event_store`, `signer`, `publisher`, `active_account`) and producing zero or more signed events. The actor runs actions on its tokio runtime; results route through `InternalEvent` back to the actor for atomic state update.

Action authoring contract for the framework's own contributors (not exposed at FFI):

```rust
#[async_trait]
pub trait Action: Send + Sync + 'static {
    type Output: Send + 'static;
    async fn run(self, cx: &ActionCx) -> Result<Self::Output>;
}
```

Built-in actions (long-term product catalog): the AppAction variants listed in §6.3 each map to one Action implementation. v1 ships only the generic kernel actions named in [`docs/plan.md`](../plan.md). Custom actions are first-class via a sister crate pattern (apps add their own actions crate that depends on `nmp-actions`).

Atomicity invariant: an action's local event-store commit, side-effect intent, and ledger transition happen as actor messages with one parent action id. The action future runs on the tokio runtime, but all state mutation happens in `handle_message`. There is no public API that lets a developer publish, upload, sign, or issue an NWC request without a renderable action-ledger row.

The ledger is general, not relay-only. It can represent local optimistic commit, signer prompt, per-relay publish attempt, HTTP upload, NWC request, retry, repair, partial failure, timeout, and final status. Relay publishes additionally track attempted/acked/failed/timed-out by relay plus required success count.

### 7.6 Views

`nmp-views` defines `ViewSpec` and all built-in `ViewPayload` variants:

| View | Inputs | Payload |
|---|---|---|
| Profile | `pubkey` | latest kind-0 parsed; pre-formatted display name; verified domain |
| Contacts | `pubkey` | parsed kind-3 follow list, with per-followee metadata |
| Mailboxes | `pubkey` | parsed kind-10002 |
| Mutes | `pubkey` | parsed kind-10000 |
| Blossom servers | `pubkey` | parsed kind-10063 |
| Timeline | `filter` (kind, authors, hashtags, time window) | sorted slice with pagination cursor |
| Thread | `root_event_id` | tree with per-node metadata |
| Replies | `event_coord` | flat list with per-reply metadata |
| Reactions | `event_coord` | grouped count by emoji + per-pubkey list |
| Conversation list | `account_pubkey` | sorted DM threads with unread counts and latest message preview |
| Conversation | `peer_pubkey` | paginated decrypted messages |
| Zap history | `account_pubkey` | bidirectional list |
| Wallet balance | `wallet_id` | balance + pending transactions |
| WoT rank | `pubkey` | trust score + reasoning |
| Search | `query`, `kinds`, `time_window` | result list |

Each payload type carries **pre-formatted** display strings (timestamps in user locale, npub-shortened forms, sat amounts). No platform-side formatting — the kernel owns that decision.

**Best-effort field contract (per doctrine D1).** Every display-bearing field in every view payload is **non-optional** and has a defined placeholder when the underlying data is missing:

| Field | Placeholder when missing |
|---|---|
| Display name | Shortened npub: `npub1abc…xyz` |
| Picture URL | Deterministic identicon URI derived from pubkey |
| NIP-05 verified domain | empty string (UI conditionally renders a checkmark only when non-empty) |
| Timestamp string | "just now" |
| Reaction count | 0 |
| Zap total | 0 sats |
| Content body (if missing) | empty string (the item still renders; only the body region is blank) |

When the underlying data arrives — kind:0 for an author, kind-9735 zap receipts for a note, the actual decrypted body for a DM — the view payload updates in place, the platform's reactive primitive detects the change, and only the affected cell re-renders. No spinner is ever shown for already-rendered cells.

**Stale freshness is exposed, not gated.** Each enriched-from-cache field may optionally carry a sibling field `xxx_freshness: FreshnessHint` (recent, hours_old, days_old, never_verified). UI may choose to render a small badge. The framework never withholds the underlying value based on freshness.

**Concrete example: lean timeline payload.**

```rust
#[derive(Clone, uniffi::Record)]
pub struct TimelineView {
    pub cursor: Cursor,
    pub items: Vec<TimelineItem>,
    pub has_more: bool,
}

#[derive(Clone, uniffi::Record)]
pub struct TimelineItem {
    pub id: String,                   // event id hex
    pub author_pubkey: String,
    pub author_display: String,       // never empty; npub-shortened if no kind:0
    pub author_picture: String,       // never empty; identicon URI if no kind:0
    pub author_nip05_domain: String,  // empty if not verified
    pub content_preview: String,      // pre-truncated for list display
    pub created_at_display: String,   // pre-formatted, locale-aware
    pub reaction_summary: ReactionSummary,
    pub zap_sats_total: u64,
    pub reply_count: u32,
    pub repost_of: Option<EventCoord>,
    pub quote_of: Option<EventCoord>,
}
```

`TimelineItem` is a flat summary. The full event content, raw tags, signature, and provenance live in the event store inside Rust and do not cross FFI. This is D5 applied: snapshots are screen-shaped, not store-shaped. Chat list carries summaries; a detail view loads full content on demand.

View warmth: a view stays cached for 30 seconds after its last claim is dropped (configurable). Re-opening within the window costs zero relay traffic and zero re-sync.

Post-v1 content rendering contract: protocol-aware content parsing lives in Rust, not in platform shells. The content layer emits serializable nodes for text, links, NIP-19/NIP-21 entities, hashtags, media hints, mentions, quotes, and truncation boundaries. Platform shells render those nodes and may style them, but they do not parse Nostr content or decide URL/media safety policy.

### 7.7 Web of Trust

`nmp-wot` ships as an optional subsystem (gated by `AppConfig.wot_enabled`). On enable:

- Loads the active account's follow graph to a configurable depth (default 2).
- Computes per-pubkey trust scores (default algorithm: simple in-degree weighted by depth; pluggable via a trait).
- Exposes a global filter: when on, every view applies the score threshold before emitting; pubkeys below the threshold are tagged but rendered with a "low trust" UI hint (the renderer chooses; the payload exposes the score).

Computation is incremental; updates to follow lists update scores without recomputing from scratch.

### 7.8 Sync engine (live REQ plus NIP-77 backfill)

Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.

**Position in the stack.**

```
View opens → Live REQ handler starts → Planner consults coverage → Sync engine reconciles gaps → EventStore inserts → ViewBatch emits
                                ↓ (fallback)
                                bounded fetch / REQ scan
```

**Watermarks as a first-class type.** The engine reads and writes the `watermarks` table introduced in §7.1. A watermark answers two questions:

- Has this `(filter, relay)` pair ever been synced?
- If so, up to what timestamp?

Answers to those questions inform every backfill, every fallback-loader decision, and every "is this cache miss authoritative?" check.

**Three triggers, all built-in.**

1. **App foreground.** On `AppAction::Foreground`, the engine schedules an incremental sync for the active user's home filter (kind:1, kind:6, kind:7 matching followed authors) against their write relays. Runs in the tokio runtime; emits `SyncState` updates as it progresses; no UI blocking.
2. **View open.** When a view opens whose filter has a gap (per watermark/coverage), the engine reconciles the gap concurrently with the live REQ tail. Progress is visible in `SyncState`; the view payload streams in as events land.
3. **Relay reconnect.** On reconnect, the planner re-establishes live REQs and schedules a coverage-aware gap fill. The gap between disconnect and reconnect is filled by sync when possible, not by re-fetching from scratch.

**Manual sync as an action.** `AppAction::RunSync { spec }` lets apps trigger arbitrary reconciliations (e.g., "sync this user's last 30 days of articles"). Same engine, different trigger.

```rust
pub struct SyncSpec {
    pub filter: Filter,
    pub relay: String,
    pub time_window: Option<(u64, u64)>,
    pub direction: SyncDirection,           // Pull, Push, Bidirectional
    pub on_completion: SyncCompletionAction,
}
```

**Per-relay capability negotiation.** Not every relay implements NIP-77. The engine maintains per-relay capability metadata, probed lazily on first contact. Unsupported relays cause the planner to fall back to bounded fetch/REQ scanning for that relay only — other relays in the same fan-out may still use sync.

**Instrumentation.** Every reconciliation reports bytes-on-the-wire vs. equivalent-REQ-bytes (estimated). The aggregate is exposed in `DebugDiagnostics.sync_savings` and rendered in the proof app's performance overlay.

**SyncState in AppState.** Visible to UI:

```rust
pub struct SyncState {
    pub active: Vec<SyncJob>,     // currently-running reconciliations
    pub last_completed: Option<SyncJobReport>,
    pub watermarks_summary: WatermarksSummary,  // coverage stats per relay
}
```

UI rendering is optional — most apps will not show sync activity directly — but the data is there for proof-mode dashboards and for power-user surfaces.

### 7.9 Wallet

`nmp-wallet` is staged. NIP-47 Wallet Connect lands first because it is a bounded client protocol over the action ledger and secret capabilities. Cashu/NIP-60 and nutzaps/NIP-61 come later because they require a durable wallet state machine, mint metadata cache, proof/token indexes, redemption state, and repair flows.

The long-term module unifies four payment surfaces:

| Surface | NIP | Required state | User-visible |
|---|---|---|---|
| NWC | 47 | `nwc_connect_uri` | `WalletState.balance`, transaction list |
| Lightning zap | 57 | LUD-16 address discovery | `Zap` action → status → receipt |
| Cashu | 60 | Mint URLs, proofs | `WalletState.cashu` |
| Nutzap | 61 | Inherits Cashu | Pending nutzap queue |

`WalletState` is a `uniffi::Record` field of `AppState`. Wallet attachment is an action; payment is an action; receipt verification is automatic. No wallet UI hook mutates proof, token, nutzap, or transaction state directly; those records live in the actor-owned domain store and transition through the action ledger.

### 7.10 Messaging

`nmp-messages` implements NIP-17 over NIP-44 + NIP-59 after the kernel release. Initial product support targets:

- 1:1 DMs
- Group DMs (via multiple recipient gift-wraps)
- Read receipts are local conversation state initially; protocol-level NIP-25 reactions on rumors are a later policy decision.

Conversations are derived views; the conversation list and conversation views above (§7.6) are the user-visible surface. Decryption happens inside the actor (or inside the NSE crate, §7.14, when triggered from background). Plaintext never crosses FFI other than as fields of `ConversationView`.

The module is intentionally stricter than common fallback patterns: if recipient inbox relays are unknown, a DM send action fails closed with diagnostic state instead of publishing gift wraps to default/public relays. Read state is local conversation state unless and until an explicit protocol-level receipt policy is added.

### 7.11 Blossom

`nmp-blossom` exposes BUD-01/BUD-02. Uploads and downloads are actions over the shared capability bridge: SHA-256 hashing, HTTP upload/download, and auth-event signing are capabilities, while retry/repair/progress are action-ledger state. Progress flows through `MediaState`. Server selection follows the active account's kind-10063 blossom-servers list; first server wins, with fallback to the next on failure.

### 7.12 Guardrails

`nmp-guardrails` is enabled only with `cfg(debug_assertions)`. In debug builds, every event going into the store, every filter being constructed, every action being dispatched passes through a checker. v1 checks:

- bech32 entity (`npub`, `note`, `nevent`, `naddr`) passed where hex pubkey/event id is expected
- `limit` on a replaceable-event filter (always wrong; replaceable events should be fetched by `(kind, pubkey)`, not by limit)
- Subscription opened with no relays resolvable
- Missing required tags on event being built (NIP-defined)
- Filter with `authors: []` (always matches nothing; almost always a bug)
- Action dispatched while no account is active when one is required
- Cache miss with no fallback loader registered

Violations produce a structured `DebugDiagnostics` entry in `AppState.debug` plus an `eprintln!` with documentation URL. The release-build cost is zero.

### 7.13 Testing surface

`nmp-testing` ships:

- `MockRelay` (re-exported from `nostr-relay-builder`).
- `EventFactory::new(seed)` for deterministic event/key generation.
- `SimulatedClock` injected at `AppConfig.clock`.
- `NetworkChaos` for injecting drops/latency at the relay-pool layer.
- `snapshot_state(app)` returning a normalized JSON `AppState` for diffing.
- `script(actions)` for replaying action sequences against a headless `FfiApp` and asserting on emitted updates.

The core actor is testable without networking. Every action variant has a corresponding unit test. Cross-platform consistency tests (§3.5) run the same `script` on all four targets and diff the JSON.

### 7.14 Background notification decryption

`nmp-nse` is a minimal sibling crate with one purpose: decrypt an inbound encrypted event without spawning the full actor. It exposes:

```rust
#[uniffi::export]
pub fn decrypt_push(
    encrypted_event_json: String,
    keyring: Arc<dyn KeyringCapability>,
    storage_path: String,
) -> Option<DecryptedPush>;

#[derive(uniffi::Record)]
pub struct DecryptedPush {
    pub sender_pubkey: String,
    pub sender_display: String,
    pub body_preview: String,             // pre-formatted, length-capped
    pub conversation_id: String,
    pub kind: u32,
}
```

iOS Notification Service Extension and Android background workers link only this crate. Memory and time budgets (iOS NSE ~24MB / 30s) are observed by design: no relay connections, no full event store load, only the minimal state needed to decrypt and format a preview.

This resolves `aim.md` §7.5: the smaller surface is a sibling crate that shares persistence with the full app.

### 7.15 Offline action queue

Decision for `aim.md` §7.6: the queue lives in the actor with durable persistence via the storage backend.

Mechanism:

- Every action that produces a publishable event is staged as a record `(action_id, scheduled_at, payload)` in a `pending_publishes` table/store on insert.
- On successful relay-side acknowledgement (OK message), the record is deleted.
- On reconnect, all pending records are re-tried in `scheduled_at` order.
- Records older than a TTL (default 7 days) emit a `Toast` and are removed.
- `created_at` on the event is fixed at the time of original dispatch, not at the time of eventual publish — preserving causal order.

The queue is visible via `OutboxState.pending` and `OutboxState.failed`; users can clear failed entries via a diagnostic action.

### 7.16 Performance instrumentation (`nmp-metrics`)

A framework subsystem, not an afterthought. The proof app (§4.6, §12) is the primary consumer; production apps can also surface the same dashboard behind a debug flag.

**Always-on counters** (release builds), zero or near-zero overhead:

- FFI calls per second (dispatch / reconcile).
- FFI payload size histogram (bytes per `AppUpdate`).
- Snapshot frequency: `FullState` vs `ViewBatch` per second.
- Active view count.
- Per-view payload byte budget vs actual.
- Sync watermarks coverage (per relay: % of opened filters fully synced).
- Sync bytes-saved vs equivalent-REQ-bytes, cumulative.
- Cache hit rate (event store reads served without relay traffic).
- Cache candidate count vs canonical matched result count.
- Relay provenance rows and primary-relay selection counts.
- Relay status/capability counters and reconnect count.
- Action ledger counts by status, including per-relay publish status.
- Domain-store row counts by namespace.
- Active monitor count and monitor progress summaries.
- Actor message queue depth (high water mark + current).
- Outstanding subscriptions per relay.

**Debug-build instrumentation**, higher cost:

- `AppState` clone duration p50/p99.
- View recompute duration per view per emit.
- Tokio runtime stats (active tasks, blocking calls).
- Memory footprint of the actor's working set.
- Per-platform marshaling time (recorded by the reconciler).

Exposed via `AppState.debug` in debug builds; accessible via the `EmitDiagnosticSnapshot` action in release builds (writes a diagnostic JSON export to a path returned via `Effect::DiagnosticReady`). The runtime update transport remains FlatBuffers; this JSON file is an inspection artifact. The proof app renders this live as an in-app overlay.

**Budgets** (initial targets; revised after Phase 9 measurement on real devices):

| Metric | Budget |
|---|---|
| `FullState` payload | ≤ 64 KB |
| `ViewBatch` payload | ≤ 32 KB |
| Per-`AppUpdate` marshaling (Rust→native) p99 | ≤ 4 ms |
| `ViewBatch` frequency under hashtag firehose | ≤ 60 Hz |
| Actor queue depth, steady-state | < 16 |
| Memory footprint (timeline of 1k authors, 10k events cached) | ≤ 200 MB |
| Sync bytes-saved on 10k-event backfill | ≥ 95% vs REQ |
| Cold-start to first painted timeline frame | ≤ 1.5 s on mid-range mobile |

Exceeding any budget in the proof app is treated as a framework defect, tracked as a bug.

### 7.17 Future module integration contract

Post-v1 modules must be thin policy layers over the v1 kernel. A module fails review if it implements its own relay pool, persistence engine, signer lifecycle, subscription scheduler, action runner, or platform cache. It may define policy, view payloads, domain-store records, and actions.

| Future module | v1 substrate it must consume |
|---|---|
| Sessions | Logical account scope, monitor lifecycle, domain store, signer capabilities, action ledger |
| Sync | Canonical filter matcher, relay capabilities, cache coverage, monitor progress, domain store |
| Relay policy / outbox | Relay-target resolver, relay metadata store, provenance, canonical filters, action ledger |
| Messaging | Conversation domain store, signer/decrypt capabilities, relay resolver, action ledger |
| Wallet / nutzaps | Action ledger, secret capabilities, domain store, HTTP/NWC capabilities, monitor lifecycle |
| Blossom | Hash/HTTP/sign capabilities, action ledger, media domain records |
| WoT | Canonical filters, monitor lifecycle, graph domain records, local rank/filter views |
| Product views/content | Event store, domain store, view registry, serializable parsed nodes |

Diagnostics must expose monitor status, relay capabilities, sync progress, action ledger rows, domain-store sizes, and per-account active capabilities so module behavior is inspectable without platform-specific debugging.

---

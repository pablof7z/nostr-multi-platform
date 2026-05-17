# Product Specification — Nostr Multi-Platform Framework

> **Working name:** `nmp` (Nostr Multi-Platform). Final name TBD per `aim.md` §7.7. Crate names below use the `nmp-*` prefix; substitute when renamed.

> **Status:** Draft 0. This document is the contract for what the framework provides as a product — its public surfaces, its guarantees, its developer experience, its acceptance criteria. It sits between `aim.md` (the north star) and the eventual design + implementation work. It is decisive where it can be and explicit about open decisions where it cannot.

> **Required prior reading:** `docs/aim.md`, then `rmp-architecture-bible.md` upstream at `rust-multiplatform/rmp`.

---

## 1. Product summary

A Cargo workspace shipping a single Rust core, FFI bindings for Swift/Kotlin/TypeScript, a wasm target, a scaffolding CLI, and reference platform shells for iOS, Android, desktop, and web. It composes the `rust-nostr` crate family plus an OS keyring crate, a NIP-46 connect crate, a NIP-47 NWC crate, a Blossom crate, and a relay-builder into an opinionated application framework. The framework owns: protocol state, caching, relay routing (NIP-65 outbox), subscription lifecycle, signing orchestration, derived views, sessions, wallets, NIP-17 messaging, NIP-77 sync, web-of-trust, and developer guardrails. Platform code renders state and dispatches user intents — nothing else.

The framework treats common Nostr-correctness failures (stale replaceable events, lost subscriptions, mis-routed publishes, double-publication, multi-account desync, leaked secrets across FFI, naive cache invalidation) as **product defects in the framework** rather than as developer mistakes. The public API is designed so that the wrong thing is hard to type.

---

## 2. Audience and use cases

**Primary audience.** Application developers building Nostr clients for production distribution on iOS, Android, desktop, and web — including LLM-driven and inexperienced developers who lack the protocol literacy to navigate Nostr's footguns unaided.

**Secondary audience.** Existing Nostr client teams considering a port to Rust + multi-platform, who want a substrate they can compose rather than reimplement.

**Tertiary audience.** Tooling, agent, and bot authors who want the framework's event store + actions + sync as a headless Rust library, without UI.

**In scope.**

- General-purpose social clients (timeline, threads, profiles, follows, reactions, reposts, quotes).
- DM-first messengers (NIP-17 over NIP-44 + NIP-59).
- Long-form publishing tools (NIP-23).
- Wallets and zap-centric apps (NIP-47 / NIP-57 / NIP-60 / NIP-61).
- Media-heavy clients (Blossom BUD-01/02).
- List managers and curation tools.

**Out of scope for v1.**

- Relay implementations (we depend on `relay-builder` for tests; we do not ship a production relay).
- New NIP authorship.
- Game engines, AR, low-latency audio/video pipelines (the bible's Pika has these because it has voice/video calls; we do not adopt that scope).
- Non-Nostr protocol support (Bluesky, ActivityPub, etc.).

---

## 3. Success criteria

Acceptance is **demonstrable, not aspirational**. A claim that the framework works is provable by running these:

### 3.1 Zero-to-running starter

```bash
nmp init my-app
cd my-app && just run-ios   # works
just run-android            # works
just run-desktop            # works
just run-web                # works
```

Result on each platform: a starter app with login (private key + NIP-46 bunker), a "following" timeline, compose, profile view, profile edit, and a DM inbox + thread. End-to-end build + first launch ≤ 5 minutes on a developer laptop with the framework's `nix develop` shell active, ≤ 15 minutes from cold without Nix.

### 3.2 The "few hundred lines" test

Across the four platform shells of the starter app, total non-generated platform code must fit within these budgets (excluding asset declarations and boilerplate `main`):

| Platform | Budget (LOC, hand-written) |
|----------|----------------------------|
| iOS (SwiftUI) | ≤ 400 |
| Android (Compose) | ≤ 400 |
| Desktop (iced) | ≤ 600 (iced is more verbose; this is the bible's pattern) |
| Web (wasm + TS/JSX shell) | ≤ 400 |

Exceeding any budget is a framework-design failure: it means rendering logic is being forced to compensate for missing surface in the core.

### 3.3 Bug class extinction

Each of these classes must be **structurally impossible** to introduce via the framework's public API. Each is paired with a regression test in `crates/nmp-testing`.

1. Stale replaceable event (kind 0/3/10000-19999/30000-39999) retained in state after a newer one arrives.
2. Subscription leaked after its UI is destroyed.
3. Publish of an event to relays the author has not declared as write relays, without explicit override.
4. DM published to public relays.
5. Two account contexts having overlapping mutable state.
6. Cache miss returning empty without triggering a fallback fetch.
7. Profile-edit action that updates the local cache but fails to publish (or vice versa).
8. Two concurrent UI subscriptions for the same filter producing two relay REQs.
9. NIP-46 signing session lost on app suspend/resume without prompt.
10. Re-published event missing its original `id` due to re-signing.

Each test asserts the framework refuses the broken usage (compile-time, type-system, or runtime panic in debug) or routes around it transparently.

### 3.4 LLM-friendliness

A novice or LLM-driven developer, given only `docs/aim.md`, `docs/product-spec.md`, the generated bindings, and the starter app, can implement a new screen (e.g., "show all kind-1 events tagged with a given hashtag") that:

- compiles on first try with no edits to the core,
- correctly routes to the right relays automatically,
- correctly closes its subscriptions when navigated away from,
- correctly handles cache misses and live updates.

We treat this as a property of the spec: if it fails repeatedly with capable LLMs, the API surface is wrong, not the LLM.

### 3.5 Cross-platform consistency

A scripted action sequence (defined in `crates/nmp-testing`) run against the starter app on all four platforms produces byte-identical `AppState` JSON snapshots after each action. Divergence is a framework defect, not a platform issue.

---

## 4. Deliverables

### 4.1 Workspace

The on-disk layout from `aim.md` §5 is canonical. Concretely, v1 ships the following crates as published artifacts on crates.io:

| Crate | Role | FFI? |
|---|---|---|
| `nmp-core` | Actor, `AppState`, `AppAction`, `AppUpdate`, event store, planner, sessions, outbox routing | Pure Rust |
| `nmp-ffi` | UniFFI scaffolding, `FfiApp`, `AppReconciler`, capability traits | UniFFI |
| `nmp-wasm` | wasm-bindgen wrapper | wasm-bindgen |
| `nmp-actions` | Built-in actions catalog | Pure Rust |
| `nmp-views` | Derived view types and view-handle protocol | Pure Rust |
| `nmp-wot` | Web-of-trust graph + filter | Pure Rust |
| `nmp-sync` | NIP-77 negentropy sync | Pure Rust |
| `nmp-wallet` | NIP-47/57/60/61 unified wallet | Pure Rust |
| `nmp-messages` | NIP-17 conversation layer | Pure Rust |
| `nmp-blossom` | Blossom client wrapper | Pure Rust |
| `nmp-guardrails` | Debug-build runtime checks | Pure Rust |
| `nmp-testing` | Mock relay, factories, simulated time | Pure Rust |
| `nmp-nse` | Decrypt-only crate for iOS NSE + Android push (see §7.14) | UniFFI, minimal |
| `nmp-cli` | Scaffolding tool | Binary |

The CLI is also published to npm as `@nmp/cli` for non-Rust developers, wrapping the same binary via npx.

### 4.2 Bindings

Generated bindings are **checked into git** under `bindings/{swift,kotlin,typescript}/`. Developers consuming the workspace as a path dependency do not need a host build to regenerate. The CI lane regenerates and diffs on every PR touching FFI-exposed types; binding drift fails the build.

### 4.3 Starter app

The CLI scaffolds a complete starter project. Behavior is detailed in §8.

### 4.4 Examples

`examples/chat-{ios,android,desktop,web}` track the starter app but include richer features (groups via NIP-29, zaps end-to-end, Blossom uploads, NIP-46 bunker pairing) and serve as the canonical "what does production-grade integration look like" reference for each platform.

### 4.5 Documentation set

| Document | Purpose | Owner |
|---|---|---|
| `docs/aim.md` | North star | Stable |
| `docs/product-spec.md` | This doc | Stable, versioned |
| `docs/design/*.md` | Per-subsystem design docs (filled in by the next session) | Iterates |
| `docs/recipes/*.md` | How to build common features (timeline, thread, zap, DM, group) | Iterates |
| `docs/nips.md` | NIP support matrix with version pins | Iterates |
| `docs/migration.md` | Upgrade guidance per minor/major | Iterates |

The bible itself stays upstream at `rust-multiplatform/rmp`; we link, not vendor.

---

## 5. Developer experience

### 5.1 The path from nothing to a running app

```
$ npx @nmp/cli init relay-cat
? Organization (reverse-DNS): com.example
? Platforms: ◉ iOS  ◉ Android  ◉ Desktop  ◉ Web
? Storage backend (default for non-web): ◉ LMDB  ○ SQLite  ○ nostrdb  ○ In-memory
? Web storage backend: ◉ IndexedDB (default)  ○ OPFS
? Default relays (comma-separated): wss://relay.damus.io,wss://nos.lol
? Wallet: ◉ NWC  ○ Cashu  ○ None
? Signers to include: ◉ Local key  ◉ NIP-46 bunker  ◉ NIP-07 (web)  ◉ Amber (Android)
? Use Nix flake: Yes
✓ Scaffolded relay-cat in ./relay-cat
```

Then:

```
$ cd relay-cat && nix develop
$ just run-desktop      # native window opens to login screen in ~30s
$ just run-ios          # simulator boots, app launches
$ just run-android      # emulator boots, app launches
$ just run-web          # vite dev server on :5173
```

The starter app on first launch presents login. Logging in with a private key or pairing a bunker yields a working following-timeline with live updates, compose, profile, and DMs.

### 5.2 The platform-developer's day

After scaffold, the developer's loop is:

1. Touch SwiftUI/Compose/iced/TSX files in the platform shell.
2. Touch action variants in `app/src/actions.rs` and action handlers in `app/src/core/` for new features.
3. Touch view definitions in `app/src/views/` to add new derived views.
4. `just gen-bindings` after changing FFI-visible types.
5. Re-run.

The developer should not be writing relay code, subscription bookkeeping, cache invalidation, or replaceable-event handling. Ever. If they catch themselves doing so, that is the symptom of either a missing built-in or a framework bug.

### 5.3 What the developer never has to do

Concrete list, exhaustive:

- Pick relays per subscription or publish (outbox handles it).
- Implement REQ/CLOSE bookkeeping.
- De-duplicate events across relays.
- Track replaceable-event supersession.
- Wire a kind-5 delete event into their UI state.
- Wire NIP-40 expiration into their UI state.
- Persist signed events anywhere other than via actions.
- Encrypt/decrypt DMs.
- Wrap/unwrap NIP-59 gift wraps.
- Schedule background relay reconnection.
- Cache profile metadata.
- Maintain a follow-graph cache.
- Implement zap receipt verification.
- Implement NWC request/response correlation.
- Implement Blossom upload chunking.
- Hop to main thread on platform — the framework's reconciler emits hints; the platform shim handles it.

---

## 6. The framework API surface

This section specifies what the developer sees. Implementation lives behind it.

### 6.1 The App handle

`FfiApp` (Swift/Kotlin) / `NmpApp` (TS) is the single object created at startup. Per RMP bible, it is a `uniffi::Object` constructed once per process.

```rust
#[derive(uniffi::Object)]
pub struct FfiApp { /* opaque */ }

#[uniffi::export]
impl FfiApp {
    /// Construct the app. Spawns the actor thread. Loads persisted sessions.
    /// `config` carries data directory, default relays, storage backend choice,
    /// feature flags. Infallible at the FFI boundary; catastrophic failure panics.
    #[uniffi::constructor]
    pub fn new(config: AppConfig) -> Arc<Self>;

    /// Snapshot of current state. Cheap clone.
    pub fn state(&self) -> AppState;

    /// Fire-and-forget action dispatch. Never blocks, never returns a Result.
    /// Results land as state changes.
    pub fn dispatch(&self, action: AppAction);

    /// Start the update listener. Must be called exactly once per process.
    /// The reconciler is invoked from a background thread; native must hop.
    pub fn listen_for_updates(&self, reconciler: Arc<dyn AppReconciler>);

    /// Register platform capabilities. Each setter is idempotent and safe to
    /// call multiple times. See §6.5.
    pub fn set_keyring(&self, keyring: Arc<dyn KeyringCapability>);
    pub fn set_push(&self, push: Arc<dyn PushCapability>);
    pub fn set_external_signer(&self, signer: Arc<dyn ExternalSignerCapability>);
    pub fn set_network_monitor(&self, mon: Arc<dyn NetworkMonitorCapability>);
    pub fn set_blob_picker(&self, picker: Arc<dyn BlobPickerCapability>);
}
```

`AppConfig` is a `uniffi::Record` containing only platform-resolved primitives (paths, lists of relay URLs, feature-flag booleans). No `Arc<dyn ...>` types in the config — capabilities are registered separately via setters so each can be bridged on its own schedule.

### 6.2 AppState

`AppState` is a `uniffi::Record`. It is the entire UI's source of truth. It is cloned across FFI on every `FullState` update.

Top-level shape (v1, illustrative; final shape resolved in the design phase):

```rust
#[derive(Clone, uniffi::Record)]
pub struct AppState {
    pub rev: u64,
    pub router: Router,
    pub session: SessionState,
    pub store_summary: StoreSummary,        // counts, last sync, prune stats
    pub views: ViewSnapshots,               // snapshot of all open view payloads
    pub conversations: ConversationsState,
    pub wallet: WalletState,
    pub media: MediaState,
    pub wot: WotState,
    pub sync: SyncState,
    pub outbox: OutboxState,
    pub busy: BusyFlags,
    pub toast: Option<Toast>,
    pub debug: Option<DebugDiagnostics>,     // Some(_) only in debug builds
}
```

`AppState` does **not** include the entire event store contents. Events are reached through `ViewSnapshots` which carry only the events relevant to currently-open views. This is the v1 resolution of `aim.md` §7.1: full state snapshots, but the "state" is a projection of open views — bounded by what the UI is showing.

### 6.3 AppAction

`AppAction` is a flat `uniffi::Enum` of every user intent and lifecycle event. v1 catalog (illustrative; the actions crate may add domain-specific variants):

```rust
#[derive(Clone, uniffi::Enum)]
pub enum AppAction {
    // Lifecycle
    Bootstrap,
    Foreground,
    Background,
    NetworkChanged { online: bool },

    // Sessions
    AddAccountPrivateKey { nsec_or_ncryptsec: String, passphrase: Option<String> },
    AddAccountBunker { connect_uri: String },
    AddAccountExternal { kind: ExternalSignerKind },
    ActivateAccount { pubkey: String },
    RemoveAccount { pubkey: String, wipe: bool },

    // Routing
    Navigate { screen: Screen },
    Pop,
    PopToRoot,

    // Views
    OpenView { id: ViewId, spec: ViewSpec },
    CloseView { id: ViewId },
    RefreshView { id: ViewId },

    // Writes (delegated to nmp-actions)
    SendNote { content: String, mentions: Vec<String>, reply_to: Option<EventCoord> },
    React { target: EventCoord, emoji: String },
    Repost { target: EventCoord },
    Quote { target: EventCoord, comment: String },
    FollowUser { pubkey: String },
    UnfollowUser { pubkey: String },
    MuteUser { pubkey: String },
    UpdateProfile { patch: ProfilePatch },
    PublishLongForm { article: ArticleDraft },
    SendDm { recipient: String, body: String, attachments: Vec<BlobRef> },
    OpenConversation { peer: String },
    MarkConversationRead { peer: String, up_to: u64 },

    // Wallet
    AttachWallet { config: WalletConfig },
    DetachWallet,
    Zap { target: ZapTarget, sats: u64, comment: String },
    AcceptNutzap { id: String },

    // Media
    UploadBlob { source: BlobSource, server: Option<String> },
    CancelUpload { id: String },

    // Sync
    RunSync { spec: SyncSpec },

    // Outbox
    OverrideRelaysForNext { relays: Vec<String> },

    // Diagnostics
    ClearToast,
    EmitDiagnosticSnapshot,
}
```

Doctrine:

- Variants describe **user intent**, not desired state mutations.
- Each variant is constructible without side effects from native code.
- No variant carries an `Arc<dyn Trait>` or callback — capabilities are bridged separately.
- A `tag()` method (`pub fn tag(&self) -> &'static str`) returns a log-safe label that never reveals secrets (mnemonics, nsec, plaintext DMs).

### 6.4 AppUpdate

`AppUpdate` is the outbound stream. Bible doctrine: snapshots by default; granular variants only where profiling warrants. v1 starts with:

```rust
#[derive(Clone, uniffi::Enum)]
pub enum AppUpdate {
    FullState(AppState),                    // primary path; carries rev internally
    ViewBatch { rev: u64, views: Vec<ViewDelta> },   // optimization for hot views
    SideEffect { rev: u64, effect: Effect },        // see below
}

#[derive(Clone, uniffi::Enum)]
pub enum Effect {
    ToastShown { kind: ToastKind, body: String },
    BunkerPairingReady { qr: String, uri: String, expires_at: u64 },
    NipAuthChallenge { relay: String, challenge: String },
    DiagnosticReady { path: String },
}
```

Decisions captured here for `aim.md` §7.1:

- **Default is `FullState`.** First-class.
- **`ViewBatch` exists from day one** because view churn dominates Nostr UI updates and full-state churn would burn CPU on serialization. The planner emits at ≤ 60Hz aggregated.
- **`SideEffect` is reserved for ephemeral, non-state data** (pairing URIs that should not persist in `AppState`, NIP-42 auth challenges, generated diagnostic blobs).
- All update variants carry a monotonic `rev` and platforms enforce the stale guard.

### 6.5 Capabilities

Each capability is a Rust trait with `#[uniffi::export(callback_interface)]`. Native implements it; Rust calls it. Bible-pure: native reports raw data, Rust decides policy. v1 capabilities:

```rust
#[uniffi::export(callback_interface)]
pub trait KeyringCapability: Send + Sync + 'static {
    fn store(&self, account_id: String, blob: Vec<u8>);
    fn load(&self, account_id: String) -> Option<Vec<u8>>;
    fn delete(&self, account_id: String);
    fn list(&self) -> Vec<String>;
}

#[uniffi::export(callback_interface)]
pub trait PushCapability: Send + Sync + 'static {
    fn register(&self);                    // Rust asked native to register
    fn unregister(&self);
}

#[uniffi::export(callback_interface)]
pub trait ExternalSignerCapability: Send + Sync + 'static {
    fn sign(&self, request: SignRequest);  // native returns via reverse callback
    fn cancel(&self, request_id: String);
}

#[uniffi::export(callback_interface)]
pub trait NetworkMonitorCapability: Send + Sync + 'static {
    fn start(&self);
    fn stop(&self);
}

#[uniffi::export(callback_interface)]
pub trait BlobPickerCapability: Send + Sync + 'static {
    fn pick(&self, request: PickRequest);  // native opens picker, returns via callback
}
```

Each capability is **idempotent** (`start` after `start` is a no-op) and **bounded** (the trait surface is minimal; no native code decides policy). Capabilities can be added in additional minor versions; doing so does not break existing apps because all setters are optional.

### 6.6 Subscriptions / views

Decision captured here for `aim.md` §7.2 and §7.3:

**Views are opened via `dispatch(OpenView)` with a platform-generated `ViewId`, and updates arrive as `ViewBatch` entries keyed by that id.** Materialization is lazy in `nmp-core` — view payloads live in the actor and are projected into `ViewSnapshots`/`ViewBatch` on every change.

Rationale vs. opaque `ViewHandle` reference types:

- TEA-pure (every state change goes through one action / one update channel).
- Trivially serializable for wasm (no shared handles to manage across wasm boundary).
- Subscription lifecycle is unified with action lifecycle — no separate cancellation surface.
- Per-platform reactive wrappers (`@Observable` slices on iOS, `Flow<ViewPayload>` on Android, signals on web, iced sub-subscriptions on desktop) can be generated from the `ViewBatch` stream by the platform shim — generated by the CLI, not hand-written.

`ViewSpec` is an enum of supported view kinds; v1 covers profile, contacts, mailboxes, mutes, blossom-servers, timeline, thread, replies, reactions, conversation-list, conversation, zap-history, wallet-balance, wot-rank, search. Each maps to a typed payload variant.

Optimization escape hatch: a future `ViewHandle` opaque type can be added as an opt-in for very high-rate views (e.g., NIP-77 sync progress) where round-tripping through `AppUpdate` is wasteful. v1 does not ship this.

---

## 7. Subsystem specifications

### 7.1 EventStore

Single instance per `FfiApp`, owned by the actor. Public to the framework (not to native).

Behaviors guaranteed at insert time:

| Concern | Behavior |
|---|---|
| Duplicate id | Merge relay provenance set; keep earliest `received_at`; do not overwrite. |
| Replaceable kinds (0, 3, 10000-19999) | Compare `(pubkey, kind)` against existing; keep newest `created_at`; tie-break by lexicographically smallest `id`. |
| Parameterized replaceable (30000-39999) | Compare `(pubkey, kind, d-tag)`; same supersession rule. |
| Kind 5 (delete) | On insert, scan referenced `e` and `a` tags and remove matching events authored by the deleter. Persisted as tombstone so later re-insertion is suppressed. |
| NIP-40 expiration | Schedule a tokio timer to remove the event at the expiration timestamp; on actor restart, scan and re-schedule. |
| NIP-26 delegation | Validate delegation tag at insert; reject malformed. |
| Signature validity | Verified by the protocol crate before insert. |
| Provenance | Every event records the set of relays it has been seen on; the relays-of-origin set is read-only after insert (further appends only). |

Storage backend is configurable via `AppConfig.storage_backend` (LMDB default for native, IndexedDB default for web). The store is a thin trait wrapper over the `rust-nostr` `nostr-database` crate's `NostrDatabase` trait; backend selection at construction time only.

GC: a claim-based collector tracks `view_id → Vec<event_id>` references. View close drops claims. A periodic `prune()` removes events with zero claims that are also absent from declared "pinned" sets (sessions' contact-list events, sessions' relay-list events).

Fallback loader: a `FallbackLoader` trait the actor calls on cache miss. Default implementation queries open relays; users can override via `AppConfig` to add custom sources (CDN cache, local mirror, etc.).

### 7.2 Subscription planner

Owns the mapping from `ViewSpec` → `Vec<Filter>` → `Vec<RelayUrl>` → on-the-wire REQ.

Behaviors:

- **Coalescing.** Filters that are equal or subsumable into a single broader filter share one REQ per relay. The planner maintains a filter-graph and recomputes on view open/close.
- **Auto-close.** REQs without consumers are CLOSE'd. One-shot filters (those with no live subscribers, only an `until` upper bound) are CLOSE'd on EOSE.
- **Buffering.** Inbound events are batched to ≤ 60Hz per view (configurable). Batches turn into one `ViewBatch` per tick.
- **Backpressure.** If platform-side rendering falls behind, the planner drops `ViewBatch` updates in favor of a single `FullState` catch-up. View payload semantics make this lossless.
- **Reconnect.** On relay reconnect, all active REQs are re-sent transparently. View payloads do not reset.

### 7.3 Outbox routing

Default behavior for every action and subscription: route by NIP-65.

Resolution algorithm:

- **Subscription with `authors` filter.** Resolve each pubkey's write relays via the gossip cache; fetch the kind-10002 if unknown; union the resulting relay sets; deduplicate.
- **Subscription with no `authors` filter.** Use the active session's read relays.
- **Publish.** Send to the author's write relays. If the event is a DM or has `p` tags representing notification recipients, also send to those recipients' inbox relays.
- **Override.** `OverrideRelaysForNext { relays }` action sets a one-shot override consumed by the next publish action. Used for testing, bunker pairing flows, and developer escape hatches.

The gossip cache is the `nostr-gossip` crate; backend selection (in-memory vs SQLite) follows the storage backend choice.

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

Signers are managed entirely in `nmp-core`. The set of signer kinds is fixed at v1:

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

Built-in actions (v1): the AppAction variants listed in §6.3 each map to one Action implementation. Custom actions are first-class via a sister crate pattern (apps add their own actions crate that depends on `nmp-actions`).

Atomicity invariant: an action's published events and the corresponding `EventStore.insert` happen as a single actor message. The action future runs on the tokio runtime, but the commit happens in `handle_message`. There is no public API that lets a developer publish without also inserting.

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

Each payload type carries **pre-formatted** display strings (timestamps in user locale, npub-shortened forms, sat amounts). Per bible doctrine: no platform-side formatting.

View warmth: a view stays cached for 30 seconds after its last claim is dropped (configurable). Re-opening within the window costs zero relay traffic.

### 7.7 Web of Trust

`nmp-wot` ships as an optional subsystem (gated by `AppConfig.wot_enabled`). On enable:

- Loads the active account's follow graph to a configurable depth (default 2).
- Computes per-pubkey trust scores (default algorithm: simple in-degree weighted by depth; pluggable via a trait).
- Exposes a global filter: when on, every view applies the score threshold before emitting; pubkeys below the threshold are tagged but rendered with a "low trust" UI hint (the renderer chooses; the payload exposes the score).

Computation is incremental; updates to follow lists update scores without recomputing from scratch.

### 7.8 NIP-77 Negentropy sync

`nmp-sync` exposes a high-level API:

```rust
pub struct SyncSpec {
    pub filter: Filter,
    pub relay: String,
    pub time_window: Option<(u64, u64)>,
    pub direction: SyncDirection,           // Pull, Push, Bidirectional
    pub on_completion: SyncCompletionAction,
}
```

`RunSync { spec }` action triggers; progress flows through `SyncState`; completion materializes as either an `Effect::SyncComplete` or a follow-up action depending on `on_completion`. Background sync is a scheduled action driven by app foregrounding and configurable cadence.

### 7.9 Wallet

`nmp-wallet` unifies four payment surfaces:

| Surface | NIP | Required state | User-visible |
|---|---|---|---|
| NWC | 47 | `nwc_connect_uri` | `WalletState.balance`, transaction list |
| Lightning zap | 57 | LUD-16 address discovery | `Zap` action → status → receipt |
| Cashu | 60 | Mint URLs, proofs | `WalletState.cashu` |
| Nutzap | 61 | Inherits Cashu | Pending nutzap queue |

`WalletState` is a `uniffi::Record` field of `AppState`. Wallet attachment is an action; payment is an action; receipt verification is automatic.

### 7.10 Messaging

`nmp-messages` implements NIP-17 over NIP-44 + NIP-59. v1 supports:

- 1:1 DMs
- Group DMs (via multiple recipient gift-wraps)
- Read receipts (NIP-25 reactions on rumors are not supported in v1; we will revisit)

Conversations are derived views; the conversation list and conversation views above (§7.6) are the user-visible surface. Decryption happens inside the actor (or inside the NSE crate, §7.14, when triggered from background). Plaintext never crosses FFI other than as fields of `ConversationView`.

### 7.11 Blossom

`nmp-blossom` exposes BUD-01/BUD-02. Uploads and downloads are actions. Progress flows through `MediaState`. Server selection follows the active account's kind-10063 blossom-servers list; first server wins, with fallback to the next on failure.

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

---

## 8. CLI

### 8.1 Commands

```
nmp init [<path>]                  Scaffold a new project (interactive prompts).
nmp add ios | android | desktop | web   Add a platform to an existing project.
nmp gen bindings [swift|kotlin|typescript]  Regenerate bindings.
nmp gen view <name>                Scaffold a new view kind in the project's views crate.
nmp gen action <name>              Scaffold a new action variant + handler.
nmp gen screen <name>              Scaffold a screen across all platforms.
nmp doctor                         Diagnose toolchain / build environment.
nmp upgrade                        Bump nmp-* dependency versions and run migrations.
```

### 8.2 What `nmp init` generates

The scaffold matches the bible's `rmp init` pattern, adapted for Nostr:

```
<project>/
├── Cargo.toml                     # workspace
├── nmp.toml                       # project config (name, org, bundle ids, platforms)
├── rust/
│   ├── Cargo.toml                 # cdylib + staticlib + rlib
│   ├── uniffi.toml
│   └── src/
│       ├── lib.rs                 # FfiApp wiring + uniffi::setup_scaffolding!()
│       ├── config.rs              # AppConfig defaults
│       ├── state.rs               # AppState aggregation
│       ├── actions.rs             # project-specific action variants
│       ├── views.rs               # project-specific view variants
│       └── core/                  # actor extensions (sub-modules per domain)
├── ios/                           # SwiftUI shell, XcodeGen project
├── android/                       # Compose shell, Gradle project
├── desktop/                       # iced shell
├── web/                           # vite + wasm-bindgen shell
├── bindings/{swift,kotlin,typescript}/   # generated, checked in
├── examples/                      # empty placeholder
├── justfile                       # full build orchestration
├── flake.nix                      # if --nix
└── .github/workflows/             # ci.yml, release.yml
```

The starter app's feature set is fixed:

- Login (private key + bunker)
- Following timeline
- Compose (note, reply, repost, react)
- Profile view + edit
- Conversation list + conversation
- Settings (relay config, account switcher, debug diagnostics)

The starter is the canonical demonstration of the framework. It is also the foundation of the cross-platform consistency test (§3.5).

### 8.3 `nmp doctor`

Checks: Rust toolchain version, cross-compilation targets installed, Xcode (macOS only), Android SDK + NDK, JDK 17, `cargo-ndk`, `xcodegen`, `just`, `node`, `wasm-pack`. Reports per-platform readiness and prints exact install commands for what is missing.

---

## 9. Build & toolchain

The bible's pipeline is adopted verbatim:

- `cdylib + staticlib + rlib` from `nmp-core`.
- iOS: cross-compile, build `Nmp.xcframework`, link as non-embedded static framework via XcodeGen.
- Android: `cargo-ndk` produces per-ABI `.so` into `jniLibs/`; JNA loads at runtime.
- Desktop: direct path dependency; no FFI.
- Web: `wasm-pack build` with `nmp-wasm`; output consumed by a vite shell.
- `just` is the entry point for every build, gen, and run.
- A Nix flake provisions the entire toolchain.

CI lanes (GitHub Actions):

- `pre-merge` — `cargo check --workspace`, `cargo test --workspace`, `just gen bindings` + diff check, all four platform builds.
- `nightly` — examples build, cross-platform consistency tests, binary size budgets, NIP support matrix regeneration.

---

## 10. Resolutions for `aim.md` §7 open questions

| Question | Resolution in this spec | Section |
|---|---|---|
| §7.1 State granularity | `FullState` default + `ViewBatch` from day one; `SideEffect` for ephemeral non-state data; all carry `rev`. | §6.4 |
| §7.2 Where views live | Materialized lazily in `nmp-core`; surfaced as snapshots in `AppState.views` and as `ViewBatch` deltas. Opt-in opaque handles deferred. | §6.6 |
| §7.3 Cross-FFI subscription protocol | Single update stream (`AppReconciler.reconcile`) carrying `AppUpdate`. Platform shims (generated by CLI) adapt to `@Observable` / `Flow` / signals / iced subscriptions. | §6.1, §6.6 |
| §7.4 NIP-46 bunker as capability | Internal to `nmp-core`; not exposed as a `CallbackInterface`. Pairing flow surfaces as `Effect::BunkerPairingReady` for native rendering of QR/URI. | §6.4, §7.4 |
| §7.5 Background decryption | `nmp-nse` sibling crate with `decrypt_push()`; no full actor. | §7.14 |
| §7.6 Offline action queue | In-actor with durable storage in the chosen backend; visible via `OutboxState.pending`. | §7.15 |
| §7.7 Naming | Unresolved. Working name `nmp`. Final-name decision precedes any crates.io / npm publishing. | — |

---

## 11. Non-goals (explicit)

- Replicating every NIP. v1 supports a defined subset (see `docs/nips.md` once authored); others can be added per release.
- Pluggable wire protocols (no Bluesky/ActivityPub adapter in v1).
- Server-side: no relay implementation beyond test doubles.
- Bring-your-own actor: the actor pattern is fixed and not configurable.
- Native business logic escape hatches: the framework does not provide hooks for "I want to do my own thing in Swift" — that is the bug the framework exists to prevent.
- Theming systems: visual identity is the consumer's job. The starter app ships a minimal neutral theme.
- Localization: pre-formatted strings in views default to English in v1; localization is a follow-up that lives entirely in Rust.
- Push notification routing: we depend on the consumer's APNs/FCM setup; we decrypt + format only.

---

## 12. Phasing

Phase plans live in `docs/design/`; this spec stipulates only the ordering and gating between phases. Implementation does not begin before §7 of `aim.md` is fully resolved in design docs.

| Phase | Scope | Gating exit criterion |
|---|---|---|
| 0. Foundations | Workspace scaffold, `nmp-core` skeleton, actor + AppState/Action/Update shells, `nmp-ffi`, generated bindings round-trip, headless integration test runs | A `FullState` snapshot crosses Swift, Kotlin, TS without panic. |
| 1. Event store + views | EventStore with all insert invariants, planner, outbox, profile/timeline/contacts views | Demo: subscribe to a timeline, see live events, see replaceable updates supersede correctly. |
| 2. Sessions + writes | Multi-account, signers (local + bunker first), actions catalog covering write paths | Demo: log in, post a note, post a reply, react, follow/unfollow. |
| 3. Messaging | NIP-17 conversation layer, NSE crate | Demo: end-to-end DM between two simulated users in the test harness. |
| 4. Wallet | NWC + zaps + Cashu/nutzaps | Demo: pay a zap; receive a zap receipt; see balance update. |
| 5. Sync + WoT | NIP-77 sync action, WoT subsystem | Demo: backfill a fresh device from a single relay; WoT filter visibly reorders timeline. |
| 6. Web | `nmp-wasm`, web starter shell, OPFS/IndexedDB backend | Cross-platform consistency tests pass on web. |
| 7. CLI + starter | `nmp init` produces the full four-platform starter; `nmp doctor`; `nmp gen *` | The §3 success criteria are reproducible by a developer following only the published docs. |

Each phase ends with a tagged minor release on crates.io once §7.7 of `aim.md` is resolved and naming is final.

---

## 13. Open questions remaining

These are not resolved in this spec and require further design before implementation:

1. **Web persistence semantics.** OPFS vs IndexedDB tradeoffs; how do we expose the picker; what happens on Safari without OPFS.
2. **Wallet UI contract for NIP-60.** Cashu mint trust / proof state visibility in `WalletState`.
3. **Signer policy for sub-actions.** When an action composes multiple sub-actions each requiring signing (e.g., publishing a note and a relay-list update in one user step), how is the bunker prompted — one prompt or N?
4. **Migration story between minor versions.** AppState shape evolution: schema-versioned snapshots, or schemaless / additive only?
5. **Telemetry.** Do we ship optional, off-by-default telemetry into the framework for debugging? (Default proposal: no; consumer adds it.)
6. **Pluggable view kinds.** Are project-specific `ViewSpec` variants first-class (enum extension is awkward in Rust), or are they added by string-keyed payloads with consumer-side decoding?
7. **Final name.** `aim.md` §7.7 remains open.

These questions are tractable in the design phase and do not block the rest of this spec.

---

## 14. Glossary

- **Action.** A `uniffi::Enum` variant + an async fn in `nmp-actions` that implements it. Fire-and-forget at the FFI boundary.
- **Actor.** A dedicated OS thread that owns `AppState` and processes messages from a `flume` channel. Bible-canonical.
- **AppState.** A `uniffi::Record` carrying everything the UI needs to render. Cloned across FFI.
- **AppUpdate.** The outbound message stream from actor to platform. Variants: `FullState`, `ViewBatch`, `SideEffect`.
- **Bible.** `rmp-architecture-bible.md` upstream at `rust-multiplatform/rmp`. The architectural standard.
- **Capability.** A native-implemented Rust trait (`#[uniffi::export(callback_interface)]`) that lets Rust ask the OS to do something. Bounded; policy-free.
- **EventStore.** The reactive single source of truth for all Nostr events. Owned by the actor; not exposed at FFI.
- **Outbox routing.** NIP-65-based automatic relay selection for both reads and writes.
- **Reconciler.** `AppReconciler` callback trait. Receives `AppUpdate`s on a background thread; the platform shim hops to the UI thread.
- **View.** A pre-built derived projection of `EventStore` contents. Opened by `OpenView` action; payload arrives via `AppState.views` / `ViewBatch`.
- **Capability bridge.** Synonym for capability; the RMP bible's term.
- **ViewSpec / ViewPayload.** The opened-view descriptor and the materialized data.

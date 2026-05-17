# Product Specification — Nostr Multi-Platform Framework

> **Working name:** `nmp` (Nostr Multi-Platform). Final name TBD per `aim.md` §7.7. Crate names below use the `nmp-*` prefix; substitute when renamed.

> **Status:** Draft 0. This document is the contract for what the framework provides as a product — its public surfaces, its guarantees, its developer experience, its acceptance criteria. It sits between `aim.md` (the north star) and the eventual design + implementation work. It is decisive where it can be and explicit about open decisions where it cannot.

> **Required prior reading:** `docs/aim.md`, then `rmp-architecture-bible.md` upstream at `rust-multiplatform/rmp`.

---

## 1. Product summary

A Cargo workspace shipping a single Rust core, FFI bindings for Swift/Kotlin/TypeScript, a wasm target, a scaffolding CLI, and reference platform shells for iOS, Android, desktop, and web. It composes the `rust-nostr` crate family plus an OS keyring crate, a NIP-46 connect crate, a NIP-47 NWC crate, a Blossom crate, and a relay-builder into an opinionated application framework. The framework owns: protocol state, caching, relay routing (NIP-65 outbox), subscription lifecycle, signing orchestration, derived views, sessions, wallets, NIP-17 messaging, NIP-77 negentropy sync, web-of-trust, and developer guardrails. Platform code renders state and dispatches user intents — nothing else.

The framework treats common Nostr-correctness failures (stale replaceable events, lost subscriptions, mis-routed publishes, double-publication, multi-account desync, leaked secrets across FFI, naive cache invalidation, withheld cached data, blocking-on-fetch UI patterns) as **product defects in the framework** rather than as developer mistakes. The public API is designed so that the wrong thing is hard to type.

---

## 1.5 Cardinal doctrines

Five named principles that subsume the rest of this spec. Every API decision answers to at least one of these; conflicts between them resolve in the order listed.

### D1. Best-effort rendering — render now, refine in place

Apps built with this framework **never withhold cached data and never block on fetches**. Every view payload field carries a value, not a "loading" status. Missing display names default to a shortened npub; missing pictures default to a deterministic identicon URI; missing timestamps default to "now". When a more authoritative value (e.g., the author's kind:0) arrives later, the view payload updates in place and the affected cell re-renders. The UI never sees a spinner gating already-renderable content.

The doctrine is enforced by the view payload **types**: display fields are non-`Option`, placeholders are part of the type contract, and freshness is exposed (when relevant) as an optional badge hint, not a render gate. There is no `if has_profile { render } else { spinner }` pattern available in the API — the framework does not provide one.

This rules out, by construction, the most common Nostr-client failure modes:

- Hiding a post because the author's profile hasn't loaded yet.
- Replacing cached profile metadata with a spinner because "we might have something newer."
- Refusing to render threads because the root event isn't in cache.
- Profile-picture flicker between cached and placeholder.

### D2. Negentropy first, REQ second

NIP-77 negentropy reconciliation is the framework's **default backfill mechanism**. Every (filter, relay) pair the app touches is treated as a tracked sync target with a watermark. When a subscription needs historical data, the planner consults the watermark and prefers sync over REQ scanning. REQ falls back only when the relay does not support NIP-77.

This is not a feature you opt into. It is the engine. See §7.8.

### D3. Outbox routing is automatic; manual relay selection is the opt-out

Per NIP-65, every read and write is automatically routed to the relevant relays without the developer specifying them. Subscriptions with `authors` filters route to those authors' write relays; publishes go to the author's write relays plus tagged recipients' inbox relays; discovery falls back to a configurable indexer set.

The developer **never picks relays per operation**. If they catch themselves doing so, either they want the explicit override path (`OverrideRelaysForNext`, used only for testing and bunker pairing), or the framework has a bug.

This rules out, by construction:

- Posts to relays the author hasn't declared as write relays.
- DMs leaked to public relays.
- Reads against a default relay set that misses an author's actual relays.
- Hand-rolled fan-out logic in app code.

### D4. Single writer per fact; caches derive

The "single source of truth" doctrine does not mean one cache — there are five layers (durable event store, in-memory working set, view payloads, gossip cache, platform reactive shadow). It means **one writer per fact**, and every downstream cache derives from the writer mechanically. Cache invalidation is not a concept in the public API. Recomputation happens in the actor; the platform receives new derived state.

### D5. Snapshots bounded by what's open

What crosses FFI is the projection through currently-open views, not the underlying event store. `AppState` carries small screen-shaped data plus a map of `ViewId → ViewPayload` for views currently in use. Closing a view evicts its payload from the snapshot. The event store itself never crosses FFI. See §6.2 and the FFI architecture appendix (§A1).

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
| `nmp-metrics` | Performance instrumentation (counters, budgets, exposed via `AppState.debug`) | Pure Rust |
| `nmp-testing` | Mock relay, factories, simulated time, perf-replay harness | Pure Rust |
| `nmp-nse` | Decrypt-only crate for iOS NSE + Android push (see §7.14) | UniFFI, minimal |
| `nmp-cli` | Scaffolding tool | Binary |

The CLI is also published to npm as `@nmp/cli` for non-Rust developers, wrapping the same binary via npx.

### 4.2 Bindings

Generated bindings are **checked into git** under `bindings/{swift,kotlin,typescript}/`. Developers consuming the workspace as a path dependency do not need a host build to regenerate. The CI lane regenerates and diffs on every PR touching FFI-exposed types; binding drift fails the build.

### 4.3 Starter app

The CLI scaffolds a complete starter project. Behavior is detailed in §8.

### 4.4 Examples

`examples/chat-{ios,android,desktop,web}` track the starter app but include richer features (groups via NIP-29, zaps end-to-end, Blossom uploads, NIP-46 bunker pairing) and serve as the canonical "what does production-grade integration look like" reference for each platform.

### 4.5 The proof app (`nmp-proof`)

A kitchen-sink stress-test app, built using the framework, on all four platforms. It is **not** the starter app — the starter stays minimal so newcomers can read it. The proof app exists to validate the framework at scale and to gate v1 release.

Feature set:

- Multi-account login (3 signer kinds), 5 simultaneous accounts visible in a switcher.
- Following timeline subscribed to a user with 1,000+ follows.
- Hashtag firehose subscribed to a high-throughput tag (e.g., `#nostr`).
- Thread view rendering a controversial event with hundreds of replies + reactions + zaps.
- Search over the local store.
- DM inbox with 50+ active conversations (NIP-17 gift-wrapped).
- Long-form reader (NIP-23).
- Wallet operations: NWC + Cashu + zaps in both directions + nutzap claim.
- Blossom upload + view.
- Background sync via NIP-77 negentropy on foreground.
- Web-of-trust toggle visibly reordering the timeline.
- Offline queue: airplane mode → compose → reconnect → publishes land.

The proof app also ships a **performance overlay** (toggleable, debug-build default-on) rendering the live counters and budgets from §7.16. The overlay is implemented entirely in platform code reading from `AppState.debug` — no Rust-side UI logic.

The proof app is the substrate for cross-platform consistency tests (§3.5): the same scripted action sequence runs against the proof app on all four platforms and `AppState` JSON snapshots must match.

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

**Platform shadow is reorganized by domain key, not `ViewId` (ADR-0005).** While the FFI delivers `AppState.views` as a `HashMap<ViewId, ViewPayload>`, the per-platform wrapper layer (generated by `nmp gen`) reorganizes the shadow into typed domain-keyed dictionaries — `profiles: [PubKey: ProfileView]`, `reactionSummaries: [EventId: ReactionSummary]`, `conversations: [PubKey: ConversationView]`, etc. — so components read by domain concept (pubkey, event id) rather than by framework handle. `ViewId` remains an internal token used by the FFI; component code never sees it. Refcounted wrappers (`useProfile`, `@Profile`, `rememberProfile`) manage subscription lifecycle behind the domain-keyed API. See ADR-0005 for the per-view-kind cache-key table.

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

The component-facing API on each platform is *not* `ViewId`-based. Per ADR-0005, generated wrappers (`useProfile(pubkey)`, `@Profile`, `rememberProfile(pubkey)`, etc.) expose a refcounted, domain-keyed surface that translates component mount/unmount into `OpenView`/`CloseView` dispatches and writes incoming payloads into typed domain-keyed dictionaries on the platform side. App developers think in domain concepts; the framework handles subscription lifecycle and refcounted sharing behind the wrapper.

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

Fallback loader: a `FallbackLoader` trait the actor calls on cache miss for events not covered by sync watermarks. Default implementation queries open relays; users can override via `AppConfig` to add custom sources (CDN cache, local mirror, etc.).

### 7.2 Subscription planner

Owns the mapping from `ViewSpec` → `Vec<Filter>` → `Vec<RelayUrl>` → on-the-wire REQ.

Behaviors:

- **Sync-first backfill.** Before issuing a REQ for historical data, the planner consults sync watermarks (§7.1). If the `(filter, relay)` pair is fully synced past the requested window, no wire traffic; serve from cache. If partial, run an incremental negentropy reconciliation against the gap. If never synced and the relay supports NIP-77, run an initial reconciliation. Only when NIP-77 is unsupported does the planner fall back to filter-based REQ scanning.
- **Live tail via REQ.** Negentropy is for historical/backfill data. Live subscriptions (no `until` upper bound) use REQ with `since = now()` against the same relay set. Sync handles "what happened before"; REQ handles "what happens next."
- **Coalescing.** Filters that are equal or subsumable into a single broader filter share one REQ per relay. The planner maintains a filter-graph and recomputes on view open/close.
- **Auto-close.** REQs without consumers are CLOSE'd. One-shot filters (those with no live subscribers, only an `until` upper bound) are CLOSE'd on EOSE.
- **Buffering.** Inbound events are batched to ≤ 60Hz per view (configurable). Batches turn into one `ViewBatch` per tick.
- **Backpressure.** If platform-side rendering falls behind, the planner drops `ViewBatch` updates in favor of a single `FullState` catch-up. View payload semantics make this lossless.
- **Reconnect.** On relay reconnect, the planner first runs an incremental negentropy top-up from the watermark, then re-establishes live REQs. View payloads do not reset; the gap between disconnect and reconnect is filled by sync.

### 7.3 Outbox routing

Per doctrine D3, NIP-65 routing is the default for every read and write. The developer does not specify relays per operation. This subsystem is the implementation.

**Resolution algorithm.**

| Operation | Relay set |
|---|---|
| Subscription with `authors` filter | Union of each pubkey's write relays (kind-10002), deduplicated. Pubkeys without known mailboxes trigger an opportunistic kind-10002 fetch from indexer relays. |
| Subscription with `p` tag filter or notifications | Union of each tagged pubkey's inbox relays. |
| Subscription with neither | Active session's read relays. |
| Publish of any signed event | Author's write relays. |
| Publish with `p` tags (DMs, mentions, reactions) | Author's write relays **plus** each tagged pubkey's inbox relays. |
| DM (NIP-17 gift-wrapped) | **Only** the recipient's inbox relays. Never the author's write relays. Never the active session's "default" relays. |
| Discovery (kind-10002 fetch for unknown pubkeys) | Configurable indexer relay set (default: a curated list of high-coverage relays). |

**Why this prevents specific failure modes.**

- "Publish leaked to wrong relays" → impossible at the API level. The developer cannot supply a relay list to `SendNote`. They can only override via the explicit one-shot `OverrideRelaysForNext` action, which is debug-flagged in logs.
- "DM accidentally public" → impossible. The DM publish path consults only inbox relays; there is no code path that takes both a gift-wrapped event and the author's write relays.
- "Reads missing an author's actual relays" → impossible if the author's kind-10002 is reachable; opportunistically fetched on first contact.
- "Hand-rolled fan-out logic" → no API surface for it.

**Per-pubkey relay-list lifecycle.**

- First contact with an unknown pubkey → enqueue kind-10002 fetch from indexer relays.
- Fresher kind-10002 arrives → invalidate dependent subscriptions, recompute relay sets, re-issue REQs as needed.
- Kind-10002 missing for a pubkey after N seconds → fall back to indexer set for reads only; do not publish to indexers.

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

`TimelineItem` is a flat summary. The full event content, raw tags, signature, and provenance live in the event store inside Rust and do not cross FFI. This matches the precedent set by the bible's reference implementation (Pika): chat list is summaries; current chat loads full content on demand.

View warmth: a view stays cached for 30 seconds after its last claim is dropped (configurable). Re-opening within the window costs zero relay traffic and zero re-sync.

### 7.7 Web of Trust

`nmp-wot` ships as an optional subsystem (gated by `AppConfig.wot_enabled`). On enable:

- Loads the active account's follow graph to a configurable depth (default 2).
- Computes per-pubkey trust scores (default algorithm: simple in-degree weighted by depth; pluggable via a trait).
- Exposes a global filter: when on, every view applies the score threshold before emitting; pubkeys below the threshold are tagged but rendered with a "low trust" UI hint (the renderer chooses; the payload exposes the score).

Computation is incremental; updates to follow lists update scores without recomputing from scratch.

### 7.8 Sync engine (NIP-77 negentropy as first-class infrastructure)

Per doctrine D2, negentropy is the default backfill mechanism. The sync engine is not a feature you opt into; it is the engine the planner runs on top of.

**Position in the stack.**

```
View opens → Planner consults watermarks → Sync engine reconciles gap → EventStore inserts → ViewBatch emits
                                ↓ (fallback)
                                REQ scan
```

**Watermarks as a first-class type.** The engine reads and writes the `watermarks` table introduced in §7.1. A watermark answers two questions:

- Has this `(filter, relay)` pair ever been synced?
- If so, up to what timestamp?

Answers to those questions inform every backfill, every fallback-loader decision, and every "is this cache miss authoritative?" check.

**Three triggers, all built-in.**

1. **App foreground.** On `AppAction::Foreground`, the engine schedules an incremental sync for the active user's home filter (kind:1, kind:6, kind:7 matching followed authors) against their write relays. Runs in the tokio runtime; emits `SyncState` updates as it progresses; no UI blocking.
2. **View open.** When a view opens whose filter has a gap (per watermark), the engine reconciles the gap before — and concurrently with — the live REQ tail. Progress is visible in `SyncState`; the view payload streams in as events land.
3. **Relay reconnect.** On reconnect, the engine resumes from the watermark before re-establishing live REQs. The gap between disconnect and reconnect is filled by sync, not by re-fetching from scratch.

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

**Per-relay capability negotiation.** Not every relay implements NIP-77. The engine maintains a per-relay support flag, probed lazily on first contact. Unsupported relays cause the planner to fall back to REQ scanning for that relay only — other relays in the same fan-out still use sync.

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
- Actor message queue depth (high water mark + current).
- Outstanding subscriptions per relay.

**Debug-build instrumentation**, higher cost:

- `AppState` clone duration p50/p99.
- View recompute duration per view per emit.
- Tokio runtime stats (active tasks, blocking calls).
- Memory footprint of the actor's working set.
- Per-platform marshaling time (recorded by the reconciler).

Exposed via `AppState.debug` in debug builds; accessible via the `EmitDiagnosticSnapshot` action in release builds (writes a JSON snapshot to a path returned via `Effect::DiagnosticReady`). The proof app renders this live as an in-app overlay.

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

Two-arc plan: **infrastructure first, then a real app that stress-proofs it, then a perf pass.** Detailed plan in `docs/plan.md`. The summary table below.

### Arc 1 — Infrastructure

| Phase | Scope | Exit gate |
|---|---|---|
| 0. Foundations | Workspace, `nmp-core` actor skeleton, `AppState`/`AppAction`/`AppUpdate` shells, `nmp-ffi` round-trip, generated bindings, headless test harness | A `FullState` snapshot crosses Swift/Kotlin/TS; `rev` ordering enforced; CI green on all four platforms |
| 1. Event store + planner | EventStore with all insert invariants (replaceable, delete, expiration, dedup, provenance), claim-based GC, sync watermarks table, gossip cache, outbox routing default, subscription planner with coalescing/auto-close/buffering | Bug-extinction tests #1, #2, #3, #4, #6, #8 (§3.3) pass against `MockRelay` |
| 2. Sync engine | NIP-77 negentropy as the default backfill path, watermark-driven planner decisions, foreground/view-open/reconnect triggers, capability negotiation, bytes-saved instrumentation | Cold open of a profile cold-syncs via NIP-77; bytes-saved ≥ 95% vs equivalent REQ on 10k-event backfill |
| 3. Sessions + signers + actions | Multi-account, local-key + NIP-46 signers, the full action catalog from §6.3, offline action queue with durable storage, action atomicity (publish + store-insert as one actor message) | Bug-extinction tests #5, #7, #9, #10 pass; offline-queue replay test passes |
| 4. Views + best-effort rendering | View kinds (profile, contacts, timeline, thread, replies, reactions, conversation list, conversation), pre-formatted display fields, non-optional placeholder contract, `ViewBatch` deltas, view warmth | Best-effort doctrine enforced in tests: posts render without kind:0 present; cached kind:0 always served; in-place refinement on arrival |
| 5. Messaging | NIP-17 conversation layer over NIP-44/NIP-59, NSE crate (`nmp-nse`) with bounded memory, background decryption | DM round-trip on iOS + Android with NSE; memory budget respected |
| 6. Wallet + WoT + Blossom | NWC, zaps, Cashu, nutzaps; WoT subsystem; Blossom client | Pay/receive zap; WoT toggle reorders timeline visibly; Blossom upload/download |
| 7. Web | `nmp-wasm`, web shell, OPFS+IndexedDB backend, capability bridges for web (NIP-07, file pickers, browser storage) | Cross-platform consistency tests pass on web |

### Arc 2 — Proof app + perf

| Phase | Scope | Exit gate |
|---|---|---|
| 8. Proof app | Build `nmp-proof` (§4.5) on all four platforms; performance overlay; scripted scenarios for cross-platform consistency tests | Proof app launches and exercises every subsystem on every platform; consistency tests pass |
| 9. Performance pass | Run proof app on real devices (mid-range iOS, mid-range Android, Linux desktop, modern web browsers); collect counters; address budget regressions; tune planner, ViewBatch deltas, marshaling | All §7.16 budgets met on reference devices; performance report published to `docs/perf/v1.md` |

### Arc 3 — Release

| Phase | Scope | Exit gate |
|---|---|---|
| 10. CLI + starter app + docs | `nmp init`, `nmp doctor`, `nmp gen *`, minimal starter app polish, recipe book, NIP support matrix | §3 success criteria reproducible from published docs alone |
| 11. v1 release | Tagged release on crates.io and npm; bindings published; example apps deployed | Public availability |

**Naming** (`aim.md` §7.7) must be resolved before Phase 11.

Each phase ends with a regression test added to `nmp-testing` that locks in the gate. Subsequent phases must not break prior gates.

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
- **Watermark.** A `(filter, relay) → time` record indicating how much of that filter we have already reconciled from that relay. The basis of sync-first backfill (§7.1, §7.8).
- **Best-effort rendering.** Doctrine D1: render what's available, refine in place; never withhold cached data; never block on fetches.

---

## Appendix A. FFI architecture in detail

### A1. Why snapshots + ViewBatch (and not other patterns)

The bible mandates snapshots over FFI. For a Nostr client with timelines of thousands of events, naive full-snapshots are wasteful. We deviate as follows:

**`AppState` is bounded by what's open.** It does not contain the event store, the gossip cache, the working set, or anything proportional to the local cache size. It contains:

- Small screen-shaped state (router, session, busy flags, toast, wallet balance summary).
- A `HashMap<ViewId, ViewPayload>` populated only for currently-open views.
- Paginated view payloads (each bounded by the UI's actual rendering capacity).

The event store, gossip cache, sync watermarks, working set, and signer state all live in the actor and **never cross FFI**.

**Two outbound channels, one ordering.** `AppUpdate` carries either a full snapshot or a batch of view deltas. Both carry a monotonic `rev`. Platforms apply only updates with `rev > last_applied`; out-of-order delivery is impossible to render. Mixing `FullState` and `ViewBatch` is safe: a `FullState` at rev=N supersedes any pending `ViewBatch` with rev<N.

**The planner batches at ≤60Hz.** 500 reactions arriving in 100ms become ≤6 batched deltas, not 500 callbacks. Bible commandment 9 (no high-frequency FFI loops) is honored by construction.

**Escape hatch for ultra-high-frequency surfaces.** NIP-77 sync progress, live media decode, anything kHz-rate uses a shared-atomic + UI-thread polling pattern (declared in spec, not used by default). It is a deliberate exit from TEA-purity for surfaces where the marshaling cost would dominate.

### A2. Alternatives considered

Three serious alternatives to snapshots+ViewBatch were evaluated. Each is used in production by other apps. Each was rejected (or deferred) for specific reasons.

| Alternative | Used by | Why rejected for v1 |
|---|---|---|
| **Reactive shared SQLite.** Rust writes; both sides hold read handles; reactive query libraries (GRDB / SQLDelight / Drift) re-run queries on table writes. | 1Password (Op core), Linear, Notion mobile, most local-first apps | Surrenders doctrine. Platforms now write queries, which is display-shaping logic. Pre-formatting (timestamps, npubs, sats) either moves into native (D-violation) or materializes as columns at write time (extra schema). Web fragments — wasm SQLite doesn't share with JS the way native does. Cross-platform consistency tests get harder (per-platform query results vs byte-diffable JSON). |
| **Local relay / localhost IPC.** Rust runs an in-process Nostr relay (e.g. `LocalRelay` from `nostr-relay-builder`); platform talks Nostr over WebSocket to it. | Some Tauri apps; Citrine-style Android setups conceptually | WebSocket+JSON tax for in-process IPC. Outbox routing semantics get weird (the "relay" is local but represents many remote relays). The framework's value-add (views, actions, sessions as state) gets obscured behind a protocol that wasn't designed for it. |
| **Shared memory + signal.** Rust writes to mmap'd or shared heap; platform reads via raw pointers; FFI carries only "key X changed." | Game engines; Flutter+Skia for graphics state | Memory safety across FFI is hellish. Unsuitable for Swift/Kotlin idioms. Not portable to web. |

**The "hybrid for v2" possibility.** If Phase 9 measurement shows marshaling cost as the bottleneck on bulk-scrolling views (timeline, conversation history, search), the deliberate v2 escape is:

1. Framework owns a SQLite schema.
2. Framework scaffolds typed, parameterized reactive query bindings per platform via the CLI; platforms call `viewModel.timeline(authors).asFlow()` rather than writing SQL.
3. Schema, indexes, formatting (materialized display columns), and invalidation are owned entirely by Rust.
4. Snapshots + ViewBatch retained for small screen-shaped state and small-payload views; reactive queries used for bulk-scrolling views.
5. Web continues with message-passing (wasm SQLite doesn't bridge to JS reactively).

This is a v2 decision gated on Phase 9 data. v1 ships with snapshots+ViewBatch only.

### A3. Why `ViewBatch` from day one (vs. snapshot-only MVP)

The bible's stated default is "start with `FullState` everywhere; add granular updates only when profiling demands." We deviate because:

- Nostr timeline shape is fundamentally chatty: a single popular event arriving triggers reaction, repost, and zap-receipt events at hundreds per second.
- Full-state churn under that load is wasteful per individual update and harmful in aggregate.
- The marginal complexity of `ViewBatch` is small: it's a typed delta enum over the already-existing view payload types.
- Retrofitting `ViewBatch` later would invalidate every platform shim and reconciler implementation.

Both channels (`FullState` and `ViewBatch`) ship in v1. `FullState` remains the canonical fallback for coarse changes and the recovery path for platform-side state drift.

---

## Appendix B. Glossary of NIPs referenced

| NIP | Purpose | Where it appears |
|---|---|---|
| 01 | Base protocol, replaceable events | §7.1 |
| 05 | DNS-based identifiers | §7.6 |
| 07 | Browser signer | §7.4 |
| 09 | Deletion events | §7.1 |
| 17 | Private DMs | §7.10 |
| 19 | bech32 entities | §7.12 |
| 23 | Long-form content | §4.5 (proof app) |
| 25 | Reactions | §6.3, §7.6 |
| 40 | Expiration | §7.1 |
| 42 | Auth | §6.4 |
| 44 | Encryption | §7.10 |
| 46 | Nostr Connect / bunker | §7.4 |
| 47 | Wallet Connect | §7.9 |
| 49 | Encrypted private key | §7.4 |
| 55 | Android external signer | §7.4 |
| 57 | Lightning zaps | §7.9 |
| 59 | Gift wrap | §7.10 |
| 60 | Cashu wallets | §7.9 |
| 61 | Nutzaps | §7.9 |
| 65 | Relay-list metadata (outbox) | §7.3 |
| 77 | Negentropy | §7.8 |

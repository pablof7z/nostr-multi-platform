# Design: Kernel substrate (extension trait families)

> **Audience:** Framework contributors and module authors. Defines the concrete trait machinery any extension module implements.

> **Status:** rev 1. Adopted alongside ADR-0009 and ADR-0010.

> **Prerequisites:** `docs/design/app-extension-kernel.md` (the architectural proposal), ADR-0009 (kernel boundary), ADR-0010 (generated app enum), `docs/design/reactivity.md` (the reactive machinery these modules plug into), `docs/design/view-catalog.md` (reference view modules).

---

## 1. The five extension trait families

`nmp-core` defines five trait families. Each extension crate implements one or more of them. The kernel runtime knows nothing about a module's specific types — only that the module conforms to these traits and contributes variants to the generated per-app enums (per ADR-0010).

| Trait | Purpose | Owns | Generated FFI artifact |
|---|---|---|---|
| `DomainModule` | Durable non-Nostr records | Schema, migrations, indexes, record types | Per-domain typed records + LMDB-backed CRUD |
| `ViewModule` | Typed reactive projections | Spec, payload, delta, recompute, dependency declaration | `ViewSpec::<Module>(...)` variant + platform wrapper (`useX`) |
| `ActionModule` | Durable workflows on the action ledger | Action types, step machine, validation | `AppAction::<Module>(...)` variant + action ledger row schema |
| `CapabilityModule` | Typed native fact reports | Request/result types | Capability callback interface + result enum variants |
| `IdentityModule` | Signer scopes | Identity descriptor, signer binding policy | Identity-scope variant + secure-store entry shape |

Modules typically implement several. A protocol module (`nmp-nip01`) implements `ViewModule` (Profile, raw events) + `ActionModule` (publish a kind-1). An app module (`twitter-core`) implements `DomainModule` (drafts, settings) + `ViewModule` (compose state) + `ActionModule` (compose-and-publish).

---

## 2. `DomainModule` — durable non-Nostr records

For state that isn't a Nostr event but needs to live across restarts: drafts, compose buffers, app settings, capture queues, transcript fragments, weight logs, project metadata, etc.

```rust
pub trait DomainModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;          // unique e.g. "twitter.drafts"
    const SCHEMA_VERSION: u32;

    /// Migration steps from earlier schema versions to current.
    fn migrations() -> Vec<DomainMigration>;

    /// Secondary indexes the kernel maintains for this module's primary store.
    fn indexes() -> Vec<DomainIndex>;

    /// Register record types and CRUD APIs with the kernel.
    fn register(registry: &mut DomainRegistry);
}

pub struct DomainMigration {
    pub from_version: u32,
    pub to_version: u32,
    pub apply: fn(&mut MigrationTx) -> Result<()>,
}

pub struct DomainIndex {
    pub name: &'static str,
    pub key_fn: fn(&[u8]) -> Option<Vec<u8>>,  // extract index key from serialized record
}
```

**What the kernel owns:**

- LMDB sub-database per `NAMESPACE`.
- Migration application at startup; failure surfaces as `Effect::DomainMigrationFailed`.
- Secondary indexes with kernel-managed consistency.
- Bulk export and redact (used by ADR-0007's diagnostics; useful for GDPR-style data dumps).
- Snapshot helpers for testing.

**What the module owns:**

- Record types (Rust structs implementing `Serialize` + `Deserialize`).
- Record meaning: what the fields mean, how to validate them, when they should be created/deleted.
- Schema migration logic.

**Open question 2 resolution:** Rust-coded migrations, not declarative DSL. Modules write `fn migrate_v1_to_v2(tx: &mut MigrationTx) -> Result<()>`. Indexes use a small declarative API (`DomainIndex { name, key_fn }`). The combination keeps migrations Rust-typed while letting indexes stay terse.

**Generated FFI:** none directly; domain records cross FFI only via `ViewModule` payloads. Apps don't `dispatch(SaveDraft(...))` — they dispatch an action that writes a domain record as a side effect.

**Example:**

```rust
// crates/twitter-core/src/drafts.rs
pub struct DraftsModule;

#[derive(Serialize, Deserialize, Clone)]
pub struct Draft {
    pub id: String,
    pub author_pubkey: String,
    pub content: String,
    pub reply_to: Option<EventCoord>,
    pub created_at_ms: u64,
}

impl DomainModule for DraftsModule {
    const NAMESPACE: &'static str = "twitter.drafts";
    const SCHEMA_VERSION: u32 = 1;

    fn migrations() -> Vec<DomainMigration> { vec![] }
    fn indexes() -> Vec<DomainIndex> {
        vec![
            DomainIndex {
                name: "by_author",
                key_fn: |bytes| serde_json::from_slice::<Draft>(bytes).ok()
                    .map(|d| d.author_pubkey.into_bytes()),
            },
        ]
    }
    fn register(registry: &mut DomainRegistry) {
        registry.register::<Self, Draft>();
    }
}
```

---

## 3. `ViewModule` — typed reactive projections

The successor to the closed `ViewSpec` enum. Every view kind — kernel-Nostr (Profile, Timeline, Thread, ...), protocol-NIP (NIP-29 group state, NIP-17 conversation), or app-specific (Twitter compose, Highlighter capture queue) — is a `ViewModule`.

```rust
pub trait ViewModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;

    type Spec: Clone + Serialize + DeserializeOwned + Send + 'static;
    type Payload: Clone + Serialize + Send + 'static;
    type Delta: Clone + Serialize + Send + 'static;
    type Key: Hash + Eq + Clone + Serialize + Send + 'static;
    type State: Send + 'static;

    /// Domain key the platform shadow indexes this view under (ADR-0005).
    fn key(spec: &Self::Spec) -> Self::Key;

    /// What kinds of events / projections wake this view (ADR-0001).
    fn dependencies(spec: &Self::Spec) -> ViewDependencies;

    /// Initial open: build State and Payload from current store contents.
    fn open(ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload);

    /// Event arrived matching dependencies.
    fn on_event_inserted(ctx: &ViewContext, state: &mut Self::State, event: &Event)
        -> Option<Self::Delta>;

    /// Event removed (kind:5 delete, expiration, etc).
    fn on_event_removed(ctx: &ViewContext, state: &mut Self::State, id: &EventId)
        -> Option<Self::Delta>;

    /// Event replaced (replaceable supersession).
    fn on_event_replaced(ctx: &ViewContext, state: &mut Self::State,
                         old_id: &EventId, new_event: &Event) -> Option<Self::Delta>;

    /// Projection changed (shared projection cache update — author display, reaction count, ...).
    fn on_projection_changed(ctx: &ViewContext, state: &mut Self::State,
                              change: &ProjectionChange) -> Option<Self::Delta>;

    /// Optional: handle ticks for time-sensitive views (NIP-40 expiration, "5 minutes ago" → "now").
    fn on_tick(_ctx: &ViewContext, _state: &mut Self::State) -> Option<Self::Delta> { None }

    /// Full snapshot for FullState emission.
    fn snapshot(ctx: &ViewContext, state: &Self::State) -> Self::Payload;
}
```

**What the kernel owns:**

- Refcounted lifecycle (open/close, claims, view warmth grace).
- Composite-keyed reverse-index registration (ADR-0001).
- DeltaBuffer / within-view coalescing (ADR-0002).
- Backpressure switch to FullState (per `reactivity.md` §7.3).
- `ViewBatch` emission across FFI.
- Generated platform-shadow wrappers (`useX(spec) -> Payload`).

**What the module owns:**

- Spec / Payload / Delta / State types.
- Recompute logic (incremental vs full).
- Pre-formatted display fields per doctrine D1.
- Best-effort placeholder values when underlying data is absent.

**Cross-FFI:** the module's Spec, Payload, and Delta types are surfaced in the per-app generated enums (ADR-0010). UniFFI bindings expose them as native types on each platform.

---

## 4. `ActionModule` — durable workflows on the action ledger

Replaces the closed `AppAction` enum. Every user intent — `SendNote`, `React`, `Repost`, `CreateHighlight`, `UploadBlob`, `RunSync` — is an `ActionModule`.

```rust
pub trait ActionModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;

    type Action: Clone + Serialize + DeserializeOwned + Send + 'static;
    type Step: Clone + Serialize + DeserializeOwned + Send + 'static;
    type Output: Clone + Serialize + Send + 'static;

    /// Validate + plan the action. Returns initial step state or rejection.
    fn start(ctx: &mut ActionContext, action: Self::Action)
        -> Result<ActionPlan<Self::Step>, ActionRejection>;

    /// Drive the state machine. Input could be a capability result, a relay OK,
    /// a timeout, a user approval, etc.
    fn reduce(ctx: &mut ActionContext, id: ActionId, input: ActionInput<Self::Step>)
        -> ActionTransition<Self::Step, Self::Output>;
}

pub struct ActionPlan<Step> {
    pub initial_step: Step,
    pub initial_status: ActionStatus,
    pub deadline_ms: Option<u64>,
}

pub enum ActionTransition<Step, Output> {
    Continue { step: Step, status: ActionStatus },
    Complete { output: Output },
    Fail { reason: String, transient: bool },
    AwaitCapability { request: CapabilityRequest, next_step: Step },
    AwaitUserApproval { prompt: ApprovalPrompt, next_step: Step },
}
```

**What the kernel owns:**

- Durable ledger rows (`actions` table in the storage backend).
- Action IDs (ULID).
- Status transitions: `Pending → Running → Completed | Failed | Cancelled`.
- Retries with exponential backoff (for transient failures).
- Cancellation correlation.
- Provenance (which relays it published to, when, with what response).
- Capability request/response correlation (the action awaits a `CapabilityResult`).
- Restart recovery (actor restart re-loads in-flight actions; modules' `reduce` is called with `ActionInput::ResumedAfterRestart`).
- Diagnostic rendering for ADR-0007.

**What the module owns:**

- Action types (e.g. `SendNote { content, reply_to }`, `React { target, emoji }`).
- Step machine (validate → sign → publish → confirm).
- Validation rules.
- Final domain writes (insert the signed event into the store, append to a domain table, etc.).
- Whether user approval is required (for sensitive actions).

**Atomicity** (doctrine guarantee): the kernel ensures `module.reduce(...)`, ledger transitions, and local store writes happen as one actor message. External effects such as relay publishes cannot be rolled back after a relay accepts them, so publish steps must be ledger-correlated and restart-recoverable: a "publish accepted but local insert failed" path becomes an explicit failed/recovery ledger state, not silent divergence.

**Example:**

```rust
// crates/nmp-nip01/src/actions/send_note.rs
pub struct SendNoteAction;

#[derive(Clone, Serialize, Deserialize)]
pub struct SendNote {
    pub content: String,
    pub reply_to: Option<EventCoord>,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum SendNoteStep {
    Validating,
    AwaitingSignature,
    Publishing { signed: Event },
}

impl ActionModule for SendNoteAction {
    const NAMESPACE: &'static str = "nip01.send_note";
    type Action = SendNote;
    type Step = SendNoteStep;
    type Output = EventId;

    fn start(cx: &mut ActionContext, action: SendNote)
        -> Result<ActionPlan<SendNoteStep>, ActionRejection>
    {
        if action.content.is_empty() {
            return Err(ActionRejection::Invalid("empty content".into()));
        }
        Ok(ActionPlan {
            initial_step: SendNoteStep::Validating,
            initial_status: ActionStatus::Running,
            deadline_ms: Some(now_ms() + 30_000),
        })
    }

    fn reduce(cx: &mut ActionContext, id: ActionId, input: ActionInput<SendNoteStep>)
        -> ActionTransition<SendNoteStep, EventId>
    {
        // ... validate → sign → publish → confirm ...
    }
}
```

---

## 5. `CapabilityModule` — typed native fact reports

Implements D7 (capabilities report, never decide). The kernel ships a small set of generic capability families; modules extend with app-specific ones.

```rust
pub trait CapabilityModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;

    type Request: Clone + Serialize + DeserializeOwned + Send + 'static;
    type Result: Clone + Serialize + DeserializeOwned + Send + 'static;

    /// Trait the platform implements (becomes a uniffi::callback_interface).
    fn callback_interface_name() -> &'static str;
}
```

The generated FFI exposes one Swift protocol / Kotlin interface per `CapabilityModule`, with methods matching the request shape and a reverse-callback mechanism for results.

**Kernel-provided capability families** (each is a `CapabilityModule`):

- `KeyringCapability` — secure storage of nsec, NIP-46 connection tokens.
- `HttpCapability` — HTTP GET/PUT/POST with progress.
- `NetworkMonitorCapability` — online/offline + connection type.
- `PushCapability` — registration token + wake events.
- `FilePickerCapability` — native file picker; returns bytes + metadata.
- `LocalNotificationCapability` — schedule + cancel.
- `MediaMetadataCapability` — hash, mime, dimensions, duration.
- `ExternalSignerCapability` — launch external signer app, await result.

**Module-provided capability examples:**

- `HighlighterOcrCapability` — OCR a clipboard image; return text.
- `CutTrackerHealthKitCapability` — sample HealthKit; return raw samples.
- `PodcastDownloadCapability` — long-running download with resumable progress.

Each is a `CapabilityModule` with its own request and result types. Apps declare which they consume; codegen produces the matching callback interfaces.

---

## 6. `IdentityModule` — signer scopes

The kernel can host multiple identity kinds simultaneously. An app may have a human Nostr account, app-local agents, and ephemeral signing keys. The trait isolates "what kind of identity is this and how do I sign with it" from the kernel's session machinery.

```rust
pub trait IdentityModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;

    type Descriptor: Clone + Serialize + DeserializeOwned + Send + 'static;

    fn scope_kind() -> IdentityScopeKind;
    fn create(ctx: &mut IdentityContext, descriptor: Self::Descriptor)
        -> Result<IdentityId, IdentityError>;
    fn sign(ctx: &IdentityContext, id: &IdentityId, unsigned: &UnsignedEvent)
        -> BoxFuture<'static, Result<Event, SigningError>>;
    fn destroy(ctx: &mut IdentityContext, id: &IdentityId);
}

pub enum IdentityScopeKind {
    HumanAccount,           // user-controlled nsec / NIP-46
    AppLocal,               // app-spawned (e.g. agent identity)
    ExternalSigner,         // NIP-07, Amber, NIP-46
    Ephemeral,              // throwaway (e.g. NIP-04 challenge response)
}
```

The kernel owns identity ID assignment, secure-store persistence, session activation, and routing of `dispatch(AppAction::Sign(...))` to the right module. Apps decide which identity scopes exist in their model.

---

## 7. Codegen pipeline

`nmp gen modules` reads `nmp.toml`, resolves the declared modules and their trait implementations, and produces the per-app crate `nmp-app-<name>/`.

```
$ nmp gen modules
Reading nmp.toml...
Resolving modules:
  kernel: nmp-core
  protocol: nmp-nip01, nmp-nip02, nmp-nip10, nmp-nip25, nmp-nip65
  app: twitter-core
Generating nmp-app-twitter/...
  src/action.rs   ✓ (5 module variants)
  src/update.rs   ✓
  src/view_spec.rs ✓ (8 view kinds: 4 from nmp-nip01, 1 each from nmp-nip02 / nmp-nip25 / nmp-nip65 / twitter-core)
  src/capability.rs ✓ (8 capability traits)
  src/domain.rs    ✓ (3 domain registrations)
  src/ffi.rs       ✓
Generating bindings:
  bindings/swift/   ✓
  bindings/kotlin/  ✓
  bindings/typescript/ ✓
```

The generated crate's `Cargo.toml` includes the chosen modules as path or registry dependencies. The crate is checked into `apps/<name>/nmp-app-<name>/`. CI verifies regeneration is deterministic (same modules → same output).

Cross-crate enum composition is done by the codegen tool, not by macros, to keep the build graph linear and avoid macro recursion issues with UniFFI.

---

## 8. Module composition rules

- **Domain stores are isolated.** Module A's domain store cannot read Module B's. Cross-module reads go through view modules (which read from the event store and from projections, not from other modules' domain stores).
- **Actions can dispatch other actions.** An `ActionModule::reduce` may call `ctx.dispatch(SubAction { ... })`. Atomicity is preserved (sub-actions run on the same actor message tick).
- **Views can read projections from other modules.** Projection caches are kernel-owned (per `reactivity.md` §6); any module can read them. This is how a Timeline view (in `nmp-nip01`) reads author-display projections that come from kind:0 inserts (also in `nmp-nip01`).
- **Capabilities are module-private.** A capability registered by Module A is callable only by code that has a typed handle to it. The kernel does not route across.
- **Identities are global.** Any module's actions can sign with any registered identity (subject to authorization policy the app's identity module imposes).

---

## 9. Diagnostics integration (ADR-0007)

Each module contributes to the diagnostics surface:

- `ViewModule` shows up in `LogicalInterestStatus::Module { namespace, key }`.
- `ActionModule` shows up in the action ledger; in-flight actions render as rows.
- `CapabilityModule` shows up as pending capability requests.
- `DomainModule` exposes its index health (entry counts, last write, last migration).
- `IdentityModule` exposes its active scopes (count by scope kind).

The diagnostics screen is itself a generated `ViewModule` per app (it reads from all module surfaces and assembles a summary). The proof app in Phase 1a.7 includes it.

---

## 10. Testing patterns

Each module is testable in isolation:

```rust
#[test]
fn drafts_module_persists_across_restart() {
    let mut h = TestHarness::new();
    h.register::<DraftsModule>();
    h.start();
    h.dispatch_domain_write::<DraftsModule>(Draft { /* ... */ });
    h.simulate_restart();
    let drafts: Vec<Draft> = h.query::<DraftsModule>("by_author", &alice_pubkey);
    assert_eq!(drafts.len(), 1);
}
```

`TestHarness` mocks the actor, the storage backend, and the FFI emission. Modules can be tested without a real relay, without LMDB on disk, without UniFFI bindings.

Integration tests across modules use the real EventStore and a `MockRelay`.

End-to-end tests in `firehose-bench live` exercise the full per-app generated crate against a real relay.

---

## 11. What goes in v1 vs later

**v1 kernel substrate** (Phase 1a.1):

- `DomainModule` trait + `DomainRegistry` + LMDB backing.
- `ViewModule` trait + view registry + reverse-index integration + delta buffer integration.
- `ActionModule` trait + durable ledger + restart recovery.
- `CapabilityModule` trait + bridge plumbing.
- `IdentityModule` trait + secure-store binding.
- `nmp gen modules` codegen with output for one fixture app.

**v1 reference modules:**

- `nmp-nip01`: Event types, Filter, Profile / Contacts / Timeline view modules, SendNote / DeleteEvent actions.
- `nmp-nip02`: Contacts module (re-exported for convenience; structure overlaps with nip01).
- `nmp-nip10`: Reply marker handling for thread building.
- `nmp-nip25`: Reactions view module + React action.
- `nmp-nip65`: Mailboxes view module + outbox routing helper.
- `nmp-nip77`: Sync engine (per spec §7.8, now packaged as a module).
- `nmp-blossom`: Blossom upload action + upload view module.
- `nmp-nip17`: Conversation view module + SendDm action + NSE crate.

**v1 app modules:**

- `twitter-core`: the demo app (compose UI state, settings).
- `fixture-todo-core`: the non-Nostr fixture module proving the boundary.

**v1.x and beyond:**

- `nmp-nip29` (groups), `nmp-nwc`, `nmp-cashu`, `nmp-nip57` (zaps), etc.
- Highlighter-lite, TENEX-lite, etc. as demonstration extension modules.

---

## 12. Open questions still to settle

- **Sub-action atomicity ordering.** When `reduce` dispatches a sub-action, is the sub-action visible to other modules' reducers in the same actor tick? Recommended default: yes (single tick), but needs validating against use cases.
- **Module hot-reload during dev.** Can `nmp gen modules` re-run incrementally? Not v1; rebuild from scratch is acceptable for v1.
- **Per-platform wrapper customization.** Should modules be able to override the default wrapper shape (e.g., a Swift wrapper that uses `@AppStorage` instead of `@Observable`)? Probably yes but as v1.5.
- **Cross-module migration coordination.** Module A migrates schema → Module B needs to know. Sketched as a "migration manifest" but needs design.
- **Modules with no Rust-side state.** Pure protocol modules (e.g., `nmp-nip19` for bech32 encoding) might have no DomainModule / ViewModule / ActionModule — just utility code. Allowed; not all modules implement all traits.

---

## 13. Validation

This design is validated when:

1. Phase 1a.1 (kernel substrate prototype) ships with one fixture module (`fixture-todo-core`) demonstrating each of the five trait families. Codegen produces a working `nmp-app-fixture` crate. Desktop iced app renders a TODO list, no business logic in Swift / iced.
2. Phase 1a.2 onward (Twitter clone) implements the demo entirely as extension modules with no `nmp-core` patches needed.
3. A future Highlighter-lite, TENEX-lite, or podcast-lite module can be added without changes to `nmp-core` traits. Demonstrated on paper for v1; demonstrated in code post-v1.

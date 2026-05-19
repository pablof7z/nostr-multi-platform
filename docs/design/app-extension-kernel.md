# Design Proposal: App Extension Kernel Boundary

> **Status:** Proposed
> **Date:** 2026-05-17
> **Scope:** How NMP should support real apps without baking app-specific business logic into the framework core.

## 1. Problem

NMP's north star is correct: Rust owns protocol logic, state, cache, relay management, signing orchestration, derived views, and app policy; platform code renders state and executes OS capabilities. The danger is interpreting that as "NMP core should contain every product concept an app might need."

That would be the wrong abstraction.

The reviewed apps all need Rust-owned business logic, but their business logic is not the same:

- Highlighter needs rooms, artifacts, highlights, comments, capture drafts, Blossom uploads, share queues, and NIP-29/NIP-84/NIP-22/NIP-73 reference semantics.
- Podcast apps need podcasts, episodes, transcripts, downloads, provider credentials, player state, summarization, and content search.
- TENEX needs projects, conversations, agents, reports, workspaces, command/status streams, and artifacts.
- Win the Day needs planning, live agent runs, app-local agent identity, user approval flows, and personal records.
- Cut Tracker needs HealthKit facts, weight logs, macro math, coach state, notifications, voice drafts, and privacy/export scopes.

The lesson is not that these nouns belong in `nmp-core`. The lesson is that NMP must provide a kernel where app crates can define those nouns safely, durably, observably, and with generated platform bindings while still obeying the "no native business logic" rule.

## 2. Decision

NMP should be designed as a **Nostr-native app kernel with first-class app extension modules**.

The framework core provides generic infrastructure:

- actor runtime and unidirectional state flow,
- verified Nostr event store,
- subscription planner,
- relay routing and publish pipeline,
- signer/session plumbing,
- domain store substrate for non-Nostr records,
- typed view registry,
- durable action ledger,
- capability bridge,
- platform-shadow/codegen machinery,
- diagnostics and test harnesses.

App crates provide business domains:

- records and migrations,
- view specs, payloads, and reducers,
- actions and state machines,
- app policies and validation,
- app-specific identities and permissions,
- capability request/result types when the generic set is not enough.

The rule:

> If implementing Highlighter, TENEX, Win the Day, Cut Tracker, or Podcast requires adding app nouns to `nmp-core`, the extension boundary is wrong.

## 3. Layering

NMP should separate four layers.

| Layer | Owns | May contain app nouns? |
|---|---|---|
| `nmp-core` kernel | actor, event store, planner, ledger, domain-store traits, view/action/capability registries, diagnostics | No |
| NMP protocol modules | reusable Nostr standards such as NIP-29 groups, NIP-22 comments, NIP-46, NIP-73, Blossom, NIP-17 | Only protocol nouns |
| App core crate | app domain records, views, actions, policies, custom capability reports | Yes |
| Platform shell | rendering, local OS handles, capability execution, generated wrappers | No policy nouns beyond UI labels |

Examples:

- `nmp-nip29` may know what a NIP-29 group is.
- `highlighter-core` may know what a reading room, capture draft, or photo-always publish invariant is.
- `nmp-core` should know neither.

## 4. Extension Points

### 4.1 Domain Modules

Apps need durable records that are not Nostr events. They must not be encoded as fake Nostr events, and they must not live in SwiftData, Core Data, UserDefaults, SQLite wrappers, or platform caches as the source of truth.

NMP should expose a namespaced domain-store substrate:

```rust
pub trait DomainModule {
    const NAMESPACE: &'static str;

    fn migrations() -> Vec<DomainMigration>;
    fn indexes() -> Vec<DomainIndex>;
    fn register(registry: &mut DomainRegistry);
}
```

The kernel owns storage, migrations, indexing, snapshots, diagnostics, and backup/export plumbing. The app module owns record meaning.

Examples:

| App concept | Belongs in |
|---|---|
| `HighlightDraft` | `highlighter-core` domain module |
| `EpisodeTranscript` | `podcast-core` domain module |
| `ProjectReport` | `tenex-core` domain module |
| `WeightLog` | `cut-tracker-core` domain module |
| `DailyPlan` | `win-core` domain module |

### 4.2 View Modules

The current view catalog is too social-client shaped if treated as the whole product surface. Built-in views are useful, but apps need to register their own typed views.

NMP should make `ViewSpec` extensible through app modules:

```rust
pub trait ViewModule {
    type Spec;
    type Payload;
    type Delta;
    type Key;

    fn key(spec: &Self::Spec) -> Self::Key;
    fn dependencies(spec: &Self::Spec) -> ViewDependencies;
    fn open(ctx: &ViewContext, spec: Self::Spec) -> (Self::Payload, ViewState);
    fn reduce(ctx: &ViewContext, state: &mut ViewState, input: ViewInput) -> Option<Self::Delta>;
    fn snapshot(ctx: &ViewContext, state: &ViewState) -> Self::Payload;
}
```

The kernel owns lifecycle, refcounts, view warmth, dependency tracking, coalescing, `ViewBatch` delivery, and generated platform wrappers. The app owns the projection.

Highlighter is the reference pattern: a Rust core emits `Delta(subscription_id, DataChangeType)`, and Swift's `EventBridge` routes those changes into view-scoped stores. NMP should generate this domain-keyed platform-shadow machinery from view modules rather than forcing each app to hand-write it.

### 4.3 Action Modules And The Durable Ledger

NMP should not ship an `AgentApproval` feature. It should ship a durable action ledger that any app action can use.

```rust
pub trait ActionModule {
    type Action;
    type Step;
    type Output;

    fn start(ctx: &mut ActionContext, action: Self::Action) -> ActionPlan<Self::Step>;
    fn reduce(ctx: &mut ActionContext, id: ActionId, input: ActionInput<Self::Step>)
        -> ActionTransition<Self::Step, Self::Output>;
}
```

The kernel owns:

- action ids,
- durable status rows,
- retries,
- cancellation,
- provenance,
- per-relay publish attempts,
- capability request/response correlation,
- restart recovery,
- diagnostic rendering.

The app owns:

- what the action means,
- whether a user approval is required,
- validation rules,
- state transitions,
- final domain writes.

Examples:

| Workflow | NMP owns | App owns |
|---|---|---|
| Agent approval | ledger, pending/running/done/error status, provenance | what approval means, who can approve, what action executes |
| Highlighter capture | media/upload capability facts, ledger, retry state | photo-always invariant, OCR selection policy, publish shape |
| Podcast ingest | download/transcription capability facts, ledger | feed rules, transcript chunking, knowledge model |
| Cut Tracker logging | HealthKit capability facts, ledger | source precedence, macro math, coach recommendations |
| TENEX report send | publish/artifact status, ledger | project/report semantics and permissions |

### 4.4 Capability Modules

Capabilities report raw facts. They do not decide app policy.

The kernel should provide a small set of generic capability families:

- keyring/secure storage,
- HTTP/download/upload,
- file picker and file import,
- network monitor,
- push token/background wake,
- local notification scheduling,
- media hashing and metadata,
- external signer launch/return.

Apps can add capability types for OS-specific fact sources:

- OCR result,
- HealthKit samples,
- speech transcription result,
- audio playback position,
- share-extension payload,
- camera/imported image metadata.

Capability results enter the actor as typed messages. The app module decides how to transform those facts into domain records, view deltas, or action transitions.

### 4.5 Identity Scopes

NMP should not assume "the active Nostr user" is the only identity. It should provide identity scopes without knowing app policy:

```rust
pub enum IdentityScope {
    HumanAccount(AccountId),
    AppLocal { namespace: String, id: String },
    ExternalSigner { connection_id: String },
    Ephemeral { purpose: String },
}
```

The kernel owns signer binding, secret storage, NIP-46/session mechanics, and diagnostics. App modules decide which identities exist and what they are allowed to do.

This supports app-local agents, feedback identities, human accounts, external signers, and provider credential owners without adding "agent" or "feedback" semantics to `nmp-core`.

### 4.6 Typed Nostr References

NMP should own generic, protocol-correct reference primitives because they are Nostr semantics, not app business logic.

The core/protocol layer should expose typed pointers for:

- event id references,
- addressable `a` coordinates,
- pubkeys,
- external `i` references,
- URLs and relay hints,
- group `h` tags,
- imeta/media references,
- quote/comment/repost/reaction target roles.

Protocol modules can add standard interpretation for NIPs. App modules compose those references into app meaning.

For example, NMP may know how to parse and validate NIP-73 artifact references. Highlighter decides that a given artifact is a book, podcast episode, or reading-room object.

### 4.7 Codegen Contract

For each app module, NMP should generate:

- UniFFI records/enums for exposed specs, payloads, deltas, actions, and capability reports,
- Swift/Kotlin/TypeScript platform-shadow dictionaries keyed by the module's view keys,
- refcounted wrappers such as `useRoomHome(groupId)` or `@RoomHome`,
- bridge routing from `ViewBatch` to typed platform caches,
- diagnostic names for views, actions, and domain stores.

Generated code is allowed to know app nouns because it is generated for that app. `nmp-core` is not.

## 5. What Stays Out Of NMP Core

These concepts should not be added to `nmp-core`:

- agent approvals,
- daily plans,
- weight cuts,
- podcast episode ingest,
- transcript chunking,
- highlighter capture policy,
- OCR selection rules,
- TENEX project/report semantics,
- app-specific friend/whitelist rules,
- coach prompts,
- provider/model settings beyond generic credential capability plumbing.

If multiple apps repeat a pattern, extract the deterministic substrate, not the product noun. For example:

- Extract a durable action ledger, not "agent approval."
- Extract media upload/draft primitives, not "highlight capture."
- Extract domain-store migrations/export/redaction, not "weight logs."
- Extract signer/session scopes, not "coach identity."

## 6. Acceptance Tests For The Boundary

The proof slices should be treated as extension-boundary tests, not as features that NMP core must contain.

### 6.1 Highlighter-lite

Implement an app module with:

- `RoomHomeSpec { group_id }`,
- artifact/highlight/comment projections,
- `CreateHighlight` action,
- optional `ShareToGroup` action,
- media upload capability result,
- generated Swift wrappers.

Pass condition: no `Room`, `Highlight`, `Artifact`, or `CaptureDraft` nouns are added to `nmp-core`. Only reusable protocol modules may contain NIP-specific nouns.

### 6.2 Personal-coach-lite

Implement an app module with:

- `TodayDashboardSpec`,
- `LogWeight` or `LogMeal` action,
- local HealthKit/sample capability reports,
- an app-defined pending user approval action,
- export/redaction scope.

Pass condition: NMP core does not know about meals, weight, health, or approvals, but the app still has durable state, diagnostics, generated wrappers, and no Swift business logic.

### 6.3 TENEX-lite

Implement an app module with:

- `ProjectListSpec`,
- `ConversationSpec`,
- `SendMessage` action,
- report/artifact records,
- high-rate stream batching.

Pass condition: NMP core does not know about projects, agents, or reports, while still providing subscription lifecycle, action ledger, signer/publish pipeline, and platform-shadow updates.

### 6.4 Podcast-lite

Implement an app module with:

- `PodcastLibrarySpec`,
- `EpisodeDetailSpec`,
- download/transcript capability reports,
- playback facts from native,
- app-owned transcript/search domain records.

Pass condition: NMP core does not know about podcasts or transcripts, but Rust still owns the state machine and native remains a renderer plus capability executor.

## 7. Consequences

This design changes how the current docs should be interpreted:

- The built-in social view catalog becomes a set of reusable Nostr modules and examples, not the limit of the framework.
- `ViewSpec` cannot remain a closed enum owned only by NMP if app crates need typed views. It needs an app-extension registry or generated app-level enum that wraps kernel and app view variants.
- `AppAction` cannot remain a closed enum owned only by NMP if app crates need typed actions. It needs an app-extension registry or generated app-level enum that wraps kernel and app action variants.
- The domain store and action ledger become v1 kernel requirements, not later product features, because they are the boundary that keeps app policy out of native shells without moving it into NMP core.
- Codegen becomes central. Without generated wrappers, each serious app will recreate Highlighter's manual `EventBridge` pattern.

## 8. Open Design Questions

1. Should app extension modules compile into one generated app enum, or should the runtime use a type-erased registry at the FFI boundary?
2. How much of a module's storage schema should be declared declaratively versus ordinary Rust migrations?
3. Can UniFFI comfortably expose generated app-level enums across multiple extension crates, or do we need a thin app-specific FFI crate per app?
4. Should protocol modules live as separate crates (`nmp-nip29`, `nmp-blossom`) or as optional features on `nmp-views` / `nmp-actions`?
5. What is the minimum v1 app-extension API needed before building the social-client proof app, so we do not overfit the kernel to the proof app's nouns?

## 9. Recommended Next Step

Before adding more built-in product views, implement the smallest extension-path prototype:

1. A `DomainModule` trait with one durable non-Nostr record type.
2. A `ViewModule` trait with a generated app-level `ViewSpec`.
3. An `ActionModule` trait backed by the generic action ledger.
4. Generated Swift wrappers for one app-defined view.
5. A small fixture app module that proves the platform shell has no business logic and `nmp-core` has no app nouns.

That prototype should be accepted only if the same mechanism can plausibly express the Highlighter-lite, Personal-coach-lite, TENEX-lite, and Podcast-lite slices above.

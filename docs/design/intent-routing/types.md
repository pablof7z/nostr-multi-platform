# Intent-classed routing + NIP-50 search §3 — Type surface

> Parent: `docs/design/intent-routing.md`.
> Cross-refs: planner integration in `planner.md` (§4); search FFI lives in
> `nmp-nip50` (higher-order), see ADR-0071 2026-06-22 amendment.

## 3. Type surface

### 3.1 `EventClass`

```rust
// nmp-core::routing::class

#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum EventClass {
    /// kind:1, kind:6, kind:7, generic public-feed traffic.
    PublicNote,
    /// kind:0 — profile metadata.
    Profile,
    /// kind:10002 — NIP-65 relay list.
    RelayList,
    /// kind:30023 — NIP-23 long-form articles.
    LongForm,
    /// NIP-37 drafts.
    /// - `kind:31234` — encrypted draft envelope (parent).
    /// - `kind:1234`  — encrypted checkpoint, references parent via
    ///   `["a", "31234:<pubkey>:<d>"]`.
    /// Both route to the user's NIP-51 kind:10013 list (personal).
    Draft,
    /// NIP-54 wikis.
    /// - `kind:30818` — addressable wiki entry.
    /// - `kind:818`   — merge request.
    /// - `kind:30819` — redirect.
    /// All three route to the *publishing author's* kind:10102 list
    /// (publisher-keyed; see `planner.md` §4.1).
    Wiki,
    /// kind:4 / kind:14 — direct messages. Reserved variant; routing
    /// wiring (kind:10050 / NIP-17) is deferred to its own ADR.
    DM,
    /// NIP-29 group-messaging kinds. Kept for diagnostic clarity. NEVER
    /// participates in `class_relays`; NIP-29 events use the existing
    /// `InterestShape::relay_pin` lane (ADR-0071).
    GroupMessage,
    /// Anything not enumerated above. Falls through to NIP-65 routing.
    Other,
    // NOTE (2026-06-22): EventClass::Search was removed. Search routing does
    // not need a planner class. The generic InterestShape.search wire-filter
    // field is sufficient; relay selection from kind:10007 is performed at the
    // higher layer by SearchRelayListProjection (nmp-nip51), consumed by
    // nmp-nip50. See ADR-0071 higher-order search amendment.
}

impl EventClass {
    /// Concrete v1 table (extend as NIPs land):
    /// - 0          → Profile
    /// - 1, 6, 7    → PublicNote
    /// - 4, 14      → DM           (variant reserved; routing TBD)
    /// - 818,
    ///   30818,
    ///   30819      → Wiki
    /// - 1234,
    ///   31234      → Draft        (checkpoint + parent share class)
    /// - 10002      → RelayList
    /// - 30023      → LongForm
    /// - NIP-29
    ///   group kinds → GroupMessage
    /// - everything else → Other
    pub fn from_kind(kind: u32) -> Self { /* table */ }

    /// Routing family: which resolver method serves this class.
    pub fn routing_family(&self) -> RoutingFamily { /* see §3.3 */ }
}

pub enum RoutingFamily {
    /// Active account's NIP-51 list. No author argument.
    /// Used by: Draft.
    /// NOTE (2026-06-22): Search was removed from this family. kind:10007
    /// relay selection is higher-order (see nmp-nip50/SearchRelayListProjection).
    Personal,
    /// Publisher's NIP-51 list, consulted per author at compile time.
    /// Used by: Wiki.
    PublisherKeyed,
    /// Existing relay_pin lane (ADR-0071). Used by: GroupMessage.
    /// `class_relays` is never called for this family.
    RelayPin,
    /// No class routing — falls through to NIP-65 / four-lane planner.
    /// Used by: PublicNote, Profile, RelayList, LongForm, DM (v1), Other.
    None,
}
```

### 3.2 Extended `InterestShape`

```rust
pub struct InterestShape {
    // ... existing fields unchanged ...

    /// NIP-50 search string. Emits `search` on the wire filter and refuses
    /// merges with different search values (merge Rule 10).
    pub search: Option<String>,

    /// Optional class hint set by the consumer. When `None`, the planner
    /// derives the class from `kinds` via `EventClass::from_kind`. When
    /// `Some`, the value wins — used by apps that emit ambiguous kinds.
    /// NOTE (2026-06-22): `EventClass::Search` is no longer a valid value
    /// here; search shapes are expressed solely via the `search` field above
    /// and routed at the higher layer in `nmp-nip50`.
    pub class_hint: Option<EventClass>,
}
```

Both fields are `Option`; `search` is substrate-only, while NIP-50 query
parsing, views, ranking, relay selection from kind:10007, and projection live
in the owning search module (`nmp-nip50`).

### 3.3 Extended `OutboxResolver`

```rust
pub trait OutboxResolver: Send + Sync {
    // existing
    fn write_relays(&self, author: &Pubkey) -> Vec<RelayUrl>;
    fn read_relays(&self, author: &Pubkey) -> Vec<RelayUrl>;

    /// Personal NIP-51 routing — active account context, no author.
    /// Used for classes whose NIP-51 list is intrinsically self-keyed
    /// (Draft: "where I store my drafts").
    /// NOTE (2026-06-22): kind:10007 search relays are NOT routed through
    /// this method. Search relay selection is performed at the higher layer
    /// by `nmp-nip50` via `SearchRelayListProjection` from `nmp-nip51`.
    /// Returns `None` when no list / no app default exists.
    fn class_relays_personal(&self, class: &EventClass) -> Option<Vec<RelayUrl>>;

    /// Publisher-keyed NIP-51 routing — consult the publishing author's
    /// list. Used for Wiki (kind:10102 reflects "the relays I want my
    /// wiki content to live on"). Lazy-fetched per author the first time
    /// a class-routed interest names them; cached as long as a live
    /// interest references them.
    /// Returns `None` when:
    ///   - the author's list hasn't been fetched yet (the planner
    ///     emits a pending-fetch diagnostic and falls back to NIP-65),
    ///   - or the list exists but is empty AND no app default is set.
    fn class_relays_for_author(
        &self,
        class: &EventClass,
        author: &Pubkey,
    ) -> Option<Vec<RelayUrl>>;

    /// Blocked relays for the active account (kind:10006). Applied as a
    /// final filter against every compiled plan and every publish
    /// target list. Personal-scope only — there is no "Bob blocks this
    /// relay" semantics in v1.
    fn blocked_relays(&self) -> std::collections::BTreeSet<RelayUrl>;
}
```

Why two `class_relays_*` methods, not one with `Option<&Pubkey>`:
personal-class lists have no meaningful author argument (it would
always be `None`); publisher-keyed lists always do. Two methods carry
the intent at the type level. The planner picks which to call by
inspecting `class.routing_family()`.

### 3.4 `PublishTarget`

```rust
pub enum PublishTarget {
    /// Default. Class-aware NIP-51 routing with NIP-65 fallback +
    /// blocked-relay filter. Replaces the old "Auto" semantics — every
    /// existing call site inherits class routing implicitly.
    Auto,
    /// Caller pins the relay set. Blocked-relay filter still applies.
    Explicit { relays: Vec<RelayUrl> },
}
```

No new `AutoByClass` variant. `Auto` is upgraded. Existing call sites
(Chirp, gallery, M11 tests) get class routing automatically. P5 of the
rollout is an audit pass to verify no existing call site relies on
NIP-65-only behavior for an event the new `EventClass::from_kind`
would classify away from `Other`.

### 3.5 Input-intent type surface

> **Implementation note (commit `956f79f0`, `feat/input-intent-resolver-1804`).**
> All types below live in `nmp-core::substrate::intent` and are re-exported from
> `nmp_core::substrate::*`. The module is noun-free (D0: doctrine-lint 0 findings).
> The orchestrator lives in `crates/nmp-intent`. NIP-05 reverse lookup lives in
> `crates/nmp-nip05`. The FFI setter is `crates/nmp-ffi/src/app_config_intent.rs`.

The input-intent resolver is the single framework front door for raw user-entered
text. It classifies before it dispatches — references, relay URLs, and NIP-05
identifiers are routed to their existing loaders; only free text reaches the
NIP-50 search surface (§3.6 below).

#### Scope identity

```rust
// nmp-core::substrate::intent::id
pub struct InputScopeId {
    pub namespace: String,
    pub name: String,
}
impl InputScopeId {
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self;
    pub fn label(&self) -> String;                     // "namespace.name"
    pub const NOSTR_REF_NAMESPACE: &'static str = "nostr";
    pub const NOSTR_REF_NAME:      &'static str = "ref";
    pub fn nostr_ref() -> Self;                        // synthetic always-allowed "nostr.ref"
}
```

`InputScopeId` uses owned strings (not a hashed discriminant like `SearchScopeId`);
equality is `(namespace, name)` pair equality. The synthetic `nostr.ref` scope id is
always-allowed and covers all NIP-19/21 direct references.

#### Resolved input (for recognizers)

```rust
// nmp-core::substrate::intent::recognizer
pub enum TextSearchTargets {
    UserPreferred,
    AppDefault,
    Explicit(Vec<String>),       // noun-free mirror of nmp_nip50::SearchTargets
}

pub enum ResolvedInputKind {
    Reference   { uri: String, entity_class: String },  // "profile" | "event" | "address"
    RelayUrl    { url: String },
    Nip05Shape  { identifier: String },
    FreeText    { text: String },
}

pub struct ResolvedInput {
    pub raw: String,
    pub kind: ResolvedInputKind,
}
```

#### Classification output

```rust
// nmp-core::substrate::intent::target
pub struct InputIntentRequest {
    pub input: String,
    pub scopes: Vec<InputScopeId>,
    pub text_targets: TextSearchTargets,
}

pub enum InputIntentTarget {
    DirectRef  { uri: String },
    Nip05      { identifier: String },
    RelayUrl   { url: String },
    TextQuery  { request_json: String },    // opaque serialized nmp_nip50::SearchRequest
    Registered { payload_json: String },    // opaque recognizer payload
}

pub struct InputIntentCandidate {
    pub scope:  InputScopeId,
    pub target: InputIntentTarget,
}

pub enum InputIntentRejection {
    SecretLike,                               // carries NO copy of the input — never logged
    Unparseable,
    UnregisteredScope { scope: InputScopeId },
    DisallowedScope   { scope: InputScopeId },
}

pub enum InputIntentClassification {
    Candidates(Vec<InputIntentCandidate>),
    Rejection(InputIntentRejection),
}
```

Key invariants:
- `SecretLike` is returned for `nsec`, `nostr:nsec`, and `ncryptsec` inputs and
  carries **zero** copy of the input — it is never logged, stored, or echoed.
- A valid reference whose entity class is excluded by the app's requested scopes →
  `Rejection(DisallowedScope)` (not silently dropped).
- `CacheOnly` is **not** a `SearchTargets` / `TextSearchTargets` variant. Cache-only
  behavior is implicit: when relay resolution produces an empty set (no kind:10007
  list, no app-default, or `Explicit([])`) the search fanout is skipped and only
  cache results are returned. Apps that want typeahead-only search pass
  `TextSearchTargets::Explicit(vec![])`.

#### Recognizer trait

```rust
pub trait InputScopeRecognizer: Send + Sync {
    fn scope(&self) -> InputScopeId;
    /// Pure/sync/IO-free — runs inside `classify()`.
    fn recognize(&self, input: &ResolvedInput) -> Option<InputIntentTarget>;
    /// Pure/sync/IO-free — called for free-text inputs.
    fn text_candidate(&self, free_text: &str, targets: &TextSearchTargets) -> Option<InputIntentTarget>;
}
```

#### Registrar + registry

```rust
// nmp-core::substrate::intent::registry
pub const INPUT_SCOPE_LEDGER_SEAM: &str = "input_scope";

pub trait InputScopeRegistrar {
    fn register_input_scope(&self, recognizer: Arc<dyn InputScopeRecognizer>);
}

pub enum InputScopeDisposition { Installed, YieldedToExisting }

pub struct InputScopeRegistry { /* Mutex<Vec<Arc<dyn InputScopeRecognizer>>> */ }
impl InputScopeRegistry {
    pub fn new() -> Self;
    pub fn register(&self, r: Arc<dyn InputScopeRecognizer>) -> InputScopeDisposition;
    pub fn recognizers(&self) -> Vec<Arc<dyn InputScopeRecognizer>>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}
impl InputScopeRegistrar for InputScopeRegistry { /* yielding-default on dup scope id */ }
```

`InputScopeRegistrar` is added to the `AppHost` super-trait + blanket impl
(`crates/nmp-core/src/substrate/app_host/mod.rs`), following the same pattern as
`SearchScopeRegistrar`. The registrar is separate from `SearchScopeRegistrar`; they
share the namespaced-label vocabulary and composition-ledger seam style but are
distinct registry instances with distinct seam names (`"input_scope"` vs
`"search_scope"`). No `linkme`/`inventory` implicit activation.

#### Orchestrator (`nmp-intent`)

```rust
// crates/nmp-intent/src/lib.rs
#[must_use]
pub fn classify(
    req: &InputIntentRequest,
    recognizers: &[Arc<dyn nmp_core::substrate::InputScopeRecognizer>],
) -> InputIntentClassification
```

Pure/sync/IO-free. Frozen precedence:

1. Secret reject — `Nip19Entity::Nsec` or `nostr:nsec` / `ncryptsec` prefix → `Rejection(SecretLike)`.
2. NIP-19/21 ref — via `resolve_open_uri`; `nostr.ref` synthetic scope; if ref's
   entity class not in allowed scopes → `Rejection(DisallowedScope)`.
3. Relay URL — `ws://` / `wss://` normalised → `InputIntentTarget::RelayUrl`.
4. NIP-05 shape — `name@domain` shape (pure parse, **no HTTP**) → `InputIntentTarget::Nip05`.
5. Registered recognizers — snapshot from registry, first match wins.
6. Free-text → `InputIntentTarget::TextQuery` via `nmp_nip50::SearchRequest` JSON.
7. Refusals — `DisallowedScope` / `UnregisteredScope` when no scope matches.

#### NIP-05 crate (`nmp-nip05`)

NIP-05 HTTP resolution is IO and lives outside `classify()`:

```rust
// crates/nmp-nip05/src/parse.rs (pure)
pub fn parse_nip05(identifier: &str) -> Option<(String, String)>   // (name, domain)

// crates/nmp-nip05/src/lib.rs (IO layer)
pub struct ResolveNip05Command {
    pub name: String,
    pub domain: String,
    pub correlation_id: Option<String>,
}
impl nmp_core::substrate::ProtocolCommand for ResolveNip05Command {
    fn run(self: Box<Self>, ctx: &mut ProtocolCommandContext<'_>)
        -> Result<(), ProtocolCommandError>;
    // Spawns a thread → blocking ureq HTTP GET /.well-known/nostr.json?name=<name>
    // → resolved pubkey → ActorCommand landing via RefNamespace::Profile resolve-ref seam.
}
```

Mirrors the `nmp-nip57::FetchLnurlInvoice` ProtocolCommand pattern. HTTP is
blocked behind the `native` Cargo feature (mirroring `nmp-nip57`). The resolved
pubkey is delivered back to the actor via the existing
`nmp_app_resolve_ref(RefNamespace::Profile)` seam, not through a new surface.

### 3.6 Search FFI surface

**This surface lives in `nmp-nip50` (higher-order crate), not in `nmp-core`.**
The core substrate contributes only the `InterestShape.search` wire-filter field.
Everything below — query types, relay selection from kind:10007, cache scan,
result projection, dedup, and ranking — is owned by `nmp-nip50` and consumed
from `SearchRelayListProjection` (in `nmp-nip51`). Blocked-relay subtraction is
applied by `nmp-nip50` using the generic `blocked_relays()` resolver method.

The search surface consumes text search only. It is not the generic "whatever
the user typed" parser: direct Nostr references, NIP-05 identifiers, relay
URLs, and group targets are classified by the input-intent front door below
and routed to their existing loaders before any NIP-50 fanout is considered.

```rust
pub enum SearchScope {
    /// kind:0 events. Cache-side: scans name, display_name, about, nip05.
    Users,
    /// kind:30023 long-form. Cache-side: scans title, summary,
    /// first 4 KB of content.
    LongForm,
    /// Caller-specified kinds. Cache-side scan disabled.
    Kinds(std::collections::BTreeSet<u32>),
    /// Power-user escape hatch — caller builds the full InterestShape.
    /// The `search` field is filled in by `nmp-nip50`; `class_hint` must not
    /// be `EventClass::Search` (that variant no longer exists).
    Custom(InterestShape),
}

pub enum SearchTargets {
    /// Use the active account's NIP-51 kind:10007 list. If the list is
    /// empty or missing, fall back to the app-provided default search
    /// relays (`DefaultRelayLists::search`, §3.6). If both are empty,
    /// no relay REQ is emitted — only cache results are returned.
    UserPreferred,
    /// Caller pins a relay set. Blocked-relay filter still applies.
    Explicit(Vec<RelayUrl>),
    /// Skip the active account list and use the app-provided default search
    /// relays directly. If empty, only cache results are returned.
    AppDefault,
    /// Cache only — no network. Returns immediately with whatever the
    /// local substrate scan finds. Useful for inline typeahead UI.
    CacheOnly,
}

pub struct SearchQuery {
    pub query: String,
    pub scope: SearchScope,
    pub targets: SearchTargets,
    pub limit: Option<u32>,
}

pub struct SearchResultView {
    pub call_id: SearchCallId,
    /// Cache-side matches available synchronously at view-creation time.
    /// Sorted by relevance heuristic (substring start position, then
    /// `created_at` desc).
    pub cache_hits: Vec<SearchHit>,
    /// Relay matches appended as they arrive, deduplicated by event_id.
    /// First-arrival wins (whether cache or any relay).
    pub relay_hits: Vec<SearchHit>,
    /// Per-relay status — which relays have replied, EOSEd, errored.
    pub relay_status: BTreeMap<RelayUrl, SearchRelayStatus>,
}

pub struct SearchHit {
    pub event_id: EventId,
    pub author: Pubkey,
    pub kind: u32,
    pub created_at: u64,
    pub matched_field: SearchField,   // Name | About | Title | Body | …
    pub snippet: String,              // ~80 chars around the match
    /// Single source — the path that delivered the event first.
    /// First-arrival-wins per the dedupe semantics (`cache-search.md` §7.2).
    pub source: SearchHitSource,
}

pub enum SearchHitSource {
    Cache,
    Relay(RelayUrl),
}
```

The FFI surface is one function:

```rust
pub fn open_search(query: SearchQuery) -> SearchResultView
```

ADR-0071 decision 15 defines registered input scopes; only free text becomes `SearchQuery`.

### 3.7 Kernel-init defaults

```rust
pub struct DefaultRelayLists {
    pub search: Vec<RelayUrl>,
    pub drafts: Vec<RelayUrl>,
    pub wiki: Vec<RelayUrl>,
    // future: dm, etc.
}

// passed to Kernel::new alongside existing config:
pub fn build_kernel(/* ... */, defaults: DefaultRelayLists) -> Kernel { /* ... */ }
```

Apps choose the v1 fallbacks for each class. Empty `Vec` means "no
fallback" — class routing falls all the way through to NIP-65 (for
publishes) or to the four-lane planner (for subscribes).

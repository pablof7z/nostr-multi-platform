# Intent-classed routing + NIP-50 search — design

> **Status:** Design draft. No code yet.
> **ADR:** `docs/decisions/0020-intent-classed-routing-and-search.md`.
> **Date:** 2026-05-18.

## 1. Goals

1. Apps call one function to search users / long-form / arbitrary kinds,
   get cache hits synchronously, and stream relay hits as they arrive.
2. The kernel knows which relay class an event belongs to and routes
   accordingly, without app code naming relay URLs.
3. NIP-51 lists with routing semantics become live routing inputs,
   observed and applied the same way kind:10002 is today:
   - **10006** blocked relays (global filter).
   - **10007** search relays → `EventClass::Search`.
   - **10013** draft relays (nip44-encrypted) → `EventClass::Draft`.
   - **10102** good wiki relays → `EventClass::Wiki` (publisher-keyed).
4. The diagnostic UI sees every class-routed decision as a distinct
   lane — no silent "the kernel just did something."

## 2. Non-goals

- A general full-text search engine. Cache-side search is opportunistic
  string scanning, not Lucene.
- DM routing (NIP-17 / kind:10050). Defer to its own ADR; this design
  reserves the `EventClass::DM` variant and decodes kind:10050 into the
  fact stream without consuming it.
- NIP-72 communities, NIP-90 DVMs, kind:30002 named relay sets — all
  default to `EventClass::Other` / NIP-65 routing in v1.
- Good wiki authors (kind:10101) — content allowlist, not relay routing.

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
    /// (publisher-keyed; see §4.1).
    Wiki,
    /// kind:4 / kind:14 — direct messages. Reserved variant; routing
    /// wiring (kind:10050 / NIP-17) is deferred to its own ADR.
    DM,
    /// NIP-29 group-messaging kinds. Kept for diagnostic clarity. NEVER
    /// participates in `class_relays`; NIP-29 events use the existing
    /// `InterestShape::relay_pin` lane (ADR-0012).
    GroupMessage,
    /// Search REQs — not an event class on the wire, but used by the
    /// planner to pick the search-relay set.
    Search,
    /// Anything not enumerated above. Falls through to NIP-65 routing.
    Other,
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
    /// Used by: Search, Draft.
    Personal,
    /// Publisher's NIP-51 list, consulted per author at compile time.
    /// Used by: Wiki.
    PublisherKeyed,
    /// Existing relay_pin lane (ADR-0012). Used by: GroupMessage.
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

    /// NIP-50 search string. When `Some`, the planner routes via search
    /// relays (kind:10007) and emits the `search` field on the wire
    /// filter. Refuses to merge with shapes that have a different
    /// `search` value (new merge Rule 10).
    pub search: Option<String>,

    /// Optional class hint set by the consumer. When `None`, the planner
    /// derives the class from `kinds` via `EventClass::from_kind`. When
    /// `Some`, the value wins — used by `SearchScope::Custom` and by
    /// apps that emit ambiguous kinds.
    pub class_hint: Option<EventClass>,
}
```

Both fields are `Option` so existing call sites keep working with
`InterestShape { ..Default::default() }`.

### 3.3 Extended `OutboxResolver`

```rust
pub trait OutboxResolver: Send + Sync {
    // existing
    fn write_relays(&self, author: &Pubkey) -> Vec<RelayUrl>;
    fn read_relays(&self, author: &Pubkey) -> Vec<RelayUrl>;

    /// Personal NIP-51 routing — active account context, no author.
    /// Used for classes whose NIP-51 list is intrinsically self-keyed
    /// (Search: "where I search," Draft: "where I store my drafts").
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

### 3.5 Search FFI surface

```rust
// nmp-core::search

pub enum SearchScope {
    /// kind:0 events. Cache-side: scans name, display_name, about, nip05.
    Users,
    /// kind:30023 long-form. Cache-side: scans title, summary,
    /// first 4 KB of content.
    LongForm,
    /// Caller-specified kinds. Cache-side scan disabled.
    Kinds(std::collections::BTreeSet<u32>),
    /// Power-user escape hatch — caller builds the full InterestShape.
    /// `search` and `class_hint = Some(Search)` are filled in by the kernel.
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
    /// First-arrival-wins per the dedupe semantics (§7.2).
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

### 3.6 Kernel-init defaults

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

## 4. Planner integration

### 4.1 Per-author class-routing partition

The user's framing: *"my list is about where I publish and where I read
when not specifying authors; bob's list is when I want to see what wiki
bob published. If the REQ has authors:[bob, alice], it uses bob's 10102
for kinds:[30818], authors:[bob], alice's 10102 for kinds:[30818],
authors:[alice]."*

This drives the partition logic:

```
interest: shape = { kinds: [30818], authors: [bob, alice] }
       → class = Wiki (from_kind(30818))
       → routing_family = PublisherKeyed
       → split per author:
           sub-shape A: { kinds: [30818], authors: [bob] }
                        relays = class_relays_for_author(Wiki, bob)
                                 .unwrap_or_else(|| nip65.write_relays(bob))
           sub-shape B: { kinds: [30818], authors: [alice] }
                        relays = class_relays_for_author(Wiki, alice)
                                 .unwrap_or_else(|| nip65.write_relays(alice))
```

When `class.routing_family() == Personal`, the partition does not split
by author — the active account's `class_relays_personal(class)` answers
the whole interest. When `class.routing_family() == None`, the planner
skips class routing entirely and runs the existing four-lane partition.

This is implemented as a new partition case `case_g_class_routed` that
runs after `case_a_authors` and before `case_e_relay_pinned`. NIP-29
events still take the `relay_pin` lane because their `EventClass::GroupMessage`
has `routing_family == RelayPin`, and the partition cases gate on family.

### 4.2 New merge rule

**Rule 10 — `search` equality.** Two shapes refuse to merge unless their
`search` fields are equal (including both being `None`). Reasoning:
broadening a search would silently change semantics; narrowing would
lose results.

### 4.3 Blocked-relay post-filter (fail loud)

After all partition cases run and the per-relay plan is assembled, the
compiler subtracts `outbox.blocked_relays()` from every `RelayPlan`'s
relay URL set. If a relay is partially blocked, the plan emits a
diagnostic `RelayBlocked { url, removed_interests }` so the UI can
explain the shrinkage.

**If every relay in the plan is subtracted, the compiler returns
`PlannerError::AllRelaysBlocked`** — no silent empty plan. The publish
engine maps the equivalent error to `PublishOutcome::AllRelaysBlocked`.
This is a deliberate fail-loud choice (ADR-0020 decision 7); the UX
must surface this clearly because a user who blocked all their relays
by mistake will otherwise see nothing happen.

### 4.4 Lazy 10102 fetch lifecycle

When `case_g_class_routed` encounters a Wiki interest naming an author
whose kind:10102 hasn't been fetched yet:

1. The planner returns the current plan with that author's lane routed
   via NIP-65 fallback (so reads aren't blocked on the fetch).
2. The resolver enqueues a one-shot kind:10102 fetch for the author
   against the active account's read relays.
3. When the fetch completes (or EOSEs empty), the resolver invalidates
   the planner's cache for the affected interest, triggering a
   recompile that re-routes to the now-known wiki relays.
4. The kind:10102 subscription is kept alive (replaceable, tailing)
   for as long as any class-routed interest references the author.
   When the last interest ends, the subscription closes and the
   author's entry is evicted from the per-author fact cache.

This keeps the working set bounded by active view lifetimes.

## 5. NIP-51 fact stream

| NIP-51 kind | Class / role               | Resolver method consumes it            | Encrypted? | Per-author? |
|-------------|----------------------------|----------------------------------------|------------|-------------|
| 10006       | blocked (global filter)    | `blocked_relays()`                     | no         | no          |
| 10007       | `Search`                   | `class_relays_personal(&Search)`       | no         | no          |
| 10013       | `Draft` (NIP-37)           | `class_relays_personal(&Draft)`        | **yes** (nip44 to self) | no |
| 10102       | `Wiki` (NIP-54)            | `class_relays_for_author(&Wiki, _)`    | no         | **yes**     |
| 10050       | DM (NIP-17)                | decoded only; routing deferred         | no         | no          |
| 30002       | named — see §5.1           | not consumed in v1                     | n/a        | n/a         |

### 5.1 Kind:30002 named relay sets — deferred

Named sets are addressable per `d` tag. No canonical convention maps `d`
values to `EventClass` variants. v1 doesn't consume them; apps that need
named-set routing use `PublishTarget::Explicit` after reading the list
themselves via the existing nmp-nip51 view. A future ADR may add a
runtime `(d_value, EventClass)` binding API.

### 5.2 Fact-stream wiring

```rust
pub struct Nip51RoutingFacts {
    pub search: Vec<RelayUrl>,                            // from kind:10007
    pub blocked: BTreeSet<RelayUrl>,                      // from kind:10006

    /// From kind:10013 (NIP-37 draft relays). Encrypted; the resolver
    /// surfaces it only after NIP-44 self-decryption succeeds.
    pub drafts: Vec<RelayUrl>,

    /// From kind:10050. Decoded only — `class_relays_personal(&DM)`
    /// does not consume this yet. Field exists so the future DM ADR
    /// can land without re-plumbing the fact stream.
    pub dm: Vec<RelayUrl>,

    /// Per-author kind:10102 lists. Lazy-populated when a class-routed
    /// Wiki interest first names the author. Evicted when the last
    /// such interest ends.
    pub wiki_per_author: HashMap<Pubkey, Vec<RelayUrl>>,
}
```

Wiring steps:

1. Register the kinds with the `nmp-nip51` decoder — add 10006, 10007,
   10013, 10050, 10102 to `ALL_KINDS`.
2. **Subscribe to the personal lists** (10006, 10007, 10013, 10050) as
   part of the active-account boot sequence, alongside the existing
   kind:10002 NIP-65 fetch. These four are replaceable, so each is
   exactly one tailing subscription.
3. **For kind:10013 only**, decrypt `.content` via the active signer's
   NIP-44 self-decryption. The decrypted blob contains the `"relay"`
   tags. Parsing is identical post-decryption.
4. **Per-author 10102 fetches** happen lazily, driven by the planner's
   `case_g_class_routed` partition (§4.4). The resolver owns the
   per-author subscription lifecycle.
5. Project the decoded relay URLs into the `Nip51RoutingFacts` slice.

The hot path (planner partition) reads from `Nip51RoutingFacts`
allocation-free (D8); projection from raw events happens at fact-
ingestion time.

## 6. Diagnostic discipline

The four-lane model in
`docs/design/subscription-compilation/diagnostics.md` §5.0 stretches to
**five lanes**:

1. NIP-65
2. Hint
3. Provenance
4. UserConfigured (incl. indexer)
5. **ClassRouted** — sourced from NIP-51 lists; carries
   `class: EventClass` and `via: Personal | PublisherKeyed(author)`.

Plus one global subtractive filter:

- **Blocked** — events removed because the target relay is in kind:10006.
  Surfaced as a separate diagnostic stream, not a routing lane (it never
  *adds* a relay, only subtracts). When the subtraction empties a plan,
  the planner errors with `AllRelaysBlocked` rather than continuing.

The diagnostic doc (`docs/design/subscription-compilation/diagnostics.md`)
is updated as part of P3's deliverable — same PR that introduces the
`ClassRouted` role tag.

## 7. Cache-side search

### 7.1 Scan scopes

- **`SearchScope::Users` (kind:0).** Linear over the kind:0 substrate
  slice. Profile cache is small (≤ ~10k entries in practice). Match
  against lowercased `name`, `display_name`, `about`, `nip05`. Substring
  match only — no fuzzy, no stemming.
- **`SearchScope::LongForm` (kind:30023).** Linear over the long-form
  substrate slice, scanning `title` tag, `summary` tag, and body prefix
  (capped at the first 4 KB of `.content`).

Both run synchronously inside `open_search` before returning the view.
Wall-clock budget: ≤ 5 ms for 10k profiles, ≤ 20 ms for 1k articles.
Above those sizes, switch to a proper inverted index in v2.

### 7.2 Dedupe and merge

Dedupe key is `event_id`. **First arrival wins**, whether the path is
cache or any of the N fan-out relays. Each `SearchHit` records a single
`source: SearchHitSource` — the first path that delivered the event.
Duplicate arrivals are silently dropped.

Ordering in the view:

- `cache_hits` is populated synchronously, sorted by relevance heuristic
  (substring start position, then `created_at` desc). This is what the
  app renders before any relay responds.
- `relay_hits` is appended in arrival order, deduplicated against
  `cache_hits` and against itself. Apps may resort client-side; the
  kernel provides arrival order as the canonical stream.

### 7.3 Fanout policy

`SearchTargets::UserPreferred` fans REQ out to **all** relays in the
user's kind:10007 list — no cap. No NIP-11 `supported_nips` probing;
relays that don't implement NIP-50 surface as zero-result lanes in the
per-relay diagnostic. If kind:10007 is missing/empty, fall back to
`DefaultRelayLists::search`; if that's also empty, only cache results
are returned.

## 8. Migration / rollout plan

| Phase | Deliverable                                                         | Gate                                                |
|-------|---------------------------------------------------------------------|-----------------------------------------------------|
| P1    | `EventClass` + `from_kind` + `RoutingFamily` + unit tests           | All existing tests still pass.                      |
| P2    | `InterestShape::{search, class_hint}` + Rule 10 + partition case_g  | Determinism gate green; five-lane diagnostic asserts.|
| P3    | NIP-51 routing facts substrate slice + two-method resolver trait    | Five-lane diagnostic doc updated; NIP-44 decrypt for 10013 wired through M6 signer; lazy 10102 fetch lifecycle test green. |
| P4    | `SearchQuery` FFI + `SearchResultView` + cache scan + relay fanout  | Integration test against `search.nos.lol` for kind:0 + kind:30023. |
| P5    | `PublishTarget::Auto` upgrade + blocked-relay filter + fail-loud    | Audit: every existing call site that emits a non-`Other` class still publishes correctly; Chirp's M11.5 exit gate adds "no event reached a blocked relay" assertion. |

Each phase is its own commit / PR. P3 is the hinge — once NIP-51 becomes
a fact stream and the resolver trait expands, P4 and P5 are mechanical.

## 9. FFI / app-developer ergonomics

```swift
// Searching users — one call, streaming view.
let view = kernel.openSearch(.init(
    query: "satoshi",
    scope: .users,
    targets: .userPreferred,
    limit: 50
))
for await delta in view.deltas {
    // render new hits as they arrive
}

// Publishing a draft — no app-side relay knowledge.
kernel.publish(event: draftEvent, target: .auto)
// Kernel: kind 31234 → EventClass::Draft (Personal family)
//                    → class_relays_personal(Draft)
//                    → user's decrypted kind:10013 list
//                    → subtract blocked_relays() → dispatch.

// Publishing a checkpoint — same class, same routing.
kernel.publish(event: checkpoint, target: .auto)
// Kernel: kind 1234 → EventClass::Draft → same kind:10013 relays.

// Publishing a wiki entry — publisher-keyed routing.
kernel.publish(event: wikiEvent, target: .auto)
// Kernel: kind 30818 → EventClass::Wiki (PublisherKeyed family)
//                    → class_relays_for_author(Wiki, signer.pubkey)
//                    → my kind:10102 list (publishing as self)
//                    → subtract blocked_relays() → dispatch.

// Reading multi-author wikis — per-author partition.
let interest = LogicalInterest {
    shape: InterestShape {
        kinds: [30818], authors: [bob, alice], ..
    }, ..
}
// Kernel: splits the interest. Bob's events route via bob's 10102,
// alice's via alice's 10102. Each author's 10102 fetched lazily.
```

The "app authors forget" failure mode the user flagged is closed by:

- `Auto` being class-aware from day one — apps opt out, not in.
- `blocked_relays` enforced kernel-side on every target.
- `EventClass::from_kind` being a static table — apps never look up
  "which relay class does kind 31234 belong to?" themselves; the parent
  draft (31234) and its checkpoints (1234) share routing automatically.

## 10. Test surface (M-gate criteria)

- **Unit:** `EventClass::from_kind` covers every kind in the codebase's
  `kind` constants. **Checkpoint↔parent class equivalence:**
  `from_kind(1234) == from_kind(31234) == EventClass::Draft`. Rule 10
  merge refusal. Blocked-relay subtraction. `RoutingFamily` mapping for
  every variant.
- **NIP-44 decryption gating:** kind:10013 surfaces only when a signer
  is attached and self-decrypt succeeds.
- **Per-author Wiki partition:** interest with `authors=[bob, alice],
  kinds=[30818]` compiles into two distinct sub-shapes, one routed via
  bob's 10102 and one via alice's. Property-test for N authors.
- **Lazy 10102 lifecycle:** first interest naming `bob` triggers a
  one-shot kind:10102 fetch; second interest hits the cache; closing
  the last interest evicts bob from `wiki_per_author`.
- **Fail-loud blocked:** plan with every relay in `blocked_relays()`
  returns `PlannerError::AllRelaysBlocked`; `PublishEngine` maps to
  `PublishOutcome::AllRelaysBlocked`.
- **Integration:** real-relay search test against `search.nos.lol` for
  kind:0 and kind:30023. Mirror of
  `crates/nmp-testing/tests/real_relay_outbox.rs`.
- **Diagnostic:** five-lane assertion fixture covering one example per
  lane plus one blocked-relay subtraction and one `AllRelaysBlocked`
  failure path.

## 11. Future work

Things deliberately out of scope for v1 but documented as known
extension points:

- **DM routing (NIP-17 / kind:10050).** Variant reserved; fact stream
  decoded. Own ADR.
- **Named relay sets (kind:30002).** No class binding in v1. Future
  ADR may add a runtime `(d_value, EventClass)` registration API.
- **NIP-72 communities, NIP-90 DVMs.** Default to `EventClass::Other`
  today. Future ADRs if usage demands.
- **Good wiki authors (kind:10101).** Author allowlist, not relay
  routing. Wiki views may consume independently of this design.
- **Cross-account routing for personal-class lists.** Currently only
  Wiki uses publisher-keyed routing. If a future NIP defines a
  per-author Search or Draft list, the trait already supports it —
  add a new `EventClass` and its `RoutingFamily::PublisherKeyed`
  mapping.
- **Proper inverted index** for cache-side search when corpus
  exceeds the linear-scan budget (~10k profiles, ~1k articles).
- **NIP-11 probing** of search relays if blind fanout proves too noisy
  in practice.

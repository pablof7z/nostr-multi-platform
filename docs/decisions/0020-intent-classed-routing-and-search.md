# ADR-0020 — Intent-classed routing + NIP-50 search

> **Status:** Implemented. `EventClass`, `RoutingSource::ClassRouted`, and the
> bounded `InterestShape.search` field all shipped
> (`crates/nmp-core/src/substrate/routing.rs`,
> `crates/nmp-core/src/substrate/view.rs:54`).
> **Decision 15 (input-intent resolver) — implemented** (`feat/input-intent-resolver-1804`,
> commit `956f79f0`): noun-free `InputScopeRegistrar` + `InputScopeRegistry` in
> `nmp-core::substrate::intent`; orchestrator crate `nmp-intent` (`classify()` pure/sync);
> NIP-05 reverse-lookup crate `nmp-nip05` (async `ResolveNip05Command`);
> `NmpApp::input_scope_registry` field + `app_config_intent.rs` FFI setter.
> `classify()` precedence is frozen: (1) secret-reject (nsec/ncryptsec, no echo),
> (2) NIP-19/21 ref via `resolve_open_uri`, (3) relay URL, (4) NIP-05 shape (pure parse,
> no HTTP), (5) registered recognizers, (6) free-text → `nmp_nip50::SearchRequest`,
> (7) refusals. `CacheOnly` is implicit (empty relay resolution → FTS-only), NOT a
> `SearchTargets` variant. See `docs/design/intent-routing/types.md` §3.5.
> **Companion:** `docs/architecture/crate-boundaries.md` §3.1 (routing lanes),
> §5 (routing contract), §8 (protocol-crate ownership).
> **Depends on:** ADR-0021 (relay roles — Indexer + AppRelay). ADR-0021
> promotes `Indexer` to a top-level `RoutingSource` variant and adds
> `AppRelay`; this ADR's `ClassRouted` lane is sibling to those.
> **Extends:** ADR-0012 (`relay_pin` and the third routing lane). The
> per-interest `relay_pin` field stays for NIP-29; this ADR adds a kernel-
> resident classifier driven by event kind and NIP-51 lists, plus a
> per-author dimension for publisher-keyed classes.

> **Ownership split (Issue #1561).** `nmp-planner` / `nmp-core` own the
> generic search/index **seams**: the bounded `InterestShape.search`
> wire-filter field, filter serialization, merge equality, generic
> blocked-relay subtraction, the `substrate::search` module
> (`SearchScopeRegistrar` / `SearchScopeProvider` / `SearchScopeRegistry` —
> a noun-free registry that protocol crates populate at composition time;
> the kernel compiles providers into `nmp-store::CompiledIndexSpec` and
> installs them via the cache-serve hook), and the account-config self-kind
> bootstrap including kind:10007 in `SELF_KINDS_TAILING`
> (`crates/nmp-core/src/kernel/requests/startup.rs`). NIP-50 query semantics,
> app-facing search actions/views, result ranking, relay selection from
> kind:10007, and search-hit projection live in the owning search module
> (`nmp-nip50`). NIP-51 kind:10007 relay-list parsing lives in `nmp-nip51`
> (`SearchRelayListProjection`). There is **no** `EventClass::Search` variant
> and no search *routing class*: search routes purely via the generic
> `InterestShape.search` wire-filter field. The noun-free `substrate::search`
> registry seam and the kind:10007 account-config bootstrap are the extent of
> search logic that lives in core — no NIP-50 query semantics or relay-selection
> policy reside there.
> **Input-intent amendment (2026-06-22).** User-entered text is not
> automatically a NIP-50 search. A framework-owned input resolver first
> classifies raw strings through generic parsers and namespaced scopes
> registered by composed NMP/app modules, or rejects invalid/secret input.
> Direct references route through the existing open/resolve-reference path;
> only free-text search requests enter the `nmp-nip50` search surface.
> **Higher-order search amendment (2026-06-22).** The `EventClass::Search`
> variant and `RoutingFamily::Personal` usage for search are withdrawn. Search
> routing does not need and does not use a planner routing class. The
> investigation confirmed: search routes today purely via the generic
> `InterestShape.search` wire field + the kind-agnostic ingest chokepoint
> (ADR-0057) + generic blocked-relay subtraction. The planner's
> `EventClass::Search` / `RoutingFamily::Personal` / `case_g_class_routed`
> machinery is unnecessary for search and was never implemented (only
> `EventClass::Search` existed, used solely in diagnostic JSON). Relay
> selection from kind:10007 is performed at the higher layer by
> `SearchRelayListProjection` (in `nmp-nip51`), with app-default fallback and
> blocked-relay subtraction applied in `nmp-nip50`. `EventClass` is retained
> for Draft / Wiki / GroupMessage routing only; Search is no longer a member.
> See also: issues #1561 (search anchoring), #1804 (input-intent resolver),
> #1794 (relay routing), #1811 (cache FTS inverted index).

## Context

Two requests motivated this ADR, both surfaced 2026-05-18 with a user-pushed
update to NIP-37 and NIP-51 landing mid-discussion:

1. **Search.** Apps want to search users (kind:0 `name`/`about`) and long-form
   content (kind:30023 body). NIP-50 specifies the wire shape (a `search`
   string in the filter). The user's preferred search relays are enumerable
   from NIP-51 kind:10007. The generic field and `nmp-nip50` primitives now
   exist; the remaining product gap is the framework front door that decides
   which registered intent/search scope handles raw user input, or whether it
   falls through to free-text search.

2. **Specialized-relay routing.** NIP-51 enumerates several relay lists
   carrying routing semantics: kind:10006 blocked relays, kind:10007 search
   relays, **kind:10013 draft relays (nip44-encrypted, used by NIP-37)**,
   **kind:10102 good wiki relays (used by NIP-54)**, kind:10050 DM relays
   (NIP-17), and arbitrary user-defined kind:30002 relay sets. NIP-37
   defines drafts as kind:31234 (parent) plus kind:1234 (checkpoints —
   encrypted snapshots that reference the parent via an `["a", …]` tag).
   NIP-54 defines wiki entries as kind:30818 with `kind:818` merge requests
   and `kind:30819` redirects sharing routing. App authors will routinely
   forget to route by purpose; the kernel is the right place to enforce it
   because — per D3 — outbox routing is automatic and view modules never
   name relay URLs.

Both share infrastructure: a new `EventClass` classification + a NIP-51
fact stream feeding the `OutboxResolver` + a planner partition case that
splits per-author class routing in the same way NIP-65 outbox routing
already does.

## Decision

The fifteen calls below are the v1 contract. The companion design doc spells
out the types, the planner integration, and the rollout.

**(1) Add NIP-50 search through a protocol-owner split.** New bounded
`search: Option<String>` field on `InterestShape` is a generic wire-filter
primitive owned by the planner/core substrate. A `SearchScope` /
`SearchTargets` FFI surface, query semantics, result projection/ranking, and
cache-first behavior belong to the owning search module (expected:
`nmp-nip50`) and must consume substrate primitives rather than place NIP-50
policy in `nmp-core`. Per-call relay selection is driven by the user's
NIP-51 kind:10007 list when present; parsing that list is owned by
`nmp-nip51`.

**(2) Add `EventClass` as a kernel-resident classification.** Built-in
kind → class table: PublicNote / Profile / RelayList / LongForm / Draft
(covers 1234 + 31234) / Wiki (covers 818 + 30818 + 30819) / DM (reserved,
routing deferred) / GroupMessage (kept for diagnostics, never routes
through `class_relays` — uses the existing `relay_pin` lane) / Other.
**Note (2026-06-22 amendment):** `EventClass::Search` was over-reach and
has been removed. Search does not need a planner routing class. The
`InterestShape.search` wire-filter field is sufficient; relay selection is
higher-order, performed by `nmp-nip50` via `SearchRelayListProjection` from
`nmp-nip51`. If `EventClass` is extended in the future, Search remains out
of scope unless a planner-level routing need is proven.

**(3) Two-shape resolver trait.** NIP-51 lists fall into two routing
families, and the `OutboxResolver` trait grows methods for each:
- **Personal lists** (active-account context, no author argument):
  drafts (10013), blocked (10006). Method:
  `class_relays_personal(class) -> Option<Vec<RelayUrl>>`.
  **Note:** kind:10007 search relays are NOT consumed here; they are read
  by `SearchRelayListProjection` in `nmp-nip51` and consumed by `nmp-nip50`
  directly — this is part of the higher-order search model (2026-06-22
  amendment).
- **Publisher-keyed lists** (per-author, consulted for the author of the
  events being routed): wiki (10102) is the only v1 entry. Method:
  `class_relays_for_author(class, author) -> Option<Vec<RelayUrl>>`.
- Globally-applied: `blocked_relays() -> BTreeSet<RelayUrl>` as a
  post-planning filter on every plan and every publish target.

The two methods exist because the user's framing partitions cleanly:
*"my list is about where I publish and where I read when not specifying
authors; bob's list is when I want to see what wiki bob published."*

**(4) Per-author class-routing partition in the planner.** Multi-author
interests with a publisher-keyed class split per author: for
`authors=[bob, alice], kinds=[30818]`, the planner emits two routing
decisions — bob's events via bob's 10102, alice's via alice's 10102.
Reuses the existing `case_a_authors` partition pattern (NIP-65 already
does this for write-relay outbox).

**(5) NIP-51 becomes a routing fact stream.** The crate-level disclaimer
in nmp-nip51 (*"never feeds routing"*) is lifted for kinds 10006, 10007,
10013, 10102. Kind:10050 decodes but is wired for the future DM ADR
only. Kind:30002 (named sets) has no class binding in v1. Replaceable
list updates flow into the resolver the same way kind:10002 (NIP-65)
does today, plus:
- **kind:10013** requires the active signer's NIP-44 self-decryption
  before its relay tags are usable.
- **kind:10102** is fetched lazily per author — only when a class-routed
  interest names that author. Subscriptions are kept alive for active
  authors, dropped when the last interest for them ends.

**(6) `PublishTarget::Auto` becomes class-aware by default.** No new
variant. The existing `Auto` is upgraded: NIP-51 class routing applies
when a list exists, NIP-65 fallback otherwise, blocked-relay filter
always. Existing call sites (Chirp, gallery, tests) inherit the new
behavior implicitly. P5 of the rollout is an audit, not a migration.

**(7) Blocked-relay filter is fail-loud.** If the post-planning filter
subtracts every relay from a plan, the planner returns
`PlannerError::AllRelaysBlocked` (a new variant). The publish engine
maps it to `PublishOutcome::AllRelaysBlocked` and the subscription path
surfaces a kernel toast. No silent empty plans.

**(8) Search fanout: blind, no cap, app-default fallback.** When
`SearchTargets::UserPreferred` resolves to N relays from kind:10007,
the kernel fans REQ out to all N in parallel without probing NIP-11
`supported_nips` — relays that don't implement NIP-50 surface as
zero-result lanes in the per-relay diagnostic. If kind:10007 is missing
or empty, fall back to the app-provided default list. Same fallback
chain for every class.

**(9) Search merge semantics.** Dedupe key is `event_id`. First arrival
wins, whether from local cache or any relay: a single `SearchHit::source`
records the path that delivered the event. Cache scan is synchronous at
view creation; relay hits are appended as they arrive.

**(10) App-provided defaults at kernel init.** Apps pass a
`DefaultRelayLists { search, drafts, wiki, … }` to kernel construction.
Each field is the v1 fallback when the active user has no corresponding
NIP-51 list. If a field is empty, no class routing for that class —
the planner falls through to NIP-65.

**(11) Routing policy: "all classes when list exists."** If the active
account (for personal-class lists) or the relevant author (for wiki)
has a NIP-51 list whose class maps to an `EventClass`, the kernel
honours it. No specialized-vs-common distinction; apps may still pin
with `PublishTarget::Explicit`.

**(12) Search fanout privacy: "all relays in list."** No per-call
deduplication or subset selection. Apps that want a single-relay-per-call
privacy profile must use `SearchTargets::Explicit(vec![one_relay])`. No
hidden cap.

**(13) `EventClass::GroupMessage` is kept but documented as
non-participating in `class_relays`.** NIP-29 routing remains on the
existing `relay_pin` lane (ADR-0012). The variant exists for
diagnostic clarity ("this plan took the relay_pin lane because the
event was classified GroupMessage") and for the future case where
NIP-29 or a successor gains a NIP-51 list.

**(14) DM routing deferred to its own ADR.** `EventClass::DM` variant
reserved, kind:10050 decoded into the fact stream but no
`class_relays(DM)` lookup wired. A future ADR addresses NIP-17 routing
and gift-wrap relay semantics without re-plumbing this design.

**(15) Input-intent resolution is a separate front door, not NIP-50 search.**
Apps may pass raw user-entered strings to a framework resolver, but the
resolver must classify before it searches. NIP-19/NIP-21 references route to
`open_uri` / `resolve_ref`; NIP-05 identifiers start the NIP-05 lookup path
and then resolve a profile reference; relay URLs normalize into relay metadata
or relay-management views; NIP-29 group targets use the existing `relay_pin`
group lane; only free text becomes a `SearchQuery`. Apps declare allowed
namespaced scopes, and composed crates own their domain semantics
(`nmp-nip29` groups, `nmp-nip50` text search, future `nmp-nip60` wallet
search). NMP owns classifier reuse, routing, relay fanout, blocked-relay
subtraction, diagnostics, and secret rejection.
No app should need to know that `naddr1...` is a direct load while `"pablo"`
is eligible for cache plus NIP-50 text search.

## Consequences

### Wins

- Apps get search via one FFI call with cache-first synchronous results.
- Apps can also hand NMP raw input and select registered scopes without making
  `nmp-core` learn every protocol or app domain.
- Drafts go to draft relays, wikis to (publisher's) wiki relays, without
  app authors needing to know which kinds map to which class.
- Blocked relays are kernel-enforced — no path bypasses them, including
  the existing `Auto` and `Explicit` publish targets.
- NIP-51 lists stop being decorative: they become operationally
  load-bearing the same way kind:10002 is today.
- The default-Auto-becomes-class-aware decision means existing apps gain
  class routing automatically without a per-call-site migration.

### Costs / risks

- **Resolver trait expands non-trivially.** Two new method shapes
  (`_personal` and `_for_author`) plus `blocked_relays`. Every fake
  `OutboxResolver` in tests needs updating. The two-method split is
  the right abstraction for the spec we have, but it's a real surface
  area increase.

- **Per-author 10102 fetches multiply background subscriptions.**
  Subscribing to every wiki-author's kind:10102 is `unique_authors`
  extra background subs. Mitigation is lazy fetch (only fetch when a
  class-routed interest for that author is live) plus eviction when
  the last interest ends. Working-set memory grows with active wiki
  audience but is bounded by view lifetimes.

- **`Auto` becoming class-aware silently changes behavior** for every
  existing publish call site. This is deliberate per decision (6). If a
  Chirp user has somehow configured a wiki-relay list, a kind:30818
  Chirp publish that previously went to NIP-65 outbox will start going
  to their kind:10102 list. P5 audit must check no current code path
  emits a class-routed kind without expecting class routing.

- **Fail-loud on fully-blocked plans** means a misconfigured blocked-
  relay list breaks the user's session. UX must surface
  `AllRelaysBlocked` clearly enough that the user can diagnose. This
  is preferable to silently-no-events but requires UX investment.

- **NIP-44 decryption gating for kind:10013.** The fact-stream
  projection for draft relays depends on ADR-0015 / M6 signer NIP-44
  self-decrypt being available. P3 cannot ship code-complete until that
  capability lands.

- **No NIP-11 probing of search relays.** Users with non-NIP-50 relays
  in their kind:10007 list see dead lanes. Acceptable — per-relay
  diagnostic surfaces this — but worth documenting prominently in the
  FFI docs so app authors aren't surprised by empty result sets.
- **Input resolution adds another public seam.** The seam is justified only if
  it reuses the existing NIP-19/NIP-21, NIP-05, relay, NIP-29, `open_uri`, and
  `resolve_ref` paths instead of becoming a parallel parser or policy engine.

### Non-decisions deferred

- **DM routing** — kind:10050 + NIP-17 gift-wrap semantics get their
  own ADR.
- **Named relay sets (kind:30002)** — no canonical class binding in v1;
  app-specific extensions can come later.
- **NIP-72 communities, NIP-90 DVMs** — default to
  `EventClass::Other` (NIP-65 routing). Future ADRs if usage demands.
- **Good wiki authors (kind:10101)** — content allowlist, not a relay
  list; out of scope for this ADR. Wiki views may consume it
  independently.

## Alternatives considered

- **App-API only (kernel stays passive).** Rejected. The user's framing
  was explicit: *"app developers will always forget to do those checks."*
  Pushing the burden to apps is the failure mode the kernel exists to
  prevent.
- **Single `relay_pin = Some(url)` per interest, app-populated.**
  Rejected. That's ADR-0012, which works for NIP-29 because NIP-29 has a
  canonical pin (the group host). Drafts / wikis / search require the
  kernel to *derive* relay sets from the active account's (or the
  publisher's) NIP-51 lists — that derivation is what's new here.
- **A single `class_relays(class, Option<author>)` method.** Rejected.
  Personal-class lists have no meaningful author argument (it would
  always be `None` or the active account); publisher-keyed lists always
  do. Two methods carry the type-level intent; one method with `Option`
  hides it.
- **NIP-11 probe of search relays.** Rejected per decision (8). Adds
  plumbing for marginal benefit; surfaces in the per-relay diagnostic
  instead.
- **Treat every raw input as NIP-50 search first.** Rejected. Direct Nostr
  references, relay URLs, NIP-05 names, and group identifiers have stronger
  semantics than free-text search and must not fan out as blind search REQs.
- **`AllRelaysBlocked` as a silent empty plan.** Rejected per decision
  (7). The user-facing failure mode of "I blocked all my relays and now
  nothing publishes" is easier to debug when loud.

## References

- ADR-0012 — `relay_pin` and the third routing lane.
- ADR-0015 — M6 signer design (NIP-44 self-decrypt dependency).
- ADR-0021 — Relay roles: Indexer + AppRelay (this ADR's prerequisite).
- ADR-0057 — Unified kind-agnostic ingest chokepoint (search routes through this).
- `docs/architecture/crate-boundaries.md` §3.1 — the seven routing lanes
  (NIP-65, Hint, Provenance, UserConfigured, ClassRouted, Indexer, AppRelay)
  and the worker-vs-planner abstraction split that the as-built routing code
  cites directly.
- `docs/architecture/crate-boundaries.md` §5, §8 — the routing contract and
  the protocol-crate ownership boundary (`nmp-nip50` query, `nmp-nip51`
  list parsing).
- `docs/design/intent-routing.md` — companion design doc (updated to match
  the higher-order search model per the 2026-06-22 amendment).
- Issue #1561 — NIP-50 search anchoring (origin of the ownership split).
- Issue #1804 — Input-intent resolver (front door for raw user text).
- Issue #1794 — Relay routing (blocked-relay subtraction in publish engine).
- Issue #1811 — Cache-side full-text inverted index (`text_search_visit`
  store seam + LMDB FTS sub-databases + crate-registered scope registry).
- NIP-37 (drafts + checkpoints), NIP-50 (search), NIP-51 (lists),
  NIP-54 (wikis), NIP-29 (relay groups), NIP-51 PR #1985 (kind:10086).

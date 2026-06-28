# Internal Machinery

The developer-facing shape is small, but NMP still owns the machinery needed to
make it correct across platforms.

## Read Pipeline

For reads, NMP internally:

```text
opens a live query
  -> compiles source expressions into interests/dependent interests
  -> plans relays with outbox routing or explicit relay pins
  -> records reverse admission/wake indexes for the session
  -> replays cached/store data
  -> admits matching live events into Rust-owned read state
  -> emits bounded typed projections
  -> tears everything down when the handle closes
```

Protocol crates own the meaning of protocol queries. The live query machinery
owns lifecycle, replay ordering, dependency tracking, and teardown.

The current `ObservedProjection` path contains the closest replay-before-live
and scoped-admission invariant. Reuse or narrow that invariant first; do not
preserve tick-polled `ActiveObservedProjection` controller semantics just because
they share the name. The goal is not a parallel read path or a broad new engine.
It is to prove that a small typed descriptor can compile into the safe recipe
feature authors need while deleting polling-based lifecycle repair.

Dynamic source implementations should be consolidated only as far as their
semantics truly match. Start with one private shape reconciler around
observed-projection open/close, keep source-specific reducers local, and promote
a general source-reduction core only after multiple source families prove they
share the same diff, fail-closed, teardown, and dependent-interest rules.

Event-to-session admission is a protected invariant, but reverse indexes are not
accepted just because they sound more sophisticated. The baseline can stay a
scoped observer fanout if it is bounded and proves replay, relay provenance, and
missed-wake correctness. A session family must declare the event/store/source
changes that can wake it and the admission shape that bounds fanout. Add
per-kind, author, tag, relay, or output reverse indexes only when scoped fanout
cannot satisfy that invariant without broad scans, polling, or missed hydration.
The destination is still not a native-owned refresh trigger. Store ingest, relay
delivery, source changes, and mailbox changes should enqueue bounded, coalesced
work for the owning session/output.

## Write Pipeline

For writes, NMP internally:

```text
receives a typed write intent
  -> constructs or finalizes unsigned event data through the owning feature
  -> signs with the selected signer through the capability/signer port
  -> stores the signed event when appropriate
  -> plans publish relays
  -> dispatches to relays
  -> records publish status and errors as state
  -> updates projections through normal ingest/store paths
```

Protocol crates own protocol-specific draft and route policy. App crates compose
protocol flows when product behavior spans protocols.

Existing one-door pieces should remain the substrate: dispatch envelope, action
modules, signer port, publish policy, publish command, publish engine, local
store, retry/cancel controls, and typed status outputs. `EventDraft` and
`PublishContext` name invariants across those pieces, not automatic new types or
permission to add a second publish stack.

## Actor And Store Rules

The actor remains the single writer. New nondeterministic inputs enter as typed
actions, capability results, or injected seams. Reducers remain replayable from
message history.

The event store and indexes stay inside Rust. Shells receive typed projection
state, not raw store handles. Raw signed events may be exposed through explicit
inspection/export features, but not as the default app data path.

Projection delivery must preserve hard invariants: typed schema identity,
stale-frame drops, clear/tombstone semantics, full-pull cold start paths,
bounded replay, deterministic row ownership, and one merge contract per output.
Hiding projection tiers from app developers cannot mean deleting these executor
guarantees.

Wake/admission structure is in the same category. It is internal machinery, and
the right shape may be today's scoped fanout, a narrow reverse index, or a queue
per session family. What is not optional is the invariant: no polling, no broad
native refresh, no missed hydration, bounded wake cost, and deterministic stale
wake dedupe/drop rules. The ADR must name which event/store/source changes wake
each session family and why the chosen structure is sufficient.

The mechanisms are not automatically sacred. FlatBuffers, sidecar registration,
projection manifests, output namespaces, incremental apply, and generated host
adapters each need an invariant and a rejected simpler alternative. Keep them
where they are the cheapest way to preserve cross-platform decode, stale-frame
protection, render-cache correctness, or wire/CPU bounds. Collapse or delete
them where session-scoped output demand gives the same guarantee with less
surface area.

## Browser Runtime And Storage Lifecycle

Browser runtime architecture is part of the signoff surface, not a separate web
afterthought. The browser storage owner is `nmp-browser-runtime` or its successor
runtime crate, not legacy `nmp-wasm` ABI glue. OPFS/SQLite-style storage needs an
async-before-start seam so the runtime can open the store, inject the
`Arc<dyn EventStore>` or equivalent, and only then dispatch synchronous app
start. A browser path that silently falls back to in-memory storage because async
initialization was inconvenient fails the architecture gate.

Dedicated Worker constraints are real architecture constraints. OPFS
`SyncAccessHandle` storage works only in a dedicated Worker, so compile-only
wasm tests or main-thread browser smoke tests do not prove the storage path.
The proof is a Worker conformance gate that opens, writes, reads, closes, and
reopens durable data in a real browser. Multi-tab contention is an ADR question:
choose a durable-tab/Web-Locks policy or an explicit ephemeral secondary-tab
policy before implementation. Silent degradation is not allowed.

This also bounds `nmp-gallery` web proof. A gallery web app that builds only its
TypeScript shell, uses a placeholder wasm build, or degrades when the worker is
missing cannot prove the runtime architecture.

## Protocol Taxonomy And Kind Ownership

Generic layers must not grow per-NIP or per-kind branch tables. Kind constants,
replaceable/addressable predicates, and reusable kind taxonomy live in the
canonical kinds/protocol owner, such as `nmp-kinds` or the protocol crate that
owns the semantic rule. Generic routing, store, planner, and runtime layers
receive semantic context from the protocol-aware caller; they do not rediscover
NIP meaning by switching on raw kind numbers.

This is the same simplification rule as route provenance: keep one canonical
definition of protocol facts, pass typed context through the seam, and delete
duplicate local classifiers. A generic layer may inspect generic event shape
needed for storage or routing, but it must not encode "NIP-29 means X" or
"NIP-17 means Y" in a table that protocol crates must later fight.

## Existing Primitive Mapping

The new model should reuse or retire current primitives deliberately:

- `LogicalInterest` and the interest registry remain acquisition internals.
  Product apps should not compose them directly.
- cache-serve and store replay remain hydration internals behind observed
  sessions.
- `ObservedProjectionRegistrar` is the replay-before-live primitive to prove
  descriptor sessions against first.
- dependent interests are the current dynamic-source substrate; they either stay
  internal or collapse into a smaller reconciler/source core.
- `SnapshotRegistry`, `DeclaredProjections`, typed sidecars, incremental apply,
  and `UpdateFrame` remain executor machinery until session-scoped demand proves
  which pieces can be deleted.
- `ActionModule`, `DispatchEnvelope`, `PublishTarget`, publish policy, signer
  ports, and the publish engine remain the one write doorway. `PublishTarget`
  as it exists today may need to be widened or paired with provenance; the
  invariant to retain is one doorway, not the exact current enum shape.
- explicit relay seams must carry audited route provenance, not native relay
  choice or an anonymous relay list.

## Complexity Justification Gate

No internal mechanism in this proposal is accepted just because it already
exists. Before implementation, each mechanism must be red/blue-team reviewed:

```text
mechanism
  -> simplest plausible alternative
  -> invariant defended by the complex design
  -> evidence that the simpler alternative fails
  -> consolidation/deletion opportunity
  -> public/private/compatibility classification
```

The default outcome should be deletion or consolidation unless the mechanism
protects one of these invariants:

- replay correctness: cached data is seen before future live data;
- privacy: private events cannot leak to public relays;
- routing correctness: outbox, inbox, host relay, and explicit routes are not
  guessed by shells;
- single-writer state: product facts have one Rust owner;
- cross-platform parity: Swift, Kotlin, TypeScript, TUI, and browser shells do
  not reimplement protocol behavior;
- bounded reactivity: hot paths do not poll, wake unnecessarily, or ship
  unbounded snapshots;
- wake correctness: event ingest and dependency changes reach affected sessions
  without broad polling or native refresh code;
- downstream necessity: `nmp-gallery`, Highlighter, or Podcast Player cannot
  express a real flow with the simpler design.

Examples of complexity that must be justified, not inherited:

- whether `ObservedProjection` needs its current shape or can be a smaller
  replay-before-live helper under sessions;
- whether `ReducedSource`, dependent interests, pointer sources, and active
  observed projection controllers can collapse into one source-reconciliation
  helper, or whether current feed-local `ReducedSource` machinery should stay
  private until another non-feed family proves the same semantics;
- whether projection tiers need to remain as executor internals, and whether the
  app-facing output manifest can hide them completely;
- whether opening a session can be the scoped output declaration, leaving global
  declared projections only for always-on app chrome or compatibility;
- whether generated host adapters are enough, or whether hand-authored row
  caches can conform to one shared merge contract without becoming product
  state;
- whether `PublishContext` is a real missing type or only a naming layer over
  existing publish target, behavior, command, and policy data;
- whether off-actor feed mutation and feed-render memo/provider machinery can be
  deleted by routing viewport/load-older through actor/session actions;
- whether live counts can remain typed projections instead of a dedicated
  primitive;
- whether compatibility APIs such as raw `open_interest` need to remain public,
  and if so which scopes may call them.

The ADR should record rejected simpler alternatives. If the only defense is
"this is how the current code works," the mechanism is not justified.
Likewise, a claimed compatibility API must prove live consumers. Zero-caller
legacy code, duplicate extraction leftovers, stale aliases, and public shims with
no current consumer default to deletion, not formalization.
The `dispatch_action_json` lesson is the pattern to repeat: retention claims
must be rechecked against live call sites before being accepted. If a public
method survives only because old docs said TUI, Marmot, Chirp, gallery, or a
downstream app used it, verify the current callers. No live product caller means
delete the public method or move it to a test/diagnostic scope.

## Stress Tests

The design must survive these cases before implementation starts:

- **NDK-style subscribe:** a developer asks for kind:1 notes by a dynamic author
  set and receives one typed output while NMP splits the work across author
  relays, cache replay, live relays, and later mailbox changes.
- **NIP-29 group:** a group view is host-relay-pinned, admits only events with
  valid relay provenance/context, fails closed without group context, and tears
  down with the handle.
- **Component embed storm:** many visible event bodies create profile/event/embed
  claims; repeated refs share demand; closing components releases demand and
  generated/host caches clear without stale rows.
- **Account switch:** active-account sessions replan sources and routes without
  native timers, wildcard fallthrough, or stale projections from the old account.
  The known tick-polled active-account observer copy is a regression test case:
  account change must fire from sign-in/sign-out/account-switch events, not from
  snapshot tick polling.
- **Malformed or stale frame:** generated/host merge code applies updates
  transactionally; decode poison cannot corrupt the baseline; clear/tombstone
  frames remove rows.
- **Read-your-writes:** a locally signed event enters the store/ingest path before
  relay delivery, so projections update through the same reducer path as relay
  events, not native optimism.
- **Private routing:** NIP-17 and similar private writes never fall back to public
  outbox routes when recipient inboxes are unknown.
- **Downstream proof:** Highlighter web, Highlighter iOS, Podcast Player, and
  gallery can all express their flows without native-owned protocol policy or
  app-domain nouns in NMP crates.

## Lifecycle Ownership Matrix

#2316 is only fixed if every fragment of one feature's live state has an owner.
This matrix is the minimum proof map. If a row remains hand-wired by an app or
native shell after migration, the architecture has not solved the root problem.

| Lifecycle fragment | New owner | First proof | Failure mode |
|---|---|---|---|
| Relay interest / acquisition | typed session descriptor compiled by feature/app Rust | P1 descriptor proof over one existing observed session | product code still calls raw `open_interest` or native relay subscribe |
| Route planning | descriptor route policy plus planner | P1 planned and relay-pinned routes; P5 dynamic author routes | relays are passed as casual shell options or all explicit routes collapse together |
| Cache/index replay | session lifecycle using store/cache replay before live activation | P1 replay-before-live tests; P4 component refs | store seeding, cache warming, or shell retry is needed to make rows appear |
| Observed sink | internal observed projection/session sink | P1 close-both and replay activation tests | observer outlives handle or receives filterless future events |
| Admission predicate | feature-owned admission shape plus route provenance | P1/P4/P5 relay-pinned and embed admission tests | matching tags alone admit protocol-pinned data without source proof |
| Serialization/output sidecar | output schema owner and generated adapter contract | P3/P4 projection merge and decode-poison tests | output key is emitted by multiple producers or host cache has different merge rules |
| Projection emission | session-scoped output demand; always-on chrome only by explicit composition | P3 demand owns output proof | `declare_consumed_projections` remains the app's incomplete manifest |
| Snapshot/tick delivery | actor-owned coalesced update path | P2 tick deletion proof; FF-004 | snapshot tick reconciles product state or repairs missed session work |
| Reactive dependencies | private shape reconciler/dynamic-source machinery | P2/P5 source change and empty-source tests | empty source becomes wildcard; app computes follow/group/member sets |
| Teardown | typed handle ownership/refcount in Rust | P1/P4 handle close tests across shared refs | shell release/reclaim timers or claim-every-render keep demand alive |
| Error/status | typed output state, not exceptions or native side tables | P3/P6 status outputs | native infers success from missing errors or one-shot callbacks |

This is also the YAGNI guard. A proposed primitive is justified only if it makes
one or more rows structurally impossible to miswire. A primitive that merely
renames existing hand wiring is rejected.

## Timer Boundary

The no-polling rule applies to product state reconciliation, session routing,
source tracking, relay planning, projection refresh, and reducer correctness.
Those must be event-driven.

Capability executors and presentation affordances may still use timers when the
platform API is inherently sampled or throttled: media playback position,
download progress throttling, speech/audio progress, animation affordances, or a
"copied" label clearing after a delay. Those timers report raw capability or
presentation facts; they do not decide durable state, open relay demand, refresh
projections, or repair missed reducer work.

Every remaining tick observer or timer that touches reducer/session state needs
an explicit invariant, owner, and deletion or formalization decision.

## Non-Goals

- Do not expose a generic raw event callback as the main app API.
- Do not make `open_interest` the app read model.
- Do not let native compute dynamic source sets, route relays, or mutate event
  tags for protocol correctness.
- Do not collapse every protocol-specific publishing rule into `nmp-core`.
- Do not turn `LiveQuery` into an object that owns protocol meaning. Protocol
  and app crates own meaning; live query machinery owns lifecycle.
- Do not present this document as shipped API before ADR and migration work land.
- Do not move product domains into NMP crates because one downstream app needs
  them.
- Do not let compatibility paths remain public teaching examples once the typed
  lifecycle exists.

## Implementation Plan

Each phase must leave the repo shippable and reduce at least one public concept,
duplicate lifecycle recipe, or hidden desync state.

The milestone ladder below is not a big-bang rewrite. It is a proof program with
workstreams that converge into the same endpoint:

| Workstream | Scope | Must deliver before signoff |
|---|---|---|
| A. Baseline and ratchets | old public doors, tick users, direct publish paths, downstream native policy | counted baseline, owner for each count, and CI/doctrine gate for "does not increase" |
| B. Composition/defaults | `register_defaults`, `nmp init`, app roots, protocol feature installers | explicit feature composition as the production model; presets labeled tutorial/compatibility with live consumers, support window, owner, and deletion/formalization gate, or deleted |
| C. Session descriptor | one typed lifecycle owner over acquisition, replay, sink, output, and teardown | one simple real session migrated without a new engine or public `ObservedProjection` API |
| D. Dynamic sources | follow/list/group/thread/embed source sets | private reconciler proof for source arrival, withdrawal, empty-source fail-closed, fallback, and teardown |
| E. Output/projection contract | projection ownership, schema/version, sidecars, host caches | one owner per output key, collision failure, shared merge semantics across generated/host adapters |
| F. Read routing/admission | outbox routing, relay-pinned sessions, private reads, explicit overrides | route provenance in read descriptors and replay/live admission; no shell relay policy |
| G. Write routing/publish | event construction/finalization, signer selection, publish route provenance | one publish doorway distinguishes automatic, host-pinned, verified inbox, manual, and imported routes |
| H. Signer/status runtime | local, NIP-07, NIP-46, NIP-55-style, named product, agent, imported event | Rust-owned pending/ready/failed/signed status and parked continuation model across platforms |
| I. Service/capability sessions | widgets, AppIntents, CarPlay, remote commands, Live Activities, Handoff, media/STT/AI | app/service sessions or typed capability results; no `KernelModel.shared` UI-process dependency for correctness |
| J. Generated adapters/codegen | action builders, output schemas, row caches, FFI/runtime bridges | generated or contract-tested drift prevention for every cross-platform payload used by migrated flows |
| K. Downstream proofs | Highlighter, Podcast Player, `nmp-gallery`, sanity checks from 29er/Olas | each acceptance matrix passes or triggers a named kill criterion; downstream app nouns stay out of NMP crates |
| L. Durable docs/ADR retirement | ADR, builder guide, product specs, templates, wiki, issues | local packet retired; durable docs corrected in place; tactical work lives only in GitHub issues |

Ordering rules:

- A and B happen first. Without baselines and explicit composition, later work can
  look cleaner while old public doors keep growing.
- C proves the lifecycle owner before D or E generalize anything.
- D may stay private until at least two source families prove identical
  semantics. Do not promote `ReducedSource` because one feed path exists.
- E must land before broad host migration, or every shell will invent its own
  merge/cache contract again.
- F and G must preserve route provenance before downstream NIP-29, NIP-17,
  Podcast NIP-F4, or pre-signed/imported flows migrate.
- H and I are required for real apps, not follow-up polish. A design that only
  works while the foreground UI process is open is not the NMP app architecture.
- K is a signoff gate, not a post-ADR chore.
- L prevents this packet from becoming another parallel source of truth.

Every implementation slice should have this shape:

```text
baseline old pattern
  -> add or narrow typed owner
  -> migrate one real caller family
  -> add a ratchet or contract test
  -> delete, privatize, or label the old path
```

Do not merge a slice that only adds the new owner while leaving the old public
path as an equally valid production path. Dual paths are allowed only as
migration-scoped compatibility with owner, consumers, and deletion criteria.

Each slice also needs a deletion ledger:

| Question | Required answer |
|---|---|
| What module/crate/public method/code path gets smaller or disappears? | Name the target. "Cleaner API" is not enough. |
| What old pattern count moves down or is frozen? | Link to the baseline grep/test/manual inventory. |
| What simpler alternative was tried first? | Delete, inline, narrow, or reuse before adding a new layer. |
| What invariant prevents deletion today? | Replay, route proof, privacy, parity, boundedness, or live external consumers. |
| What proves the new owner replaces the old path? | Migrated caller, test/ratchet, docs correction, and old-path deletion/privatization. |

New modules are allowed, but they start unproven. A new crate, registry, adapter,
context object, or executor tier must delete or privatize at least one existing
public concept or duplicate lifecycle recipe in the same milestone, or it remains
an ADR question rather than implementation work. This is how the migration avoids
turning `FeatureSession`, route provenance, generated adapters, and service
sessions into additive layers over the old machinery.

**P-1: Concept disposition and live-consumer audit.**
Before P0 inventory turns into implementation work, classify every disputed
concept and public door:

Current recipe inventory to verify against live code before implementation:

| Current recipe/surface | Owns today | Misses or leaks |
|---|---|---|
| raw `open_interest` / `nmp_app_open_interest` | acquisition and store/cache eligibility | no typed output, admission owner, projection lifecycle, or app-visible delivery contract |
| `open_observed_projection` | replay-before-live sink registration, scoped future delivery, close token | still asks feature authors to pair acquisition/output/schema/route policy manually |
| `open_feed` / feed sessions | one feed-shaped source compiler, dependent interests, observed sinks, feed output | feed-local semantics; not proof that group/thread/ref/search sources share one public primitive |
| group/search feature `open_*` recipes | feature-specific route/source/projection bundle | repeated lifecycle recipe; not a general session contract for app-defined features |
| refs/embeds / `resolve_ref` | component demand and some typed ref outputs | cross-shell raw worker/ref adapters, claim/reclaim loops, and hand caches still need convergence |
| `declare_consumed_projections` | cost brake for some built-in push outputs | incomplete manifest; not tied to session demand or host-registered outputs |
| snapshot tick observers | periodic repair/reconcile hook for some runtimes | hides missing event-driven wakes; cannot remain product-state scheduler |
| `PublishTarget::Explicit` / `UnsignedEventToRelays` | exact relay set and route bypass | loses whether the route is host-pinned, private-inbox verified, manual, diagnostic, or imported |

The implementation plan can amend this table as live code changes, but it must
not remove a row by rephrasing the old recipe as the new architecture. A row is
retired only when its old public surface is deleted, privatized, or scoped to
diagnostic/test/migration with live consumers and a removal/formalization gate.

| Surface | Target disposition | Required evidence |
|---|---|---|
| `register_defaults()` | production use rejected; tutorial/migration shim only if retained | live callers, support window, owner, generated scaffold behavior, deletion/formalization target, scaffold gate change |
| `nmp init` scaffold | production scaffold emits explicit feature composition | `nmp-cli` template and `dx_scaffold_gate` updated or separate tutorial command created |
| raw `open_interest` / `nmp_app_open_interest` | substrate/protocol/diagnostic/migration only | current callers classified; public docs stop marking it as product PASS without scope |
| `NmpApp::open_feed`, `open_observed_projection`, `nmp_app_open_interest`, `resolve_ref` | existing surfaces to unify under typed descriptor contract | contract map showing which lifecycle rows they already own and which they miss; do not invent migration evidence for stale or missing symbols |
| `ObservedProjection` | private replay-before-live machinery | minimal proof that descriptor can reuse it without exposing it |
| `ReducedSource` / source reconcilers | private until cross-family proof | feed, pointer, account, group/list/thread sources compared by semantics |
| projection tiers / declared projections | private executor/cost machinery | output ownership and session-scoped demand plan |
| JSON dispatch/raw publish APIs | migration/test/diagnostic or deleted | current live consumers, removal/formalization gate |
| explicit relay publish shapes | one route-provenance contract | mapping for manual, host-pinned, verified inbox, imported, diagnostic |
| generated adapters/codegen | mandatory where contract cannot be otherwise proven | per-output/action drift risk and gate |

P-1 output is a table in the ADR, not a code abstraction. It prevents the common
failure where a migration begins before the team knows which old public doors
are being deleted, privatized, or formalized.

## New-Code Rules

Once the ADR accepts this direction, new code must obey these rules even before
the full migration is done:

- Product screens and shells must not call raw `open_interest` or equivalent
  relay-subscription doors.
- Product reads must enter through a typed feature/ref/session descriptor or a
  named substrate/protocol-internal/diagnostic/test/migration scope.
- New app-facing examples and templates must not teach `register_defaults()` as
  the production mental model.
- New compatibility shims or aliases must not be added unless the caller list,
  support window, owner, and deletion/formalization gate are declared at the same
  time.
- New output keys must declare one owner, schema version, merge contract, and
  collision behavior.
- New dynamic sources must specify empty-source behavior and must fail closed
  unless the feature declares an explicit fallback.
- New timer/tick logic must prove it is capability sampling or presentation
  affordance, not reducer/session/projection reconciliation.
- New session families must declare their event/store/source wake conditions and
  reverse-index or queue strategy.
- New event-producing writes must enter through typed actions/builders and
  publish status, never native JSON publish paths.
- New explicit relay writes must preserve route provenance.
- New app-feature APIs are allowed for app runtime capabilities, but they must
  be typed/versioned and may not own Nostr protocol policy.

**P0: Baseline and freeze old patterns.**
Inventory public read/write doors and duplicate lifecycle recipes in
`nmp-core`, `nmp-defaults`, `nmp-native-runtime`, `nmp-browser-runtime`,
`nmp-ffi`, `nmp-codegen`, `nmp-gallery`, Highlighter, and Podcast Player. Add or
extend grep gates so new product code cannot add raw `open_interest` app reads,
new tick reconcilers, native relay timers, or native publish/tag construction.
Classify every existing raw read/write door as substrate, protocol-internal,
diagnostic/test, migration shim, or product API to remove/formalize.
Record baseline counts for: raw `open_interest` public callers, `nmp init`
teaching `register_defaults()` as production architecture, tick observers,
`declare_consumed_projections` app-facing docs, duplicate explicit-relay publish
representations, native-owned relay/policy/tag construction, hand-authored
projection merge caches, downstream direct NDK/FFI publish paths,
protocol read/projection producers registered from app or FFI glue instead of
their reusable NMP feature crate, especially `nmp.follow_list`,
`@nostr-dev-kit/ndk` product fetch/sign/publish usage, direct `NDKEvent`
construction, `event.publish`/raw relay publish calls, native-owned network
policy such as `hl.network.wifi_only`, shell-side `tagsJson`/`p:`/`e:`/`a:`
protocol parsing, fire-and-forget event writes, unsupported local-key/NIP-46
bridges in web runtimes, `NmpApp::open_feed`, stale `nmp_app_open_feed`
docs/tests, and `nmp_app_open_interest` app doors. The first PR should add
ratchets so these counts cannot grow and stale surface names do not become fake
compatibility requirements.
Gates: `cargo test -p nmp-testing --test doctrine_lint_smoke` and
`cargo test -p nmp-testing --test feed_public_surface_retired`.

**P1: Prove a descriptor over existing safe machinery.**
Add the smallest private descriptor facade that compiles into
`ObservedProjection::from_shape`, `OpenObservedInterest`, replay limits,
consumer ids, relay pins, and close. Do not add a new lifecycle engine. Use one
real session as proof. Acceptance: replay-before-live and close-both invariants
still pass in observer replay, descriptor idempotence, reducer parity, and
`nmp-defaults` feed open/close tests; no new public API is taught. The proof
must cover relay pins, cache replay, source changes, and open/close teardown.
The migrated session must publish a session-family contract covering
acquisition, route planning, replay, live sink, admission, output, wakes,
teardown, and error/status state. If any fragment remains caller-authored
outside that contract, P1 has not solved #2316.
It may use an existing feed/search/group-style observed session, but it should
not also claim gallery/component-ref migration. P1 proves the lifecycle owner;
P4 proves the first cross-shell ref/embed migration.

**P2: Extract shape reconciliation and delete tick use where events exist.**
Consolidate the duplicated open/close-on-shape-change controllers behind one
private reconciler. Migrate active-account, browser feed, native feed, and
pointer-source controllers only where their semantics match. Delete
`register_snapshot_tick_observer` usage for identity/source/mailbox/refcount
changes that already have event hooks. The account-change detector is an explicit
proof case: active-account sessions must replan from sign-in/sign-out/switch
events exactly once per change, not by checking the active account on every
snapshot tick. For each remaining tick observer, either add the missing explicit
event source or document a bounded actor-scheduled invariant with a staged
deletion gate. "Compatibility" alone is not a reason to keep it. Use the existing
cache-serve wakeup pattern as the reference: live/store events enqueue coalesced
work, and actor ticks only drain already-declared work.

**P3: Make scoped session demand own scoped output demand.**
Prove that opening a session can declare its typed output. Keep
`DeclaredProjections` only for always-on app chrome, compatibility, or measured
wire/CPU wins. Acceptance: `public_typed_projection_decode` still proves
external decode; generated adapters still handle full, delta, clear, stale-frame,
baseline, transactional merge, and D6 poison semantics. `declare_consumed_projections`
must stop being taught as the app manifest for screen/session outputs.

**P4: Migrate component refs and gallery embeds first.**
Move `ProfileRef`, `EventEmbed`, URI decoding, relay hints, embed envelopes, and
row-delta caches behind typed sessions and generated adapters. Delete shell
retry timers, claim-every-render behavior, and duplicated URI decoding where the
Rust path owns it. This is the first cross-shell proof because gallery exercises
Swift, Kotlin, web, TUI, and desktop rendering without app-domain policy.
Include gallery auth/signing component coverage. The live tree already has a
gallery web root and browser runtime crate; the proof must account for deferred
gallery packaging/CI, `web/nmp-gallery build:wasm`, raw worker ref API, silent
runtime degradation, and web retry/reclaim loop instead of pretending the web
shell does not exist. Fix ref/projection retry timers and desktop/TUI
claim-on-render or claim-on-tick behavior before treating the registry as a
copyable downstream template. Copy-to-clipboard timers are presentation
affordances; they are not the architecture failure unless they start owning
product state.
P4 starts only after P1 proves the descriptor lifecycle and P3 proves output
ownership/merge semantics. It is the first real migrated session family across
all shells, not a prerequisite for the minimal descriptor proof.

**P5: Migrate dynamic and composite reads only after P1-P4 hold.**
Feed, group, search, pointer-source, thread refs, and live-count outputs move to
the same descriptor model. Source-specific reducers stay local unless a shared
core deletes duplication. Acceptance: feed reduced-source tests, real-relay
reduced-source tests, group/search tests, and empty-source fail-closed tests
cover account switch, source change, relay pin, cache replay, and teardown.

**P6: Collapse write variants by invariant, not by new names.**
Generated builders keep using `DispatchEnvelope` and `ActionModule`. First try
to unify `UnsignedEvent`, `UnsignedEventToRelays`, pre-signed publish, signer
selection, target/provenance, correlation id, and policy validation without
adding new public types. Existing `PublishTarget` may be widened or paired with a
small provenance field; using it unchanged is not sufficient because
`Explicit { relays }` lacks the audit class/reason. Add a named draft/context
type only if it deletes branching or duplicate route/privacy/protocol state.
Gates: publish policy, D10 private routing, signer continuation, generated
builder round-trip, and action-result tests. Explicit relay cleanup is part of
this phase: delete dead explicit-target
fields or route every explicit publish through one canonical internal seam with
one attribution/status model. Route provenance is the critical missing invariant:
manual explicit relay, NIP-29 host pin, verified NIP-17 inbox, and
external/verbatim publish must not collapse into an indistinguishable
`Explicit` bucket. Tests must prove generic raw publish cannot accidentally
bypass NIP-29 `h`-tag/group-route proof or NIP-17 verified-inbox policy, and
that remote signer continuations preserve route provenance plus correlation id.
The first implementation slice should attempt the smallest carrier change:
extend or split the existing target/reason/status pipeline so provenance class
travels through `PublishCommand`, parked signer publish obligations, engine
records, retry/resume, and status output. A broad draft/context object is a
second-choice representation, not the destination. If the audit finds two
explicit-relay variants that are genuinely different invariants, document both;
if they differ only by caller history, collapse them.

**P7: Prove downstream apps before declaring the architecture final.**
Highlighter must express home feed, room chat, search, comments, share-to-room,
capture, feedback, signer flows, artifact share, article lookup, and room
discussion through app-owned Rust bundles and NMP runtime dispatch. Direct web
NDK relay/filter/tag/sign/publish paths, Swift rich-text protocol projection,
Swift-owned sync policy, Swift `tagsJson` parsing, and fire-and-forget writes
must be removed or formally rejected by the ADR. SSR-only fetches and migration
shims must be labeled as such; they cannot be the product runtime model. The ADR
must decide whether Highlighter web is in the NMP target runtime, a labeled
SSR/migration exception, or deliberately out of scope.

Highlighter's acceptance matrix must cover onboarding/profile, rooms/invites/
members, highlights, comments, capture, share queue, curation/bookmarks,
podcast, search/SSR, NIP-05, Blossom, and feedback. Each row names the Rust
session/action/builder, route policy, signer path, cache/offline policy,
publish-status output, and deletion or exception criterion for the current
Swift/TypeScript path. A row is not migrated if it only wraps the old NDK or
native JSON publish door with a friendlier name.

Required Highlighter matrix shape:

| Flow family | Current path to classify | Target proof | Deletion/exception criterion |
|---|---|---|---|
| Web onboarding/profile | NDK event build/sign/publish, `$subscribe`/`fetchEvents`, local signer/session storage | typed Rust action/builder, signer status, route provenance, cache policy | NDK path deleted, or ADR labels SSR/diagnostic/out-of-scope with owner and removal gate |
| Web rooms/invites/members/chat | NDK relay sets, `$subscribe`, direct sign/publish, tag parsing | NIP-29 group session/action through Rust, kind-agnostic read, host route status | direct NDK group runtime count ratchets down to zero unless explicitly excluded |
| Highlights/comments/capture/share | TS/Swift tag walkers, `tagsJson`, raw comments/replies | Rust descriptors for NIP-10/NIP-22/article/highlight refs and typed publish status | semantic parsing removed from shells; presentation-only transforms documented |
| Blossom/NIP-05/search/SSR | web direct fetch/cache/publish paths | typed capability/result or labeled SSR cache boundary | cache/write owner named; no hidden product truth in web storage |
| Signer/session/offline policy | web/local/native signer inference and Wi-Fi/cache policy | Rust-owned signer/offline/cache state plus raw native capability facts | shells stop deciding signer completion, retry, route, or offline eligibility |

Podcast Player must express playback, queue, feed subscription, NIP-F4, Blossom
publish, explicit write relays, widgets, settings actions, signer runtime, and
feedback without moving podcast nouns into NMP. Bespoke durable FFI, silent
compatibility paths, stale Swift-store docs, and `nmp-signer-broker` pinning
must converge on generic typed dispatch/projection/capability seams and the
current NIP-46 runtime direction. Widget extensions, AppIntents/Siri, CarPlay,
remote commands, Live Activities/Handoff, and cold/suspended process behavior
must prove app-runtime/service sessions rather than native-owned state.

Podcast's acceptance matrix must cover playback/queue/gestures, feed
subscription, OPML/catalog/search/transcripts, widgets, AppIntents/Siri,
CarPlay, remote commands, Live Activities/Handoff, NIP-F4 show/feed/episode/list
publish, Blossom upload/reference publish, local/NIP-46/NIP-55/per-podcast-key/
agent signer paths, explicit relay/server lists, legacy settings, and generated
app FFI. NIP-F4 is not migrated while the path only returns `relay_pending`,
stores constructed JSON, or requires the app to infer relays/signers in native
code.

Required Podcast matrix shape:

| Flow family | Current path to classify | Target proof | Deletion/exception criterion |
|---|---|---|---|
| Playback/queue/gestures | Swift state, remote/headphone gestures, App Group mirrors | Rust-owned playback/queue state; native reports raw media/command facts | Swift can render/execute only; no queue mutation or gesture policy outside Rust |
| Feed/subscription/catalog/search/transcripts | app Rust plus Swift stores/import surfaces | app Rust sessions/actions and capability results; no NMP podcast nouns | native DB/UserDefaults classified as render/import cache or deleted |
| Widget/AppIntent/Siri/CarPlay/remote/LiveActivity/Handoff | UI-process singleton, `KernelModel.shared`, polling, App Group snapshots, OS surfaces | service/app-lifetime sessions or typed capability results; cold-start proof; action completion/result distinct from dispatch acceptance | no `KernelModel.shared` correctness dependency; no polling wait loop; native only reports raw OS command/capability facts |
| NIP-F4/Blossom publish | constructed JSON, `relay_pending`, `publish_dispatched`, explicit write relays/server lists | build/sign/route/store/publish/status with route/server provenance and key-storage capability | user-facing e2e proves ack/error/retry/exhausted status; stale diagnostics deleted |
| Signers/relays/settings | local, NIP-46, NIP-55, per-podcast key, agent, legacy relay settings, plaintext key stores | one signer/status/route provenance model plus secure key capability | native no longer infers signer timeout, relay policy, key ownership, or publish success |
| Generated app APIs | hand-authored C/Swift action glue, JSON/pointer FFI, direct `KernelModel.shared` handles | generated or contract-tested typed app APIs | hand glue is app-local and non-protocol, or generated/drift-gated; event-producing APIs use typed publish/status |

`nmp-gallery` must express component refs, embeds, auth/signing components, and
renderer caches without shell protocol state, raw worker ref protocols,
timer-based state clearing, or claim-every-render/tick behavior. It becomes the
conformance fixture for refs/profile, refs/event envelopes, copied/native
components, typed dispatch, and renderer caches only after Swift, Kotlin,
TypeScript, TUI, and desktop shells all use the same generated cache and ref
lifecycle semantics. The auth/signing matrix must distinguish read-only
rendering, local signer, remote signer, and unauthenticated embed cases instead
of claiming "auth/signing" generically.

Required gallery matrix shape:

| Flow family | Current path to classify | Target proof | Deletion/exception criterion |
|---|---|---|---|
| Web runtime | deferred `build:wasm`, TS-only app check, raw Worker `resolve_ref`/`release_ref`, retry/reclaim loop | `web/nmp-gallery build:wasm` builds/stages runtime artifact; gallery e2e fails in degraded/no-wasm mode; generated typed ref API; deterministic owner close | no correctness `setInterval`; raw worker protocol hidden or deleted |
| Component refs/embeds | Swift/Kotlin/TS/TUI/desktop URI/ref adapters, raw namespace/shape/liveness constants, and claim loops | typed `ProfileRef`/`EventEmbed` sessions plus generated or contract-tested host handles | shell adapters are generated/lifecycle-only, not protocol policy |
| Merge/cache parity | hand caches and `projection_merge_cache` variants | full/delta/clear/tombstone/stale/decode-poison/baseline tests across shells | no platform owns independent merge semantics |
| Auth/signing components | Android NIP-55 proof plus partial/visual other shells | per-shell read-only/local/remote/unauthenticated matrix | generic "auth/signing covered" claim removed until each shell is classified |
| Composition root | `register_defaults()` and `consume_all_builtin_projections()` showcase path | explicit feature composition or labeled tutorial compatibility | production examples stop teaching hidden defaults |

Any downstream flow that requires native-owned policy or a bespoke framework
door is a design failure, not downstream migration debt.

**P8: Correct durable docs and delete compatibility teaching paths.**
Update architecture API-surface docs, overview/DX docs, builder-guide pages for
subscription planning, publish and ledger, walkthroughs, action-triggered
subscriptions, ADRs, wiki pages, and any episode/transcript-derived teaching
material that currently presents projection tiers or defaults as app-facing
concepts. The `nmp init` template must be corrected according to the ADR
decision: either production scaffold with explicit feature composition and
policy builders, or clearly labeled tutorial preset. Compatibility APIs may
remain only with scope labels, doctrine gates, and deletion criteria.
At minimum, audit and rewrite stale guidance in `docs/product-spec/api-surface.md`,
builder-guide mental-model/codegen/walkthrough pages, subscription planning and
publish guides, wiki app-composition pages, and any generated template that
teaches `register_defaults`, `open_interest`, projection tiers, or
`declare_consumed_projections` as the normal product architecture.
Also audit wiki pages that still teach `nmp.feed.home`, generic defaults,
sidecar projection rituals, raw/pre-signed publish branches, or direct taps as
current architecture. They should either be corrected in place or explicitly
retired before this packet becomes durable documentation.

Named wiki/doc pages to reconcile, because they currently encode important but
possibly stale architecture claims:

| Page | Why it matters | Resolution rule |
|---|---|---|
| `docs/wiki/guides/reduced-source.md` | describes `ReducedSource`, `FeedParams`, and `open_feed` as app-facing dynamic-feed architecture | keep only if the ADR explicitly accepts that public surface; otherwise rewrite around typed sessions and private source reconciliation |
| `docs/wiki/guides/publish-outbox-pipeline.md` | documents both per-relay reasons and the dead/live explicit-route split | preserve route-reason/status lessons, but resolve `RoutingContext::explicit_targets` versus `PublishTarget::Explicit` to one real seam |
| `docs/wiki/guides/nip29-wiring.md` | contains both correct NMP/app ownership boundaries and older explicit-target wording | keep the ownership boundary; update route wording to match the chosen publish seam |
| `docs/wiki/guides/nmp-gallery-app.md` | records `nmp_app_gallery_register` calling `register_defaults()` and older claim/open-author behavior | migrate to explicit composition or label as gallery/tutorial compatibility with owner and removal gate |
| `docs/wiki/guides/operator-data-leaf-apps-only.md` | correctly states operator policy belongs only in leaf apps | carry this into durable composition/defaults docs so simplification does not reintroduce hidden defaults |
| `docs/wiki/guides/signer-broker-handshake-loop.md` and NIP-46 research pages | record event-driven signer reconnect/cancel and protocol/transport separation | signer runtime plan must preserve this split and avoid a signer-specific second framework |
| `docs/wiki/guides/action-module-adr.md` | records the typed action/effect boundary and remaining dual-action seam | write-flow work must retire dual dispatch seams rather than layering generated builders over them |
| `docs/decisions/0009-app-extension-kernel-boundary.md` | teaches app extension/read-model assembly through extension seams and observed projection wiring | update once service sessions and typed sessions own extension/app-service demand |
| `docs/decisions/0046-composition-is-a-library-not-a-generator.md` | treats defaults composition as the reusable app assembly model | amend around explicit production composition and labeled tutorial/compat presets |
| `docs/decisions/0053-host-declared-projection-subscriptions.md` and `docs/decisions/0062-observer-scoped-read-model-catchup.md` | preserve host-declared projection/tier and observed catchup language | rewrite around session-scoped output demand while preserving replay-before-live invariant |
| builder-guide 02, 15, 19a/19b/19c, 20, and 28 | teach mental model, codegen, walkthrough, protocol-module, and action-triggered-subscription flows using old public seams | update examples to typed sessions/actions and explicit composition, or label them historical |
| `docs/product-spec/api-surface.md`, `docs/product-spec/cli-toolchain-phasing.md`, `docs/ffi-surface.md`, `docs/wasm-surface.md`, `docs/recipes/app-shapes.md` | expose old app API, CLI, FFI, wasm, and recipe surfaces as product architecture | rewrite public API story around typed sessions/actions, browser-runtime ownership, and compatibility allowlists |

P8 is not a doc-polish phase. If a stale wiki or builder-guide page remains as
normal guidance, new agents will rebuild the old architecture from that page.
The architecture is not finalized until the surviving facts have one durable
owner and the stale pages are corrected or retired.

## Fitness Functions And Ratchets

P0 should convert the new-code rules into repeatable checks. The exact scripts
can change, but each check needs an owner, baseline, target, and enforcement
mode before implementation starts.

| ID | Rule | Baseline Source | Target | Enforcement |
|---|---|---|---|---|
| FF-001 | Product code does not add raw `open_interest` app reads. | grep public FFI/runtime/shell callers and builder-guide examples | count never increases; product callers trend to zero | doctrine lint or `nmp-testing` grep gate |
| FF-002 | `register_defaults()` is not the production app mental model. | `nmp-cli` templates, examples, builder guide, gallery/podcast composition roots | templates teach explicit feature composition or labeled tutorial preset | template test plus doc grep gate |
| FF-003 | App-facing docs do not expose projection tiers or `declare_consumed_projections` for screen/session output. | docs/product-spec, builder guide, wiki, templates | public docs teach typed outputs and session-scoped demand | docs lint grep gate |
| FF-004 | Product state reconciliation does not use snapshot tick polling. | `register_snapshot_tick_observer` call sites and downstream timers | reducer/session/projection tick users trend to zero or have explicit invariant | grep gate plus owner list |
| FF-005 | Dynamic sources fail closed. | feed/source/dependent-interest tests | every migrated source has empty-source and fallback tests | crate tests for source families |
| FF-006 | Output keys have one owner and collision behavior. | projection contract table, host-registered projections, built-ins | composition fails on unowned/colliding keys unless alias/replace is declared | registry test/codegen check |
| FF-007 | Generated/host caches share merge semantics. | Swift/Kotlin/TypeScript/TUI/desktop ref caches, gallery raw worker refs, and `projection_merge_cache` | full/delta/clear/stale-frame behavior covered for generated adapters across every shipped shell | cross-language decode/merge tests |
| FF-008 | Explicit relay publishes preserve route provenance. | `PublishTarget::Explicit`, protocol plans, pre-signed publish APIs | manual, NIP-29, verified inbox, and imported/verbatim routes are distinguishable | publish policy and retry/resume tests |
| FF-009 | Private routes fail closed. | D10 tests, NIP-17 inbox tests, Marmot/private publish paths | no unknown-inbox fallback to public/outbox | `nmp-core`, `nmp-nip17`, doctrine tests |
| FF-010 | Downstream shell protocol policy decreases. | Highlighter NDK usage, `$subscribe`, Swift `tagsJson`, native Wi-Fi policy, Podcast signer/relay inference, gallery URI/ref parsing, web retry loops, desktop/TUI claim ticks | counts do not increase; release gates drive them down | downstream grep gates or migration checklists |
| FF-011 | App-feature APIs stay typed and non-protocol unless event-producing. | Podcast STT/TTS/agent/provider APIs and generated FFI | app runtime APIs are classified; event-producing ones use typed publish | API-surface classification test |
| FF-012 | Clean-room app docs work without issue/wiki spelunking. | generated app plus builder guide | new app can open/read/write one feature with typed sessions/actions | walkthrough test or manual UAT checklist |
| FF-013 | Session wakes are declared, bounded, and event-driven. | cache-serve wakeups, logical-interest indexes, tick observers, downstream refresh pulls | no session family depends on broad polling or native refresh triggers; reverse indexes exist only where scoped fanout is insufficient | session wake/admission tests |
| FF-014 | Rust outputs semantic facts; shells format presentation. | signer labels, SF Symbols, short npubs, relative time, display strings in Rust projections | semantic tokens only in Rust outputs; presentation helpers stay in shells/TUI/test fixtures | grep gate plus projection review |
| FF-015 | Session wake fanout is bounded. | observer lists, logical-interest registries, source reconcilers, downstream ref claims | each migrated session family declares max wake scope or measured fanout budget | reactivity benchmark or targeted stress test |
| FF-016 | Active-session memory scales with open owners, not event history. | ref caches, feed sessions, projection caches, gallery component refs | memory bound documented and tested for repeated open/close and embed storms | leak/refcount stress test |
| FF-017 | FFI/update cadence is coalesced and bounded. | snapshot tick observers, UpdateFrame emission, downstream render caches | no migrated view emits above view budget or serializes one frame per event without proof | update cadence benchmark or fixture |
| FF-018 | Every accepted session family has one lifecycle contract. | P1/P4/P5/P7 migrated sessions and current hand-wired open/replay/projection recipes | acquisition, route, replay, live sink, admission, output, wakes, teardown, and error/status are owned by one session contract | `live_query_descriptor_contract` plus per-session contract tables |
| FF-019 | Default public author reads use planned outbox routing. | feed/search/ref sessions, `GenericOutboxRouter`, mailbox cache, direct NDK comparisons | author-scoped public reads prove NIP-65/mailbox routing, mailbox-change replanning, unified output delivery, and explicit exceptions for relay-pinned/private/search routes | `read_route_planning_contract` or targeted planner/session tests |
| FF-020 | Reusable protocol projections are owned by their protocol/NMP feature crate. | `nmp.follow_list` and other protocol projections registered from app/FFI glue | app crates consume protocol outputs; they do not register reusable protocol read models | architecture ratchet over projection owner registry and app/FFI call sites |
| FF-021 | Legacy aliases and compatibility shims require live consumers and deletion gates. | JSON dispatch, defaults presets, old open/read/publish doors, stale aliases, downstream-claimed callers | no retained shim lacks caller list, support window, owner, and deletion/formalization criterion; zero live consumers means delete | `compatibility_surface_contract` plus live call-site audit |
| FF-022 | Browser storage/runtime lifecycle is runtime-owned and worker-proven. | `nmp-wasm`, `nmp-browser-runtime`, OPFS/SQLite crates, gallery web build, browser conformance workflows | browser storage opens before start, runs in the right Worker context, proves durability in real Chrome, and fails loudly when wasm/worker is missing | `browser_storage_lifecycle_contract` plus gallery web e2e |
| FF-023 | Generated catalogs/manifests have one writer. | signer catalog, Android manifest queries, iOS plists, TS relay config, release manifest, client identity | native/web artifacts derive from Rust or release manifests; drift gates compare back to the true source, not only peer artifacts | codegen `--check`, release-manifest gate, signer-catalog parity tests |
| FF-024 | Protocol taxonomy and kind predicates are single-sourced. | `nmp-kinds`, protocol crates, router/planner/store generic layers | generic layers do not switch on per-NIP tables; protocol-aware callers pass semantic class/context | kind-predicate authority lint and router generic-layer tests |
| FF-025 | Metadata privacy gate is centralized. | outbound finalizers, NIP-89/client identity, public and explicit publish arms | client metadata appears only on public-routable unsigned events and never on private/imported/pre-signed/reserved surfaces | metadata privacy contract tests |
| FF-026 | Binding generation reduces drift instead of moving old doors. | C-ABI, JNI, UniFFI experiments, FlatBuffers, runtime workers | generated binding work deletes hand-maintained drift or narrows compatibility; it does not preserve old public semantics under new glue | binding-surface diff review plus codegen drift gate |

## Current Baseline Snapshot

This is an initial 2026-06-28 snapshot from live grep counts. It is evidence for
the dossier, not a durable source of truth. These counts include docs and tests
unless noted, so signoff still requires manual classification into production,
test, historical doc, tutorial compatibility, diagnostic, or delete.

| Surface | Count | What it means |
|---|---:|---|
| NMP `open_interest` family, excluding `docs/new-arch` | 45 files / 122 matches | old read door is still broadly taught and tested |
| NMP defaults/projection declarations | 144 files / 526 matches | hidden defaults and projection-declaration language are not a small local cleanup |
| NMP `ObservedProjection` family | 190 files / 885 matches | observed projection is a major existing concept and must be deleted, privatized, or explicitly justified |
| NMP explicit publish route family | 198 files / 2148 matches | route provenance cannot be fixed by naming alone; dead/live explicit seams need one owner |
| NMP tick/polling markers | 33 files / 53 matches | every retained tick needs presentation/capability classification or deletion |
| Highlighter direct Nostr/policy markers | 116 files / 609 matches | Highlighter web/runtime migration is a first-class gate, not a footnote |
| Podcast service/publish markers | 229 files / 773 matches | service sessions and publish status are downstream proof obligations |
| `nmp-gallery` old registration/ref/wasm markers | 48 files / 242 matches | gallery is useful proof only after wasm/ref lifecycle and defaults are resolved |

The raw counts are intentionally uncomfortable. They argue against layering a
new architecture beside the current one. The first implementation slices must
produce a before/after deletion ledger:

```text
old public doors deleted or privatized:
old docs corrected or retired:
old tests moved to compatibility/historical:
new public concepts added:
net permanent concepts:
```

If a slice adds `FeatureSession`, `ObservedProjection`, `ReducedSource`, route
metadata, generated adapters, or binding machinery without removing an equal or
larger old surface, it fails the simplification test even if it is internally
coherent.

Current dossier status:

| Area | Status | Required next proof |
|---|---|---|
| Typed feature sessions | Partial | first feature session deletes old lifecycle recipes or proves they are migration-only |
| `ObservedProjection` | Unproven | public registrar is deleted/privatized, or a cross-app invariant justifies it |
| `ReducedSource` | Unproven | source planning stays descriptor-local unless multiple features prove one shared abstraction |
| Publish route provenance | High risk | one carrier survives through build, sign, retry, local ingest, status, and replay |
| Downstream apps | Not ready | Highlighter, Podcast Player, and gallery matrices classify every old-pattern row |
| Browser runtime/storage | Not ready | real wasm/Worker/OPFS lifecycle proof exists and fail-closed behavior is tested |
| Generated catalogs/manifests | Not ready | one writer is named and drift gates run in check mode |
| Durable docs | Not ready | old architecture teaching is corrected in place or retired |

Useful baseline commands:

```bash
rg -n "nmp_app_open_interest|open_interest" crates apps docs --glob '!target/**'
rg -n "register_defaults|declare_consumed_projections" crates docs apps --glob '!target/**'
rg -n "nmp\\.follow_list|follow_list|register_.*projection|open_observed_projection" crates apps docs --glob '!target/**'
rg -n "GenericOutboxRouter|MailboxCache|Nip65|NIP-65|outbox" crates apps docs --glob '!target/**'
rg -n "register_snapshot_tick_observer|sleep|Timer|setInterval" crates apps /path/to/downstream --glob '!target/**'
rg -n "PublishTarget::Explicit|PublishRaw|UnsignedEventToRelays" crates --glob '!target/**'
rg -n "short_npub|format_ago|SF Symbol|status_label|display_label|avatar_initials" crates docs --glob '!target/**'
rg -n '@nostr-dev-kit|NDKEvent|event\\.publish|\\$subscribe|tagsJson|hl.network.wifi_only' /Users/pablofernandez/Work/hl --glob '!**/.git/**'
rg -n "KernelModel\\.shared|Task\\.sleep|CarPlay|AppIntent|Nip46|signer|dispatchSilent|snapshot\\(|relay_pending|UnsignedEventToRelays" /Users/pablofernandez/Work/podcast-player --glob '!**/.git/**'
rg -n "build:wasm|resolve_ref|release_ref|setInterval|claim|consume_all_builtin_projections|register_defaults" web/nmp-gallery apps/nmp-gallery --glob '!**/.git/**'
```

Counts are not success by themselves. They are ratchets: they prevent new old
patterns while the milestone ladder deletes or privatizes the existing ones.

Baseline every architecture milestone with the existing gates:

```bash
git status -sb
git diff --check
cargo test -p nmp-testing --test doctrine_lint_smoke
cargo run -p nmp-testing --bin doctrine-lint -- --workspace-d8
cargo run -p nmp-testing --bin doctrine-lint -- --workspace-native
cargo test -p nmp-testing --test doctrine_native_smoke
cargo test -p nmp-testing --bin doctrine-lint
```

When touching actions, publish, FFI, or codegen:

```bash
bash ci/check-native-action-boundary.sh --self-test && bash ci/check-native-action-boundary.sh
bash ci/check-dispatch-envelope-gates.sh --self-test && bash ci/check-dispatch-envelope-gates.sh
cargo run --quiet -p nmp-codegen -- gen action-builders --platform ts --check --out web/packages/runtime-web/src/actionBuilders.generated.ts
```

Existing NMP tests that should become phase gates where relevant:

```bash
cargo test -p nmp-testing --test feed_public_surface_retired
cargo test -p nmp-testing --test public_typed_projection_decode
cargo test -p nmp-testing --test cache_serve_replay_fixtures
cargo test -p nmp-testing --test reduced_source_relay_e2e
cargo test -p nmp-testing --test m2_subscription_compilation_audit
cargo test -p nmp-testing --test m8_subscription_lifecycle
cargo test -p nmp-testing --test framework_magic_contract
cargo test -p nmp-testing --test nip17_dm_inbox_routing
cargo test -p nmp-testing --test dx_scaffold_gate
cargo test -p nmp-testing --test dx_login_timeline_gate
cargo test -p nmp-testing --test conformance_catalog_complete
```

Missing gates to create before declaring the design implementable:

```bash
cargo test -p nmp-testing --test architecture_surface_ratchet
cargo test -p nmp-testing --test live_query_descriptor_contract
cargo test -p nmp-testing --test projection_merge_contract
cargo test -p nmp-testing --test publish_route_provenance_contract
cargo test -p nmp-testing --test docs_architecture_teaching_ratchet
cargo test -p nmp-testing --test downstream_architecture_acceptance
cargo test -p nmp-testing --test service_session_contract
cargo test -p nmp-testing --test gallery_web_runtime_contract
cargo test -p nmp-testing --test highlighter_web_runtime_ratchet
cargo test -p nmp-testing --test highlighter_component_session_contract
cargo test -p nmp-testing --test podcast_nipf4_publish_contract
cargo test -p nmp-testing --test app_feature_api_classification
cargo test -p nmp-testing --test no_polling_downstream_gate
cargo test -p nmp-testing --test read_route_planning_contract
cargo test -p nmp-testing --test protocol_projection_ownership_contract
cargo test -p nmp-testing --test compatibility_surface_contract
cargo test -p nmp-testing --test session_wake_fanout_contract
cargo test -p nmp-testing --test active_session_memory_contract
cargo test -p nmp-testing --test update_cadence_contract
cargo test -p nmp-testing --test browser_storage_lifecycle_contract
cargo test -p nmp-testing --test generated_catalog_manifest_contract
cargo test -p nmp-testing --test protocol_taxonomy_authority_contract
cargo test -p nmp-testing --test metadata_privacy_gate_contract
cargo test -p nmp-testing --test binding_surface_strategy_contract
```

## Kill Criteria

Stop, redesign, or ask for a human decision if any of these become true:

- The first descriptor proof cannot sit on existing `ObservedProjection` /
  dependent-interest machinery without creating a second read lifecycle.
- Typed sessions reduce names but not the number of public concepts a product
  author must understand.
- Route provenance requires broad publish-context plumbing that adds more
  permanent concepts than it removes.
- A downstream app can express its real flows only by keeping native-owned Nostr
  policy, direct NDK subscriptions, native publish JSON, or app-domain nouns in
  NMP crates.
- Highlighter web remains the shipping product runtime while direct NDK fetch,
  filter, sign, publish, or cache paths cannot be ratcheted downward.
- Generated adapter/schema work cannot prevent cross-platform payload drift.
- Ratchets cannot be automated or reviewed cheaply enough to stop new
  old-pattern usage.
- P0 cannot produce counted baselines plus "does not increase" ratchets for the
  old public doors and downstream policy leaks.
- P1 cannot prove one lifecycle owner without creating a second read engine or
  preserving the old public door as an equal production path.
- The team cannot decide whether Highlighter web is target-runtime, SSR/
  migration exception, or out of scope.
- `nmp init` cannot generate production architecture without hidden defaults, and
  a separate tutorial preset cannot be clearly labeled.
- The team cannot decide which downstream migrations are release gates versus
  follow-up issues.
- Podcast headless/runtime surfaces require native-owned state to work.
- Podcast NIP-F4 publishing cannot progress beyond constructed JSON,
  `relay_pending`, or native-selected relays/signers.
- Widget/AppIntent/CarPlay/remote/Live Activity/Handoff flows require the UI
  process's `KernelModel.shared` or shell-local durable product state to be
  correct.
- Gallery web cannot build or run against the same generated ref/session
  contract as other shells.
- Ref lifecycle requires retry/reclaim timers or claim-every-render/tick
  behavior to stay correct.
- The signer support matrix cannot converge on one Rust-owned status and
  continuation model.
- Explicit relay selection is meant to be a user-visible product affordance, but
  the product cannot specify its owner, audit text, and route guarantees.
- Generated app-feature APIs expand into a second framework instead of deleting
  hand-written glue and old public doors.

## Fitness Checks

The destination is not reached until these are true:

- No public builder guide asks product apps to manually pair raw interest open,
  observer registration, replay, projection sidecar, and teardown.
- No production state reconciliation depends on snapshot tick polling.
- New app reads enter through typed session descriptors or named
  substrate/protocol-internal/diagnostic/test/migration acquisition scopes.
- Every dynamic source has deterministic diffing, explicit fallback policy, and
  no wildcard result from empty demand.
- Projection tiers are absent from app-facing docs and starter apps.
- Rendering row/delta caches implement one shared/generated merge contract.
  Hand-authored caches may exist only as thin, proven adapters; they cannot own
  product facts or independent merge semantics.
- Publish builders route through the existing typed action doorway and publish
  engine.
- Publish/action status is visible as typed Rust-owned output.
- `nmp-gallery`, Highlighter, and Podcast Player can express their current core
  flows without native-owned protocol state or app-domain logic inside NMP.
- Clean-room app docs can build a simple app without reading ADRs, issues, wiki
  pages, or old design chats.
- Every retained internal mechanism has a written invariant, rejected simpler
  alternative, and deletion/consolidation decision.
- Baseline counts for old patterns move down or stay flat behind ratchets; no
  milestone is allowed to add a second route, projection, or publish lifecycle
  while claiming progress toward simplification.

## Candidate Direction For ADR

This packet is a candidate direction for the ADR, not the accepted ADR. It
should be the starting point because it is grounded in #2313/#2316, current
code, downstream app audits, and prior transcript evidence, but P-1/P1 evidence
may still reject or narrow parts of it. The ADR should preserve, amend, or
discard these candidate decisions explicitly instead of treating this directory
as authority.

Candidate decisions that now have enough evidence to carry forward unless P-1/P1
contradicts them:

- app-facing reads use typed sessions/descriptors, not raw `open_interest`;
- default public reads are planned/outbox-routed unless a descriptor explicitly
  declares relay-pinned, private, or audited explicit routing;
- app Rust crates may define custom sessions, outputs, reducers, builders, and
  capability needs without moving protocol work to native or app nouns to NMP;
- projection producer ownership replaces public Tier-1/Tier-2 language, with
  schema owner/version, collision failure, and explicit alias/replace rules;
- event construction/finalization, signing, and publishing are separable phases
  inside one Rust-owned action/publish path;
- route provenance is required for automatic, host-pinned, verified inbox,
  manual, diagnostic, and imported/verbatim publish paths;
- `register_defaults()` is not production architecture;
- NIP-29 is a kind-agnostic group envelope/host-route wrapper for writes and a
  group-context admission owner for reads, not a chat/reply/comment namespace.

The ADR still needs to record technical representations:

- final public naming: `typed session`, `FeatureSession`, or per-feature open
  helpers over one descriptor model;
- whether private `ObservedProjection` is retained as-is or narrowed under the
  descriptor;
- whether private `ReducedSource`/feed machinery becomes a smaller source
  reconciler after cross-family proof;
- the exact internal representation for route provenance if existing publish
  fields cannot carry it cleanly;
- the supported signer matrix across local keys, NIP-07, NIP-46 browser/native,
  NIP-55-style platform signers, named product signers, agent signers, and
  imported pre-signed events;
- which compatibility APIs remain available, with live consumers, scope,
  minimum support window, and deletion/formalization gate;
- which first executable gates prove the direction before broad migration starts.

The ADR or product owner must still settle product/scope decisions:

- whether Highlighter web is an NMP target-runtime migration gate, an SSR/
  migration exception, or out of scope for this architecture;
- Highlighter's offline/cache policy, signer support matrix, and NDK deletion or
  exception criteria;
- Podcast's service-session/native-mirror table and the NIP-F4 route/signer/
  Blossom publishing contract;
- gallery web runtime status, signing/auth matrix, and generated ref lifecycle;
- which downstream app migrations are release gates versus follow-up issues;
- whether manual explicit relay selection is user-visible product functionality,
  and if so what ownership, audit text, and guarantees the product promises.

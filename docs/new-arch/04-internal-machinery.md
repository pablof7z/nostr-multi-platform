# Internal Machinery

The developer-facing shape is small, but NMP still owns the machinery needed to
make it correct across platforms.

## Read Pipeline

For reads, NMP internally:

```text
opens a typed session descriptor
  -> compiles source expressions into interests/dependent interests
  -> plans relays with outbox routing or explicit relay pins
  -> records reverse admission/wake indexes for the session
  -> replays cached/store data
  -> admits matching live events into Rust-owned read state
  -> emits bounded typed projections
  -> tears everything down when the handle closes
```

Protocol crates own the meaning of protocol queries. The typed session machinery
owns lifecycle, replay ordering, dependency tracking, and teardown.

The current `ObservedProjection` path contains the closest replay-before-live
and scoped-admission invariant. Reuse or narrow that invariant first; do not
preserve tick-polled `ActiveObservedProjection` controller semantics just because
they share the name. The goal is not a parallel read path or a broad new engine.
It is to prove that a small typed descriptor can compile into the safe recipe
feature authors need while deleting polling-based lifecycle repair.

Cache-serve and live ingest must feed the same projection dispatch path. First
registration should drain matching store/cache rows before the first visible
frame is considered satisfied, and that drain must not re-enter `store.insert`.
Wakeups should be keyed by completed served-interest/admission shapes so a
session does not double-serve the same cached rows when live delivery resumes.

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

Browser proof gates must distinguish storage feasibility from full runtime
feasibility. `nmp-sqlite-wasm` can be held to a real Worker/OPFS conformance
gate because it does not need the signing stack. A full `nmp-browser-runtime`
wasm CI gate may remain blocked until the `secp256k1-sys` C wasm sysroot problem
is solved or the runtime is split around that dependency. Do not let that block
hide storage regressions, and do not pretend a TS-only or no-wasm shell proves
the Rust runtime.

This also bounds `nmp-gallery` web proof. A gallery web app that builds only its
TypeScript shell, uses a placeholder wasm build, or degrades when the worker is
missing cannot prove the runtime architecture.

Minimum browser worker contract:

```text
main thread loads app shell
  -> Worker loads the wasm runtime from a known served path/content type
  -> main/worker handshake proves runtime version and capability set
  -> Worker prepares durable storage before start when durable mode is required
  -> snapshot/update callback is installed before app start
  -> app start installs explicit features and pending wake handlers
  -> NIP-07 or browser-only capabilities round trip through main-thread brokers
  -> missing worker/wasm/storage emits typed failure state or fails the proof
```

`hello -> prepare_store -> start` ordering is architectural, not cosmetic. A
runtime that starts first and silently substitutes in-memory storage because OPFS
or wasm setup was inconvenient has changed persistence semantics. A runtime that
registers the snapshot callback after start can lose initial state. A runtime
that lets the worker touch `window.nostr` directly has crossed the browser
capability boundary; the worker must emit a signer request, the main thread
executes `window.nostr`, and the response re-enters Rust through the same signer
continuation/status path.

Browser degraded mode must be explicit per app. A tutorial or diagnostic page may
run in degraded mode if the UI says so and no product-runtime proof depends on
it. A shipping product runtime, gallery conformance proof, or downstream web
migration cannot count degraded/no-wasm/no-worker behavior as success. Missing
wasm, missing Worker support, OPFS failure, Web Locks contention, or unsupported
signer capability should surface as typed runtime status, not as silent success.

Current worker requests should converge on the same public concepts as native
and TUI:

| Current worker/request family | Target concept | Retirement rule |
|---|---|---|
| `resolve_ref` / `release_ref` | typed `ProfileRef` / `EventEmbed` sessions | raw worker protocol hidden or diagnostic only after generated handles land |
| `search_open` / `search_close` | typed `Search` session | no separate search lifecycle recipe if descriptor can express it |
| `group_events_open` / discovery | typed NIP-29 group sessions | host route provenance and admission owned by group feature |
| `dispatch_bytes` | typed action / generated builder doorway | remains transport, not an app-authored JSON/event publish door |
| `begin_sign` / `deliver_signer_response` | signer capability result | parked continuation and status owned by Rust |
| relay config / diagnostics | typed output or diagnostic session | product relay policy stays Rust-owned |

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
- the intent classifier remains the one route from arbitrary user text into
  refs, relay URLs, NIP-05, app scopes, search, or rejection. Product shells may
  collect input; they do not parse Nostr meaning or route secrets.
- offline-first publish intent/ledger state remains the write root. Signing,
  route resolution, relay IO, retry, resume, cancel, local ingest, and status
  attach to one replayable publish identity instead of scattered callbacks.
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
- **Input classifier:** a pasted value or typed command is rejected as a secret,
  resolved as NIP-19/NIP-21, relay URL, NIP-05, registered app scope, or search
  through one Rust-owned classifier. A Swift/TypeScript shell cannot invent a
  parallel protocol parser for convenience.
- **Offline publish:** a write created while offline records local publish
  intent/status first, then later signs, resolves routes, retries, cancels, or
  exhausts through the same publish identity. A relay call with no local status
  ledger fails the proof.
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

Media has a fast path, not an exemption. Volatile playback position, buffered
range, download byte progress, waveform/speech progress, or haptic cadence may
flow as throttled capability/render facts so the UI does not churn full snapshots
at audio tick rate. Durable state such as queue order, current episode, sleep
timer policy, auto-advance, segment boundary, persisted position, downloaded
file ownership, and publish/share result remains Rust-owned and enters through
actions or capability results. A CarPlay, AppIntent, remote command, widget, or
Live Activity loop that sleeps and rechecks Rust state to repair UI correctness
is polling, not media sampling.

Every remaining tick observer or timer that touches reducer/session state needs
an explicit invariant, owner, and deletion or formalization decision.
This includes `recv_timeout` or timeout-based cancellation loops when they are
used as readiness checks. Blocking receive, explicit cancellation messages, OS
callbacks, capability results, and actor wake events are fine; "wake up every N
milliseconds and see whether the condition changed" is polling regardless of the
primitive used.

Downstream audits must classify timers before judging them. The
`no_polling_downstream_gate` should report at least these buckets:

- correctness polling: sleeps, intervals, retry loops, claim loops, or refresh
  tasks that make Rust/session/projection state eventually correct;
- OS timeline fallback: WidgetKit, Live Activity, Handoff, or CarPlay loops that
  substitute native mirrors for Rust-owned state;
- external protocol/provider status reads: Cashu mint quotes, STT/TTS/LLM/
  transcript APIs, or other upstream services that expose no push callback, with
  Rust-owned job state and cancellation;
- media-clock sampling: volatile playback/download/progress facts only;
- tests and diagnostics: bounded waits that do not ship as product behavior;
- animation/presentation timers: purely visual affordances.

Correctness polling and OS timeline fallback fail unless an ADR records a narrow
architectural exception and a deletion/formalization gate. External protocols
that are inherently status-read based do not get sleep loops in protocol code;
they get one typed read/status capability and explicit, bounded kernel/session
scheduling. External status reads, media sampling, tests, diagnostics, and
presentation timers may remain only if they cannot mutate durable product truth
or hide missed session wakes.

## Non-Goals

- Do not expose a generic raw event callback as the main app API.
- Do not make `open_interest` the app read model.
- Do not let native compute dynamic source sets, route relays, or mutate event
  tags for protocol correctness.
- Do not collapse every protocol-specific publishing rule into `nmp-core`.
- Do not turn `LiveQuery` into an object that owns protocol meaning. Protocol
  and app crates own meaning; typed read-session machinery owns lifecycle.
- Do not present this document as shipped API before ADR and migration work land.
- Do not move product domains into NMP crates because one downstream app needs
  them.
- Do not let compatibility paths remain public teaching examples once the typed
  lifecycle exists.

## Implementation Plan

Each phase must leave the repo shippable and reduce at least one public concept,
duplicate lifecycle recipe, or hidden desync state.

The milestone ladder below is not a fixed full-trajectory plan. It is a
direction map. Keep only the next one to five slices PR-ready, execute one,
measure what changed, then choose the next one to five. A detailed plan for every
downstream app would become stale and would violate the repo's planning
discipline by turning into a parallel backlog.

The ladder is useful because it names the forces that must keep converging:

| Workstream | Scope | Direction proof before area counts as architecture evidence |
|---|---|---|
| A. Baseline and ratchets | old public doors, tick users, direct publish paths, downstream native policy | counted baseline, owner for each count, and CI/doctrine gate for "does not increase" |
| B. Composition/defaults | `register_defaults`, `nmp init`, app roots, protocol feature installers | explicit feature composition as the production model; presets labeled tutorial/compatibility with live consumers, support window, owner, and deletion/formalization gate, or deleted |
| C. Session descriptor | one typed lifecycle owner over acquisition, replay, sink, output, and teardown | one simple real session migrated without a new engine or public `ObservedProjection` API |
| D. Shape/source reconciliation | follow/list/group/thread/embed/account/ref source sets | private reconciler proof for source arrival, withdrawal, empty-source fail-closed, fallback, account switch, replay, route replanning, and teardown |
| E. Output/projection contract | projection ownership, schema/version, sidecars, host caches | one owner per output key, collision failure, shared merge semantics across generated/host adapters |
| F. Read routing/admission | outbox routing, relay-pinned sessions, private reads, explicit overrides | read route policy plus relay/source admission proof in descriptors and replay/live admission; no shell relay policy |
| G. Write routing/publish | publish intent ledger, event construction/finalization, signer selection, publish route provenance | one publish doorway distinguishes automatic, host-pinned, verified inbox, manual, and imported routes while preserving local offline-first status |
| H. Input/ref/action classification | user text, NIP refs, relay URLs, NIP-05, search, app scopes, diagnostics | one Rust-owned classifier maps arbitrary input into typed refs, sessions, actions, or rejection; no shell protocol parsing |
| I. Signer/status runtime | local, NIP-07, NIP-46, NIP-55-style, named product, agent, imported event | Rust-owned pending/ready/failed/signed status and parked continuation model across platforms |
| J. Service/capability flows | widgets, AppIntents, CarPlay, remote commands, Live Activities, Handoff, media/STT/AI | first try typed actions, headless invocation, capability results, or last-emitted mirror frames; no public service-session noun unless those fail |
| K. Generated adapters/codegen | action builders, output schemas, row caches, FFI/runtime bridges, schema/binding/storage migrations | generated or contract-tested drift prevention for every cross-platform payload used by migrated flows; FF-031 holds for every migrated contract |
| L. Single-writer cross-cutting gates | generated catalogs/manifests, protocol taxonomy, metadata privacy, client identity | each has one owner, no per-NIP/product branch tables in generic layers, and a ratchet or design-review gate before it counts as proof |
| M. Downstream/browser proofs | Highlighter, Podcast Player, `nmp-gallery`, browser runtime, sanity checks from 29er/Olas | each selected slice moves a classified acceptance row toward migrated/deleted/scoped/ratcheted; downstream app nouns stay out of NMP crates; degraded wasm/worker/runtime modes fail closed |
| N. Durable docs/ADR retirement | ADR, builder guide, product specs, templates, wiki, issues | local packet retired; durable docs corrected in place; tactical work lives only in GitHub issues |

Ordering rules:

- A and B happen first. Without baselines and explicit composition, later work can
  look cleaner while old public doors keep growing.
- C proves the lifecycle owner before D or E generalize anything. #2307-style
  P2 work is allowed before C only as a lifecycle-deletion prerequisite: it may
  remove duplicate tick-polled reconcilers, but it does not promote
  `ReducedSource` or a source core as architecture.
- D may stay private until at least two non-trivial source families prove
  identical semantics. Do not promote `ReducedSource` because one feed path
  exists; active-account, pointer/ref, group/list, and feed sources must either
  converge on one reconciler or remain private feature-local machinery.
- E must land before broad host migration, or every shell will invent its own
  merge/cache contract again.
- F and G must preserve route provenance before downstream NIP-29, NIP-17,
  Podcast NIP-F4, or pre-signed/imported flows migrate.
- H and I are required for real apps, not follow-up polish. A design that only
  works while the foreground UI process is open is not the NMP app architecture.
- J is deliberately not initial public vocabulary. Service-like flows must prove
  typed actions, headless invocation, capability results, and last-emitted mirror
  frames are insufficient before a service-session abstraction is accepted.
- M is a direction gate, not post-ADR polish. A downstream row cannot count as
  proof until it is classified and moved by an actual slice.
- N prevents this packet from becoming another parallel source of truth.

Rolling planning loop:

```text
select 1-5 slices from the highest-risk gates
  -> verify existing seams first
  -> migrate one real caller family
  -> delete, privatize, or scope an old path
  -> add the ratchet that prevents backsliding
  -> re-baseline counts and choose again
```

Do not keep a stale per-phase roadmap alive after reality changes. If a slice
does not reduce old-pattern counts or produce a stronger ratchet, the next move
is to narrow or reject the abstraction, not to continue down the old plan.

Current rolling-horizon recommendation:

| Step | Slice | Why this is first | Must get smaller |
|---|---|---|---|
| 1 | P-1a NMP public-door and pre-ADR source-of-truth classification dossier | freezes the old architecture before any new name lands and records which ADR/doc facts are candidates to fold, survive, or retire after redesign acceptance | unclassified raw read/write/defaults/projection/doc doors stop being treated as compatibility requirements |
| 2 | P0a reproducible surface-ratchet checks | turns old-door counts into exact scripts, commit, exclusions, owners, and "does not increase" gates | raw `open_interest`, filterless observer doors, hidden defaults teaching, projection-tier teaching, snapshot ticks, direct publish doors, and shell policy sites stop growing |
| 3 | P0b explicit composition/defaults/scaffold proof | proves B before deeper machinery and stops tutorial convenience from masquerading as production architecture | production `register_defaults()` teaching, scaffold defaults, and gallery/defaults hidden composition shrink or become tutorial/migration-scoped |
| 4 | first typed descriptor proof over an existing real session | proves #2316 lifecycle ownership before broad adoption and forces the abstraction to retire one old recipe | one hand-wired open/replay/sink/output/teardown recipe becomes private or compatibility-scoped; default proof is an existing feed/session path; Highlighter `RoomHome` is the default downstream proof only if downstream adoption is the blocking question, while gallery `EventEmbed` counts only as a core descriptor proof unless the explicit question is ref lifecycle |
| 5 | #2307 event-driven observed-projection reconciler, or swap before row 4 if the chosen proof sits on its tick-repair path | deletes duplicated lifecycle repair without making the descriptor carry old polling semantics | duplicated `ActiveObservedProjection`/`DynamicObservedProjection` modules and account/source snapshot-tick usage |

Only these rows should be treated as near-term plan, and rows 4-5 may swap based
on the first caller's actual dependencies. Later P3-P8 material is a direction
map and acceptance matrix; choose later slices after the counts and ratchets from
steps 1-5 are real. If a different risk blocks ADR signoff sooner, replace step
5 with the smallest publish-provenance proof or Highlighter web inventory, but do
not claim a descriptor proof from that replacement.

#2320 is not step 5 as a durable edit. Before redesign acceptance, it is a
classification dossier: each ADR/doc fact is marked folded into redesign, folded
into another owner, still-current standalone, or retire/delete candidate. Actual
ADR deletion, README rewrites, and builder-guide/wiki corrections happen with or
after the accepted redesign ADR, not before the architecture direction is
settled.

Current downstream proof selector:

| Question to answer now | Best first proof | Counts only if this old surface shrinks | Falsifies the direction if |
|---|---|---|---|
| Can one typed lifecycle owner replace hand-wired read recipes? | an existing feed/session path by default; Highlighter `RoomHome` if downstream proof is blocking; gallery `EventEmbed` only as a core descriptor proof | raw open/replay/sink/output/teardown wiring becomes private, deleted, or compatibility-scoped for that caller | the proof needs a second read engine or still asks the app to assemble `ObservedProjection`, sidecars, source repair, and teardown |
| Can browser/runtime support the same architecture as native? | gallery `EventEmbed` plus real wasm/Worker startup and generated host handle | placeholder `build:wasm`, raw worker ref messages, and correctness `setInterval` release/reclaim loops disappear or become diagnostic only | web can only pass by silently degrading to no-wasm/no-worker/in-memory runtime behavior |
| Can headless/service surfaces use the same model? | Podcast AppIntent or CarPlay action returning Rust-owned pending/error/completion state | `KernelModel.shared`, App Group correctness mirrors, and polling wait loops stop being correctness paths | a second service/headless framework is needed or native must own queue/playback policy |
| Can write-side provenance remain small? | Highlighter share-to-room or Podcast NIP-F4 publish through the existing publish doorway | fire-and-forget raw writes, anonymous explicit relay status, and optimistic `last_published_at` shrink or become scoped | the only fix is a broad publish stack while old raw publish paths stay production-equal |
| Can docs stop recreating the old architecture? | #2320 ADR/source-of-truth classification before redesign acceptance, then full fold/retire after the redesign ADR lands | stale ADRs, builder-guide pages, and wiki pages stop teaching `register_defaults`, raw `open_interest`, projection tiers, or public `ReducedSource` | the accepted redesign has to coexist with old ADR guidance as a parallel public architecture |

Selection rule: after the P-1/P0 baseline, pick the proof whose risk is
currently blocking adoption. Prefer an existing feed/session path for the first
descriptor proof unless it depends on the #2307 tick-repair path; in that case,
run the reconciler deletion first. Use Highlighter `RoomHome` when the blocking
question is downstream adoption, and use gallery `EventEmbed` only when the
blocking question is specifically ref/embed lifecycle. A gallery `EventEmbed`
core-only proof does not satisfy P4 cross-shell ref/web proof. Use Podcast only
when the open question is service/headless behavior; use the publish proof only
after the route-provenance carrier can be attempted inside the existing publish
path. Do not start with a clean-room demo unless the goal is only documentation
UAT; it cannot prove old surfaces shrink.

Near-term slice contracts:

| Slice | First seam to try | PR-sized acceptance | Failing answer |
|---|---|---|---|
| P-1a source-of-truth/public-door dossier | live symbols, current issue bodies, current ADR/doc owners | every old read/write/defaults/projection/runtime door is classified as delete, private, formalize, diagnostic/test, or migration with owner/support window/removal gate | stale docs or closed issue text are treated as compatibility requirements |
| P0a ratchet baseline | `rg` inventories plus `nmp-testing` doctrine gates | old-pattern counts have exact commands, path exclusions, baseline commit, owner, and "does not increase" enforcement path | illustrative grep counts become a fake gate no one can reproduce |
| P0b explicit composition/defaults proof | existing `register_substrate`, native/browser builders, explicit installers | one production start/scaffold stops hiding social defaults while substrate remains easy to install; any preset is labeled tutorial/migration/test with consumers and deletion/formalization gate | a new builder/bundle object wraps `register_defaults()` while hidden composition remains the normal path |
| P1 first typed read descriptor proof | existing feed/session or observed-session machinery, not a new engine | one real caller owns acquisition, route/admission, replay, sink, output, wakes, status, and teardown; one old open/replay/output/close recipe is deleted, privatized, or migration-scoped | descriptor/handle is a facade and the caller still assembles `open_interest`, `ObservedProjection`, sidecars, output declarations, or close tokens |
| P2 event-driven reconciliation proof | existing identity/source/mailbox/refcount event hooks and #2307 controller copies | tick-polled account/source observed-projection repair is deleted where events exist; remaining tick use is classified with owner and deletion/formalization gate | a fifth reconciler or compatibility alias is added around the old polling semantics |
| P6 route-provenance proof, if write risk replaces P2/P1 | existing `PublishTarget`, `RelaySelectionReason`, `PublishRecord`, parked signer obligation, and status pipeline | one producer that still knows route meaning supplies provenance class/reason through build/finalize, signing, retry/resume, local ingest, and status without reviving `RoutingContext::explicit_targets` | route provenance requires a broad second publish context while `Explicit { relays }` remains product-equal and anonymous |

The first implementation wave should normally choose P-1a, P0a, P0b, P1, and
P2. Swap P6 into that wave only if write provenance is the current ADR-blocking
risk. Downstream matrices, browser storage, generated catalogs, and broader
P3-P8 gates guide the next selection, but they are not excuses to delay the
smallest proof that one old lifecycle recipe can actually disappear.

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
turning typed descriptor/handle, route provenance, generated adapters, and
service/capability-flow abstractions into additive layers over the old machinery.

Do not create `nmp-live-query`, `nmp-source`, `nmp-service-session`,
`nmp-route-provenance`, or similar new crates before at least two migrated
callers prove the same abstraction and one old public door is retired. Start in
the existing feature, protocol, app, or substrate owner; promote only after the
shared invariant is demonstrated.

If a new crate is still justified, the release manifest is a blocking
architecture gate, not release clerical work. The crate must be explicitly
classified in `release/nmp-release.toml` as public or private, with a reason for
private packages and normal release gates (`ci/check-release-manifest.sh`,
package dry-run) proving it is not an accidental framework surface.

Per-phase retirement checklist:

| Question | Failing answer |
|---|---|
| Which old public door did this phase delete, privatize, or scope? | "None; new code will use the new path later." |
| Which docs/templates stopped teaching the old recipe? | "Docs will be updated after implementation." |
| Which live callers still use the old path and why? | "Unknown" or "maybe downstream." |
| What makes the old path impossible to grow? | "Code review discipline." |
| What is the removal/formalization trigger? | "When we have time." |

A phase that adds typed sessions, route provenance, generated adapters, or
service/capability-flow abstractions while leaving old public recipes as equally
valid production paths fails even if the new tests pass.

## Proof Ladder

The implementation plan should advance only if each rung proves both behavior and
simplification. A green test is not enough if the old public recipe remains a
normal path.

| Rung | Proof | Continue only if | Stop or narrow if |
|---|---|---|---|
| 0. Classified baseline | every old door is production/internal/test/doc/migration/delete, with live issue state checked | the team knows what is being deleted, privatized, or formalized; closed issue bodies are treated as evidence, not active architecture mandates | important callers remain unclassified or stale issue/wiki wording is treated as current truth |
| 1. First lifecycle owner | one simple real session owns acquisition, replay, sink, output, wakes, status, and teardown | old recipe count decreases or is compatibility-scoped | typed descriptor/handle wraps the old recipe without retiring it |
| 2. Event-driven reconciliation | #2307-style reconcilers collapse and tick polling is removed where event hooks exist | duplicate controllers and account/source polling disappear | a fifth reconciler or compatibility alias is added |
| 3. Clean-room app UAT | a generated/scaffolded app opens and renders one migrated feature after a real lifecycle proof exists | the app author never touches `open_interest`, projection tiers, or sidecars | tutorial/demo code is used as proof while old callers remain untouched |
| 4. Output/adapter contract | one output uses generated/contract-tested full/delta/clear/stale/poison semantics across shells | shell caches are render adapters only | any shell owns independent merge or product cache semantics |
| 5. Publish route provenance and read admission proof | write routes retain class/reason through status/retry, and reads reject replay/live events without route/source proof | anonymous explicit-route and relay-pinned read paths collapse to one carrier or are deleted | publish provenance requires broad wrapper plumbing or read-side proof leaks into shell relay policy |
| 6. Downstream proof | Highlighter, Podcast Player, and gallery matrices pass or trigger kill criteria | no app exports Nostr policy into native shells or NMP app nouns | a downstream app needs native-owned policy to ship |
| 7. Retirement | accepted facts move into durable docs/issues and `docs/new-arch` is retired | this packet is no longer needed as current authority | docs/new-arch becomes a parallel plan |

Each rung has a stop point. If rungs 1-2 do not reduce concepts or call sites,
the architecture should be narrowed before touching downstream apps. If rung 3
works only as a demo while real callers still use old recipes, it is not proof.
If rungs 4-6 require app shells to own Nostr policy, the architecture is
preserving NMP by exporting complexity and should be rejected.

**P-1: Concept disposition and live-consumer audit.**
Before P0 inventory turns into implementation work, classify every disputed
concept and public door:

Current recipe inventory to verify against live code before implementation:

| Current recipe/surface | Owns today | Misses or leaks |
|---|---|---|
| filterless `KernelEventObserver` / `register_event_observer` | all accepted-event fanout to caller-owned filtering | no declared acquisition, replay shape, relay pin, bounded owner, typed output, or teardown; encourages live-only/global read models |
| raw `open_interest` / `nmp_app_open_interest` | acquisition and store/cache eligibility | no typed output, admission owner, projection lifecycle, or app-visible delivery contract |
| `KernelAction::OpenUri` / input-intent dispatch | URI/text/action classification plus direct raw interest/view routing | can bypass typed ref/session descriptors unless mapped to `ProfileRef`, `EventEmbed`, search, or app-owned actions |
| `nmp.browse_relay` | relay-pinned interest from host action | diagnostic/prototype unless wrapped by an audited app Rust descriptor with output/status/teardown |
| `open_observed_projection` | replay-before-live sink registration, scoped future delivery, close token | still asks feature authors to pair acquisition/output/schema/route policy manually |
| `open_feed` / feed sessions / feed controllers | one feed-shaped source compiler, dependent interests, observed sinks, feed output, pull/load-older controllers, perspectives | feed-local public machinery; `FeedRegistry`, `FeedController`, `PullFeedController`, `FeedPullPager`, and custom perspectives need private/session disposition |
| group/search feature `open_*` recipes | feature-specific route/source/projection bundle | repeated lifecycle recipe; not a general session contract for app-defined features |
| refs/embeds / `resolve_ref` | component demand and some typed ref outputs | cross-shell raw worker/ref adapters, claim/reclaim loops, and hand caches still need convergence |
| pull cursor / `nmp_mirror_pull_page` | raw ingest-log pagination for mirror/export/history | must not become screen-state reconstruction or a bypass around typed output sessions |
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
| `KernelEventObserver` / `register_event_observer` / C-ABI observer callbacks | parser/cache internals only; product filterless fanout rejected | Rust/C/WASM public observer symbols classified; doctrine no-raw-tap gate forbids production reintroduction; old docs corrected around declared sessions |
| raw `open_interest` / `nmp_app_open_interest` | substrate/protocol/diagnostic/migration only | current callers classified; public docs stop marking it as product PASS without scope |
| `KernelAction::OpenUri`, input intents, `nmp_app_open_uri`, intent classify/dispatch | typed ref/search/action entrypoints or delete dead variants | no direct raw-interest routing remains in production; `Start/Stop/OpenView/CloseView/RunDiagnostics` are runtime/diagnostic or removed |
| `nmp.browse_relay` | diagnostic/prototype or audited app-Rust relay-pinned session | owner, purpose, non-product scope, or output/status/teardown contract |
| `NmpApp::open_feed`, `open_observed_projection`, `nmp_app_open_interest`, `resolve_ref` | existing surfaces to unify under typed descriptor contract | contract map showing which lifecycle rows they already own and which they miss; do not invent migration evidence for stale or missing symbols |
| feed registries/controllers/perspectives/load-older | private machinery or compatibility behind sessions | `FeedRegistry`, `FeedController`, `PullFeedController`, `FeedPullPager`, `PerspectiveRegistry`, custom perspectives, and `nmp_app_load_older_feed` classified by live callers |
| pull cursor / `nmp_mirror_pull_page` / raw-log ABI | mirror/export/history/pagination/diagnostic only | no product screen uses raw log pull to reconstruct live UI state |
| runtime lifecycle FFI | runtime control only | start/configure/stop/reset/foreground/background/liveness/callbacks cannot encode product lifecycle policy |
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
- New schema, binding, storage, or render-cache migrations must declare the old
  and new writers, compatibility window, deletion trigger, fixture/codegen gates,
  and downgrade/fail-closed behavior before a dual path ships.

**Classified baseline requirement before P0.**
Raw grep counts are useful smoke alarms, but they are not a migration plan. Before
P0, classify each old-pattern family by lane:

```text
production product path
substrate/protocol-internal path
diagnostic/export/test path
generated artifact
durable doc/current teaching
historical/stale doc
migration shim with owner and deletion/formalization gate
delete now
```

Run this classification for `open_interest`, `KernelEventObserver` /
`register_event_observer` / C-ABI observer callbacks, `open_feed`/feed sessions,
feed controllers/perspectives/load-older, `KernelAction`/input-intent URI doors,
`nmp.browse_relay`, pull cursors/mirror ABI, runtime lifecycle FFI,
`resolve_ref`/component refs, `ObservedProjection`, `ReducedSource`/source
reconcilers, `register_defaults`, `declare_consumed_projections` /
`consume_all_builtin_projections`, explicit publish routes, snapshot tick
observers, browser degraded-runtime fallbacks, and gallery reclaim loops.

The output is a public-door disposition ledger. The table below is the required
template; the actual P-1 artifact is incomplete until every `classify` placeholder
is replaced with live `rg` evidence, owner issue, support window if retained, and
target disposition.

| Door/concept | Live production callers | Non-production callers | Target disposition | First deletion/formalization proof |
|---|---:|---:|---|---|
| filterless `KernelEventObserver` / event-observer ABI | classify | classify | parser/cache internal, diagnostic/test, or delete | declared session/observed sink migrates any product read model |
| raw `open_interest` / `nmp_app_open_interest` | classify | classify | internal/diagnostic/compat or delete | first typed session migrates equivalent product read |
| URI/input intent raw routing | classify | classify | typed ref/search/action or diagnostic/delete | OpenUri maps to typed sessions without raw interest bypass |
| `nmp.browse_relay` | classify | classify | diagnostic/prototype or audited app session | relay browser is scoped or formalized with output/teardown |
| `open_feed` / feed session APIs | classify | classify | narrow into typed session or compatibility | feed/session convergence proof |
| feed controllers/perspectives/load-older | classify | classify | private under sessions or compatibility | pagination/action model replaces public feed machinery |
| pull cursor / mirror raw-log ABI | classify | classify | mirror/export/history/pagination only | product screens use typed outputs, not raw log pull |
| runtime lifecycle FFI | classify | classify | runtime/capability control only | no product feature lifecycle hidden in start/stop/reset/callbacks |
| `resolve_ref` / worker ref protocol | classify | classify | generated typed ref session or diagnostic | gallery ref lifecycle proof |
| `ObservedProjection` public registrar | classify | classify | private machinery or narrowed seam | descriptor proof over replay-before-live |
| `ReducedSource` / feed source compiler | classify | classify | private dynamic-source reconciliation | two source-family proof before generalizing |
| defaults/projection declarations | classify | classify | explicit composition/output demand or tutorial | scaffold/docs ratchet and composition ledger |
| anonymous explicit publish routes | classify | classify | one provenance carrier or delete dead seam | publish route contract |
| snapshot ticks/reclaim timers | classify | classify | event-driven wakes or bounded presentation only | no-polling/reconciler proof |

Do not use unclassified call sites as evidence that a compatibility surface must
survive. Unknown defaults to "not yet justified," not "keep."

**Schema, binding, and state migration gate.**
Typed sessions and generated adapters change contracts, not just names. Any
slice that touches output schemas, FlatBuffers/update frames, C/JNI/UniFFI/worker
bindings, host render caches, runtime storage, or app mirror state must ship with
one migration story:

```text
single writer for the fact
  -> schema/version or storage compatibility rule
  -> generated fixture/check gate
  -> old host cache/binding/state path classified
  -> deletion or support-window trigger
```

Dual reads, dual writes, or dual caches are allowed only as migration-scoped
compatibility with a named owner, support window, and fail-closed behavior. They
must never become two equal sources of truth. A generated binding that preserves
old `open_interest`, JSON publish, raw worker refs, or hand merge semantics under
new names fails this gate.

This preserves the FlatBuffers/version-pin and snapshot/projection lessons from
the wiki evidence: wire/schema drift and transactional merge failures are
architecture defects, not CI trivia. The first schema-affecting migrated session
must prove full, delta, clear/tombstone, stale-frame, decode-poison, baseline
recovery, fixture regeneration, and old-cache deletion or compatibility scope in
the same slice.

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
filterless event-observer public symbols and product call sites,
input-intent/URI action doors, `nmp.browse_relay`, feed controller/perspective
public APIs, pull cursor/mirror raw-log APIs, runtime lifecycle FFI,
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
real session as proof. The default first candidate is the existing feed-session
path because it already owns compile, teardown, and close; choose a different
caller only if the P-1 dossier names the specific surface and why it is a better
proof. Acceptance: replay-before-live and close-both invariants
still pass in observer replay, descriptor idempotence, reducer parity, and
`nmp-defaults` feed open/close tests; no new public API is taught. The proof
must cover the chosen caller's route policy, cache replay, dependency/source
changes if that caller has them, and open/close teardown.
The migrated session must publish a session-family contract covering
acquisition, route planning, replay, live sink, admission, output, wakes,
teardown, and error/status state. If any fragment remains caller-authored
outside that contract, P1 has not solved #2316.
For the first accepted session family, route planning/admission and wake fanout
must be executable enough to reject bad replay: a relay-pinned cached event
without stored relay provenance or another protocol-approved admission proof is
not accepted merely because its tags match. Broad `read_route_planning_contract`
and `session_wake_fanout_contract` can remain post-ADR; bounded first-session
variants are ADR-blocking.
It may use an existing feed/search/group-style observed session, but it should
not also claim gallery/component-ref migration. P1 proves the lifecycle owner;
P4 proves the first cross-shell ref/embed migration.

Rolling-horizon note: #2307 moves before P1 only when the chosen first caller
sits on duplicated tick-repair machinery. A descriptor that depends on duplicated
tick repair can pass tests while leaving the old lifecycle problem intact. A
descriptor proof over an existing path that does not depend on that repair may go
first, but it still must record whether account/source observed-projection
reconciliation is irrelevant, event-driven, or migration-scoped with owner and
removal gate.

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

#2307 is the concrete proof slice for P2. It identifies four near-copy
reconcilers plus account-keyed snapshot-tick polling:

- `nmp-defaults/src/runtimes/active_observed_projection.rs`;
- `nmp-native-runtime/src/runtimes/active_observed_projection.rs`;
- `nmp-native-runtime/src/op_feed_defaults/dynamic_observer.rs`;
- `nmp-browser-runtime/src/feed.rs`'s private `DynamicObservedProjection`.

The acceptable result is one D0-clean event-driven reconciler that composes the
existing `ObservedProjectionRegistrar` and `IdentityChangeRegistrar`, migrates
current active-account consumers, deletes the duplicate controllers, and adds a
ratchet against reintroducing snapshot-tick account sampling. A compatibility
alias or a fifth reconciler fails P2.

Near-term P2 scope:

- add one small private `ObservedProjectionReconciler` under existing substrate
  ownership, not a new crate and not a public app API;
- delete the defaults/native/browser active/dynamic observed-projection copies
  that only differ by runtime placement;
- remove account/source `register_snapshot_tick_observer` usage where
  sign-in/sign-out/account-switch/source-change events already exist;
- keep any still-unmigrated tick users on an explicit allowlist with owner,
  reason, and deletion/formalization issue;
- add tests for no-account no-op, sign-in opens once, account switch closes then
  opens, sign-out closes, failed open leaves no stale current id, and no
  snapshot-tick account sampling regression.

If this slice cannot delete the duplicate controllers without creating a new
read engine or broad `AppHost` extension, stop and narrow the architecture before
adding typed-session surface.

**Later Gates P3-P8.**
P3-P8 below are an acceptance matrix, not the current implementation plan. They
become implementation slices only when selected by the rolling loop and backed by
an issue/ADR section with exact baseline, owner, and deletion target.

**Later gate P3: Make scoped session demand own scoped output demand.**
Prove that opening a session can declare its typed output. Keep
`DeclaredProjections` as private executor/cost machinery until session-scoped
demand proves an equivalent chokepoint and footgun guard. It is not automatically
legacy, but it is also not app-facing composition language. Acceptance:
`public_typed_projection_decode` still proves external decode; generated adapters
still handle full, delta, clear, stale-frame, baseline, transactional merge, and
D6 poison semantics. `declare_consumed_projections` must stop being taught as the
app manifest for screen/session outputs.

**Later gate P4: Migrate component refs and gallery embeds first.**
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
Missing wasm, missing Worker storage, or OPFS/SQLite open failure may produce
typed diagnostics, but it cannot silently become a successful in-memory product
runtime for proof purposes. The web runtime either proves the durable worker path
or the e2e gate fails closed. P4 may classify and fail-close web without
counting that as browser proof. It counts as browser proof only when the app
package's own wasm build, Worker startup, OPFS lifecycle, generated ref adapter,
and Playwright path consume the same artifact.
P4 starts only after P1 proves the descriptor lifecycle and P3 proves output
ownership/merge semantics. It is the first real migrated session family across
all shells, not a prerequisite for the minimal descriptor proof.

**Later gate P5: Migrate dynamic and composite reads only after P1-P4 hold.**
Feed, group, search, pointer-source, thread refs, and live-count outputs move to
the same descriptor model. Source-specific reducers stay local unless a shared
core deletes duplication. Acceptance: feed reduced-source tests, real-relay
reduced-source tests, group/search tests, and empty-source fail-closed tests
cover account switch, source change, relay pin, cache replay, and teardown.

**Later gate P6: Collapse write variants by invariant, not by new names.**
Generated builders keep using `DispatchEnvelope` and `ActionModule`. First try
to unify `UnsignedEvent`, `UnsignedEventToRelays`, pre-signed publish, signer
selection, target/provenance, correlation id, and policy validation without
adding new public types. Existing `PublishTarget` may be widened or paired with a
small provenance field; using it unchanged is not sufficient because
`Explicit { relays }` lacks the audit class/reason. Add a named draft/context
type only if it deletes branching or duplicate route/privacy/protocol state.
Gates: publish policy, D10 private routing, signer continuation, generated
builder round-trip, and action-result tests. #1538 was closed by PR #1600, so
the dead `RoutingContext::explicit_targets` seam is already gone. Explicit relay
cleanup is no longer about choosing between two seams; it is about preserving
provenance through the surviving `PublishTarget::Explicit`/publish-status path.
Do not reintroduce a broad routing context to solve this. Route provenance is
the critical missing invariant:
manual explicit relay, NIP-29 host pin, verified NIP-17 inbox, and
external/verbatim publish must not collapse into an indistinguishable
`Explicit` bucket. Tests must prove generic raw publish cannot accidentally
bypass NIP-29 `h`-tag/group-route proof or NIP-17 verified-inbox policy, and
that remote signer continuations preserve route provenance plus correlation id.
The first implementation slice should attempt the smallest carrier change:
extend or split the existing target/reason/status pipeline so provenance class
travels through `PublishCommand`, parked signer publish obligations, engine
records, retry/resume, and status output. A broad draft/context object is a
second-choice representation, not the destination.

**Later gate P7: Prove downstream apps before declaring the architecture final.**
Highlighter must express home feed, room chat, search, comments, share-to-room,
capture, feedback, signer flows, artifact share, article lookup, and room
discussion through app-owned Rust features and NMP runtime dispatch. Direct web
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

Highlighter web needs an inventory table before the ADR can count it as covered:
path, flow, product-runtime vs SSR-only vs diagnostic, target Rust session/action,
signer path, route policy, cache owner, status output, and deletion gate. A broad
"SSR exception" is not enough. SSR exceptions are read-only public data by
default, with explicit cache TTL/durability, no signer/session truth, no silent
production memory fallback, and separate treatment for NIP-05 server writes.
The Highlighter NMP bridge itself is not proof while the actual product runtime
still depends on NDK/Blossom/session packages for product reads, writes, local
sessions, caches, and signer behavior. The ADR must classify those paths as
target-runtime migration, SSR/public-cache exception, diagnostic, or out of
scope before Highlighter web is used as evidence.

Required Highlighter matrix shape:

| Flow family | Current path to classify | Target proof | Deletion/exception criterion |
|---|---|---|---|
| Web onboarding/profile | NDK event build/sign/publish, `$subscribe`/`fetchEvents`, local signer/session storage | typed Rust action/builder, signer status, route provenance, cache policy | NDK path deleted, or ADR labels SSR/diagnostic/out-of-scope with owner and removal gate |
| Web runtime cutover | NMP web bridge plus real product NDK/Blossom/session packages, degraded fallback, LocalStorage sessions | product runtime uses typed NMP sessions/actions or is explicitly out of NMP proof scope | bridge/demo paths stop counting as product proof while old runtime remains |
| SSR/public reads | server NDK reads, route loaders, Upstash cache, timeouts, front-page/room/search/artifact previews | explicit SSR/public-cache boundary with TTL, durability, no signer truth, and no product-session semantics | NDK allowed only inside labeled SSR exception or deleted |
| Managed NIP-05 | web signed auth event, API verification, KV/memory mapping, `.well-known` serving | app/server feature owns auth, durability, status, and public serving contract | not hidden under generic SSR read exception; KV/memory fallback classified |
| Blossom/media policy | direct `NDKBlossom`, avatar/media uploads, Blossom server lists, relay-advertised NIP-96 | typed capability/action with server provenance, signer status, retry/error output | web does not own Blossom route/server policy unless labeled server capability |
| Web rooms/invites/members/chat | NDK relay sets, `$subscribe`, direct sign/publish, tag parsing | NIP-29 group session/action through Rust, kind-agnostic read, host route status | direct NDK group runtime count ratchets down to zero unless explicitly excluded |
| Highlights/capture/import | web capture, selection popover, Kindle import, `NDKHighlight`, throttled publish | Rust app-feature builders/actions with publish intent/status and capability results | no direct web event build/sign/publish for shipped product path |
| Comments/discussions/reactions | TS NIP-22 parsing, filters, comment trees, reaction publish; iOS/Rust typed path partially exists | Rust descriptors for NIP-10/NIP-22/article/highlight refs and typed publish status, with per-platform status | semantic parsing removed from shells; presentation-only transforms documented |
| Blossom/NIP-05/search/SSR | web direct fetch/cache/publish paths | typed capability/result or labeled SSR cache boundary | cache/write owner named; no hidden product truth in web storage |
| Signer/session/offline policy | web/local/native signer inference and Wi-Fi/cache policy | Rust-owned signer/offline/cache state plus raw native capability facts | shells stop deciding signer completion, retry, route, or offline eligibility |
| Cache/offline surfaces | NDK LocalStorage sessions, server Upstash cache, NIP-05 KV/memory, UserDefaults Wi-Fi, App Group queue, ISBN/image caches | each cache classified as durable Rust/app state, server cache, render cache, capability inbox, or migration exception | no cache owns hidden product truth or contradicts Rust state |
| Direct publish/read paths | Rust `ActorCommand::Publish(RawEvent)`, NDK publish, relay constants, fire-and-forget writes | typed builders/actions through publish intent, route provenance, signer continuation, and final status | Rust-owned but raw/fire-and-forget paths still count as violations until migrated or scoped |
| iOS native state/capabilities | Wi-Fi preference, App Group community mirror, pending share queue, image/profile caches, relay URL bridge | each surface classified as Rust-owned policy, native render cache, capability inbox, or migration exception | no UserDefaults/App Group/relay bridge owns durable policy or product truth |
| Capture/OCR/share/Blossom | share extension drain, OCR results, camera/file handles, Blossom upload/download | raw capability results into Rust actions, queue corruption/retry, OCR failure, Blossom failure, publish retry/status proof | native performs capability only; Rust owns temp-file/result/publish lifecycle |
| Semantic parsing | TS/Swift group metadata, NIP-10/NIP-22 parents, artifact refs, comment trees, relay hints, route semantics | Rust descriptors/generated adapters own protocol parentage, canonical refs, group access/admin/member facts, and artifact canonicalization | only visual grouping/formatting remains shell-side with parity tests |
| Highlighter publish/signing | profile, NIP-05, Blossom, room create/invite/join, chat, comments, reactions, highlights, artifacts, capture, share-to-room | typed builders/actions with correlation ids, signer route, route provenance, final publish/action status | no direct `event.publish()`, raw NDK sign/publish, or dispatch-accepted-as-success path |

Podcast Player must express playback, queue, feed subscription, NIP-F4, Blossom
publish, explicit write relays, widgets, settings actions, signer runtime, and
feedback without moving podcast nouns into NMP. Bespoke durable FFI, silent
compatibility paths, stale Swift-store docs, and `nmp-signer-broker` pinning
must converge on generic typed dispatch/projection/capability seams and the
current NIP-46 runtime direction. Widget extensions, AppIntents/Siri, CarPlay,
remote commands, Live Activities/Handoff, and cold/suspended process behavior
must prove typed actions, headless invocation, app-lifetime typed sessions, or
typed capability results rather than native-owned state.

Podcast's acceptance matrix must cover playback/queue/gestures, feed
subscription, OPML/catalog/search/transcripts, widgets, AppIntents/Siri,
CarPlay, remote commands, Live Activities/Handoff, NIP-F4 show/feed/episode/list
publish, Blossom upload/reference publish, local/NIP-46/NIP-55/per-podcast-key/
agent signer paths, explicit relay/server lists, legacy settings, and generated
app FFI. NIP-F4 is not migrated while the path only returns `relay_pending`,
`queued`/`signed`, `publish_dispatched`, optimistic `last_published_at`, stores
constructed JSON, or requires the app to infer relays/signers in native code.
Podcast also proves that service-like success cannot mean "the command was
accepted." AppIntents, CarPlay, remote commands, Live Activities, widgets, deep
links, provider jobs, and NIP-F4 publishes need typed completion/error/status
owned by Rust. Foreground singletons, native sleep loops, and Swift policy for
skip/rate/chapter/deep-link playback are failures unless explicitly narrowed to
raw capability reporting.

Required Podcast matrix shape:

| Flow family | Current path to classify | Target proof | Deletion/exception criterion |
|---|---|---|---|
| Audio execution / Now Playing mirror | AVPlayer/mpv/audio host, Now Playing state, App Group mirrors | native executes audio and reports raw progress/availability; Rust owns current episode, persisted position, sleep timer, auto-advance, queue truth | OS mirrors never become queue/playback source of truth |
| Queue mutation / gestures / remote commands | Swift reorder/prune/dedupe, headphone gestures, CarPlay/remote command mapping | Rust-owned queue and command policy; native reports raw gesture/command metadata | no queue mutation, skip interval, chapter seek, next/previous, or gesture policy outside Rust |
| Feed/subscription/catalog/search/transcripts | app Rust plus Swift stores/import surfaces | app Rust sessions/actions and capability results; no NMP podcast nouns | native DB/UserDefaults classified as render/import cache or deleted |
| Widget/AppIntent/Siri/CarPlay/remote/LiveActivity/Handoff/deep link | UI-process singleton, `KernelModel.shared`, polling, URL/Spotlight/voice-mode policy, App Group snapshots | normal typed action, short-lived headless invocation, service/app-lifetime session, or typed capability result; cold-start/locked-device proof; action completion/result distinct from dispatch acceptance | no `KernelModel.shared` correctness dependency; no polling wait loop; native only reports raw OS activation/command/capability facts |
| NIP-F4/Blossom publish | constructed JSON, `queued`/`signed`, `relay_pending`, `publish_dispatched`, optimistic `last_published_at`, explicit write relays/server lists | show/episode/list/deletion/backfill build/sign/route/store/publish/status with correlation id, signer, event id/naddr, write-relay route provenance, Blossom server provenance, retry state, and key-storage capability | user-facing e2e proves ack/error/retry/exhausted terminal status; stale diagnostics deleted |
| Signers/relays/settings/credentials | local nsec, NIP-46, NIP-55, per-podcast key, agent signer, BYOK/provider keys, Blossom/app/agent relays, legacy relay settings | one signer/status/route/server provenance model plus secure key/provider capability | native no longer infers signer timeout, relay/server policy, key ownership, provider truth, or publish success |
| Agent/provider job lifecycle | STT/TTS/local agents, OpenRouter/Ollama/ElevenLabs/AssemblyAI/Perplexity jobs, transcript/TTS generation | typed app-feature API with credential source, provider request, cancellation, retry/backoff, progress, cost/status, result, and explicit external-polling exception when provider lacks push | provider polling classified separately from correctness polling; job state remains Rust-owned |
| Native TTS/generated episode artifacts | Swift synthesis/stitching, timed transcript files, generated media import | native executes binary/audio capability; Rust owns temp lifecycle, provenance, durable episode result, failure, and cleanup | Swift file writes cannot silently become product truth |
| Key storage/product signers | `podcast-keys.json`, Keychain/provider keys, per-podcast signer, agent signer | named product signer/key-storage capability decision with explicit security model and status | file-backed keys accepted or rejected by ADR; no ambiguous "temporary/final" split |
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
| Web runtime | deferred `build:wasm`, TS-only app check, raw Worker `resolve_ref`/`release_ref`, retry/reclaim loop | `web/nmp-gallery build:wasm`, Worker startup, OPFS lifecycle, generated ref adapter, and Playwright consume the same `nmp-browser-runtime::wasm` artifact; degraded/no-wasm/no-worker mode fails closed | no correctness `setInterval`; raw worker protocol hidden or deleted; generic browser conformance is not used as gallery proof |
| Component refs/embeds | Swift/Kotlin/TS/TUI/desktop URI/ref adapters, raw namespace/shape/liveness constants, worker payloads, and claim loops | typed `ProfileRef`/`EventEmbed` sessions plus generated or contract-tested host handles; refs survive relay readiness/reconnect without shell retry; open/close tests reject duplicate/stale rows | shell adapters are generated/lifecycle-only, not protocol policy |
| Merge/cache parity | hand caches and `projection_merge_cache` variants | full/delta/clear/tombstone/stale/decode-poison/baseline tests across shells | no platform owns independent merge semantics |
| Browser storage/degraded mode | OPFS/SQLite/Worker preparation, in-memory fallback, Web Locks/tab contention, quota/private-mode failure | durable second-launch, offline read, offline publish queue, second-tab policy, private/quota failure, and explicit gallery UI/status behavior for in-memory fallback | no silent in-memory success in product proof |
| Auth/signing components | Android NIP-55 proof plus partial/visual other shells; browser NIP-07/local/NIP-46 wiring incomplete | per-shell read-only/local/remote/unauthenticated matrix, including browser NIP-07, local key, NIP-46, rejection, wrong-account, unavailable extension, and degraded cases | generic "auth/signing covered" claim removed until each shell is classified |
| Composition root | `register_defaults()` and `consume_all_builtin_projections()` showcase path | explicit feature composition or labeled tutorial compatibility | production examples stop teaching hidden defaults |

Any downstream flow that requires native-owned policy or a bespoke framework
door is a design failure, not downstream migration debt.

Do not import downstream workaround plans as architecture. In particular, a
Podcast repair plan that checks Blossom/publish state on every snapshot tick or
polls action results as correctness proof conflicts with this packet. The
acceptable model is event/job-state driven: upload completion, credential change,
relay ack/error, user retry, or provider callback enqueues work, and Rust emits
typed pending/error/completion state.

Podcast key storage is a product/security decision, not a hidden implementation
detail. If current product planning treats `podcast-keys.json` as final, the ADR
must either accept that explicitly as the named product signer store with its
security model, or reopen it and require a secure-store capability. It cannot
remain an unstated "temporary but final" split.

**Later gate P8: Correct durable docs and delete compatibility teaching paths.**
Update architecture API-surface docs, overview/DX docs, builder-guide pages for
subscription planning, publish and ledger, walkthroughs, action-triggered
subscriptions, ADRs, wiki pages, and any episode/transcript-derived teaching
material that currently presents projection tiers or defaults as app-facing
concepts. The `nmp init` template must be corrected according to the ADR
decision: either production scaffold with explicit feature composition and
policy builders, or clearly labeled tutorial preset. Compatibility APIs may
remain only with scope labels, doctrine gates, and deletion criteria.
At minimum, audit and rewrite stale guidance in `docs/product-spec/api-surface.md`,
`docs/architecture/external-consumers.md`, `docs/recipes/app-shapes.md`,
builder-guide mental-model/codegen/walkthrough pages, subscription planning and
publish guides, wiki app-composition pages, `crates/nmp-testing/tests/dx_scaffold_gate.rs`,
and any generated template that teaches `register_defaults`, `open_interest`,
projection tiers, or `declare_consumed_projections` as the normal product
architecture.
Also audit wiki pages that still teach `nmp.feed.home`, generic defaults,
sidecar projection rituals, raw/pre-signed publish branches, or direct taps as
current architecture. They should either be corrected in place or explicitly
retired before this packet becomes durable documentation.

Named wiki/doc pages to reconcile, because they currently encode important but
possibly stale architecture claims:

| Page | Why it matters | Resolution rule |
|---|---|---|
| `docs/wiki/guides/reduced-source.md` | describes `ReducedSource`, `FeedParams`, and `open_feed` as app-facing dynamic-feed architecture | keep only if the ADR explicitly accepts that public surface; otherwise rewrite around typed sessions and private source reconciliation |
| `docs/wiki/guides/store-first-interest-registration.md` | frames demand as something a projection pushes and preserves registration-order terminology from the old lifecycle recipe | rewrite around session/output-owned demand, replay-before-live, and one owner for interest, sink, output, activation, and teardown |
| `docs/wiki/guides/publish-outbox-pipeline.md` | documents both per-relay reasons and the dead/live explicit-route split | preserve route-reason/status lessons, but resolve `RoutingContext::explicit_targets` versus `PublishTarget::Explicit` to one real seam |
| `docs/wiki/guides/nip29-wiring.md` | contains both correct NMP/app ownership boundaries and older explicit-target wording | keep the ownership boundary; update route wording to match the chosen publish seam |
| `docs/wiki/guides/nmp-gallery-app.md` | records `nmp_app_gallery_register` calling `register_defaults()` and older claim/open-author behavior | migrate to explicit composition or label as gallery/tutorial compatibility with owner and removal gate |
| `docs/wiki/guides/operator-data-leaf-apps-only.md` | correctly states operator policy belongs only in leaf apps | carry this into durable composition/defaults docs so simplification does not reintroduce hidden defaults |
| `docs/wiki/guides/signer-broker-handshake-loop.md` and NIP-46 research pages | record event-driven signer reconnect/cancel and protocol/transport separation | signer runtime plan must preserve this split and avoid a signer-specific second framework |
| `docs/wiki/guides/action-module-adr.md` | records the typed action/effect boundary and remaining dual-action seam | write-flow work must retire dual dispatch seams rather than layering generated builders over them |
| `docs/builder-guide/05b-substrate-traits.md` | teaches `nmp_defaults::register_defaults(app)` as normal composition and not using it as an anti-pattern | rewrite around explicit production composition; any preset is tutorial/migration/test compatibility only |
| `docs/builder-guide/21-framework-magic.md`, `docs/builder-guide/23-glossary.md`, and `docs/design/framework-magic/test-scaffolding.md` | teach public `ReducedSource`, `open_feed`, `open_interest`, `resolve_ref`, and framework-magic scaffolding surfaces | update to typed sessions/actions and private source reconciliation, or mark historical |
| `docs/decisions/0020-intent-classed-routing-and-search.md` and `docs/design/intent-routing/types.md` | preserve the one-classifier rule for user text, protocol refs, relay URLs, NIP-05, app scopes, search, and secret rejection | carry the typed input classifier into the public app model and reject shell-local protocol parsing |
| `docs/design/offline-first-publish-intents.md` and `docs/builder-guide/12-publish-and-ledger.md` | preserve local publish intent before signer, route planner, sockets, retry, or cancel | absorb this as the write-flow root and attach route provenance/status to the same ledger identity |
| `docs/decisions/0009-app-extension-kernel-boundary.md` | teaches app extension/read-model assembly through extension seams and observed projection wiring | update once typed actions, headless invocation, app-lifetime typed sessions, or capability results own extension/app-service demand |
| `docs/decisions/0036-composition-root-followset-expansion.md`, `docs/decisions/0042-m2-open-interest.md`, and `docs/decisions/0063-reference-resolution.md` | older ADRs can preserve defaults/open-interest/reference-resolution as public recipes | correct in place so `open_feed`, `open_interest`, and `resolve_ref` cannot survive as renamed typed sessions without lifecycle ownership |
| `docs/decisions/0046-composition-is-a-library-not-a-generator.md` | treats defaults composition as the reusable app assembly model | amend around explicit production composition and labeled tutorial/compat presets |
| `docs/decisions/0053-host-declared-projection-subscriptions.md` and `docs/decisions/0062-observer-scoped-read-model-catchup.md` | preserve host-declared projection/tier and observed catchup language | rewrite around session-scoped output demand while preserving replay-before-live invariant |
| `docs/product-spec/overview-and-dx.md`, `docs/product-spec/doctrine.md`, and `docs/product-spec/subsystems.md` | expose old feed/projection vocabulary in durable product docs | rewrite around typed sessions, explicit composition, and private executor machinery |
| wiki source-authority and raw app-composition pages | can preserve pre-GitHub-issues planning authority and defaults-as-canonical composition | mark historical or correct against AGENTS/GitHub-issues-only tactical authority and explicit composition |
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

This table is not executable by itself. When any FF row becomes work, the owning
GitHub issue or ADR section must name its owner, exact baseline command/script,
commit, path exclusions, classification rules, target, and enforcement mode. Do
not treat an illustrative grep count as a ratchet until that reproducibility
record exists.

| ID | Rule | Baseline Source | Target | Enforcement |
|---|---|---|---|---|
| FF-001 | Product code does not add raw `open_interest` app reads. | grep public FFI/runtime/shell callers and builder-guide examples | count never increases; product callers trend to zero | doctrine lint or `nmp-testing` grep gate |
| FF-002 | `register_defaults()` is not the production app mental model. | `nmp-cli` templates, examples, builder guide, browser start, and gallery/podcast composition roots | templates and runtime starts teach explicit feature composition or labeled tutorial preset | template test plus doc grep gate |
| FF-003 | App-facing docs do not expose projection tiers or `declare_consumed_projections` for screen/session output. | docs/product-spec, builder guide, wiki, templates | public docs teach typed outputs and session-scoped demand | docs lint grep gate |
| FF-004 | Product state reconciliation does not use snapshot tick polling. | `register_snapshot_tick_observer` call sites and downstream timers | reducer/session/projection tick users trend to zero or have explicit invariant | grep gate plus owner list |
| FF-005 | Dynamic sources fail closed. | feed/source/dependent-interest tests | every migrated source has empty-source and fallback tests | crate tests for source families |
| FF-006 | Output keys have one owner and collision behavior. | projection contract table, host-registered projections, built-ins | composition fails on unowned/colliding keys unless alias/replace is declared | registry test/codegen check |
| FF-007 | Generated/host caches share merge semantics. | Swift/Kotlin/TypeScript/TUI/desktop ref caches, gallery raw worker refs, and `projection_merge_cache` | full/delta/clear/stale-frame behavior covered for generated adapters across every shipped shell | cross-language decode/merge tests |
| FF-008 | Publish route provenance is preserved. | `PublishTarget::Explicit`, `RelaySelectionReason`, `PublishRecord.relay_reasons`, protocol plans, and pre-signed/imported publish APIs | automatic, host-pinned, verified private inbox, manual, imported/verbatim, and diagnostic route classes remain distinguishable through target/command, signer parking, engine record, durable record, retry/resume, and status output | `publish_route_provenance_contract` |
| FF-009 | Private routes fail closed. | D10 tests, NIP-17 inbox tests, Marmot/private publish paths | no unknown-inbox fallback to public/outbox | `nmp-core`, `nmp-nip17`, doctrine tests |
| FF-010 | Downstream shell protocol policy decreases. | Highlighter NDK usage, `$subscribe`, Swift `tagsJson`, native Wi-Fi policy, Podcast signer/relay inference, gallery URI/ref parsing, web retry loops, desktop/TUI claim ticks | counts do not increase; release gates drive them down | downstream grep gates or migration checklists |
| FF-011 | App-feature APIs stay typed and non-protocol unless event-producing. | Podcast STT/TTS/agent/provider APIs and generated FFI | app runtime APIs are classified; event-producing ones use typed publish | API-surface classification test |
| FF-012 | Clean-room app docs work without issue/wiki spelunking. | generated app plus builder guide | new app can open/read/write one feature with typed sessions/actions | walkthrough test or manual UAT checklist |
| FF-013 | Session wakes are declared, bounded, and event-driven. | cache-serve wakeups, logical-interest indexes, completed served-interest shapes, tick observers, downstream refresh pulls | store-served and live events enter one projection dispatch path; first registration drains synchronously enough to avoid blank first frame; no session family depends on broad polling or native refresh triggers | session wake/admission tests |
| FF-014 | Rust outputs semantic facts; shells format presentation. | signer labels, SF Symbols, short npubs, relative time, display strings in Rust projections | semantic tokens only in Rust outputs; presentation helpers stay in shells/TUI/test fixtures | grep gate plus projection review |
| FF-015 | Session wake fanout is bounded. | observer lists, logical-interest registries, source reconcilers, downstream ref claims | each migrated session family declares max wake scope or measured fanout budget | reactivity benchmark or targeted stress test |
| FF-016 | Active-session memory separates handle ownership from cache claims. | ref caches, feed sessions, projection caches, gallery component refs, model/event keep-warm claims | open handles/refcounts bound session machinery; event/model keep-warm and eviction claims are owned by store/cache policy and tested separately for repeated open/close and embed storms | leak/refcount and eviction/claim stress tests |
| FF-017 | FFI/update cadence is coalesced and bounded. | snapshot tick observers, UpdateFrame emission, downstream render caches | no migrated view emits above view budget or serializes one frame per event without proof | update cadence benchmark or fixture |
| FF-018 | Every accepted session family has one lifecycle contract. | P1/P4/P5/P7 migrated sessions and current hand-wired open/replay/projection recipes | acquisition, route, replay, live sink, admission, output, wakes, teardown, and error/status are owned by one session contract | `typed_session_descriptor_contract` plus per-session contract tables |
| FF-019 | Default public author reads use planned outbox routing. | feed/search/ref sessions, `GenericOutboxRouter`, mailbox cache, direct NDK comparisons | author-scoped public reads prove NIP-65/mailbox routing, mailbox-change replanning, unified output delivery, and explicit exceptions for relay-pinned/private/search routes | `read_route_planning_contract` or targeted planner/session tests |
| FF-020 | Reusable protocol projections are owned by their protocol/NMP feature crate. | `nmp.follow_list` and other protocol projections registered from app/FFI glue | app crates consume protocol outputs; they do not register reusable protocol read models | architecture ratchet over projection owner registry and app/FFI call sites |
| FF-021 | Legacy aliases and compatibility shims require live consumers and deletion gates. | JSON dispatch, defaults presets, old open/read/publish doors, stale aliases, downstream-claimed callers | no retained shim lacks caller list, support window, owner, and deletion/formalization criterion; zero live consumers means delete | `compatibility_surface_contract` plus live call-site audit |
| FF-022 | Browser storage/runtime lifecycle is runtime-owned and worker-proven. | `nmp-sqlite-wasm`, `nmp-browser-runtime`, `nmp-wasm`, OPFS/SQLite crates, gallery web build, browser conformance workflows, `secp256k1-sys` wasm feasibility | storage opens before start, runs in the right Worker context, proves durability in real Chrome, distinguishes storage-only wasm proof from full runtime/signing feasibility, and fails loudly when wasm/worker is missing | `browser_storage_lifecycle_contract` plus gallery web e2e |
| FF-023 | Generated catalogs/manifests have one writer. | signer catalog, Android manifest queries, iOS plists, TS relay config, release manifest, client identity | native/web artifacts derive from Rust or release manifests; drift gates compare back to the true source, not only peer artifacts | codegen `--check`, release-manifest gate, signer-catalog parity tests |
| FF-024 | Protocol taxonomy and kind predicates are single-sourced. | `nmp-kinds`, protocol crates, router/planner/store generic layers | generic layers do not switch on per-NIP tables; protocol-aware callers pass semantic class/context | kind-predicate authority lint and router generic-layer tests |
| FF-025 | Metadata privacy gate is centralized. | outbound finalizers, NIP-89/client identity, public and explicit publish arms | client metadata appears only on public-routable unsigned events and never on private/imported/pre-signed/reserved surfaces | metadata privacy contract tests |
| FF-026 | Binding generation reduces drift instead of moving old doors. | C-ABI, JNI, UniFFI experiments, FlatBuffers, runtime workers | generated binding work deletes hand-maintained drift or narrows compatibility; it does not preserve old public semantics under new glue | binding-surface diff review plus codegen drift gate |
| FF-027 | Filterless accepted-event observers are not a product read-model door. | `KernelEventObserver`, `register_event_observer`, `nmp_app_register_event_observer`, `NmpEventObserverCallback`, worker observer equivalents | parser/cache internals may observe accepted events; product read models use declared sessions or private observed sinks | no-raw-tap doctrine lint plus compatibility surface audit |
| FF-028 | Existing seams are tried before new framework surface. | `NmpAppBuilder`, `BrowserAppBuilder`, `AppHost` narrow registrars, `ObservedProjectionRegistrar`, dependent interests, intent classifier, `ActionModule`, publish engine | first implementation records why existing seams were reused, narrowed, or insufficient; no new crate/engine lands without retiring an old public door | `existing_seam_first_contract` plus design-review checklist |
| FF-029 | Typed input classification has one production owner. | `nmp-intent`, URI/search/ref/open-action doors, shell text parsers | user text, NIP refs, relay URLs, NIP-05, app scopes, search, and secret rejection route through one Rust-owned classifier | `typed_intent_classifier_contract` |
| FF-030 | Publish status is offline-first and intent-rooted. | `PublishAction`, `PublishCommand`, publish records/status, retry/cancel, explicit route callers | signing, route resolution, relay IO, retry/resume/cancel, local ingest, and status attach to one local publish intent identity | `publish_intent_ledger_contract` plus route-provenance contract |
| FF-031 | Schema, binding, and storage migrations do not create dual truth. | FlatBuffers/update-frame schemas, generated bindings, host render caches, browser/native storage, App Group/UserDefaults mirrors | every migrated contract has one writer, version/fixture/codegen gates, classified compatibility paths, and deletion/support-window triggers | `schema_migration_contract` plus codegen fixture checks |

## Current Baseline Snapshot

This is an initial 2026-06-28 snapshot from live grep counts. It is evidence for
the dossier, not a durable source of truth. These counts include docs and tests
unless noted, so any selected slice still requires manual classification into
production, test, historical doc, tutorial compatibility, diagnostic, or delete.
Before creating a ratchet issue, rerun the count with the exact script, commit,
workspace roots, downstream paths, and exclusions recorded in the issue. If the
rerun materially differs, the rerun wins; this table remains a discomfort signal,
not a source-of-truth baseline.

| Surface | Count | What it means |
|---|---:|---|
| NMP filterless observer family, excluding `docs/new-arch` | 13 files / 32 matches | the old observer door is mostly lint/docs after cleanup, but must stay classified so stale transcripts do not resurrect it as architecture |
| NMP `open_interest` family, excluding `docs/new-arch` | 45 files / 122 matches | old read door is still broadly taught and tested |
| NMP URI/input intent action family | 26 files / 106 matches | URI/action classify doors can bypass typed sessions if left as raw-interest routing |
| NMP `nmp.browse_relay` family | 10 files / 172 matches | single-relay browse is a public escape hatch unless scoped diagnostic or formalized |
| NMP feed controller/perspective/load-older family | 44 files / 220 matches | `open_feed` is not the whole surface; public feed controllers need disposition |
| NMP pull cursor/mirror raw-log family | 36 files / 206 matches | raw history pull is useful but must not become product screen state |
| NMP runtime lifecycle FFI family | 113 files / 413 matches | lifecycle FFI is large enough to require runtime-only classification, not accidental product lifecycle |
| NMP defaults/projection declarations | 144 files / 526 matches | hidden defaults and projection-declaration language are not a small local cleanup |
| NMP `ObservedProjection` family | 190 files / 885 matches | observed projection is a major existing concept and must be deleted, privatized, or explicitly justified |
| NMP explicit publish route family | 198 files / 2148 matches | route provenance cannot be fixed by naming alone; dead/live explicit seams need one owner |
| NMP tick/polling markers | 33 files / 53 matches | every retained tick needs presentation/capability classification or deletion |
| Highlighter direct Nostr/policy markers | 116 files / 609 matches | Highlighter web/runtime migration is a first-class gate, not a footnote |
| Podcast service/publish markers | 229 files / 773 matches | service/capability flows and publish status are downstream proof obligations |
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

If a slice adds typed descriptor/handle, `ObservedProjection`, `ReducedSource`, route
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
rg -n "KernelEventObserver|register_event_observer|nmp_app_register_event_observer|NmpEventObserverCallback" crates apps docs --glob '!target/**'
rg -n "KernelAction|OpenUri|nmp_app_open_uri|nmp_app_intent_classify|nmp_app_intent_dispatch" crates apps docs --glob '!target/**'
rg -n "nmp\\.browse_relay|BrowseRelay" crates apps docs web --glob '!target/**'
rg -n "FeedRegistry|FeedController|PullFeedController|FeedPullPager|FeedSessionBuild|PerspectiveRegistry|CustomPerspectiveDef|register_custom_perspective|nmp_app_load_older_feed" crates apps docs --glob '!target/**'
rg -n "nmp_mirror_pull_page|PullCursor|PullCursorRegistration|pull_page_over" crates apps docs --glob '!target/**'
rg -n "nmp_app_start|nmp_app_configure|nmp_app_stop|nmp_app_reset|nmp_app_lifecycle_foreground|nmp_app_lifecycle_background|nmp_app_is_alive|nmp_app_set_update_callback" crates apps docs --glob '!target/**'
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

Small ADR-blocking proof gates to create before declaring the design
implementable. These protect the first executable slice and the core
simplification rules; broader downstream conformance remains issue-backed
post-ADR work.

```bash
cargo test -p nmp-testing --test architecture_surface_ratchet
cargo test -p nmp-testing --test typed_session_descriptor_contract
cargo test -p nmp-testing --test projection_merge_contract
cargo test -p nmp-testing --test first_session_route_planning_contract
cargo test -p nmp-testing --test first_session_wake_fanout_contract
cargo test -p nmp-testing --test publish_route_provenance_contract
cargo test -p nmp-testing --test docs_architecture_teaching_ratchet
cargo test -p nmp-testing --test compatibility_surface_contract
cargo test -p nmp-testing --test no_filterless_observer_contract
cargo test -p nmp-testing --test existing_seam_first_contract
cargo test -p nmp-testing --test typed_intent_classifier_contract
cargo test -p nmp-testing --test publish_intent_ledger_contract
cargo test -p nmp-testing --test signer_status_runtime_contract
cargo test -p nmp-testing --test schema_migration_contract
```

If browser is used as the first proof target, add a small fail-closed browser
direction gate before ADR acceptance. Full
`browser_storage_lifecycle_contract` remains post-ADR unless browser is selected
as the proof slice.

Post-ADR issue backlog candidates, not proof that must all exist before the ADR:

```bash
cargo test -p nmp-testing --test downstream_architecture_acceptance
cargo test -p nmp-testing --test service_session_contract
cargo test -p nmp-testing --test podcast_service_completion_contract
cargo test -p nmp-testing --test gallery_web_runtime_contract
cargo test -p nmp-testing --test gallery_ref_lifecycle_contract
cargo test -p nmp-testing --test gallery_auth_signing_matrix_contract
cargo test -p nmp-testing --test highlighter_web_runtime_ratchet
cargo test -p nmp-testing --test highlighter_web_scope_decision_gate
cargo test -p nmp-testing --test highlighter_component_session_contract
cargo test -p nmp-testing --test podcast_nipf4_publish_contract
cargo test -p nmp-testing --test app_feature_api_classification
cargo test -p nmp-testing --test no_polling_downstream_gate
cargo test -p nmp-testing --test read_route_planning_contract
cargo test -p nmp-testing --test protocol_projection_ownership_contract
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
- The first implementation reaches for a new crate, engine, registry, or broad
  `AppHost` extension before proving existing builders, narrow registrars,
  intent, observed-session, and publish seams cannot carry the invariant.
- Typed sessions reduce names but not the number of public concepts a product
  author must understand.
- The first ADR publicizes `FeatureBundle`, `LiveQuery`, `ServiceSession`,
  `ReducedSource`, or `ObservedProjection` as app-developer vocabulary instead of
  using explicit installers, typed descriptor/handle, typed actions, capability
  results, and typed outputs/status.
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
- The public filterless observer door reappears as "advanced" app API instead of
  parser/cache internal machinery, diagnostic tooling, or migration-scoped
  compatibility.
- The first clean-room app requires app authors to understand `ObservedProjection`,
  `ReducedSource`/source tiers, route provenance internals, headless/capability
  flow mechanics, feed controller registries, or runtime lifecycle FFI.
- Service sessions require a second lifecycle/output/wake/store/status model
  instead of reusing typed actions, typed sessions, headless invocation, or
  last-emitted Rust mirror frames.
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
- Input intent simplification lets shells parse NIP refs, relay URLs, NIP-05,
  app scopes, or secrets independently.
- Publish simplification removes local offline-first publish intent/status and
  turns writes into signer or relay callbacks.

## Fitness Checks

The destination is not reached until these are true:

- No public builder guide asks product apps to manually pair raw interest open,
  observer registration, replay, projection sidecar, and teardown.
- No production state reconciliation depends on snapshot tick polling.
- New app reads enter through typed session descriptors or named
  substrate/protocol-internal/diagnostic/test/migration acquisition scopes.
- No product read model uses filterless accepted-event fanout and self-filters
  instead of declaring acquisition, replay, output, and teardown.
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

- the first ADR adds at most one new public read noun, typed descriptor/handle;
  features are installed through explicit installers, and `LiveQuery`,
  `ObservedProjection`, `ReducedSource`, and service-session abstractions stay
  private/rejected until separately proven;
- existing seams are tried first: `AppHost`/registrars, native/browser builders,
  observed-session machinery, dependent interests, intent/action modules, and
  publish carriers must be reused, narrowed, or shown insufficient before new
  framework surface lands;
- app-facing reads use typed sessions/descriptors, not raw `open_interest` or
  filterless `KernelEventObserver`/event-observer fanout;
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

- final public naming: `typed session` or per-feature open helpers over one
  descriptor/handle model;
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
- Podcast's service-like/native-mirror table and the NIP-F4 route/signer/
  Blossom publishing contract;
- gallery web runtime status, signing/auth matrix, and generated ref lifecycle;
- which downstream app migrations are release gates versus follow-up issues;
- whether manual explicit relay selection is user-visible product functionality,
  and if so what ownership, audit text, and guarantees the product promises.

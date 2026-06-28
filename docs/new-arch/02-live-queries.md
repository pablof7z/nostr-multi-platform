# Live Queries

A screen or component opens a typed session for the state it wants to render.
Headless/service-like surfaces use typed actions, short-lived headless
invocation, capability results, or last Rust-emitted mirror frames first; they
open typed sessions only after a proof shows resident state is required:

```text
open(HomeFeed { account })
open(Profile { pubkey })
open(GroupFeed { group_id, host_relay })
open(Search { query })
open(ProfileRef { pubkey, owner })
open(EventEmbed { event_ref, owner })
open(LiveCountOutput { source, filter, owner })
open(AppDefinedTimeline { descriptor_id })
```

`HomeFeed` is an example feature, not a privileged framework primitive. If the
same filter/source/ranking can be expressed by a generic session descriptor, it
should use that path. A hand-coded feed door is acceptable only while it proves
the descriptor model or protects a measured hot path with a deletion/formalize
decision.

The app receives a handle:

```text
SessionHandle {
    query_key,
    owner_id,
    output_key,
    scope,
}
```

Native and web shells render the typed output associated with that handle. When
the owner goes away, the app closes the handle. The shell does not open raw relay
subscriptions, replay cache rows, register observers, compute dynamic sources,
or own durable product caches.

The public read noun is a typed session descriptor plus handle. The ADR may
choose typed per-feature open helpers, a generic descriptor API, or a hybrid, but
the accepted shape must let NMP crates and app Rust crates define new read
models without requiring native shells to hand-author relay subscriptions.
Do not publicize `LiveQuery` in the first ADR. If a later ADR uses the name, it
must mean this descriptor/handle contract and must not become a second public
lifecycle engine.

The examples above are Rust-defined descriptor helpers or generated host
bindings. They are not permission for a shell to pass `filter_json`, relay lists,
output schemas, or reducer names as a raw subscription dictionary. A generic
custom feature is acceptable only when an app Rust crate owns the descriptor,
route/admission policy, output schema, and teardown contract.

#2316 rules out a thin convenience wrapper. A session is real only when one
contract owns acquisition, route planning, replay, observed sink, admission,
output/schema, source dependency wakes, error/status state, and teardown. If the
implementation still asks a feature author to wire those mechanisms separately,
then the API got easier to call but the architecture did not change.

The candidate should start as a small typed descriptor plus handle, not a new
lifecycle engine. It compiles into existing or consolidated machinery:

```text
source expression
  -> acquisition demand / OpenObservedInterest
  -> route policy / relay pin / planner mode
  -> cache-store replay limits and replay-before-live open
  -> observed sink / ObservedProjectionCommandHandle
  -> admission predicate and owner id
  -> dynamic dependency tracking / ReplaceDependentInterestSet
  -> typed output key, schema owner, and status projection
  -> generated adapter/cache contract
  -> delivery through UpdateFrame
  -> close handle / dependent-interest replacement / sink teardown
```

This is the architectural door missing from the current API. The first
implementation should prove the descriptor can reuse, narrow, or delete the safe
parts of the `ObservedProjection` pattern and dependent-interest machinery
before adding any new public lifecycle surface. `open_interest` is only
acquisition. It can fetch events without making them visible to the app, so it
should not be the public app read model. It can remain available to substrate,
debug, test, and migration code that is explicitly acquiring events without
claiming an app-visible output lifecycle.
The same is true for public filterless observers. A `KernelEventObserver`,
`register_event_observer`, C-ABI observer callback, or worker equivalent that
receives accepted events and self-filters later is not a session contract. It
lacks scoped acquisition, replay identity, relay provenance, bounded output, and
close ownership. Parser/cache internals may observe accepted events; product read
models must use declared sessions or private observed sinks.

The proof must start from existing surfaces, not from a fresh abstraction. Try to
generalize or narrow `open_feed`, `resolve_ref`, `ObservedProjectionRegistrar`,
dependent interests, and current feature sessions first. If those can express
the lifecycle with clearer names and smaller public API, prefer that over adding
a public `LiveQuery` object.
Typed session descriptors must compile into these seams until evidence proves a
new seam is needed. The current action/codegen registry is write/action-ready,
not a generic read-module runtime; a descriptor that requires a new module
registry before it can express one real read family is suspect.
P-1 selects the smallest live deletion proof. An existing feed/session path is a
likely first candidate only if it retires more old surface than alternatives and
does not depend on unresolved tick-repair work. That proof must retire or narrow
old surface in the same slice:

- one `open_*`/feed-session recipe stops requiring caller-authored interest,
  replay, sink, sidecar/output, and close wiring;
- the equivalent raw `open_interest` product path is deleted, made private, or
  migration-scoped with owner/support-window/removal gate;
- feed controller, load-older, perspective, and source-compiler public surfaces
  used by that caller are either private under the session or explicitly
  compatibility-scoped;
- docs/templates for that caller stop teaching projection tiers,
  `ObservedProjection`, `ReducedSource`, or raw close tokens as app-developer
  vocabulary.

An NDK-style `subscribe(filter)` is a useful DX comparison, but not the
production NMP boundary. The equivalent NMP experience is:

```text
app/protocol Rust defines typed session descriptor
  -> generated host API exposes open/close/render helpers
  -> NMP owns routing, replay, admission, projection, and teardown
```

A raw shell-level event stream would make the native or web shell responsible
for protocol parsing, route policy, and read-model ownership again. That may
exist only as diagnostic, test, export, prototype, or migration tooling with
scope labels and deletion/formalization criteria.

## Push/Pull Boundary

Typed sessions are the UI-state path. They produce pushed, bounded, typed
outputs owned by Rust and merged by generated or contract-tested host adapters.
They are not host-polled snapshot getters, raw event callbacks, or arbitrary
event-log streams.

Pull/event-log surfaces still have a job: import/export, diagnostics, raw-event
inspection, pagination over event history, mirrors, and tests. They do not own
screen state, route policy, protocol parsing, or projection lifecycle. A product
screen that uses a pull cursor or raw event stream to reconstruct live UI state
has recreated the old `open_interest` problem under a different name.

The split is:

```text
typed session -> pushed typed output -> render state
pull cursor / raw event log -> inspection, history, export, diagnostics
```

If a feature needs pagination or backfill, that capability belongs inside the
session contract or behind a typed "load more" action. The shell should not
switch from pushed typed output to owning a raw event cursor because the first
page was inconvenient.

A custom app feature follows the same rule as a protocol feature: it owns a typed
descriptor, source expression, output schema, reducer, route policy, and
teardown semantics in Rust. It may use NMP routing, store, refs, planner,
capabilities, signing, and publish machinery, but it does not get a separate
native read model.

Allowed `open_interest` scopes should be exact:

- substrate internals that only acquire events;
- protocol feature implementations hidden behind typed sessions;
- diagnostics, tests, export/inspection tools, and migration shims with deletion
  criteria;
- no product screen, app shell, starter template, or builder-guide example.

This should become a machine allowlist, not a review convention. The future gate
should classify each `open_interest` symbol/call site as substrate,
protocol-internal, diagnostic/test/export, or migration-with-deletion-criteria,
and fail any product shell, product screen, starter template, or public teaching
caller.

"Expert" cannot be a permanent escape hatch. Any public product caller that
still needs raw acquisition after typed sessions exist needs an issue, an owner,
and a removal or formalization decision.

## Session Identity

A session descriptor should carry enough identity to make sharing and teardown
deterministic:

```text
query_key          stable identity for equivalent demand
owner_id           screen/component/widget/service owner
scope              account, global, protocol context, or app lifetime
source             static or reduced source expression
route_policy       planned, relay-pinned, private, or audited explicit override
projection         typed reducer/output producer
replay             bounded replay shape and replay limit
output_key         typed output namespace and row/delta contract
```

Each session family must also have a contract that covers every lifecycle
fragment named in #2316. This is not necessarily one public Rust type, but it
must be one owned proof:

| Fragment | Session contract must state |
|---|---|
| acquisition | what interest, ref, local index, or capability demand is opened |
| route planning | planned, relay-pinned, private, or audited explicit route policy |
| replay | which cached/store rows hydrate before live activation and what bounds apply |
| live sink | which internal observed sink or reducer receives future events |
| admission | what event/source/provenance shape is accepted and what fails closed |
| output | output key, schema owner/version, full/delta/clear/stale-frame merge contract |
| wakes | event/store/source/mailbox/capability changes that re-run reconciliation |
| teardown | owner/refcount close behavior, child demand release, and clear/tombstone output |
| errors/status | typed state emitted when source, route, replay, decode, or capability work fails |

If a feature still requires a separate caller-authored interest, replay, sink,
projection, tick observer, or close token outside that contract, the architecture
has only renamed the old local recipe.

Multiple owners may share the same `query_key`. Opening increments ownership;
closing decrements it. The final close tears down acquisition, observed sinks,
derived dependencies, and generated output rows. If a feature needs a visible
clear, it emits a typed `Cleared` or tombstone output instead of leaving stale
rows in shell state.

Handle ownership is not the same as an event/model cache claim. A session handle
says "this owner currently needs this output." A cache/model claim says "this
event, profile, ref, or derived model must remain addressable or warm across
handle churn." Keep-warm, eviction, and durable identity policies belong to the
store/model-cache owner, not to shell refcounts or open handles.

Owners are not only visible screens. App-lifetime services, widgets,
AppIntents, CarPlay, remote commands, background downloads, and signer/runtime
flows may own sessions when they need resident state. Their sessions still need
typed identity, lifecycle, bounded output, injected time, capability result
channels, and deterministic teardown; they are not permission for hidden native
stores or polling loops.

Headless or service-like owners are a design hypothesis, not settled doctrine.
They are included because Podcast Player, widgets, AppIntents, CarPlay, Live
Activities, Handoff, and signer/runtime work expose a real gap in the
screen-only model. The ADR must prove they can reuse the normal session/action/
capability machinery without becoming a second app model or a runtime-specific
framework.
Until that proof exists, "service session" is not initial public vocabulary and
must not become a crate, module, generated binding family, or app-developer
concept. Service-like flows should first be expressed as typed actions into the
normal runtime, short-lived headless runtime invocation, capability results, or
last-emitted Rust mirror frames.
Podcast is the falsification case: an AppIntent that only enqueues into a
foreground singleton, a CarPlay surface that polls until the UI store appears, or
an OS callback that reports success before Rust emits completion proves the
headless/service owner model is not solved. Highlighter is the caution in the other
direction: share extensions, NIP-05, SSR, and background work do not justify a
broad service layer if typed actions, short-lived headless invocation, or raw
capability results can carry the invariant.
Before accepting a service-session abstraction, prove the cheaper forms fail:
a typed action into the normal runtime, a short-lived headless runtime invocation,
or a last Rust-emitted mirror frame. If the proposed abstraction needs a
second lifecycle, output, wake, store, or status model, reject or narrow it.

The default service-like example is therefore not `open(PodcastPlayback {
owner = AppLifetime })`. It is:

```text
dispatch(PlaybackCommand { source = AppIntent, command })
report_capability_result(AudioCommandResult { correlation_id, raw_result })
render(last_rust_emitted_widget_or_now_playing_frame)
```

Only after that shape fails should a resident `PodcastPlayback` session become
part of the accepted model, and then only for the selected Podcast proof row.

Any future headless/service abstraction would need an explicit lifecycle
contract:

| Surface | Opens or resumes | Reports back | Must not own |
|---|---|---|---|
| Widget refresh | app-lifetime typed session, typed action, or last Rust-emitted widget frame | WidgetKit timeline request / display result if relevant | playback queue, episode truth, relay state |
| AppIntent/Siri | headless typed action/session or dispatch into existing app runtime | typed intent result or capability failure | hidden singleton-only app state that fails unless UI is open |
| CarPlay scene | scene owner session on attach, close on detach | selection/transport actions and raw CarPlay capability state | parallel navigation/playback model or polling wait loop |
| Remote command | command action into Rust playback/session state | raw OS command metadata | queue mutation or gesture policy outside Rust |
| Live Activity | Rust typed state decides desired activity state | ActivityKit executor result, throttling/failure facts | decision about current episode or activity existence |
| Handoff/resume | resume action with OS payload decoded as capability input | raw resume/handoff capability result | second navigation, playback, or account source of truth |
| Inbound OS activation / deep link / Spotlight / voice mode | typed action or short-lived headless invocation with decoded OS payload | raw activation payload and capability result | Swift-only URL policy, navigation truth, playback decision, or hidden foreground dependency |

"Dispatch accepted" is not the operation result. AppIntents, Siri, CarPlay,
remote commands, widgets, Live Activities, Handoff, and cold-start workers may
return quickly, but the Rust-owned session must still emit pending, completion,
error, or diagnostic state for the user-visible operation. A Swift singleton
enqueue, foreground-store mutation, or native callback success is not proof that
playback changed, a publish completed, or a relay operation succeeded.

Opening before relay, mailbox, identity, or source readiness is allowed. Rust
queues and replans the session when dependencies arrive. The shell should not
retry with timers.

Minimum future contract shape, if cheaper forms fail:

```text
HeadlessOwnerContract {
  owner_id: widget | app_intent | carplay_scene | remote_command |
            live_activity | handoff | app_lifetime
  account_scope: active account, explicit account, or no-account capability
  opened_by: UI runtime, extension runtime, OS callback, or cold-start worker
  output: typed status/result frame or last Rust-emitted mirror frame
  capabilities: raw OS/API effects this owner may execute
}
```

If this contract becomes necessary, it is still a normal Rust-owned typed action
or session. It may run in a minimal/headless runtime, but it is not a second app
model. A cold-start AppIntent or widget refresh must hydrate the store/kernel
state it needs, dispatch one typed action or open one typed session, emit a typed
result or mirror frame, and shut down or suspend deterministically. It must not
depend on a foreground UI singleton already being alive.

Dispatch acceptance is not completion. A service surface such as an AppIntent,
Siri command, CarPlay tap, headphone remote command, widget action, or Live
Activity update needs a typed result/status from Rust before it can report
success to the OS/user. A foreground-fallback string like "open the app first,"
a returned correlation id, or the absence of a thrown native error is not proof
that the action completed. Rust owns the operation status; native/web shells
render or hand that status to the OS.

Allowed native mirrors are write-only render/capability products of Rust state:
last widget frame, `MPNowPlayingInfo`, ActivityKit payload, Handoff payload,
secure-key capability result, media cache pointer, or downloaded file handle.
They cannot be read back as the source of playback queue, signer state, relay
policy, publish status, account identity, or durable Nostr/app facts.

Each service-like family needs proof for:

- cold start with no foreground UI process;
- resume into an already-running app runtime;
- explicit close/suspend behavior;
- no polling or sleep-wait loop for readiness;
- capability failure reported as typed Rust state;
- typed action completion/result distinct from dispatch acceptance;
- raw OS command reports for CarPlay/remote/headphone inputs, with Rust deciding
  skip interval, rate ladder, queue mutation, chapter seek, and next/previous
  policy;
- native mirror corruption or absence not corrupting Rust truth;
- repeated open/close without leaking handles or stale output rows.

Podcast Player is the concrete acceptance matrix for this family. Each surface
must classify its current path and target contract:

| Surface | Target contract |
|---|---|
| Widget | reads the last Rust-emitted widget frame or performs a bounded headless typed action; App Group files are mirrors only |
| AppIntent/Siri | cold-start safe typed action with Rust-owned pending/completion/error result; no foreground `KernelModel.shared` requirement |
| CarPlay scene | scene attach opens/resumes typed playback/navigation state; detach closes/suspends; no Task.sleep readiness loop |
| Remote/headphone command | native reports raw command metadata; Rust decides play/pause, skip interval, rate ladder, queue mutation, chapter seek, and next/previous policy |
| Live Activity | ActivityKit receives Rust semantic state and raw executor results; native does not decide current episode or activity existence |
| Handoff/Spotlight/deep link | OS payload is decoded as capability input and dispatched to Rust; navigation/playback/account truth remains Rust-owned |
| RSS/OPML/import/export | Rust parses/exports, normalizes feed URLs, dedupes stable ids, records row-level errors, conditional-fetch metadata, injected timestamps, and durable subscription results; native only provides file/share/temp/network capability facts |
| Provider/STT/TTS/agent job | classified as immediate foreground call, long-running job, capability request/result, provider catalog, agent tool, or publish action, each with durable Rust job state, typed trigger source, injected clock, correlation id, cancellation, timeout, retry/backoff, progress, cost/status, restart recovery, and terminal result/error |

Provider polling is an external-protocol exception, not a general NMP scheduling
model. It is allowed only when the provider lacks push/webhook semantics or the
provider contract explicitly requires polling. Generic "run due every N seconds"
correctness loops need a documented actor-timer invariant, injected clock,
bounded wake behavior, and a deletion/formalization gate; otherwise they are the
same polling problem under an app-feature name.

## ObservedProjection

`ObservedProjection` is the safe event-to-read-model pattern used inside a typed
read session. It is internal machinery, not a concept app developers assemble.
More precisely, it may remain a feature/runtime-internal API where reusable NMP
features compile descriptors into replay-before-live observed sessions. It must
not be shell-facing, product-app assembly language, or a reason to preserve
tick-polled controller behavior.

High-level behavior:

```text
register sink muted
open declared interest
replay matching cached/store events into that sink
activate the sink for future matching events only
emit typed projection state
close sink and interest together
```

The important invariant is ordering: a late-opened view receives matching cached
events before it starts receiving future live events, and future delivery is
scoped to the declared shape. This avoids both late hydration misses and the old
filterless observer problem.

App developers should not manually assemble this. A feature/session descriptor
uses it internally.

Delivery is not population. Fixing a missing projection by seeding the store,
pre-warming a cache, or asking the shell to retry claims is still a lifecycle
bug if the session open did not own observer registration, cache replay,
activation, output emission, and teardown. The feature/session door must make
hydration happen by construction, not by after-the-fact cache warming.
A session must not depend on a broader startup interest that happened to serve
the right rows before its sink existed. Demand and sink installation are one
contract: install the muted sink/output owner, declare the replay shape, replay
matching store rows into that owner, then activate future delivery. This is the
store-first/read-your-writes lesson from the follow-list cold-start bug: local
publish fanout can be correct and the view can still be wrong if initial
hydration raced ahead of the projection owner.
Host-declared demand must also persist before an active account exists. A
feed-kind or output declaration that only appears after login, Android imperative
open, or a shell retry can mask a kernel lifecycle bug. The kernel should know
the dormant demand, then activate or replan it when account state arrives.
Cache serve is more precise than "replay before live." The first visible frame
must be allowed to drain the bounded store/cache replay for the declared shape;
drains are budgeted by visited events, not by arbitrary author caps; completion
keys wake only the affected sessions; cache-served events notify the projection
owner without re-entering `store.insert`; and local publish still enters through
the same read-your-writes store path.

The reconciler must be event-driven. Identity changes, source changes, mailbox
updates, refcount changes, and store ingest should trigger reconciliation. A
snapshot tick observer is not the model.

Relay-pinned observed projections must also prove provenance. A NIP-29 or other
host-pinned session should not accept a matching event merely because the filter
shape matches; replay and live admission must know the event came through the
declared relay context or another protocol-approved source.
Replay tests must cover this, not only live delivery. A store replay for a
relay-pinned session needs either stored relay provenance or another explicit
protocol-approved admission proof; a matching `#h` tag alone is not enough.

## Dynamic Source Reconciliation

Dynamic source reconciliation is the model for query inputs derived from other
events, account state, or capability state.

Decision status: current NMP docs and code already contain `ReducedSource` and
`open_feed`-style machinery, but the live type is private native-feed compiler
machinery. Pointer-source, browser feed, defaults runtimes, and active-account
controllers have adjacent source-reconciliation patterns. The ADR must decide
whether these are semantically the same family before extracting anything
generic. Until then, `ReducedSource` means one current private feed-local
implementation candidate, not the architecture noun and not a public primitive.
This deliberately conflicts with older generated wiki guidance that described
`ReducedSource`, `FeedParams`, and `open_feed(FeedParams)` as the app-facing
architecture. Treat that guidance as a historical checkpoint to re-audit, not as
settled API. If the ADR keeps any feed-specific public door, it must prove why a
typed session descriptor cannot express the same demand with fewer concepts and
must name the deletion or formalization target for every other dynamic-source
door.

The dependent-source issue cluster matters here. The old `$metaSubscribe` and
pointer-source work showed that target hydration is not solved merely by adding a
subscription helper. Source arrival, withdrawal, target replay, route planning,
and output ownership have to move together under the session contract, or the
system recreates the same #2088/#2090/#2091 family of partial-read bugs.
The shipped dependent-interest pattern is the reference shape: a source opens as
an observed projection; extracted refs/authors/addresses/tags become
planner-routed dependent interests; acquisition and union delivery stay separate;
derived state such as `pointedBy` remains projection state; and sort/rank changes
must not reopen or reacquire relay demand unless the source set actually changes.

Examples:

- notes by people the active account follows;
- events by members of a NIP-51 list;
- replies to currently visible thread roots;
- target events pointed to by a stream of pointer events;
- group content from groups the account has joined;
- embeds referenced by currently visible event bodies.

The source set is not a static list. It is derived from other events or account
state:

```text
source interest or account state
  -> deterministic reducer
  -> materialized author/id/address/tag targets
  -> dependent interests/ref claims
  -> planner/router/cache path
```

When the source changes, NMP diffs the old and new targets, closes withdrawn
targets, opens new targets, and recompiles relay subscriptions. Empty output
fails closed; it never becomes wildcard acquisition. Native shells never compute
follow lists, group membership, list members, WoT expansion, or target refs.
Each migrated source family needs tests for source arrival, source withdrawal,
empty-source behavior, explicit fallback behavior, account switch, teardown, and
route replanning. These are contract tests for the session model, not merely
feed-specific tests.

`ReducedSource` is one possible private building block under typed sessions, not
a separate app API the shell has to orchestrate. It should not start as a grand
abstraction. The first implementation should extract the smallest private shape
reconciler around observed-projection open/close. A general reduced-source core
is justified only if real source families share the same diff, fail-closed,
teardown, and dependent-interest semantics without special casing.
If feed, group, thread, pointer, account, and embed sources do not share those
semantics, the simpler architecture is not to force them under one generic
`ReducedSource`. Keep separate private reconcilers or feature-local reducers and
unify only the session lifecycle contract they compile into.

Fail-closed mechanics and product fallback policy are separate. A HomeFeed may
choose an explicit public fallback source when the active account has no follows.
That fallback must be declared by the feature; it must not appear accidentally
because an empty author set became an unrestricted relay subscription.

Useful reduced-source dimensions include:

- `AuthorSet`: active account follows, list members, WoT expansion, group
  members, or app-owned author resolvers.
- `EventSet`: visible roots, quoted events, article refs, replies, bookmarks, or
  app-owned pointers.
- `AddressSet`: parameterized replaceable addresses and relay hints.
- `RelaySet`: host relay, explicit read relay, inbox relay, or protocol relay
  context.
- `TermSet`: search strings or structured query tokens.

Group membership is a first-class blocker. If a feature cannot prove the joined
group, host relay, or member set, it should fail closed or emit a typed missing
context state instead of falling through to a broad public query.

Follow-list ownership is a concrete acceptance criterion from #2313. The
follow-list read model is a reusable NIP-02/NMP feature output, not app-owned
FFI glue. The destination is that `nmp.follow_list` or its replacement is owned
by the reusable follow feature, while Chirp, Highlighter, gallery, or any other
app is only a consumer that opens sessions or renders outputs.

## Base Query Primitive Versus Feed Policy

The bad follow-feed history must not define substrate architecture. A
multi-author or tag-derived event query is a base NMP capability: it may need
source reduction, dependent interests, NIP-65 planning, cache replay, relay
provenance, and teardown. A follow feed is one product use of that capability,
with its own ranking, recency, fallback, and viewport policy.

This split matters because a bad feed implementation can otherwise leak upward
and downward at the same time. A parse-time author cap, cache-warming workaround,
or follow-feed-specific replay shortcut must not become substrate policy. The
substrate should expose the smallest private capability that can route and
hydrate dynamic source demand correctly; feed, room, search, thread, and app
features decide their own product ranking and fallback on top.

The concrete lesson is multi-author query shape. A per-pubkey fanout loop or
arbitrary 500-author cap is usually evidence that the store/query primitive is
wrong for the feature, not proof that the framework needs relay-sharding folklore
or feed-specific cache warming. If a `StoreQuery::AuthorsKind`-style primitive is
the missing reusable capability, add that small store/session primitive and keep
feed ranking/pagination policy above it.

Concrete examples:

```text
NotesByAuthors { authors = follow_list, kinds = [1] }
  -> substrate/session capability: source diff, outbox route, replay, output
  -> feed policy: ranking, recency window, fallback when follows are empty

GroupTimeline { group_id, host_relay }
  -> substrate/session capability: relay pin, replay provenance, output
  -> group policy: NIP-29 admission, membership/admin context, product filters
```

If the only reason a proposed substrate primitive exists is that one feed path
needed it, keep it private to that feature until another source family proves the
same semantics. If the primitive is genuinely reusable, the proof is that it
removes feature-local recipes without importing feed policy into the core.

The base query capacity proof is part of this split. If a source family needs
hundreds or thousands of authors, tags, addresses, or event refs, the selected
session must prove the store/query and route-planning shape can handle that
demand without a per-feature workaround. Per-pubkey loops, arbitrary global
author caps, cache-warming shortcuts, per-author versus global recency hacks, and
relay-sharding folklore are not feed policy; they are evidence that the
substrate/session query primitive is missing or underspecified. The right proof
names the bounded query primitive, admission rules, recency/window semantics,
relay planning behavior, and fallback when the source set is empty or too large.

## Routing

Every acquisition child or source lane must declare a routing mode. A simple
session usually has one lane; a composite session owns a route-policy tree. This
is feature/protocol policy, not a casual caller option passed by the shell:

- **planned route:** the normal case. NMP owns relay planning, including NIP-65
  outbox routing for author-scoped reads, mailbox/inbox discovery where relevant,
  search/discovery relay policy, configured app relays, cache replay, and
  replan-on-mailbox-change behavior. Planning is kind/protocol-aware: `p` tags
  are not always recipients, discovery kinds are not private inboxes, blocked
  relay rules still apply, indexer/search lanes are explicit, and host-pinned
  protocols carry provenance.
- **relay-pinned route:** the protocol or source owns a specific relay context,
  such as a NIP-29 group host relay. Admission must prove the event came through
  that context or another protocol-approved source.
- **private route:** private data such as NIP-17 uses protocol-owned inbox
  routing and fails closed when recipient routing is unknown.
- **explicit override:** an audited escape for a feature or app Rust crate, not a
  native shell subscription. It must carry owner, purpose, tests, and a reason
  planned or protocol-pinned routing is wrong.

The developer-facing default is planned routing. The app opens the typed session;
NMP plans relay acquisition and delivers one unified output stream under the
handle. Relay-pinned sessions are exceptions, not a parallel app read model.
For author-scoped public reads, planned means outbox/NIP-65 routing unless a
protocol or feature explicitly proves another route. NIP-29 group hosts,
protocol-private inboxes, and audited explicit relay reads are exceptions that
must be visible in the descriptor and in test coverage.
Planning is lane-aware: author outbox, AppRelay/fallback, indexer/search,
protocol host, private inbox, and diagnostic/manual lanes are different policies,
not just different URL lists. The planner should expose `unroutable_authors` or
equivalent diagnostics when it cannot route a public author set, and source
families that need NIP-51/search/indexer lanes must declare that lane rather than
smuggling fallback relays through shell options.
Composite sessions may mix lanes under one public handle: a room view might have
a NIP-29 host-pinned message lane, planned author/profile ref lanes, local index
lanes, and diagnostic/status lanes. The descriptor owns the route-policy tree and
admission/provenance for each child. The shell still sees one handle and one typed
output contract.
`LogicalInterest` is semantic acquisition demand, not a demand for one relay
subscription per author or one product-specific filter recipe. One interest and
one store query may legitimately cover many authors or kinds when the descriptor
owns routing, replay, admission, and output. Arbitrary per-author caps are not an
architecture boundary; bounded replay and wake fanout are.

App-facing examples:

```text
// planned/outbox-routed by default
timeline = app.open(NotesByAuthors {
  authors: FollowList(active_account),
  kinds: [1],
})

// relay-pinned by protocol context
room = app.open(GroupTimeline {
  group_id,
  host_relay,
})
```

In the first case NMP resolves author write relays, splits per relay, replays
cache/store data, and delivers one typed output under the session handle. In the
second case NMP pins reads to the group host context and rejects replay/live
events that lack read admission proof for that group/relay context. The shell
sees one handle in both cases.

This rule covers the `nmp_app_open_interest` confusion in #2313: the app should
not decide whether a profile, feed, group, search, or embed opens a naked
interest. It opens the typed session; the descriptor supplies route policy.
URI and input-intent doors follow the same rule. An opened `nostr:` URI, shared
text payload, relay URL, or deep link should become a typed `ProfileRef`,
`EventEmbed`, `Search`, group session, publish/action intent, or app-owned
action. It should not route directly to a raw interest view unless the surface is
diagnostic or migration-scoped.

`nmp.browse_relay` is therefore not a normal product read model. A relay browser
can exist as diagnostic/manual-inspection tooling, or as an app Rust feature
that declares a relay-pinned typed session with output, status, and teardown.
Leaving it as a generic public relay-pinned escape hatch would preserve the old
raw-read architecture under a narrower name.

NIP-29 group reads are relay-pinned by group context, not by content kind. The
group feature owns host relay provenance, group id, admin/member metadata, and
`h`-tag admission. It must not default to `kind:9`, chat/share kinds, or another
consumer-specific kind filter. A group timeline session should admit every valid
group-context event that the product has asked to display; product-specific
views may filter or rank those typed outputs after NIP-29 has done only its own
group/host admission. If a reply, reaction, article, share, or custom event has
valid group context, NIP-29 should not silently hide it because the kind is not a
chat kind.

## Component Refs

Small UI components create real data demand. The framework should model that
directly instead of making every app invent ref stores.

Examples:

```text
NostrAvatar(pubkey)
  -> ProfileRef { pubkey, owner = component_id }
  -> output refs.profile rows

NostrContentView(event_ref)
  -> EventEmbed { event_ref, owner = component_id }
  -> output refs.event rows and embed envelopes
```

Generated host adapters may keep row-delta caches for these outputs. Those
caches are rendering infrastructure. They do not decide what to fetch, how to
route, or when a ref is durable product state.

`EventEmbed` is allowed to be composite. A single visible embed owner may claim
the target event, the target author's profile, media metadata, relay hints,
article-card fields, and child refs discovered from the event body. Those child
claims should share the parent owner lifecycle so closing the embed releases the
whole demand tree.
The concrete representation can be renamed, but the invariant should look like a
typed embed projection, a canonical content-tree wire payload, and an owner token
that generated or contract-tested adapters can pass around without understanding
NIP tags. Host component registries may hold render handles; Rust owns the
semantic ref tree, recursion context, and liveness rules.
Ref sessions dedupe by ref identity and owner, close explicitly when the owner
goes away, and guard recursive embeds by depth or cycle identity. A view that
renders `nprofile`, `note`, `nevent`, or `naddr` text creates data demand, but it
does not get its own manual relay retry path or shell-owned ref cache.

URI decoding, canonical ref keys, event-reference hints, relay hints, embed kind
classification, typed article-card fields, and NIP-22 parent/thread identity
should live in Rust descriptors or generated adapters, not be reimplemented from
`p:`/`e:`/`a:` strings, `tagsJson`, or ad hoc JSON shapes in Swift, Kotlin,
TypeScript, and TUI shells.
The boundary is semantic versus presentational. Parsing tags to decide parentage,
reply roots, relay hints, article/highlight card facts, group context, route
provenance, or embed demand is semantic and belongs in Rust descriptors or
generated adapters. Formatting already-projected facts into labels, icons,
localized strings, visual grouping, or platform-specific layout is presentation
and stays in the shell.
Generated ref caches must share the same full/delta/clear/stale-frame merge
contract as other outputs; schema drift between Rust payloads and Swift/Kotlin/
TypeScript mirrors is a correctness bug, not a UI adapter preference.
Host code should not spell raw ref namespaces, shape ids, liveness flags, worker
message names, or `resolve_ref` / `release_ref` payloads in product components.
Those are transport details. The target is generated or contract-tested
`ProfileRef`, `EventEmbed`, and child-ref owner handles that compile to the raw
FFI/worker controls underneath. Keeping the raw controls public for diagnostics
or migration is allowed only with the same scope labels and deletion/formalization
gate as `open_interest`.
If web keeps a component registry for copied/rendered NMP components, the copied
registry needs a source SHA/version baseline and fixture coverage against the Rust
payload contract. The registry may choose how to mount a component; it must not
be the source of semantic ref shape, liveness, recursion, or release policy.
Web must also be classified deliberately. If web is part of the canonical gallery
component registry, each shipped component needs source/version/SHA ownership and
Rust-payload fixture gates. If `components-web` is a first-party source package
instead, it should be described that way and the copied-registry proof should not
claim web coverage.

Gallery is the first component-ref proof, and it must include every live shell:

| Shell | Current proof target | Migration smell to remove or formalize |
|---|---|---|
| iOS/Swift | `refs.profile`, `refs.event`, embed envelopes, sign-in surface | shell URI/ref adapter should become generated descriptor/adapter glue |
| Android/Kotlin | `refs.profile`, `refs.event.envelopes`, NIP-55 signer bridge | auth/signing proof is strongest here; it cannot stand in for other shells |
| Web/TypeScript | `web/nmp-gallery` runtime and component registry | browser runtime exists, but gallery packaging/CI is still deferred; raw worker `resolve_ref` messages, raw numeric namespace/shape/liveness, hand ref cache, and retry/reclaim loop remain |
| TUI | pushed snapshots and visible-profile claims | render-time URI/ref adapter and claim-on-render behavior must become generated/lifecycle-only, not protocol policy |
| Desktop | live bridge and embed/profile display | claim-every-render/tick behavior must be replaced by deterministic owner lifecycle |

Correctness timers are specifically banned here. A copied-label timer is
presentation. A `setInterval` that reclaims refs, retries after relay readiness,
clears dedupe state to make data appear, or repairs missing component output is
session machinery leaking into the shell and fails the architecture gate.
The executable ref proof needs more than a visual gallery screen: no product
`setInterval`/sleep/retry correctness loop, typed or generated ref handles only,
explicit owner open/close, release-on-owner-dispose, stale-frame rejection,
clear/delete behavior, duplicate-row rejection, recursive embed bounds, and
relay-readiness/reconnect tests that pass without shell retry.
Gallery web proof is app-local. Generic browser-runtime OPFS conformance,
storage-only wasm checks, or TypeScript typechecks do not prove `nmp-gallery`
unless the package's own wasm build, Worker startup, OPFS lifecycle, generated
ref adapter, and Playwright path consume the same artifact. Missing wasm or
Worker support may produce typed diagnostics, but it cannot silently become a
successful in-memory product runtime for proof purposes.
The package-local proof shape is explicit: `web/nmp-gallery` must build the wasm
artifact it serves, build the app, and run the browser e2e path against that same
artifact with Worker startup and mandatory durable-store preparation in proof
mode. Typed degraded/no-wasm/no-worker state may be tested, but it is excluded
from success.

## Composite Sessions

Some features are not a single relay filter. A `HomeFeed` can merge article,
highlight, interaction, and reply cursors from a follows source. A `RoomChat`
can combine group metadata, membership, host-relay routing, and message events.
A `Search` can combine relay-backed NIP-50 search with local profile indexes.

The descriptor should support composite children while preserving one public
handle. Child interests, cursors, sources, observed projections, and local
indexes remain owned by Rust.

Highlighter is the concrete composite-read proof:

```text
home feed      -> follows/source reducer + article/highlight/reply cursors
article feed   -> article source + profile/event refs + article-card output
highlight feed -> highlight source + artifact refs + author/profile refs
room lane      -> NIP-29 host route + room membership + message/comment output
search         -> NIP-50/local indexes + profile/event/article outputs
comments       -> NIP-22 thread reducer + parent/child typed output
refs           -> profile/event/embed descriptors shared by visible components
```

If these stay as native-owned NDK subscriptions, Swift tag parsing, or direct
web publish/read paths, the descriptor model has not proven the downstream case.

## Projection Delivery

The app-facing model should not expose Tier-1 versus Tier-2 projections. That
split is an internal execution detail: some producers read kernel state
directly, others are registered closures. The app should see only:

```text
this feature/query produces this typed output
```

Projection payloads should carry semantic facts, not final presentation. A
profile output can carry pubkey, name, picture URL, verification state, and
status tokens; the shell chooses short display strings, initials, colors, icon
names, localization, and relative timestamps. If a value affects protocol
meaning, routing, sorting, replay, persistence, policy, or cross-platform
parity, it belongs in Rust output. If it is only how one platform displays the
same fact, it belongs in the shell.

Opening a dynamic read session is the demand declaration for its output.
Always-on app chrome uses the same typed output ownership/schema contract as screen
state; it is merely opened by app composition or app lifetime rather than by a
visible screen handle. Legacy global declared projections remain compatibility
or private cost-brake machinery only. They are not the future app manifest and
they cannot bypass output ownership, schema owner/version, collision rules, or
merge contracts.

The simplest destination is that session open declares scoped output demand, app
composition declares always-on chrome outputs through the same ownership rules,
and global declared projections remain only as compatibility. A fuller typed
output manifest is justified only if it preserves measured wire, CPU, schema, or
codegen benefits that session-scoped demand cannot reproduce:

```text
feature installs output schemas
app composition declares always-on outputs
session descriptors declare scoped outputs
generated adapters own row/delta caches
UpdateFrame carries full, delta, clear, status, and error variants
```

Existing projection machinery can remain the internal executor. The public
contract should be output ownership and lifecycle, not projection tier mechanics.

`declare_consumed_projections` should be treated as legacy cost-brake machinery,
not as the future manifest. It narrows built-in emission after the system has
already learned how to produce more state than the app wants, and it does not
describe host-registered outputs. The target model is: feature installation
declares available output schemas, app composition declares always-on chrome, and
session open declares scoped demand. Tier names, built-in collision precedence,
and declaration gates stay private executor details or compatibility.

Projection ownership must be explicit. Each output key belongs to exactly one
feature or app crate, carries a schema owner/version, and has one merge contract.
Built-in and host-registered producers must not silently collide with
"built-ins win" precedence. If two producers claim the same key, composition
should fail early unless the owner deliberately aliases or replaces the output
under a documented compatibility rule.
The proof must include generated merge-contract tests for full, delta, clear,
stale-frame, decode-poison, and baseline recovery behavior. Public typed decode
tests are necessary but not sufficient if generated host caches can still drift.
Web `ProjectionMergeCache`-style helpers and generated TypeScript relay/config
tables are adapters, not product authorities. Their schema version, output keys,
relay policy, and merge semantics must derive from Rust/app manifest owners or
be contract-tested against them; a web-only cache/config table that outlives its
source owner is another hidden source of truth.

## Live Counts

Counts should use the same lifecycle model:

```text
LiveCountOutput {
    source,
    filter,
    route_policy,
    output_key,
}
```

The invariant is generic: apps own the product meaning of the count, such as
reactions, replies, bookmarks, listens, or unread items. NMP should first model
counts as typed projections over a session/source/filter. A dedicated
`ReactiveCount` primitive is justified only if it deletes duplicated count
machinery and has tests for relay `COUNT`, local index counts, empty-source
behavior, and teardown.
Counts must also inherit session identity, route policy, source diffing, and
teardown semantics. A count-specific primitive that reintroduces a separate
open/close/replay recipe fails the design.

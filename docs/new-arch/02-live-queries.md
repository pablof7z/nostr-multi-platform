# Live Queries

A screen, component, widget, or app service opens a typed session for the state
it wants to render or keep resident:

```text
open(HomeFeed { account })
open(Profile { pubkey })
open(GroupFeed { group_id, host_relay })
open(Search { query })
open(ProfileRef { pubkey, owner })
open(EventEmbed { event_ref, owner })
open(LiveCountOutput { source, filter, owner })
open(PodcastPlayback { owner = AppLifetime })
open(CustomFeature { source, route, output })
```

`HomeFeed` is an example feature, not a privileged framework primitive. If the
same filter/source/ranking can be expressed by a generic session descriptor, it
should use that path. A hand-coded feed door is acceptable only while it proves
the descriptor model or protects a measured hot path with a deletion/formalize
decision.

The app receives a handle:

```text
LiveQueryHandle {
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

`LiveQuery` is the candidate name for the missing app-facing door. The ADR may
choose typed per-feature open helpers, a generic descriptor API, or a hybrid, but
the accepted shape must let NMP crates and app Rust crates define new read
models without requiring native shells to hand-author relay subscriptions.

The candidate should start as a small typed descriptor plus handle, not a new
lifecycle engine. It compiles into existing or consolidated machinery:

```text
source expression
  -> acquisition demand
  -> route policy
  -> cache/store replay
  -> observed sink
  -> admission predicate
  -> dynamic dependency tracking
  -> typed projection output
  -> generated adapter/cache contract
  -> delivery through UpdateFrame
  -> teardown
```

This is the architectural door missing from the current API. The first
implementation should prove the descriptor can sit on top of the safe
`ObservedProjection` pattern and dependent-interest machinery before adding any
new public lifecycle surface. `open_interest` is only acquisition. It can fetch
events without making them visible to the app, so it should not be the public
app read model. It can remain available to substrate, debug, test, and migration
code that is explicitly acquiring events without claiming an app-visible output
lifecycle.

The proof must start from existing surfaces, not from a fresh abstraction. Try to
generalize or narrow `open_feed`, `resolve_ref`, `ObservedProjectionRegistrar`,
dependent interests, and current feature sessions first. If those can express
the lifecycle with clearer names and smaller public API, prefer that over adding
a public `LiveQuery` object.

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

Multiple owners may share the same `query_key`. Opening increments ownership;
closing decrements it. The final close tears down acquisition, observed sinks,
derived dependencies, and generated output rows. If a feature needs a visible
clear, it emits a typed `Cleared` or tombstone output instead of leaving stale
rows in shell state.

Opening before relay, mailbox, identity, or source readiness is allowed. Rust
queues and replans the session when dependencies arrive. The shell should not
retry with timers.

## ObservedProjection

`ObservedProjection` is the safe event-to-read-model pattern used inside a live
query. It is internal machinery, not a concept app developers assemble.

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

App developers should not manually assemble this. A feature or live query
descriptor uses it internally.

The reconciler must be event-driven. Identity changes, source changes, mailbox
updates, refcount changes, and store ingest should trigger reconciliation. A
snapshot tick observer is not the model.

Relay-pinned observed projections must also prove provenance. A NIP-29 or other
host-pinned session should not accept a matching event merely because the filter
shape matches; replay and live admission must know the event came through the
declared relay context or another protocol-approved source.

## ReducedSource

`ReducedSource` is the model for dynamic query inputs.

Decision status: current NMP docs and code already contain `ReducedSource` and
`open_feed`-style machinery, so the ADR must decide whether that model is being
amended, renamed, or replaced. Until then, this document uses `ReducedSource` to
name the dynamic-source invariant, not to assert that the current type shape is
settled or that a new public primitive is required.

Examples:

- notes by people the active account follows;
- events by members of a NIP-51 list;
- replies to currently visible thread roots;
- target events pointed to by a stream of pointer events;
- group content from groups the account has joined.
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

`ReducedSource` is one possible building block under `LiveQuery`, not a separate
app API the shell has to orchestrate. It should not start as a grand abstraction.
The first implementation should extract the smallest private shape reconciler
around observed-projection open/close. A general reduced-source core is justified
only if real source families share the same diff, fail-closed, teardown, and
dependent-interest semantics without special casing.

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

## Routing

Every session descriptor must declare one routing mode. This is feature/protocol
policy, not a casual caller option passed by the shell:

- **planned route:** the normal case. NMP owns relay planning, including NIP-65
  outbox routing for author-scoped reads, mailbox/inbox discovery where relevant,
  search/discovery relay policy, configured app relays, cache replay, and
  replan-on-mailbox-change behavior.
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

This rule covers the `nmp_app_open_interest` confusion in #2313: the app should
not decide whether a profile, feed, group, search, or embed opens a naked
interest. It opens the typed session; the descriptor supplies route policy.

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

URI decoding, canonical ref keys, event-reference hints, relay hints, embed kind
classification, typed article-card fields, and NIP-22 parent/thread identity
should live in Rust descriptors or generated adapters, not be reimplemented from
`p:`/`e:`/`a:` strings, `tagsJson`, or ad hoc JSON shapes in Swift, Kotlin,
TypeScript, and TUI shells.
Generated ref caches must share the same full/delta/clear/stale-frame merge
contract as other outputs; schema drift between Rust payloads and Swift/Kotlin/
TypeScript mirrors is a correctness bug, not a UI adapter preference.

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

Opening a dynamic live query is the demand declaration for its output. Always-on
app chrome may still need explicit declared outputs, but screen and session
state should be scoped to open handles, not to a global projection list.

The simplest destination is that session open declares scoped output demand, and
global declared projections remain only for always-on app chrome or
compatibility. A fuller typed output manifest is justified only if it preserves
measured wire, CPU, schema, or codegen benefits that session-scoped demand cannot
reproduce:

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

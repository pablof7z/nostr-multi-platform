# Live Queries

A screen opens a live query for the state it wants to render:

```text
open(HomeFeed { account })
open(Profile { pubkey })
open(GroupFeed { group_id, host_relay })
open(Search { query })
open(CustomEvents { source, route, output })
```

The app receives a handle:

```text
LiveQueryHandle {
    id,
    projection_key,
}
```

Native and web shells render the typed projection associated with that handle.
When the screen goes away, the app closes the handle. The shell does not open
raw relay subscriptions, replay cache rows, register observers, or maintain a
parallel cache.

`LiveQuery` is the proposed owner for one live read lifecycle:

```text
source expression
  -> acquisition demand
  -> cache/store replay
  -> observed sink
  -> admission predicate
  -> dynamic dependency tracking
  -> typed projection output
  -> delivery through UpdateFrame
  -> teardown
```

This is the architectural door missing from the current API. `open_interest` is
only acquisition. It can fetch events without making them visible to the app, so
it should not be the public app read model.

## ObservedProjection

`ObservedProjection` is the safe event-to-read-model pattern used inside a live
query.

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

## ReducedSource

`ReducedSource` is the model for dynamic query inputs.

Examples:

- notes by people the active account follows;
- events by members of a NIP-51 list;
- replies to currently visible thread roots;
- target events pointed to by a stream of pointer events;
- group content from groups the account has joined.

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

`ReducedSource` is one building block under `LiveQuery`, not a separate app API
the shell has to orchestrate.

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

# ADR-0070: Typed read sessions own app-visible read lifecycles

## Status

Accepted for the architecture redesign direction.

## Context

Issue #2316 established the root read-path problem: one visible feature state is
assembled by separately wiring acquisition, route planning, cache replay,
observed sinks, admission predicates, projection sidecars, snapshot emission,
dynamic dependencies, and teardown. Issue #2313 is the developer-facing symptom:
an app author can open raw interest, but that is only half an app read model.

Several old primitives protect real invariants. `ObservedProjection` carries
replay-before-live and scoped delivery lessons. Dependent interests and
`ReducedSource`-like code carry source-arrival/source-withdrawal lessons. Pull
cursors and raw event logs remain useful for diagnostics and export. The failure
is exposing these fragments as the way app developers assemble product screens.

## Decision

The app-visible read model is a typed session descriptor plus handle, or a
typed per-feature helper generated from such a descriptor.

A typed session owns the complete lifecycle for one read demand:

- acquisition demand;
- route policy and relay provenance requirements;
- bounded replay before live activation;
- live event/store/capability sink;
- admission predicate and fail-closed behavior;
- typed output schema/status owner;
- wake sources for event, store, source, mailbox, or capability changes;
- teardown for owner close, child demand release, and clear/tombstone output.

Native, web, TUI, and desktop shells open typed sessions and render typed
outputs. They do not hand-author relay filters, raw `open_interest` calls,
projection declarations, reducer names, dynamic source sets, or teardown recipes
for production product screens.

`open_interest` is acquisition only. It may remain substrate, protocol-internal,
diagnostic, test, export, or migration surface with owner and deletion or
formalization criteria. It is not the normal product read API.

`ObservedProjection` is private executor machinery unless a later ADR proves a
public invariant. App developers should not assemble it. `ReducedSource` is
private/provisional dynamic-source machinery unless a later ADR proves a real
app-facing need. `open_feed(FeedParams)` may become a generated helper over typed
sessions, a compatibility shim, or a retired feed-specific door; it must not stay
beside typed sessions as an equal public lifecycle model.

Empty dynamic source sets fail closed unless the feature explicitly declares a
fallback source. Empty authors, tags, refs, or groups never become wildcard
relay demand by accident.

## Consequences

Positive:

- A new app or protocol read feature has one owner for demand, replay, output,
  status, and close.
- Existing safe machinery can be reused privately without making it app
  vocabulary.
- Source changes, account switches, and teardown become contract tests instead
  of feature-local recipes.
- Shells stop reimplementing protocol parsing and read-model ownership.

Negative/tradeoffs:

- The first implementation must retire or scope old public read doors in the
  same slice; otherwise this ADR only adds a facade.
- Dynamic source families should not be over-generalized until multiple real
  sessions prove shared diff, fail-closed, teardown, and dependent-interest
  semantics.
- Some existing builder-guide and product-spec pages must be rewritten because
  they currently teach `open_interest`, `open_feed`, `ObservedProjection`, or
  `ReducedSource` as app-facing concepts.

## Alternatives considered

| Option | Why rejected |
|---|---|
| NDK-style raw `subscribe(filter)` from shell code | It moves protocol parsing, routing, cache replay, and output ownership to the shell. |
| Add an `open_feature()` wrapper over the old fragments | It improves call-site ergonomics while preserving silent desync and duplicate lifecycle recipes. |
| Make `ReducedSource` the public abstraction | It exposes source reconciliation before proving that different source families share one semantic model. |
| Keep `open_feed` as the main public read model | It privileges one feed family and leaves refs, groups, search, embeds, and app-defined reads with parallel recipes. |

## Fitness functions / enforcement

- Public product read APIs are typed session helpers or generated adapters, not
  raw filter dictionaries.
- `open_interest` callers are classified as substrate, protocol-internal,
  diagnostic/test/export, or migration-with-deletion.
- No product screen registers a filterless accepted-event observer and
  self-filters later.
- Session contract tests cover source arrival, source withdrawal, empty-source
  fail-closed behavior, replay-before-live, route replanning, output clear, and
  owner close.
- Old public read-surface counts do not increase after the first ratchet lands.

## Linked work

- #2313: app-developer API complexity.
- #2316: fragmented read lifecycle.
- #2307: event-driven observed-projection reconciler.
- #2320: stale ADR/doc cleanup.
- Amends ADR-0035, ADR-0036, ADR-0039, ADR-0042, ADR-0053, ADR-0057,
  ADR-0062, and ADR-0063 where they expose read internals as product API.

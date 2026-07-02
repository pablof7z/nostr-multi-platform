# ADR-0070: Typed read sessions own app-visible read lifecycles

## Decision

The app-visible read model is a typed session descriptor plus handle, or a
typed per-feature helper generated from that descriptor.

A typed session owns the complete lifecycle for one product read demand:
acquisition demand, route policy, replay, admission, output schema, load status,
wake sources, dynamic source replacement, teardown, and output clearing.

Shells open typed sessions and render typed outputs. They do not hand-author
relay filters, raw `open_interest` calls, projection declarations, reducer
names, dynamic source sets, or teardown recipes for production screens.

`open_interest` is acquisition machinery. `ObservedProjection` and
`ObservedProjectionSink` are scoped event-delivery and replay machinery.
`ReducedSource` names private source reconciliation machinery. These mechanisms
may exist inside substrate, protocol, diagnostics, tests, export paths, or
migration gates, but they are not product read APIs.

Empty dynamic source sets fail closed unless the typed session explicitly
declares a fallback. Empty authors, tags, refs, groups, or relay sets never
become wildcard relay demand by accident.

## Context

Visible product reads were assembled from separate pipes: acquisition, route
planning, cache replay, observed sinks, admission predicates, projection
sidecars, snapshot emission, dynamic dependencies, and close. That lets one
screen drift across many owners.

Typed sessions make one owner responsible for the whole read lifecycle while
still allowing lower-level Nostr machinery to remain private where it belongs.

## Consequences

New product reads need a descriptor and a session contract instead of a loose
set of raw plumbing calls. That costs design work up front, but source arrival,
source withdrawal, replay-before-live, fail-closed empty sets, output clear, and
owner close become testable in one place.

Lower-level read machinery can stay in the codebase when it is internal,
diagnostic, test, or export machinery. It must not be taught as the normal app
surface.

## Boundaries

Permitted:

- typed read-session descriptors and handles;
- generated helpers over typed descriptors;
- internal acquisition, observer, replay, and source-reconciliation machinery;
- diagnostic or export surfaces with a named owner.

Forbidden:

- product screens opening raw relay interests;
- app code declaring projection sinks or reducer names;
- shell-owned dynamic source replacement;
- wildcard demand from empty dynamic sources;
- parallel public lifecycle models beside typed sessions.

## Enforcement

Doctrine lint rejects raw read surfaces in product shells and starter templates.
Clean-room docs gates reject `open_interest`, `ObservedProjection`,
`ObservedProjectionSink`, and `ReducedSource` as app-facing guidance.

Session tests cover source arrival, source withdrawal, empty-source fail-closed
behavior, replay-before-live, route replanning, output clear, pagination where
applicable, and owner close.

## Related

- [ADR-0076](0076-app-facing-feed-helpers.md) - feed helpers over typed
  sessions.
- [ADR-0075](0075-trellis-private-reconciliation-substrate.md) - private
  reconciliation mechanics.
- [ADR-0072](0072-runtime-capability-and-shell-boundary.md) - shell boundary.
- [docs/product-spec/api-surface.md](../product-spec/api-surface.md) - public
  app-facing API examples.
- [docs/architecture/high-level-app-architecture.md](../architecture/high-level-app-architecture.md)
  - developer-facing architecture overview.
- #2746 - ADR current-only cleanup.

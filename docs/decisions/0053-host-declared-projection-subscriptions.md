# ADR-0053: Declared Projection Output Is Bounded Transport Machinery

## Status

Folded into ADR-0070 for app-visible read lifecycles.

## Context

This ADR originally introduced `declare_consumed_projections` to avoid sending
every built-in projection to every host on every frame. That performance concern
was real: output should be bounded by what an app can render, and expensive
debug projections should not be serialized for consumers that never read them.

The old explanation split projections into app-facing tiers and made
declaration look like part of how an app composes product reads. Issue #2316
showed that this is the wrong mental model. Projection declaration is output
transport selection. It does not own acquisition, replay, admission, source
dependencies, or teardown.

## Current Decision

Pushed typed output remains the UI-state transport. A host receives only the
bounded output its app can decode or has opened through typed session lifecycle.

Projection declarations, sidecars, manifests, and change gates are internal
output machinery. App developers should not assemble product reads by choosing
projection tiers, projection keys, observer sinks, and raw interests by hand.

Typed read sessions own product read lifecycle. A session may register or
activate projection output internally, but the session descriptor is the public
unit the app opens and closes.

## Consequences

- The pay-for-what-you-render invariant survives.
- The old projection-tier vocabulary is not current app architecture.
- `declare_consumed_projections` must not be taught as a production feature
  composition step.
- Projection output remains push-based; this ADR does not introduce host-called
  polling or generic snapshot getters.

## Fitness Functions

- New product docs explain reads as typed sessions, not projection-tier wiring.
- Output gates must not require native shells to compute protocol policy.
- Debug or diagnostics projections remain opt-in or session-scoped where
  possible.
- Any surviving projection declaration API is classified as substrate,
  compatibility, or generated/runtime plumbing.

## Historical Note

The old Tier-1/Tier-2 text captured useful implementation history, but it is no
longer the current developer model. Git history preserves that detail; this ADR
now records the live rule.

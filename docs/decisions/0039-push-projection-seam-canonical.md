# ADR-0039 — Push projection seam is canonical

- **Status:** Accepted / amended by ADR-0053 and ADR-0070
- **Date:** 2026-05-31
- **Relates to:** ADR-0025, ADR-0037, ADR-0053

**Current disposition:** pushed typed output remains the UI-state path. ADR-0070
changes the app-facing ownership model: production reads are opened as typed read
sessions that own demand, replay, output, status, and teardown. Projection keys
and sidecars are executor/output contract machinery, not the public way an app
author assembles a product read lifecycle.

## Context

Hosts need kernel-derived projection state without polling. A host-called snapshot
getter has no change signal, so it pushes platform code toward timer loops. NMP's
reactivity model instead emits changed state through the snapshot/update stream.

ADR-0053 adds host-declared projection subscriptions: hosts declare which
registered projections they want to receive. That narrows delivery; it does not
add a pull read path.

## Decision

Registered pushed projections are the single canonical FFI read seam for
kernel-derived view state.

The flow is:

1. A protocol/app crate registers a projection closure for a stable projection
   key.
2. A host declares interest in the projection keys it consumes.
3. The actor emits changed projection data in the pushed snapshot frame.
4. The host reads the projection from its `apply` callback.

Typed FlatBuffers sidecars are an encoding optimization layered on the same push
seam. They are not a separate app-facing read model.

## Rejected Shape

Do not add host-called generic snapshot getters or app-specific snapshot getters.
They create a second read seam and make polling the natural host implementation.

## Consequences

- Snapshot delivery stays event-driven.
- Projection keys are the public contract; transport encoding is an internal
  detail of that contract.
- Hosts do not own cache freshness or backfill policy.
- App-specific read state must be expressed as registered projections, not as
  bespoke C-ABI read functions.

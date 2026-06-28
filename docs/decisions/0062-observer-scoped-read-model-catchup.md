# ADR-0062: Observer Catch-Up Is Private Read-Session Machinery

## Status

Folded into ADR-0070 for typed read sessions.

## Context

Late-opening read models exposed a real kernel defect: an event can already be
present in the in-memory or durable read model before a new observer exists. If
the observer only receives future live broadcasts, it misses matching cached
events. Apps worked around this by reading storage directly, which violated the
kernel-owned acquisition boundary.

The durable invariant is delivery correctness: when a read model becomes active,
it receives matching cached/stored events before future live events, without
duplicates and without asking the app shell to hydrate from storage.

## Current Decision

Observer catch-up is internal execution machinery behind typed read sessions.

A typed read session owns:

- the acquisition shape;
- bounded replay before live activation;
- the observer or sink that receives events;
- admission and output status;
- teardown and clear/tombstone output.

`ObservedProjectionRegistrar`, muted observer activation, replay shapes, and
scoped observer delivery are not the production app API. They may remain
substrate/protocol-internal primitives used by session executors.

Public, filterless accepted-event observer lanes are rejected for product reads.
They force apps to self-filter and can turn every read model into an unbounded
event tap.

## Consequences

- Apps must not hydrate product read models by querying LMDB directly.
- Product screens open typed sessions; they do not wire observer registration,
  raw interest open, replay, activation, and projection output as separate
  steps.
- The replay-before-live invariant remains mandatory for every session executor.
- Filterless live-event taps, if any survive, are diagnostic/export/test
  surfaces with an owner and explicit scope.

## Fitness Functions

- Session contract tests cover cached replay, stored replay, live-after-replay,
  no duplicate delivery, owner close, and output clear.
- No product shell or app core adds direct store hydration for render state.
- New observer-like APIs must be scoped by shape/owner/replay or rejected.

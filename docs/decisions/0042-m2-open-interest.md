# ADR-0042: Generic Interests Are Substrate Machinery

## Status

Folded into ADR-0070 for app-visible read lifecycles.

## Context

This ADR originally removed bespoke app-feed FFI verbs such as
`open_author`, `open_thread`, and `open_firehose_tag` and replaced them with a
smaller generic interest seam. That deletion was correct: `nmp-core` must not
encode product concepts like "social timeline", "author page", or "thread
screen".

The follow-on mistake was treating raw `open_interest` as if it could become the
normal app read API. Issue #2316 showed why that fails: acquiring relay events is
only one part of a product read. A screen also needs bounded replay, admission,
source reconciliation, typed output, status, teardown, and route provenance.

## Current Decision

`open_interest` and `close_interest` are low-level acquisition machinery. They
may exist for substrate, protocol-internal, diagnostic, export, test, or
explicit migration scopes with an owner and removal/formalization trigger.

They are not the production app read API.

Production reads are typed read sessions or generated helpers over typed session
descriptors, as defined by ADR-0070. A typed session owns the whole lifecycle:
acquisition demand, replay, admission, output, wake sources, route policy,
status, and close.

Dynamic product reads such as "events from people I follow" do not become a raw
filter with a mutable authors list. They are session-owned source
reconciliation. Empty dynamic source sets fail closed unless the owning feature
declares a fallback.

## Consequences

- The old bespoke app-feed verbs stay deleted.
- `open_interest` remains available only as a scoped internal or diagnostic
  substrate door.
- App screens must not hand-author relay filters, owner ids, teardown recipes,
  or projection registrations for normal product reads.
- Feed-specific helpers such as `open_feed` may survive only as generated or
  compatibility helpers over typed sessions, not as an equal lifecycle model.

## Fitness Functions

- Product shells and starter templates must not introduce new raw
  `open_interest` read flows.
- Existing `open_interest` call sites must be classified as substrate,
  protocol-internal, diagnostic/export/test, or migration.
- Session contract tests must cover replay-before-live, source arrival,
  source withdrawal, empty-source fail-closed behavior, and owner close.

## Historical Note

The original M2 symbol delta was useful: it deleted app-specific feed verbs from
the substrate. Its surviving lesson is deletion and D0 cleanup, not a public raw
subscription API.

---
title: "Subscription Planning and Routing"
summary: "NMP turns view interests into compiled relay plans so apps do not hand-roll REQs, relay fan-out, or follow-list rewiring."
tags: [planner, routing, relays]
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
volatility: warm
confidence: high
sources:
  - "raw/repos/2026-05-28-source-map.md"
  - "raw/repos/2026-05-28-planner-current-code-sources.md"
---

# Subscription Planning and Routing

NMP does not let each view hand-format its own relay requests. A view declares
what it needs as a logical interest. The planner compiles the current set of
interests into per-relay plans, and the wire emitter diffs those plans against
what is already open.

This is the difference between "format a REQ string" and "compile a routing
plan". The latter can be rerun whenever relay metadata, account state, or view
ownership changes.

## LogicalInterest Is Not a Nostr Filter

A Nostr filter is the wire shape. `LogicalInterest` is the framework shape. It
adds identity, scope, lifecycle, hints, and deterministic ordering so the
compiler can hash, diff, and recompile without churn.

The scope matters. An active-account following timeline should not capture the
authors once and keep them forever. It should re-resolve when the active
account, follow list, or mailbox data changes.

The current implementation lives in `crates/nmp-planner`, not in the older
`crates/nmp-core/src/planner` path some builder-guide prose still references.
`nmp-core` re-exports the public surface, but the durable crate-boundary shape
is that the planner implementation is outside the kernel substrate.

`InterestShape` carries the filter-like facts (`authors`, `kinds`, tags,
event ids, address coordinates, time bounds, limits) plus client-side routing
metadata (`relay_pin`, `p_tag_routing`). `relay_pin` is never serialized into a
Nostr filter. It decides which relay receives the filter.

## Routing Inputs

The compiler and router combine several facts:

- authors and tags in the interest shape;
- mailbox data derived from relay-list events;
- relay hints and provenance;
- user-configured relays and indexers;
- explicit targets supplied by protocol modules when a generic algorithm cannot
  infer the right host.

The safe app-building path does not expose a relay URL field on normal view
open or publish actions.

`RoutingSource` preserves why a relay was selected. A single relay can carry
multiple role tags. Indexer fallback is represented as
`UserConfigured(Indexer)`, so diagnostics keep the lane model coherent instead
of inventing an extra lane for indexers.

## Recompilation

Recompilation is safe because the output is a plan, not an immediate socket
side effect. If the same inputs produce the same plan id, there is no wire
churn. If a kind:3 follow list changes, the compiler can close only removed
author slices and open only newly needed slices.

`CompiledPlan` is the handoff object: it contains a stable `plan_id`,
`per_relay` slices, and derived `unroutable_authors`. Each `RelayPlan` holds
the target URL, role tags, and merged `SubShape`s. Each `SubShape` carries the
canonical filter hash used by the wire-emitter and diagnostics.

Post-compile mutators must be explicit about whether they change the canonical
filter meaning. The coverage gate recomputes the sub-shape hash after changing
`since`; cache-floor rewrites that are only runtime watermarks do not.

## See Also

- [[rust-owned-logic-boundary|Rust-Owned Logic Boundary]] ([Rust-Owned Logic Boundary](../concepts/rust-owned-logic-boundary.md))
- [[crate-boundaries-and-module-ownership|Crate Boundaries and Module Ownership]] ([Crate Boundaries and Module Ownership](crate-boundaries-and-module-ownership.md))
- [[doctrine-enforcement-map|Doctrine Enforcement Map]] ([Doctrine Enforcement Map](../concepts/doctrine-enforcement-map.md))

## Sources

- [NMP Source Map 2026-05-28](../../raw/repos/2026-05-28-source-map.md)
- [Planner Current Code Sources](../../raw/repos/2026-05-28-planner-current-code-sources.md)

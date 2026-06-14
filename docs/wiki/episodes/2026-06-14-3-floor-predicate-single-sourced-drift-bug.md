---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: superseded
subjects:
  - floor-predicate-unification
  - shape-to-store-queries
  - shape-floor-drift
supersedes:
  - 2026-06-14-2-since-floor-migrating-from-presence-heuristic
related_claims: []
source_lines:
  - 6148-6187
captured_at: 2026-06-14T12:30:14Z
---

# Episode: Floor predicate single-sourced — drift bug found and eliminated

## Prior State

Three hand-synced copies of the shape→StoreQuery mapping existed: (1) shape_to_store_queries used by watermark_from_queries, (2) shape_floor's hand-rolled match, and (3) pin_shape_events_below_floor. Copy (2) had already drifted — its addressable branch used max-ignoring-empties (pre-B1 unsafe policy) while watermark_from_queries used min/abort.

## Trigger

Stage C verification found two residual mappings, one already drifted relative to the unified source. ADR-0056 §2.5 explicitly named these copies for deletion as a precondition for the ledger swap.

## Decision

Delete both residual copies; route all floor computation through the single shape_to_store_queries → watermark_from_queries mapping. shape_floor now delegates to watermark_from_queries (with a truncated HashSet parameter for B3's refusal). pin_shape_events_below_floor derives from shape_to_store_queries. Add drift-oracle regression test verified RED-then-GREEN.

## Consequences

- Floor computation is single-sourced (net −171/+141 lines)
- Latent shape_floor drift bug eliminated (addressable branch was unsafe)
- D2 ledger swap becomes a single-mapping migration instead of three — substantially de-risked
- Drift-oracle test prevents silent re-divergence

## Open Tail

*(none)*

## Evidence

- transcript lines 6148-6187


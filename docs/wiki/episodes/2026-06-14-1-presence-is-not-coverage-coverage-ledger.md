---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: superseded
subjects:
  - coverage-ledger
  - since-floor
  - nmp-core
  - nmp-store
supersedes:
  - 2026-06-14-1-presence-to-coverage-ledger-migration-k3
  - 2026-06-14-2-neg-open-reconciliation-un-floored-floor
  - 2026-06-14-3-floor-predicate-single-sourced-drift-bug
related_claims: []
source_lines:
  - 6027-6036
  - 6083-6115
  - 6131-6173
  - 6277-6307
  - 6329-6359
  - 6376-6399
  - 6495-6560
captured_at: 2026-06-14T16:03:45Z
---

# Episode: Presence-is-not-coverage: coverage ledger replaces presence-floor (K3)

## Prior State

The since-floor used presence (newest stored event timestamp per shape) as a proxy for coverage (timestamp through which a sync actually completed), creating a class of permanent backfill holes where the floor over-claimed what had been fetched. NEG-OPEN reconciliation was suppressed below the floor. The floor predicate had three hand-synced copies that could drift; the address-pointer branch used max-ignoring-empties instead of min/abort, and shape_floor had already drifted from the canonical mapping.

## Trigger

The H1 finding from the 16-journey review: follow-after-thread-reply suppressed an author's backfill because the presence-floor over-claimed coverage. Stage C further discovered that shape_floor's addressable branch still used the pre-B1 unsafe max-ignoring-empties policy — a latent drift bug confirming the three-copy hazard.

## Decision

Staged migration from presence-floor to per-(filter, relay) coverage ledger (CoverageRow), behind an off-by-default `coverage_ledger_enabled` flag with owner-gated activation. Six stages: A (un-floor NEG-OPEN so reconciliation self-heals below-floor gaps), B (harden heuristic — min/abort for address-pointer, NEG-OPEN liveness deadline, Etag/Ptag truncation floor refusal), C (collapse three hand-synced predicate copies into single `shape_to_store_queries`, deleting drifted residual copies), D1 (write path records honest coverage at EOSE/NEG-DONE), D2 (read swap — flag-ON with no-coverage-row REFUSES the floor rather than falling back to unsound presence), D3 (eviction-lowers-ledger backstop via CoverageGuard, atomic on both backends). Stage E (delete presence heuristic) and default-on flip are owner-gated.

## Consequences

- The presence-is-not-coverage bug class is structurally closed behind the flag; the H1 backfill-suppression bug is proven fixed flag-on and still-broken flag-off by an in-process relay journey test
- The since-floor predicate is single-sourced through shape_to_store_queries, eliminating the drift class that Stage C caught (shape_floor had already diverged)
- Eviction and ledger coverage are atomic via CoverageGuard (opaque matches closure + row key) on both Mem and LMDB backends
- The default-on flag flip is an owner-gated release-cut decision; external git-rev-pinning consumers must deliberately pin across it
- NEG-OPEN reconciliation is self-healing for below-floor gaps (unfloored filter drops the since lower bound)
- D2 resolved a spec tension: flag-ON + no-coverage-row refuses the floor (full window) rather than falling back to presence, because presence is exactly the unsound source H1 names

## Open Tail

- Stage E (delete the presence heuristic + dead flag path) awaits the flag-flip being validated in production
- Default-on activation is an owner-gated release-cut decision, not yet taken — the flag currently defaults to false on master
- External consumers (podcast-player, hl) are already on the post-keystone API and need only a routine pin bump once the flip lands

## Evidence

- transcript lines 6027-6036
- transcript lines 6083-6115
- transcript lines 6131-6173
- transcript lines 6277-6307
- transcript lines 6329-6359
- transcript lines 6376-6399
- transcript lines 6495-6560


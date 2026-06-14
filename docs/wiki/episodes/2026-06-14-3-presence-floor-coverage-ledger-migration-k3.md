---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: superseded
subjects:
  - coverage-ledger
  - presence-is-not-coverage
  - watermark-row
  - coverage-ledger-enabled-flag
supersedes: []
related_claims: []
source_lines:
  - 6050-6400
captured_at: 2026-06-14T13:26:23Z
---

# Episode: Presence-floor → coverage-ledger migration (K3)

## Prior State

The since-floor measured 'presence' (newest stored event matching a shape) but soundness requires 'coverage' (timestamp through which a sync actually completed). The gap created a class of permanent backfill holes — following a thread reply could permanently suppress an author's backfill (H1). Three hand-synced copies of the shape→query mapping existed; one (shape_floor's addressable branch) had drifted to the pre-B1 unsafe max-ignoring-empties policy.

## Trigger

H1 from the 16-journey review: presence-is-not-coverage is the deepest finding. Stage C recon found the drifted shape_floor copy. Stage B confirmed the presence heuristic needed soundness patches (address-pointer min/abort alignment, NEG-OPEN liveness deadline, Etag/Ptag truncation floor refusal) to be safe while still governing every read subscription.

## Decision

Replace presence-floor with per-(filter_hash, relay) coverage ledger (CoverageRow, downward-closed), staged behind coverage_ledger_enabled flag (default off). Unify all floor⇄serve paths through single shape_to_store_queries mapping (Stage C deleted two drifted copies). With flag ON and no coverage row, REFUSE the floor (full window) rather than fall back to unsound presence. Eviction⇄ledger coherence enforced atomically (CoverageGuard closure, lowering in same txn as delete on both Mem and LMDB). Default-on flip deferred to owner-gated release-cut PR.

## Consequences

- H1 backfill-suppression bug proven fixed with flag ON (journey test: full history backfills), still-broken with flag OFF
- Production byte-identical with flag off — safe to ship dormant
- Single-sourced predicate prevents future mapping drift (regression test locks the invariant)
- Flag-ON no-row REFUSES floor rather than falling back to presence — no unsound over-claim path exists
- Eviction-lowers-ledger backstop atomic on both backends (oldest_evicted - 1 lowering)
- External git-rev-pinning consumers (podcast-player, hl, win-the-day) must deliberately pin across the eventual default-on flip
- Stage E (delete presence heuristic) only safe after flag validated in production

## Open Tail

- Owner must decide when to cut the release that flips coverage_ledger_enabled to default-on
- Stage E (delete presence heuristic + correct stale '[LANDED M4]' docs) blocked on flag validation in production

## Evidence

- transcript lines 6050-6400


---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: product
status: superseded
subjects:
  - coverage-ledger
  - presence-floor
  - backfill-suppression
  - nmp-core
supersedes:
  - 2026-06-14-1-presence-is-not-coverage-coverage-ledger
related_claims: []
source_lines:
  - 6298-6306
  - 6329-6360
  - 6564-6626
  - 6599-6605
captured_at: 2026-06-14T17:10:22Z
---

# Episode: Presence heuristic replaced by per-(filter, relay) coverage ledger as sole floor source

## Prior State

Coverage floor was computed from a 'presence' heuristic (event watermark) that conflated 'event exists' with 'coverage complete' — causing H1 backfill-suppression bugs where a follow after a stray reply would suppress full-history requests for unsynced shapes.

## Trigger

H1 root-cause finding that presence-is-not-coverage; identified as the deepest finding in the 16-journey review. Production LRU eviction (HOT_EVENT_CEILING=10k) also threatened to create permanent coverage holes if eviction deleted below a ledger's covered_through.

## Decision

Replace presence-floor with a per-(filter_hash, relay) coverage ledger as the sole since-floor source. Staged rollout: D1 (write path, flag-off), D2 (read path, flag-off with presence fallback), D3 (eviction-coherence via CoverageGuard atomic lowering), then flag default-on (#1419), then Stage E deletes the presence heuristic entirely (#1421) — ledger is now the only floor. Refuse-the-floor on no-coverage-row (rather than fall back to unsound presence value).

## Consequences

- More initial relay traffic on cold/unsynced shapes (they now request full history until coverage is confirmed) — this is the sound behavior
- H1 backfill-suppression bug fixed flag-on, confirmed still-broken flag-off in journey test before flip
- Eviction-ledger coherence enforced: pin below ledger floor + atomic lower covered_through on eviction via CoverageGuard closure
- Breaking release nmp-v0.7.0 cut; external git-rev-pinning consumers must deliberately pin across the behavioral change
- Presence heuristic code and the flag itself fully removed — no fallback path remains

## Open Tail

- Follow-up issue #1426 filed for CI release/conformance gate that compiles every excluded crate a known downstream path-deps (the parked-crate blind spot)

## Evidence

- transcript lines 6298-6306
- transcript lines 6329-6360
- transcript lines 6564-6626
- transcript lines 6599-6605


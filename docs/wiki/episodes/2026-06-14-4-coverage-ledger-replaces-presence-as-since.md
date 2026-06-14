---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: reversal
status: superseded
subjects:
  - coverage-ledger
  - presence-to-coverage
  - since-floor-source-of-truth
  - coverage-ledger-flag
supersedes: []
related_claims: []
source_lines:
  - 6191-6327
captured_at: 2026-06-14T12:30:14Z
---

# Episode: Coverage ledger replaces presence as since-floor source-of-truth

## Prior State

The since-floor was computed from presence (newest stored event of a shape), which is unsound — a single stray stored event permanently suppresses backfill for all authors in a fanout shape. H1: follow-after-thread-reply permanently suppresses an author's backfill.

## Trigger

H1 finding from 16-journey read-path review; ADR-0056 mandated staged migration from presence-floor to per-(filter, relay) coverage ledger.

## Decision

Replace presence-based floor with per-(filter, relay) CoverageRow (covered_through timestamp) as source-of-truth. Critical design resolution: when flag is ON and no coverage row exists, REFUSE the floor (full window) rather than falling back to presence — because presence is exactly the unsound source H1 names. Flag is default-off (coverage_ledger_enabled) for safe rollout; production byte-identical until the deliberate release-cut flip.

## Consequences

- Source-of-truth shifts from presence to coverage — the deepest finding in the 16-journey review
- Per-relay WatermarkFn signature change — all callers migrated
- coverage_floor_with_fallback unified decision table prevents hand-synced second copy (honoring Stage C discipline)
- Second floor site discovered and covered: handle_reconnect (reconnection-replay path) in addition to apply_watermark_rewrite
- Journey test proves H1 fixed flag-on and still-broken flag-off, running in default cargo test lane
- Eviction coherence required (D3) — LRU eviction is live in production at HOT_EVENT_CEILING=10k
- Release-cut flag flip must be deliberate and owner-gated for external git-rev consumers

## Open Tail

- D3: eviction⇄ledger coherence (pin below ledger floor; lower covered_through atomically on eviction)
- Stage E: delete the presence heuristic + correct stale [LANDED M4] docs
- Release-cut: deliberate default-on flip of coverage_ledger_enabled

## Evidence

- transcript lines 6191-6327


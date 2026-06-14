---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: architecture
status: superseded
subjects:
  - since-floor
  - coverage-ledger
  - neg-open
  - watermark
  - presence-floor
  - shape-to-store-queries
  - cache-serve
  - coverage-row
supersedes: []
related_claims: []
source_lines:
  - 6020-6034
  - 6084-6115
  - 6131-6146
  - 6157-6184
  - 6243-6275
captured_at: 2026-06-14T11:30:42Z
---

# Episode: Since-floor migrating from presence heuristic to per-(filter, relay) coverage ledger; presence hardened and single-sourced in transit

## Prior State

The since-floor measured 'presence' (newest stored event matching the shape) rather than 'coverage' (timestamp through which sync actually completed), creating a class of permanent backfill holes — the deepest finding from the 16-journey review. NEG-OPEN inherited the floored since so reconciliation couldn't repair below-floor gaps. Three hand-synced copies of the shape→StoreQuery mapping existed; shape_floor had already drifted (its addressable branch still used the pre-B1 unsafe max-ignoring-empties policy). LRU eviction was enabled in production (HOT_EVENT_CEILING=10k) but eviction⇄floor coherence was not enforced.

## Trigger

H1/H2 read-path findings from 16-journey review; ADR-0056 staged migration from presence-floor to per-(filter,relay) coverage.

## Decision

Stage A: EligibleFilter::unfloored() drops the since lower bound on the NEG path so reconciliation self-heals below-floor gaps (floor kept on plain REQs). Stage B: three floor-soundness patches (address-pointer min/abort alignment, NEG-OPEN liveness deadline via on_idle_tick, Etag/Ptag truncated-serve floor refusal). Stage C: all three floor-computing paths unified to single shape_to_store_queries mapping with drift-oracle lock test; shape_floor drift bug caught and fixed. Stage D1: CoverageRow (downward-closed, keyed (filter_hash, relay)) writes honest coverage at EOSE/NEG-DONE behind coverage_ledger_enabled flag, default off — no read behavior change yet. Stage D2 (read-swap) dispatched but not yet landed, gated on fixture-relay backfill journey test.

## Consequences

- NEG-eligible shapes now self-heal below-floor gaps via unfloored filter
- shape_floor can no longer silently drift from the canonical watermark computation — locked by regression test
- The original dormant WatermarkRow/types/watermark.rs no longer exist (deleted in earlier #1090); CoverageRow is a fresh creation keyed differently
- Coverage ledger writes honest coverage: a since-floored EOSE records nothing, avoiding the over-claim that made presence unsound
- The dangerous default-on flag flip is deliberately deferred to a separate release-cut PR so external consumers can pin across it
- LRU eviction is confirmed enabled in production (HOT_EVENT_CEILING=10k), making D3 eviction⇄ledger coherence a real (not design-only) requirement

## Open Tail

- D2 (read-swap to coverage-based floor, flag still off) in flight
- D3 (eviction⇄ledger coherence) queued
- E (delete presence heuristic + correct stale '[LANDED M4]' docs) queued
- Release-cut flag-flip PR needed after D2/D3/E land

## Evidence

- transcript lines 6020-6034
- transcript lines 6084-6115
- transcript lines 6131-6146
- transcript lines 6157-6184
- transcript lines 6243-6275


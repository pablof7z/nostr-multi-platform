# ADR 0002: Delta-volume budget is per-view, not absolute

**Date:** 2026-05-17
**Status:** accepted
**Supersedes:** `reactivity.md` rev 0 §10.3 budget, `plan.md` Phase 1 exit gate

## Context

The initial Phase 1 exit gate set "≤ 1000 deltas/sec cumulative" under hashtag firehose. The reactivity-bench harness (run 001) reported:

- hashtag_firehose: 2,000 deltas/sec (one view, 200 events/sec).
- profile_fanout: 3,379.56 deltas/sec (50 views sharing authors, single kind:0 arrival).

The absolute budget conflates two distinct problems:

1. **profile_fanout** is structural fan-out: 50 views legitimately need to be told the same author changed. 3,379 deltas across 50 views ≈ 67/view/sec — barely over per-view target, fixable with within-view coalescing.
2. **hashtag_firehose** is a single view receiving 2,000 deltas/sec — a real within-view problem.

An absolute gate fails both. A per-view gate correctly identifies hashtag_firehose as the substantive failure and profile_fanout as a borderline case.

## Decision

The delta-volume budget is **per-view, per-second**, not absolute.

| Metric | Budget |
|---|---|
| Deltas emitted per view per second | ≤ 60 |
| Reasoning | matches 60Hz flush cadence; within-view coalescing produces exactly this in steady state |

The total deltas/sec across the buffer is naturally bounded by `60 × active_view_count`, which scales with what the app is actually rendering.

## Consequences

- Within-view coalescing at flush time becomes mandatory (`DeltaBuffer::flush()` post-processes by view id; see ADR follow-up on coalescing rules per view kind).
- No absolute deltas/sec ceiling — apps with 100 active views legitimately produce up to 6,000 deltas/sec, all small.
- The harness reports per-view-kind delta rates separately, enabling targeted optimization.

## Alternatives considered

- **Keep absolute gate, tune number.** Rejected — the right number scales with active views.
- **Per-view-kind budget.** Considered; rejected as too granular for a top-level gate. Per-view is the right level; per-kind investigation happens when a specific view kind exceeds.

## Validation

Re-run reactivity-bench after coalescing is implemented; require ≤ 60 deltas/sec/view on hashtag_firehose and profile_fanout.

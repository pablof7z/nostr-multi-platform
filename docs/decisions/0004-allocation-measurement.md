# ADR 0004: Allocation measurement plumbed via counting allocator

**Date:** 2026-05-17
**Status:** accepted

## Context

`reactivity.md` rev 0 §8 states "no allocations on hot paths" as an invariant. The reactivity-bench harness run 001 reported "allocations: unmeasured." The invariant is aspirational, not verifiable.

## Decision

Plumb a counting allocator into the harness. Two options, both acceptable:

- **`dhat`** — heap-profiling allocator with per-call-site attribution. Used in CI for the harness binary specifically.
- **Custom counting `GlobalAlloc`** — minimal wrapper around `System` allocator that increments per-allocation counters. Cheap enough to leave on in all harness runs.

Recommendation: custom counting allocator for steady-state observation (always on in harness binary); `dhat` invoked manually when investigating specific regressions.

The harness reports:

- Total allocations during the measured window.
- Allocations attributable to per-event paths (insert → reverse-index lookup → recompute → delta buffer push).
- Peak heap.

The gate "≤ 0 allocations on the steady-state per-event path" becomes verifiable.

## Consequences

- The harness binary depends on the allocator wrapper. Production builds don't.
- Some allocations are unavoidable on first-time paths (e.g., new view registration); the gate covers steady-state only, defined as "after 1,000 warmup events."
- If allocations slip in (e.g., a `Vec::push` that grows past capacity), the harness catches them before code lands.

## Alternatives considered

- **Skip the gate.** Rejected — without measurement, the invariant is decorative.
- **Use only `dhat`.** Rejected for CI — startup cost is too high for every harness run; reserve for investigation.
- **Sample-based profiling (`pprof`).** Rejected — doesn't catch low-frequency allocations that compound under firehose.

## Validation

Harness reports zero allocations on the per-event steady-state path in all scenarios after the warmup window.

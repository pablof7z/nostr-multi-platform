---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: superseded
subjects:
  - ci-gating
  - cache-baseline
  - acceptance-thresholds
supersedes: []
related_claims: []
source_lines:
  - 1370-1425
captured_at: 2026-06-18T18:30:34Z
---

# Episode: Cache performance gates: deterministic metrics are hard CI, wall-clock is report-only

## Prior State

No explicit doctrine existed on which cache-serve performance dimensions belong in hard CI vs PR-description reporting. The precedent of `s3-snapshot-pressure-gate.yml` ran hard latency gates only nightly on a fixed runner, never per-PR.

## Trigger

#1524 planning identified that events-scanned count and replay-chunk count are deterministic across machines, while query latency (p50/p99 µs) and allocation bytes/query vary by runner and allocator.

## Decision

Hard CI gates are limited to deterministic metrics: events-scanned count (must be ≤ limit + ε after #1516) and replay-chunk count (must be ≤ budget). Wall-clock latency, allocation bytes, and projection-update cadence are report-only — contributors paste before/after from the `cache-baseline` binary into their PR description.

## Consequences

- No new GitHub Actions workflow for cache performance; deterministic gates run as plain `cargo test` in existing CI
- The PR delta table format (5 dimensions: latency, alloc, events-scanned, replay-chunks, projection-updates) is documented in `docs/wiki/guides/cache-baseline.md`
- Nightly hard-gate for latency/alloc is a future option following the s3 precedent, but not implemented now

## Open Tail

- The report-only dimensions may later become nightly hard gates on a fixed runner if regression patterns emerge

## Evidence

- transcript lines 1370-1425

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-cache-performance-gates-deterministic-metrics-are.json`](transcripts/2026-06-18-3-cache-performance-gates-deterministic-metrics-are.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-cache-performance-gates-deterministic-metrics-are.json`](transcripts/raw/2026-06-18-3-cache-performance-gates-deterministic-metrics-are.json)

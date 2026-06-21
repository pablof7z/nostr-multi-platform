---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: active
subjects:
  - cache-acceptance-gates
  - store-query-visit
  - lmdb-backend-ci
supersedes:
  - 2026-06-18-3-cache-performance-gates-deterministic-metrics-are
related_claims: []
source_lines:
  - 3641-3670
  - 4237-4254
captured_at: 2026-06-18T20:35:15Z
---

# Episode: cache-store acceptance gates: hard regression contract + CI wiring gap discovered

## Prior State

No hard regression gates existed for the streaming query_visit contract; the materialization counter (CONVERSION_COUNT) was gated #[cfg(test)] and unreachable from integration tests; CI did not run nmp-testing with the lmdb-backend feature, so any LMDB-specific gate would be dead on arrival.

## Trigger

Epic #1524 required acceptance gates before closure; the Opus planning audit discovered the CI gap and the cfg-test isolation, and CI failures on PR #1549 revealed a global AtomicUsize race under parallel test execution.

## Decision

HARD gates (failing CI assertions) for materialization count, limit honoring, Mem≡LMDB parity, and early-break contract; DELTA-REPORT-ONLY (paste into PR body) for latency/allocation thresholds. Expose CONVERSION_COUNT under #[cfg(any(test, feature = "test-support"))] and re-export through nmp-core so nmp-testing can reach it. Add cargo test -p nmp-testing --features lmdb-backend CI step.

## Consequences

- Any accidental reversion of streaming query_visit to collect().take() will trip cache_no_materialization_gate in CI.
- All six StoreQuery variants have per-variant streaming proof tests, preventing a regression in one variant while others pass.
- The CONVERSION_COUNT global atomic races under parallel test execution — tests need serialization (diagnosed but not yet fixed in this session).

## Open Tail

- CONVERSION_COUNT race condition under parallel test execution: authorkind_streaming got 10 instead of 5, ptag_streaming got 4 instead of 5. Needs Mutex serialization or --test-threads=1 in the gate test harness.

## Evidence

- transcript lines 3641-3670
- transcript lines 4237-4254

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-cache-store-acceptance-gates-hard-regression.json`](transcripts/2026-06-18-2-cache-store-acceptance-gates-hard-regression.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-cache-store-acceptance-gates-hard-regression.json`](transcripts/raw/2026-06-18-2-cache-store-acceptance-gates-hard-regression.json)

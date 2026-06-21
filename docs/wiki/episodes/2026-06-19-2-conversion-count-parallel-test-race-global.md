---
type: episode-card
date: 2026-06-19
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: root-cause
status: active
subjects:
  - acceptance-gates
  - query-streaming
  - test-infrastructure
supersedes:
  - 2026-06-18-4-global-atomicusize-test-counter-is-insufficient
related_claims: []
source_lines:
  - 4237-4354
captured_at: 2026-06-19T11:51:35Z
---

# Episode: CONVERSION_COUNT parallel-test race — global atomic bleeds across test threads

## Prior State

CONVERSION_COUNT (AtomicUsize) in query_streaming.rs was shared across all test threads; each test called reset_conversion_count() then asserted the count, assuming no concurrent writes from other tests.

## Trigger

CI failures on authorkind_streaming (read 10 instead of expected 5), ptag_streaming, and limit_caps_conversions_no_over_scan (read 29 instead of ≤25) — parallel cargo test threads bled their conversion counts into each other's assertions.

## Decision

Serialize all cache_no_materialization_gate tests with a Mutex so no two tests observe the shared CONVERSION_COUNT simultaneously.

## Consequences

- All materialization-gate tests now run serially, eliminating the flaky count mismatch
- Future acceptance-gate or regression tests that assert on shared atomics must apply the same Mutex-serialization pattern
- The streaming query_visit design itself is correct; the bug was purely in test isolation

## Open Tail

*(none)*

## Evidence

- transcript lines 4237-4354

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-2-conversion-count-parallel-test-race-global.json`](transcripts/2026-06-19-2-conversion-count-parallel-test-race-global.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-2-conversion-count-parallel-test-race-global.json`](transcripts/raw/2026-06-19-2-conversion-count-parallel-test-race-global.json)

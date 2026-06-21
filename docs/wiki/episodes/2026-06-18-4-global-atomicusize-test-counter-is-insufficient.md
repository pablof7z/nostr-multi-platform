---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: root-cause
status: superseded
subjects:
  - conversion-count
  - test-observability
  - materialization-gate
supersedes:
  - 2026-06-18-2-storequery-materialized-vec-replaced-by-lazy
related_claims: []
source_lines:
  - 4237-4354
captured_at: 2026-06-18T20:58:55Z
---

# Episode: Global AtomicUsize test counter is insufficient for parallel test assertions

## Prior State

CONVERSION_COUNT (AtomicUsize) was used as a test-only observability seam for asserting materialization counts in cache_no_materialization_gate tests. Assumed atomic read/write was sufficient for correct assertions.

## Trigger

CI failures on #1549: authorkind_streaming asserted 10 instead of 5, ptag_streaming asserted 4 instead of 5, limit_caps asserted 29 instead of ≤25. Parallel test threads were racing on reset/read of the shared global counter.

## Decision

Add a Mutex serializer in the test file that holds the lock across the full reset→run→assert sequence, preventing parallel tests from interleaving counter operations.

## Consequences

- Materialization gate tests are now reliable under parallel cargo test
- Pattern established: any future global-AtomicUsize test seam must serialize reset-through-assert under a Mutex
- The AtomicUsize itself remains (it's the production observability point); only test assertions need serialization

## Open Tail

*(none)*

## Evidence

- transcript lines 4237-4354

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-4-global-atomicusize-test-counter-is-insufficient.json`](transcripts/2026-06-18-4-global-atomicusize-test-counter-is-insufficient.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-4-global-atomicusize-test-counter-is-insufficient.json`](transcripts/raw/2026-06-18-4-global-atomicusize-test-counter-is-insufficient.json)

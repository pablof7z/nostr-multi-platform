---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: product
status: superseded
subjects:
  - query-visit-streaming
  - lmdb-store-backend
  - storequery
supersedes:
  - 2026-06-18-2-query-visit-does-not-truly-stream
related_claims: []
source_lines:
  - 2469-2482
  - 2659-2661
  - 2890-2906
captured_at: 2026-06-18T19:35:30Z
---

# Episode: LMDB query_visit switches from materialized-Vec to lazy per-row streaming

## Prior State

LmdbEventStore::query_visit materialized a full Vec<StoredEvent> via run_filter() before visiting. ControlFlow::Break only stopped iteration after materialization was complete — no early-stop savings.

## Trigger

Issue #1516 required true streaming semantics: visit each row lazily, break immediately on ControlFlow::Break, with zero over-materialization.

## Decision

Replace Vec-materialization with build_filter + run_filter_visit: convert one event per LMDB cursor row, stop immediately on Break. Tie-group buffering preserves (created_at desc, id asc) ordering. Old run_filter (Vec path) retained for scan_by_*/EventIter callers.

## Consequences

- Early-stop queries now pay conversion cost only for visited rows — proven by streaming_visit_does_not_over_materialize test (1000 events, break at 10, ≤11 conversions)
- LMDB BTreeSet iterator already delivers (created_at desc, id asc) natively, eliminating the need for the old post-sort step on the streaming path
- New code extracted into query_streaming.rs to satisfy 500-LOC file cap
- Vec path (run_filter) still exists for callers that need full materialization

## Open Tail

- Future: consider migrating remaining scan_by_*/EventIter callers to visitor pattern for consistency

## Evidence

- transcript lines 2469-2482
- transcript lines 2659-2661
- transcript lines 2890-2906

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-lmdb-query-visit-switches-from-materialized.json`](transcripts/2026-06-18-2-lmdb-query-visit-switches-from-materialized.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-lmdb-query-visit-switches-from-materialized.json`](transcripts/raw/2026-06-18-2-lmdb-query-visit-switches-from-materialized.json)

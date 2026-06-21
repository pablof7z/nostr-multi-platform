---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: reversal
status: superseded
subjects:
  - store-query
  - query-visit
  - lmdb-backend
supersedes:
  - 2026-06-18-2-lmdb-query-visit-switches-from-materialized
related_claims: []
source_lines:
  - 3848-3856
  - 3892-3898
captured_at: 2026-06-18T20:04:31Z
---

# Episode: Streaming query_visit replaces materialized Vec queries

## Prior State

StoreQuery::query returned materialized Vec<QueryResult>, buffering the full result set before the caller could inspect any row. No early-stop mechanism existed; memory usage was proportional to the full matching corpus regardless of how many rows the consumer actually read.

## Trigger

Epic #1523 sub-issue #1516: implement true streaming LMDB query_visit, adopting nostrdb's visitor pattern lesson while preserving NMP's hand-rolled LMDB path.

## Decision

Replace materialized queries with lazy per-row conversion via run_filter_visit using ControlFlow::Break for early-stop. A CONVERSION_COUNT AtomicUsize counter (test-only) detects regression to full-buffer materialization. The implementation was extracted into a sibling module query_streaming.rs to stay under the 500-LOC file-size cap.

## Consequences

- Memory usage is now proportional to actually-consumed rows, not the full matching corpus
- CONVERSION_COUNT allows acceptance gates to assert conversion_count == n_break (not 10,000), preventing silent regression to collect().take()
- Module structure: query.rs (359 LOC) uses super::query_streaming::{build_filter, run_filter_visit} as a peer module, not a child module (Rust mod foo inside bar.rs looks for parent/bar/foo.rs, not parent/foo.rs)
- Acceptance gate planned: test-support feature exposes the counter to nmp-testing integration tests

## Open Tail

- #1524 acceptance gates must expose CONVERSION_COUNT under #[cfg(any(test, feature = "test-support"))] and wire into CI before the epic closes

## Evidence

- transcript lines 3848-3856
- transcript lines 3892-3898

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-streaming-query-visit-replaces-materialized-vec.json`](transcripts/2026-06-18-2-streaming-query-visit-replaces-materialized-vec.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-streaming-query-visit-replaces-materialized-vec.json`](transcripts/raw/2026-06-18-2-streaming-query-visit-replaces-materialized-vec.json)

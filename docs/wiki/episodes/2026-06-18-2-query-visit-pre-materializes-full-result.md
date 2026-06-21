---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: root-cause
status: superseded
subjects:
  - query-visit
  - lmdb-store
  - store-cache-baseline
supersedes: []
related_claims: []
source_lines:
  - 612-630
  - 1315-1370
captured_at: 2026-06-18T18:30:34Z
---

# Episode: query_visit pre-materializes full result set — early-stop is post-hoc, not streaming

## Prior State

The `EventStore::query_visit` trait with `ControlFlow::Break` was assumed to offer streaming semantics — stopping the LMDB scan when the visitor breaks early.

## Trigger

#1524 Opus plan inspection of `crates/nmp-store/src/lmdb/query.rs` found that `run_filter()` materializes a full `Vec<StoredEvent>` (capped only by `limit`), and `query_visit` then iterates the Vec. The existing test `query_visit_early_stop_after_10` passes only because it counts visitor invocations — it never asserts scan-depth was bounded.

## Decision

The current implementation is confirmed as pre-materialization, not streaming. An instrumented scan counter (behind `test-support` cfg) will be added to make this observable. #1516 must replace this with true cursor-backed streaming, and #1524 gates regression with a `#[ignore]`-flipped test asserting `events_scanned <= limit + ε`.

## Consequences

- The baseline binary (#1522) will capture `returned == scanned` equality, documenting the pre-streaming status
- Any future change that re-introduces materialization after #1516 lands will cause the scan-count gate to fail
- The `KindDtag.d_tag` field type is `Vec<u8>` not `String` — issue text was wrong, corrected in implementation planning

## Open Tail

- The scan counter and `#[ignore]` gate test are designed but not yet implemented; they land with #1524 and activate when #1516 merges

## Evidence

- transcript lines 612-630
- transcript lines 1315-1370

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-query-visit-pre-materializes-full-result.json`](transcripts/2026-06-18-2-query-visit-pre-materializes-full-result.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-query-visit-pre-materializes-full-result.json`](transcripts/raw/2026-06-18-2-query-visit-pre-materializes-full-result.json)

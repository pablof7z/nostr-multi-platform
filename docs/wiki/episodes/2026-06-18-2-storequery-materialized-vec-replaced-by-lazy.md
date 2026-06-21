---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: product
status: superseded
subjects:
  - store-query
  - lmdb-query-streaming
  - query-visitor-api
supersedes:
  - 2026-06-18-2-streaming-query-visit-replaces-materialized-vec
related_claims: []
source_lines:
  - 3827-3905
captured_at: 2026-06-18T20:17:13Z
---

# Episode: StoreQuery: materialized Vec replaced by lazy streaming query_visit with early-stop

## Prior State

StoreQuery::query() returned materialized Vec<QueryResult>, requiring full-corpus scan even when callers only need a bounded prefix

## Trigger

Epic #1523 sub-issue #1516 — adopt nostrdb's streaming query_visit lesson without direct nostrdb dependency

## Decision

Implement lazy per-row streaming via query_visit() accepting FnMut(&StoredEvent) -> ControlFlow<()>, where Break stops immediately; extract streaming logic to sibling query_streaming.rs module; add CONVERSION_COUNT AtomicUsize test instrumentation gated behind #[cfg(any(test, feature = "test-support"))]

## Consequences

- All 6 StoreQuery variants (AuthorKind, AuthorsKind, KindTime, KindDtag, Etag, Ptag) now stream lazily with per-row conversion
- Early-break stops after exactly N conversions, not full corpus — confirmed by CONVERSION_COUNT assertion in acceptance gates (#1524)
- query.rs split to 359 LOC + query_streaming.rs at 186 LOC to stay under 500-LOC gate; sibling-module declaration (not child) required by Rust module resolution (E0583 lesson)
- Planned regression gate in #1524 will trip if streaming is reverted to collect().take()

## Open Tail

- #1524 acceptance gates will harden the CONVERSION_COUNT trip-wire into CI

## Evidence

- transcript lines 3827-3905

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-storequery-materialized-vec-replaced-by-lazy.json`](transcripts/2026-06-18-2-storequery-materialized-vec-replaced-by-lazy.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-storequery-materialized-vec-replaced-by-lazy.json`](transcripts/raw/2026-06-18-2-storequery-materialized-vec-replaced-by-lazy.json)

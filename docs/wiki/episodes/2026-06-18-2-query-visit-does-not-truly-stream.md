---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: root-cause
status: superseded
subjects:
  - lmdb-query-visit
  - store-scan
  - early-stop-regression
supersedes:
  - 2026-06-18-2-query-visit-pre-materializes-full-result
related_claims: []
source_lines:
  - 1315-1367
captured_at: 2026-06-18T18:45:00Z
---

# Episode: query_visit does not truly stream — ControlFlow::Break only stops post-materialization iteration

## Prior State

query_visit was assumed to support early-stop via ControlFlow::Break on the LMDB scan itself; the existing test query_visit_early_stop_after_10 was treated as sufficient evidence that streaming works.

## Trigger

#1524 Opus plan inspected the code and found that run_filter() materializes a full Vec<StoredEvent> (line 185), and query_visit (lines 311–348) collects then iterates — ControlFlow::Break only stops the post-materialization for-loop, not the LMDB cursor scan.

## Decision

Add an instrumented AtomicU64 scan counter behind #[cfg(test, feature="test-support")] to LmdbEventStore::Inner, expose events_scanned_since_reset(), and write a regression test query_visit_scan_bounded_by_early_stop that asserts events_scanned <= limit + ε. The test is marked #[ignore] until #1516 (streaming query_visit) lands, so it becomes the regression guard preventing re-buffering.

## Consequences

- The existing query_visit_early_stop_after_10 test is necessary but not sufficient — it passes today despite full materialization because it only counts visitor invocations, not LMDB rows touched
- #1516 must refactor run_filter to stream rows to the visitor via LMDB cursor without materializing a Vec
- The #[ignore]-gated scan-counter test will flip green when #1516 lands and stay green as a permanent guard against re-introducing materialization
- Deterministic metrics (events-scanned count, replay-chunk count) are hard CI gates; wall-clock latency and allocation are report-only deltas on PR descriptions

## Open Tail

- #1516 (streaming query_visit) must land before the #[ignore] gate can be removed
- cache-baseline binary must capture pre-#1516 numbers before the streaming refactor changes scan behavior

## Evidence

- transcript lines 1315-1367

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-query-visit-does-not-truly-stream.json`](transcripts/2026-06-18-2-query-visit-does-not-truly-stream.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-query-visit-does-not-truly-stream.json`](transcripts/raw/2026-06-18-2-query-visit-does-not-truly-stream.json)

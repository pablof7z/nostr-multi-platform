---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - watermark
  - nmp-core
  - backfill
supersedes: []
related_claims: []
source_lines:
  - 2973-3016
captured_at: 2026-06-11T23:22:45Z
---

# Episode: Author-blind watermark caused new-follow backfill failure

## Prior State

The watermark_fn closure's KindTime branch queried globally across all authors for multi-author shapes, returning the newest event from any author as the floor — causing new follows to skip entire histories because the watermark sat above them.

## Trigger

Bug diagnosis (#1087) showing that for multi-author shapes, query_visit with KindTime returned the newest-from-anyone timestamp, flooring above a new follow's entire history.

## Decision

Replaced the author-blind KindTime branch with per-author AuthorKind(limit=1) queries, returning the min timestamp across all authors. Zero-author shapes now return None instead of falling into the global scan.

## Consequences

- New follows correctly backfill below the watermark
- Per-author lookups hit the existing idx_author_kind B-tree index (O(log N) per author)
- Missing watermark only costs re-download (fail-open); wrong watermark loses data

## Open Tail

*(none)*

## Evidence

- transcript lines 2973-3016


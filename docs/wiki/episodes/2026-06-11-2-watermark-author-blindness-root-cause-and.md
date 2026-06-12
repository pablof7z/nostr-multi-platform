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
  - 2970-3016
captured_at: 2026-06-11T23:31:21Z
---

# Episode: Watermark author-blindness root cause and fix

## Prior State

The `KindTime` branch in `watermark_fn` queried globally across all authors, so for multi-author shapes the watermark could floor above a new follow's entire history. Zero-author shapes fell into the same global scan.

## Trigger

Bug #1087 — new follows' backfill was broken because the watermark was too high, preventing historical events from loading.

## Decision

Option B adopted: per-author `AuthorKind(limit=1)` queries returning the minimum timestamp across authors. Zero-author shapes now return `None` instead of falling into global scan. Uses existing `idx_author_kind` B-tree index.

## Consequences

- New follows now correctly backfill from their earliest events
- Zero-author shapes no longer produce spurious watermark floors
- Missing watermark only costs re-download (fail-open); wrong watermark loses data (fail-closed)

## Open Tail

*(none)*

## Evidence

- transcript lines 2970-3016


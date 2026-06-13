---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: product
status: superseded
subjects:
  - nmp-core
  - subs-watermark-rewrite
  - since-none-backfill
  - tailing-feed-narrowing
supersedes:
  - 2026-06-13-2-1281-exempt-since-none-interests-from
related_claims: []
source_lines:
  - 8839-8841
  - 8555-8578
  - 8807-8810
  - 8839-8857
captured_at: 2026-06-13T20:42:49Z
---

# Episode: since=None watermark rewrite exemption refined to lifecycle-aware

## Prior State

T129 watermark rewrite uniformly raised all subscription since values (including since=None) to watermark+1, preventing full-history backfill for unbounded interests

## Trigger

Owner decision (#1281): since=None interests should backfill full history, not be narrowed. Initial uniform exemption merged (#1328) but broke the negentropy_skips_redundant_req e2e test — tailing feeds with since=None stopped narrowing and re-requested everything on each recompile

## Decision

Lifecycle-aware exemption: since=None is exempted from the watermark rewrite only for non-Tailing (backfill) interests; Tailing interests keep watermark narrowing (since=watermark+1) to avoid redundant REQs

## Consequences

- Backfill/historical interests with since=None now fetch full history (owner's intent satisfied)
- Tailing (live-feed) interests with since=None still narrow to since=watermark+1 (T129 regression avoided)
- Both recompile.rs and handlers.rs (reconnect-replay path) must apply the lifecycle check consistently
- Master broke on two consecutive commits before the fix landed, confirming the uniform exemption was too broad

## Open Tail

- Lifecycle-aware fix agent (a39180) has edits ready in worktree but not yet pushed/PR'd; master remains red until it lands

## Evidence

- transcript lines 8839-8841
- transcript lines 8555-8578
- transcript lines 8807-8810
- transcript lines 8839-8857


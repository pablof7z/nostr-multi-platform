---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: product
status: active
subjects:
  - nmp-core-subs
  - watermark-rewrite
  - interest-lifecycle
supersedes:
  - 2026-06-13-1-since-none-watermark-exemption-refined-to
related_claims: []
source_lines:
  - 8811-8858
  - 9139-9141
captured_at: 2026-06-13T21:35:37Z
---

# Episode: since=None watermark exemption made lifecycle-aware

## Prior State

T129's apply_watermark_rewrite narrowed ALL subscription REQs by setting since=watermark+1, including Tailing (live-feed) interests that default to since=None

## Trigger

Owner decision #1281 to exempt since=None for backfill; initial uniform implementation (#1328) regressed Tailing-feed REQ narrowing — negentropy_skips_redundant_req e2e test failed because Tailing feeds re-requested all cached events on every recompile

## Decision

Lifecycle-aware exemption: since=None interests exempt from watermark narrowing ONLY for non-Tailing (backfill/OneShot); Tailing interests keep since=watermark+1 narrowing. apply_watermark_rewrite now takes &[LogicalInterest] and calls lifecycle_for_shape to determine exemption

## Consequences

- Backfill interests with since=None now fetch full history as intended
- Tailing feeds still skip cached events (preserving T129's dedup behavior)
- apply_watermark_rewrite signature changed to accept LogicalInterest context
- New test: tailing_since_none_is_narrowed_to_watermark_plus_one
- backfill_interest() test helper added for clarity
- Fix shipped as #1337 after #1328's uniform exemption broke master

## Open Tail

*(none)*

## Evidence

- transcript lines 8811-8858
- transcript lines 9139-9141


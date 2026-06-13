---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-core-subs
  - since-rewrite
  - interest-lifecycle
  - tailing-vs-backfill
supersedes:
  - 2026-06-13-1-since-none-watermark-exemption-must-be
related_claims: []
source_lines:
  - 8811-8857
  - 8987-8991
  - 9031-9053
captured_at: 2026-06-13T21:11:15Z
---

# Episode: since=None watermark exemption refined to lifecycle-aware

## Prior State

Decision #1281 was implemented as a uniform exemption: all since=None interests (including Tailing/live feeds) were exempted from T129's watermark rewrite, meaning they would NOT narrow their REQ to since=watermark+1.

## Trigger

Master e2e test negentropy_skips_redundant_req failed — a Tailing interest with since=None stopped carrying since=watermark+1 in its REQ filter, causing live feeds to redundantly re-request already-cached events on every recompile. The test expected filters_warm to contain "since":1701 but instead got a bare {"kinds":[1]}.

## Decision

The exemption is now lifecycle-aware: since=None is exempt from watermark rewrite ONLY for non-Tailing interests (backfill/OneShot), which need full history. Tailing interests keep the since=watermark+1 narrowing. apply_watermark_rewrite now accepts &[LogicalInterest] and calls lifecycle_for_shape to distinguish.

## Consequences

- Backfill/OneShot interests with since=None now correctly fetch full history (owner's original #1281 intent)
- Tailing/live-feed interests retain watermark narrowing — no redundant REQ regression
- apply_watermark_rewrite API surface changed: now takes interest list, not just filters
- PR #1328 (uniform exemption) was superseded by #1337 (lifecycle-aware); #1340 (test-only patch from another session) flagged as redundant/superseded
- New test tailing_since_none_is_narrowed_to_watermark_plus_one added; backfill test renamed for clarity

## Open Tail

- #1340 from session f1e7c4 edits the test to match the old uniform behavior — needs closure to avoid re-breaking Tailing narrowing if merged

## Evidence

- transcript lines 8811-8857
- transcript lines 8987-8991
- transcript lines 9031-9053


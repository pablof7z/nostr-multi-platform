---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: product
status: superseded
subjects:
  - nmp-nip17
  - backfill-semantics
  - t129-watermark
supersedes: []
related_claims: []
source_lines:
  - 8130-8132
  - 8214-8216
captured_at: 2026-06-13T20:04:54Z
---

# Episode: #1281: exempt since=None interests from T129 watermark rewrite

## Prior State

T129 addSinceFromCache rewrote subscription since-floors to the store watermark for all interests, including since=None ('all time') — preventing backfill of history older than the watermark

## Trigger

Owner chose option (a): exempt since=None from the rewrite, as recommended by the analysis agent

## Decision

since=None interests are exempted from the T129 watermark rewrite, restoring full historical backfill for all-time subscriptions

## Consequences

- All-time subscriptions now backfill the complete event history rather than only events after the watermark
- Implementation PR dispatched immediately

## Open Tail

*(none)*

## Evidence

- transcript lines 8130-8132
- transcript lines 8214-8216


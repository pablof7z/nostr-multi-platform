---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: active
subjects:
  - publish-classifier
  - rate-limited
  - transient-retry
supersedes: []
related_claims: []
source_lines:
  - 569-583
captured_at: 2026-06-18T19:42:43Z
---

# Episode: rate-limited reclassified from Permanent to Transient in publish ack classifier

## Prior State

publish/state.rs classified rate-limited under PERMANENT_CODES, causing rate-limited events to be permanently failed with no retry. This was the lone outlier: closed_reason.rs already mapped rate-limited → ERR_TRANSIENT, but the publish path never aligned.

## Trigger

#1493 audit finding PT3(B); codex-design-first verified the fix.

## Decision

Reclassify rate-limited as Transient (retries with exponential backoff, then FailedAfterRetries). pow correctly stays Permanent (engine cannot add PoW without re-signing). Also fixed a doc-lie in kernel/publish_engine_wire.rs that grouped rate-limited under "permanent classes."

## Consequences

- Rate-limited events now retry with backoff instead of permanently failing
- Publish ack classifier now consistent with closed_reason.rs
- Doc-lie in publish_engine_wire.rs corrected
- PT3 finding A (hung spinner / no success terminal) reclassified as STALE — already fixed in PR #1211 (durable tri-state NWC)

## Open Tail

*(none)*

## Evidence

- transcript lines 569-583

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-rate-limited-reclassified-from-permanent-to.json`](transcripts/2026-06-18-3-rate-limited-reclassified-from-permanent-to.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-rate-limited-reclassified-from-permanent-to.json`](transcripts/raw/2026-06-18-3-rate-limited-reclassified-from-permanent-to.json)

---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: product
status: superseded
subjects:
  - nmp-core
  - publish-state
  - nip20
supersedes: []
related_claims: []
source_lines:
  - 569-583
captured_at: 2026-06-18T20:12:30Z
---

# Episode: Reclassify rate-limited from Permanent to Transient in publish ack

## Prior State

rate-limited was classified under PERMANENT_CODES in publish/state.rs, meaning rate-limited events would never be retried. Meanwhile closed_reason.rs already mapped rate-limited → ERR_TRANSIENT, creating an internal contradiction.

## Trigger

Issue #1493 audit (pt3, Finding B) identified the misclassification as a correctness issue causing published events to be abandoned on rate-limit rather than retried.

## Decision

Reclassified rate-limited as Transient (retries with exponential backoff, then FailedAfterRetries). pow correctly stays Permanent (cannot retry without re-signing into a new id). Fixed doc-lies in publish_engine_wire.rs and builder-guide that grouped rate-limited under permanent.

## Consequences

- Rate-limited publishes now retry with backoff instead of being permanently abandoned.
- Publish-ack classifier now agrees with closed_reason.rs.
- pow remains Permanent — engine cannot add PoW without re-signing.

## Open Tail

*(none)*

## Evidence

- transcript lines 569-583

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-reclassify-rate-limited-from-permanent-to.json`](transcripts/2026-06-18-3-reclassify-rate-limited-from-permanent-to.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-reclassify-rate-limited-from-permanent-to.json`](transcripts/raw/2026-06-18-3-reclassify-rate-limited-from-permanent-to.json)

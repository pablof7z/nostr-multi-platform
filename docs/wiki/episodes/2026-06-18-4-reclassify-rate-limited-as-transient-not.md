---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: root-cause
status: active
subjects:
  - pt3-async-terminal
  - publish-state
  - nmp-core
supersedes:
  - 2026-06-18-3-reclassify-rate-limited-from-permanent-to
related_claims: []
source_lines:
  - 569-583
  - 632-633
captured_at: 2026-06-18T20:25:04Z
---

# Episode: Reclassify rate-limited as Transient (not Permanent)

## Prior State

NIP-20 rate-limited ack was classified in PERMANENT_CODES in publish/state.rs, causing the publish path to treat rate-limited responses as permanent failures. Meanwhile, closed_reason.rs already mapped rate-limited to ERR_TRANSIENT — the publish path was the lone outlier.

## Trigger

Issue #1493 pt3 Finding B confirmed rate-limited was live and misclassified in the publish ack classifier.

## Decision

Reclassified rate-limited from Permanent to Transient. Rate-limited now retries with exponential backoff, then FailedAfterRetries. pow correctly stays Permanent (cannot retry without re-signing into a new id). publish_engine_wire.rs doc-lie (grouping rate-limited under "permanent classes") also fixed.

## Consequences

- Publish-ack classifier now agrees with closed_reason.rs (which already mapped rate-limited → ERR_TRANSIENT)
- The hung-spinner finding (Finding A — no async success terminal) was STALE — already fixed by PR #1211 (success logic moved from runtime.rs to reconcile.rs after the audit was written)

## Open Tail

*(none)*

## Evidence

- transcript lines 569-583
- transcript lines 632-633

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-4-reclassify-rate-limited-as-transient-not.json`](transcripts/2026-06-18-4-reclassify-rate-limited-as-transient-not.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-4-reclassify-rate-limited-as-transient-not.json`](transcripts/raw/2026-06-18-4-reclassify-rate-limited-as-transient-not.json)

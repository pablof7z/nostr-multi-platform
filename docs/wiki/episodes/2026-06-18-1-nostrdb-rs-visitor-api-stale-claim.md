---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: root-cause
status: superseded
subjects:
  - nostrdb-rs-evaluation
  - store-architecture
  - lmdb-query-visit
supersedes:
  - 2026-06-18-1-refresh-nostrdb-rs-rejection-rationale-stale
related_claims: []
source_lines:
  - 49-95
  - 125-143
captured_at: 2026-06-18T19:54:19Z
---

# Episode: nostrdb-rs visitor API stale claim corrected, rejection maintained

## Prior State

The nostrdb-rs evaluation doc claimed the Rust binding exposes only materialized Vec queries — that the C ndb_query_visit visitor path was not available in Rust. This was cited as a reason direct nostrdb adoption was deficient.

## Trigger

Issue #1515 review found current upstream nostrdb-rs now exposes fold / try_fold on its query iterators, providing visitor semantics. The recorded objection was factually stale.

## Decision

Correct the evaluation doc: acknowledge that visitor semantics are now expressible in nostrdb-rs without forking, but maintain the rejection of direct nostrdb adoption because the remaining reasons (LMDB env ownership, write policy, need for synchronous typed inserts, atomic multi-table transactions per ADR-0011) remain sound.

## Consequences

- Streaming query_visit (#1516) is confirmed as the correct NMP-native path rather than depending on nostrdb's visitor
- The rejection rationale in docs/design/nostrdb-rs-evaluation.md is now factually accurate
- Future re-evaluations must check whether nostrdb-rs gains synchronous transaction control — the current architectural gap remains

## Open Tail

- The evaluation doc should be refreshed again if nostrdb-rs adds configurable LMDB env injection or transaction-level write control

## Evidence

- transcript lines 49-95
- transcript lines 125-143

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-nostrdb-rs-visitor-api-stale-claim.json`](transcripts/2026-06-18-1-nostrdb-rs-visitor-api-stale-claim.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-nostrdb-rs-visitor-api-stale-claim.json`](transcripts/raw/2026-06-18-1-nostrdb-rs-visitor-api-stale-claim.json)

---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: superseded
subjects:
  - nostrdb-adoption
  - event-store-backend
  - evaluation-doc
supersedes:
  - 2026-06-18-1-nostrdb-rs-visitor-semantics-claim-corrected
related_claims: []
source_lines:
  - 49-93
  - 109-148
captured_at: 2026-06-18T20:58:55Z
---

# Episode: Nostrdb direct dependency remains rejected; stale visitor-API argument corrected

## Prior State

The nostrdb-rs evaluation doc rejected direct adoption partly because 'the Rust binding does not expose the C ndb_query_visit visitor path' — claiming only materialized Vec queries were available.

## Trigger

Issue #1515 review found that current nostrdb-rs now exposes fold / try_fold on query iterators, making the 'no visitor API' argument stale.

## Decision

Rejection of nostrdb-rs as a dependency still stands, but the documented reason is corrected: visitor semantics ARE now available; the real blockers remain LMDB environment ownership and synchronous typed insert outcomes + atomic multi-table transactions that nostrdb's writer model cannot provide.

## Consequences

- Evaluation doc now accurately reflects upstream evidence
- The visitor-pattern technique is adopted (via hand-rolled query_visit) without importing nostrdb
- No GPL-marked nostrdb source copied into the project
- Streamed query_visit implementation (#1516) is justified by the corrected evidence

## Open Tail

*(none)*

## Evidence

- transcript lines 49-93
- transcript lines 109-148

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-nostrdb-direct-dependency-remains-rejected-stale.json`](transcripts/2026-06-18-1-nostrdb-direct-dependency-remains-rejected-stale.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-nostrdb-direct-dependency-remains-rejected-stale.json`](transcripts/raw/2026-06-18-1-nostrdb-direct-dependency-remains-rejected-stale.json)

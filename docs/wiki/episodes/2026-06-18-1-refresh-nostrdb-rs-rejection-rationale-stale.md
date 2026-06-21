---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: superseded
subjects:
  - nostrdb-rs-evaluation
  - lmdb-store-backend
  - visitor-query-semantics
supersedes:
  - 2026-06-18-1-correct-stale-nostrdb-rs-visitor-claim
related_claims: []
source_lines:
  - 49-93
captured_at: 2026-06-18T19:35:30Z
---

# Episode: Refresh nostrdb-rs rejection rationale — stale visitor-API argument removed

## Prior State

The nostrdb-rs evaluation doc claimed the Rust binding exposes only materialized Vec queries and does NOT expose the C ndb_query_visit visitor path, making it impossible to do early-stopping scans without forking the binding.

## Trigger

Issue #1515 identified that current upstream nostrdb-rs now exposes fold/try_fold, providing visitor semantics — the recorded rejection argument was factually stale.

## Decision

Maintain rejection of nostrdb-rs but correct the rationale: remove the 'no visitor API' argument; the remaining grounds (owns LMDB env, owns writer threads, write policy conflicts with NMP's need for synchronous typed inserts and atomic multi-table transactions) are sufficient and now the sole basis.

## Consequences

- The 'no visitor' objection is no longer citable — future nostrdb re-evaluations must address the ownership/transaction arguments only
- The streaming query_visit work (#1516) can adopt visitor-shaped patterns internally without needing nostrdb's C core
- Evaluation doc now cross-links the nostrdb-notedeck-lessons.md and supersedes its preliminary lean

## Open Tail

- If nostrdb-rs ever offers pluggable Env/writer policy, the remaining rejection arguments would need fresh analysis

## Evidence

- transcript lines 49-93

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-refresh-nostrdb-rs-rejection-rationale-stale.json`](transcripts/2026-06-18-1-refresh-nostrdb-rs-rejection-rationale-stale.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-refresh-nostrdb-rs-rejection-rationale-stale.json`](transcripts/raw/2026-06-18-1-refresh-nostrdb-rs-rejection-rationale-stale.json)

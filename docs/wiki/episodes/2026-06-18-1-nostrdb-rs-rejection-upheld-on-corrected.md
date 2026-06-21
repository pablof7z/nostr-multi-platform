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
  - visitor-semantics
supersedes:
  - 2026-06-18-1-nostrdb-rs-rejection-restated-on-corrected
related_claims: []
source_lines:
  - 60-93
  - 120-142
captured_at: 2026-06-18T18:30:34Z
---

# Episode: nostrdb-rs rejection upheld on corrected grounds — visitor API now exists upstream

## Prior State

The decision doc `nostrdb-rs-evaluation.md` rejected direct nostrdb-rs adoption partly because the Rust binding did not expose the C `ndb_query_visit` path — only materialized Vec queries were believed available.

## Trigger

Review of current upstream nostrdb-rs (#1515) revealed that `fold`/`try_fold` now expose visitor semantics, making the 'no visitor API' argument stale.

## Decision

Direct nostrdb-rs adoption remains rejected, but on corrected grounds: the binding now has visitor semantics, so the rejection shifts to the real blocker — nostrdb owns its LMDB environment, ingester/writer threads, and write policy, which conflicts with NMP's need for synchronous typed insert outcomes and atomic multi-table transactions. The evaluation doc is refreshed to reflect current evidence.

## Consequences

- The 'no visitor API' argument is removed from the rejection rationale; future evaluations cannot re-raise it without re-checking upstream
- NMP still adopts nostrdb's engineering techniques (streaming reads, provenance indexes, visitor query semantics) but in its own Rust-owned EventStore model
- No GPL nostrdb source copy is permitted (doctrine constraint reaffirmed)

## Open Tail

- The refreshed decision doc must be kept current as upstream nostrdb-rs evolves — a stale-evidence recurrence should be caught by citation review

## Evidence

- transcript lines 60-93
- transcript lines 120-142

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-nostrdb-rs-rejection-upheld-on-corrected.json`](transcripts/2026-06-18-1-nostrdb-rs-rejection-upheld-on-corrected.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-nostrdb-rs-rejection-upheld-on-corrected.json`](transcripts/raw/2026-06-18-1-nostrdb-rs-rejection-upheld-on-corrected.json)

---
type: episode-card
date: 2026-06-19
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: superseded
subjects:
  - nostrdb-rs-evaluation
  - store-architecture
  - lmdb-backend
supersedes:
  - 2026-06-18-1-nostrdb-rejection-doctrine-visitor-objection-stale
related_claims: []
source_lines:
  - 49-93
  - 125-143
captured_at: 2026-06-19T11:51:35Z
---

# Episode: Nostrdb rejection rationale corrected — visitor semantics now exist upstream

## Prior State

The nostrdb-rs evaluation doc (ADR) recorded that nostrdb-rs does not expose the C ndb_query_visit path — only materialized Vec queries — and this was cited as a reason to reject direct nostrdb adoption.

## Trigger

Issue #1515 identified the claim as stale; current nostrdb-rs exposes fold / try_fold on query iterators, giving early-stopping visitor semantics without forking the binding.

## Decision

Maintain rejection of nostrdb-rs but on corrected grounds: the 'no visitor API' argument is replaced with acknowledgment that fold/try_fold exist, while rejection rests on nostrdb owning its LMDB environment, ingester/writer threads, and write policy — which conflict with NMP's need for synchronous typed inserts and atomic event/provenance/tombstone/watermark/domain writes in one RwTxn.

## Consequences

- Decision docs now reflect current upstream state; future re-evaluations won't be misled by stale capability claims
- The corrected rationale is explicitly cross-linked from nostrdb-notedeck-lessons.md §2.5 / §5
- Direct nostrdb adoption remains rejected, but selective borrowing of techniques (streaming visitors, provenance indexes) is explicitly endorsed

## Open Tail

- Issue notes the rejection doc should still be refreshed periodically because current Rust binding exposes visitor query semantics — conditions may change again

## Evidence

- transcript lines 49-93
- transcript lines 125-143

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-1-nostrdb-rejection-rationale-corrected-visitor-semantics.json`](transcripts/2026-06-19-1-nostrdb-rejection-rationale-corrected-visitor-semantics.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-1-nostrdb-rejection-rationale-corrected-visitor-semantics.json`](transcripts/raw/2026-06-19-1-nostrdb-rejection-rationale-corrected-visitor-semantics.json)

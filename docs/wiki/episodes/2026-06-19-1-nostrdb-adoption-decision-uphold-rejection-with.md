---
type: episode-card
date: 2026-06-19
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: active
subjects:
  - nostrdb-adoption
  - eventstore-backend
  - lmdb-eventstore
supersedes:
  - 2026-06-19-1-nostrdb-rejection-rationale-corrected-visitor-semantics
related_claims: []
source_lines:
  - 3-94
  - 120-142
captured_at: 2026-06-19T12:25:59Z
---

# Episode: Nostrdb adoption decision: uphold rejection with corrected evidence

## Prior State

The nostrdb-rs evaluation doc (ADR, dated 2026-05-18) rejected direct nostrdb dependency partly on the claim that the Rust binding does not expose visitor query semantics — only materialized Vec queries — making it impossible to do early-stopping, zero-allocation scans without forking the binding.

## Trigger

Issue #1515 identified that current upstream nostrdb-rs now exposes fold/try_fold on query iterators, providing visitor (early-stopping) semantics. The recorded rejection reason was stale and needed correction to remain a trustworthy architecture source.

## Decision

Uphold the rejection of nostrdb-rs as a direct dependency, but refresh the documented reasoning: the rejection now rests on nostrdb owning its LMDB environment, ingester/writer threads, and write policy — which conflicts with NMP's need for synchronous typed insert outcomes and atomic event/provenance/tombstone/watermark/domain writes in one RwTxn (ADR-0011, doctrine D4/D8). The visitor-semantics argument is corrected from 'not available' to 'now available via fold/try_fold, but insufficient alone to overcome the ownership/write-policy mismatch.'

## Consequences

- The nostrdb-rs evaluation doc is now accurate against current upstream evidence and can be cited as a trustworthy architecture source
- Future re-evaluations must verify evidence currency before citing the decision
- The epic #1523 approach — adopt nostrdb's engineering techniques (streaming reads, provenance indexes) in NMP's own Rust-owned EventStore — is explicitly affirmed as the correct path
- No second LMDB writer, no nostrdb source copy (GPL), and no native shells owning cache policy remain hard constraints

## Open Tail

- The refreshed doc notes the current Rust binding does expose visitor semantics; if nostrdb upstream adds synchronous-write or single-transaction modes, the rejection should be revisited

## Evidence

- transcript lines 3-94
- transcript lines 120-142

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-19-1-nostrdb-adoption-decision-uphold-rejection-with.json`](transcripts/2026-06-19-1-nostrdb-adoption-decision-uphold-rejection-with.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-19-1-nostrdb-adoption-decision-uphold-rejection-with.json`](transcripts/raw/2026-06-19-1-nostrdb-adoption-decision-uphold-rejection-with.json)

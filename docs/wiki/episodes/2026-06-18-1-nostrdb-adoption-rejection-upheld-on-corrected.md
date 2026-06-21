---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: superseded
subjects:
  - nostrdb-adoption
  - store-backend
  - evaluation-docs
supersedes:
  - 2026-06-18-1-nostrdb-rs-visitor-api-stale-claim
related_claims: []
source_lines:
  - 13-17
  - 62-64
  - 78-84
  - 130-141
captured_at: 2026-06-18T20:04:31Z
---

# Episode: Nostrdb adoption rejection upheld on corrected evidence

## Prior State

The decision to reject direct nostrdb-rs adoption was partly based on the claim that the Rust binding exposes only materialized Vec queries and not the C ndb_query_visit visitor path — making streaming reads impossible without forking the binding.

## Trigger

Review of current upstream nostrdb-rs found that fold/try_fold visitor semantics are now exposed, making the original objection stale. Epic #1523 required refreshing the evaluation doc before any performance PRs.

## Decision

Correct the evaluation doc: visitor semantics ARE now available in nostrdb-rs. The rejection still stands, but primarily because nostrdb owns its LMDB environment, ingester/writer threads, and write policy — conflicting with NMP's need for synchronous typed insert outcomes and atomic multi-table transactions. The 'no visitor API' argument is removed; the ownership/architecture arguments are strengthened.

## Consequences

- The nostrdb-rs evaluation doc is now a trustworthy architecture source with current evidence
- NMP can adopt nostrdb's visitor/streaming pattern internally (query_visit, run_filter_visit) without taking a GPL-marked crate dependency
- Future re-evaluations must check upstream evidence rather than relying on stale claims

## Open Tail

- The Rust binding now exposes visitor semantics — if nostrdb ever allows external LMDB environment ownership, the rejection would need another revisit

## Evidence

- transcript lines 13-17
- transcript lines 62-64
- transcript lines 78-84
- transcript lines 130-141

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-nostrdb-adoption-rejection-upheld-on-corrected.json`](transcripts/2026-06-18-1-nostrdb-adoption-rejection-upheld-on-corrected.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-nostrdb-adoption-rejection-upheld-on-corrected.json`](transcripts/raw/2026-06-18-1-nostrdb-adoption-rejection-upheld-on-corrected.json)

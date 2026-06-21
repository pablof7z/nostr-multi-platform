---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: superseded
subjects:
  - nostrdb-rs-evaluation
  - lmdb-eventstore
  - store-architecture
supersedes:
  - 2026-06-18-1-nostrdb-rs-rejection-upheld-on-corrected
related_claims: []
source_lines:
  - 49-68
  - 70-93
  - 125-143
captured_at: 2026-06-18T18:45:00Z
---

# Episode: nostrdb-rs evaluation stale-visitor claim corrected, rejection maintained on stronger grounds

## Prior State

The nostrdb-rs evaluation doc claimed the Rust binding does not expose visitor query semantics (ndb_query_visit), and this was a stated reason for rejecting direct nostrdb adoption.

## Trigger

Issue #1515 identified that current nostrdb-rs exposes fold/try_fold, making the visitor objection stale; Opus plan confirmed this across §1/§4/§6 of the evaluation doc.

## Decision

Withdraw the visitor objection from the evaluation; maintain the rejection of direct nostrdb-rs adoption on four remaining decisive grounds: (1) nostrdb owns its LMDB environment, conflicting with ADR-0011; (2) owns its ingester/writer threads; (3) owns its write policy, conflicting with NMP's need for synchronous typed insert outcomes and atomic multi-table transactions; (4) GPL licensing of nostrdb C core. Added §5b license checkpoint. Replaced §8 prose with issue cross-ref table (#1515–#1524).

## Consequences

- Future engineers cannot cite the stale 'no visitor API' argument against nostrdb-rs
- The adoption rejection is now grounded on architectural incompatibility (single-writer, write-policy ownership) and licensing — more durable reasons that survive upstream API changes
- The epic #1523 now proceeds with 'adopt techniques without dependency' as the explicit strategy, informed by corrected evidence

## Open Tail

- §5b license checkpoint may need re-evaluation if nostrdb C core relicenses

## Evidence

- transcript lines 49-68
- transcript lines 70-93
- transcript lines 125-143

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-nostrdb-rs-evaluation-stale-visitor-claim.json`](transcripts/2026-06-18-1-nostrdb-rs-evaluation-stale-visitor-claim.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-nostrdb-rs-evaluation-stale-visitor-claim.json`](transcripts/raw/2026-06-18-1-nostrdb-rs-evaluation-stale-visitor-claim.json)

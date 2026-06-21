---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: superseded
subjects:
  - nostrdb-rs-evaluation
  - store-architecture
  - lmdb-event-store
supersedes:
  - 2026-06-18-1-nostrdb-direct-dependency-remains-rejected-stale
related_claims: []
source_lines:
  - 49-68
  - 70-94
  - 126-142
captured_at: 2026-06-18T21:14:50Z
---

# Episode: Nostrdb rejection doctrine: visitor objection stale, rejection maintained on ownership/transaction grounds

## Prior State

The nostrdb-rs evaluation doc argued that direct nostrdb adoption should be rejected partly because the Rust binding only exposes materialized Vec queries, not the C ndb_query_visit visitor path. This was a key technical objection in the rejection rationale.

## Trigger

Issue #1515 identified that current nostrdb-rs upstream now exposes fold/try_fold on its query iterators, providing visitor semantics. The original rejection argument was based on stale evidence — the visitor objection was no longer true.

## Decision

Maintain the rejection of direct nostrdb-rs adoption, but on corrected grounds: visitor semantics ARE now expressible via fold/try_fold; the actual blocking concerns are (1) nostrdb owns its LMDB environment, ingester/writer threads, and write policy, conflicting with NMP's need for synchronous typed insert outcomes and atomic event/provenance/tombstone/watermark/domain writes in one transaction, and (2) GPL licensing. The evaluation doc and lessons doc were both updated to remove the stale claim and add the corrected reasoning.

## Consequences

- Epic #1523 direction (adopt nostrdb engineering lessons, not the dependency) is validated and proceeds
- Any future nostrdb adoption discussion must address LMDB environment ownership and writer control, not visitor semantics
- The StoreQuery-to-LMDB-index mapping strategy (inspired by nostrdb lessons) can use streaming visitor patterns without depending on nostrdb-rs

## Open Tail

- Issue #1517 (StoreQuery coverage audit) still open — tracking uncovered shapes that degrade to broad filter scans

## Evidence

- transcript lines 49-68
- transcript lines 70-94
- transcript lines 126-142

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-nostrdb-rejection-doctrine-visitor-objection-stale.json`](transcripts/2026-06-18-1-nostrdb-rejection-doctrine-visitor-objection-stale.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-nostrdb-rejection-doctrine-visitor-objection-stale.json`](transcripts/raw/2026-06-18-1-nostrdb-rejection-doctrine-visitor-objection-stale.json)

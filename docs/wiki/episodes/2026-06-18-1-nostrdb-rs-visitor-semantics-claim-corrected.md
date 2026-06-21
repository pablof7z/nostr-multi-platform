---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: root-cause
status: superseded
subjects:
  - nostrdb-rs-evaluation
  - store-query-visit
  - lmdb-event-store
supersedes:
  - 2026-06-18-1-nostrdb-rs-evaluation-visitor-semantics-evidence
related_claims: []
source_lines:
  - 49-93
  - 109-148
captured_at: 2026-06-18T20:35:15Z
---

# Episode: nostrdb-rs visitor-semantics claim corrected; rejection maintained on other grounds

## Prior State

The nostrdb-rs evaluation doc claimed the Rust binding only exposes materialized Vec queries and does not expose the C ndb_query_visit visitor path, making early-stopping scans impossible without forking the binding.

## Trigger

Issue #1515 identified the claim as stale; verification of current upstream nostrdb-rs showed fold/try_fold now expose visitor semantics, invalidating that specific objection.

## Decision

Maintain rejection of direct nostrdb-rs adoption (synchronous typed inserts, atomic multi-table transactions, and LMDB environment ownership still conflict), but correct the evaluation doc to acknowledge visitor semantics are available upstream. NMP's own streaming query_visit (#1516) is the internal adoption of the visitor-pattern lesson.

## Consequences

- The visitor-semantics objection is removed from the rejection rationale; future reconsideration of nostrdb-rs cannot reuse that argument.
- NMP's hand-rolled query_visit is the canonical streaming path, not a nostrdb dependency.
- The preliminary lean toward nostrdb-rs in nostrdb-notedeck-lessons.md §2.5 is formally superseded by the refreshed evaluation.

## Open Tail

*(none)*

## Evidence

- transcript lines 49-93
- transcript lines 109-148

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-nostrdb-rs-visitor-semantics-claim-corrected.json`](transcripts/2026-06-18-1-nostrdb-rs-visitor-semantics-claim-corrected.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-nostrdb-rs-visitor-semantics-claim-corrected.json`](transcripts/raw/2026-06-18-1-nostrdb-rs-visitor-semantics-claim-corrected.json)

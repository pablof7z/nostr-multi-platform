---
type: episode-card
date: 2026-05-25
session: 86221d39-67d3-484d-8979-b91cf75a5a72
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/86221d39-67d3-484d-8979-b91cf75a5a72.jsonl
salience: architecture
status: active
subjects:
  - indexer-republish
  - relay-strategy
  - event-distribution
supersedes: []
related_claims: []
source_lines:
  - 40-44
  - 53-56
captured_at: 2026-06-18T05:26:10Z
---

# Episode: Indexer republish pipeline — propagate stale/missing events back to indexers

## Prior State

Events found on personal relays that belong on indexers (kind 0, 3, 1xxxx) remained only on those personal relays. No mechanism existed to republish them to connected indexer relays, so stale or missing indexer data was never healed.

## Trigger

User directed creation of an optional (default-enabled) pipeline: whenever an event that should be on an indexer is found elsewhere, republish that same event (without resigning) to connected indexer relays. Specifically, if a newer kind:0 is found on a personal relay than what the indexer has, the newer event should be forwarded.

## Decision

Architecture designed for an indexer-republish pipeline with dedup strategy, loop prevention, and config flag. Plans/indexer-republish-plan.md was written covering ordered implementation steps.

## Consequences

- New optional pipeline (default-enabled) republishes indexer-suitable events back to indexers
- Replaceable-event supersession logic (created_at comparison) determines when republishing is warranted
- Loop prevention needed: indexers must not re-ingest events they already have at the same or newer created_at
- Config flag allows opt-out for users who don't want their client to republish

## Open Tail

- Implementation of the republish pipeline not yet started
- Interaction with relay role model (indexer vs. personal vs. write relays) needs detailed routing rules

## Evidence

- transcript lines 40-44
- transcript lines 53-56


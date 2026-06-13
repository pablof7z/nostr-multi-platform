---
type: episode-card
date: 2026-06-13
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: product
status: active
subjects:
  - follow-fan-out
  - read-your-writes
  - active-follow-set
supersedes: []
related_claims: []
source_lines:
  - 5026-5028
captured_at: 2026-06-13T19:22:03Z
---

# Episode: Read-your-writes for follows via single acquisition path

## Prior State

Local kind:3 (follow/unfollow) publishes bypassed the ingest_contacts → notify_event_observers fan-out path that relay-sourced events used, so ActiveFollowSet and FollowListProjection did not reflect local follow/unfollow actions in real time.

## Trigger

Architecture review Finding A — read-your-writes inverted for follows.

## Decision

Route local kind:3 publishes through the same ingest_contacts → notify_event_observers sequence the relay arm uses, gated on Inserted|Replaced so the duplicate relay echo never double-fires. Single acquisition path per D4.

## Consequences

- Follow and unfollow now reflect live from local publishes
- A second subtle bug was discovered: same-second kind:3 events tie-breaking to Superseded (correct NIP-01 id-tiebreak) — solved with deterministic test-clock seam
- kind:0 (profile) deliberately deferred as separate optimistic-overlay decision → issue #1193

## Open Tail

- Issue #1193: kind:0 optimistic overlay remains open

## Evidence

- transcript lines 5026-5028


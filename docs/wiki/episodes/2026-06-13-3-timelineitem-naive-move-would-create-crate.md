---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-nip01
  - nmp-core
  - timeline-item
  - crate-boundaries
  - snapshot-envelope
supersedes: []
related_claims: []
source_lines:
  - 8122-8144
captured_at: 2026-06-13T19:35:42Z
---

# Episode: TimelineItem naive move would create crate cycle — envelope-cut is the right shape (#920)

## Prior State

#920 proposed moving TimelineItem out of nmp-core into nmp-nip01 as a straightforward relocation.

## Trigger

Agent verified the live dependency graph and found nmp-nip01 → nmp-core already exists in Cargo.toml; moving TimelineItem into nmp-nip01 would create a cycle.

## Decision

GO via snapshot-envelope cut (NOT naive move): nmp-nip01 already owns ModularTimelineSnapshot/TimelineEventCard; the right shape is the envelope-cut that crate-boundaries.md already declares. Post-v1, needs staged plan.

## Consequences

- Prevents introducing a crate dependency cycle
- Confirms nmp-nip01 as the correct owner of timeline row family
- Architecture pattern matches #1283: resolve protocol branching above kernel, ship typed projection

## Open Tail

- Staged migration plan needed when scheduled post-v1

## Evidence

- transcript lines 8122-8144


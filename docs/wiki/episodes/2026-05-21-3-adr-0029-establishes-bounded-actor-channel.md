---
type: episode-card
date: 2026-05-21
session: 156aa64b-42e1-4d3b-96ce-25b31fc06fec
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/156aa64b-42e1-4d3b-96ce-25b31fc06fec.jsonl
salience: architecture
status: active
subjects:
  - actor-channel-bounds
  - shed-load-policy
  - adr-0029
supersedes: []
related_claims: []
source_lines:
  - 1422-1427
  - 1488-1492
  - 1562-1578
captured_at: 2026-06-18T05:05:38Z
---

# Episode: ADR-0029 establishes bounded actor channel + shed-load policy

## Prior State

No formal policy bounded the actor's `std::sync::mpsc` channel; backpressure was unarticulated. The architectural audit flagged the single-actor transport as a risk under load.

## Trigger

Architectural audit identified unbounded actor channel as a major structural problem. Agent C4 produced the ADR.

## Decision

ADR-0029 codifies a bounded actor channel with an explicit shed-load policy. Merged as doctrine via PR #245.

## Consequences

- ADR-0029 is now the governing doctrine for actor channel sizing and shed-load behavior
- Implementation code is still TBD — the ADR establishes the contract but the runtime change is deferred

## Open Tail

- Bounded-channel implementation must be written to fulfill ADR-0029
- CI failures on merged PR #245 need investigation

## Evidence

- transcript lines 1422-1427
- transcript lines 1488-1492
- transcript lines 1562-1578


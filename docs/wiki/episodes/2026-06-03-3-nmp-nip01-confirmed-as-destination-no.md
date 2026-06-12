---
type: episode-card
date: 2026-06-03
session: cf071d35-ee9b-4a1f-a3b8-885c651e8cce
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/cf071d35-ee9b-4a1f-a3b8-885c651e8cce.jsonl
salience: architecture
status: active
subjects:
  - nmp-nip01
  - crate-boundaries
  - timeline-item
supersedes: []
related_claims: []
source_lines:
  - 70-79
captured_at: 2026-06-11T23:04:09Z
---

# Episode: nmp-nip01 confirmed as destination — no new crate needed

## Prior State

User suspected nmp-nip01 might not be the right destination for timeline items; a new `nmp-social` or `nmp-timeline` crate was considered.

## Trigger

Agent analysis showed nmp-nip01 already composes nmp-nip18/nmp-nip57 as sibling dependencies (crate-boundaries.md §1 sanction), and the spec assigns 'the kernel-owned canonical timeline projection' to nmp-nip01. Creating a new crate would violate the repo's 'edit the spec, don't route around it' rule.

## Decision

TimelineItem's successor lives in nmp-nip01. No new crate. nmp-nip18 and nmp-nip57 are separate crates composed as dependencies, not re-implemented by nmp-nip01.

## Consequences

- crate-boundaries.md must be corrected to record the real dependency edges (nmp-nip18, nmp-nip57, nmp-feed, nmp-content) that reality already has.
- The NIP-agnostic feed substrate (nmp-feed) already exists; kind:6 reposts belong with kind:1 in nmp-nip01.

## Open Tail

*(none)*

## Evidence

- transcript lines 70-79


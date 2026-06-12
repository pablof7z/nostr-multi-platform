---
type: episode-card
date: 2026-06-12
session: 954c56b2-d292-4021-8b55-977d3fd8df4d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/954c56b2-d292-4021-8b55-977d3fd8df4d.jsonl
salience: architecture
status: active
subjects:
  - raw-event-tap-deferred
  - v-114-free-string
  - ffi-surface-judgment
supersedes: []
related_claims: []
source_lines:
  - 1605-1606
  - 1746-1751
captured_at: 2026-06-12T06:38:44Z
---

# Episode: Raw-event tap and dual free-string deferred as non-mechanical

## Prior State

Several documented debt items existed with apparent fix directions. Raw-event tap has conformance rule A5 pushing per-consumer typed projections. V-114 dual free-string symbols are a category:decision issue.

## Trigger

User asked to fix all documented technical debt where the direction is right, using judgment on whether prior direction was sound. The assistant evaluated each item against whether it was a mechanical fix or required genuine design work.

## Decision

Deliberately NOT dispatched: (1) raw-event tap — needs per-consumer projection design work, not a mechanical fix; wrong shape for a fire-and-forget agent. (2) V-114 — explicitly category:decision, recommended option (a) (collapse to one `nmp_free_string` with ABI aliases) but deferred pending user word. (3) #1090 — feature-gated on store-claims wiring, not settled debt.

## Consequences

- Raw-event tap remains a documented-and-policed escape hatch until per-consumer typed projections are designed
- V-114 dual free-string remains a latent cross-free UB trap pending user decision on option (a)
- LRU ceiling re-enable (#1090) blocked on store-claims wiring

## Open Tail

- User decision needed on V-114 option (a) vs (b)
- Raw-event tap needs design per consumer

## Evidence

- transcript lines 1605-1606
- transcript lines 1746-1751


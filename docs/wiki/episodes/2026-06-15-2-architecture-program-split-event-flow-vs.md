---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: active
subjects:
  - arch-program-scope
  - workstream-partitioning
supersedes: []
related_claims: []
source_lines:
  - 1498-1558
captured_at: 2026-06-15T12:07:53Z
---

# Episode: Architecture program split: event-flow vs authority-lifecycle

## Prior State

Single `arch-fixes.md` plan containing six workstreams (A–F) spanning event-flow ingest, acquisition one-door, publish one-door, signer/capability authority, action/projection lifecycle, and doctrine gates — mixing the kernel's event-flow architecture with its capability/lifecycle ownership architecture.

## Trigger

User asked whether workstreams B–F are architectural issues or unrelated; analysis showed D (signer/capability authority) and E (action/projection lifecycle) are same-caliber but orthogonal to the event-flow concern, while A/B/C are genuinely one thing (acquire → ingest → publish).

## Decision

Split into two coherent plans: `arch-fixes.md` becomes the event-flow architecture plan (Workstreams A/B/C + their F gates), titled 'one kind-agnostic door: acquire · ingest · publish.' D+E spin into a sibling `arch-authority-lifecycle.md` titled 'kernel owns its own authority and lifecycles.' Cross-linked, no overlap.

## Consequences

- Each plan is one coherent concern; neither becomes an everything-bucket.
- D+E are independently sequencable and can run in parallel with A/B/C.
- F gates partition: `store.insert`/`notify_event_observers` bans ride with A; D22 coverage-floor gate rides with B; shell-boundary/raw-kind FFI gates ride with their respective workstreams.

## Open Tail

- D+E sibling plan needs its own ADR (combined or per-workstream) before implementation.
- Tracking issues still needed for each workstream so the plan files remain tactical indices, not roadmaps.

## Evidence

- transcript lines 1498-1558

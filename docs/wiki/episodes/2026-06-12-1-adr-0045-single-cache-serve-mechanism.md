---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: active
subjects:
  - adr-0045
  - cache-serve
  - store-replay
  - offline-rendering
supersedes: []
related_claims: []
source_lines:
  - 4104-4131
  - 4153-4174
captured_at: 2026-06-12T06:14:07Z
---

# Episode: ADR-0045: Single cache-serve mechanism replaces staged domain-specific approach

## Prior State

ADR-0045 Rev 1 specified a staged-by-domain rollout: Stage 1 for timeline replay, Stage 2 for DM inbox replay, Stage 3 for generalization — effectively two acquisition modes (offline cache-serve vs. online relay fetch) triggered differently per domain

## Trigger

User explicitly rejected the staged approach: 'offline replay should be governed by a single mechanism… there should be a SINGLE thing way to do things, and that thing should include serving things from the cache. So the stage 1 (timeline) vs stage 2 (DMs) should not even exist; it should be the same thing and serving things from the cache should always happen regardless of whether the app is just starting and it's not even connected to relays or if the app is already connected to relays.'

## Decision

One event-acquisition mechanism: store-served first through the same dispatch path on every interest open, network REQ as refinement half — always-on, no domain stages, no offline special-casing. Offline rendering is the degenerate case where network delivers nothing. Staged-by-domain §9 rejected; landing proceeds as engineering increments E1/E2/E3 of one seam. v1-gating decision: universal cache-serve gates v1. Serve depth default: 1× visible window (deeper = per-interest opt-in).

## Consequences

- #1086 re-labeled phase:v1-blocker / priority:p1 — the only open v1 blocker
- Acceptance test restated: launch twice, second launch offline, EVERY open interest renders from the store
- ADR-0045 Rev 2 merged as PR #1102 with the single-mechanism design
- Rev 1 technical findings survive (no store.insert replay, Provenance::LocalStore marker, budgeted per-tick serve, watermark⇄serve invariant now universal by construction)
- Implementation launches as engineering increments of one seam, not per-domain stages

## Open Tail

- E1/E2/E3 implementation increments not yet started
- InterestShape→StoreQuery mapping over existing indexes
- MLS group-state exclusion from the uniform path

## Evidence

- transcript lines 4104-4131
- transcript lines 4153-4174


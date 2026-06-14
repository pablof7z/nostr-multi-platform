---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: product
status: superseded
subjects:
  - neg-open-unfloor
  - since-floor-soundness
  - eligibility-filter
supersedes: []
related_claims: []
source_lines:
  - 5997-6115
captured_at: 2026-06-14T12:30:14Z
---

# Episode: NEG-OPEN reconciliation un-floored + floor soundness patches

## Prior State

NEG-OPEN inherited the floored since from ReqFrameContext, so set reconciliation only covered [floor, ∞) — exactly the window declared boring — and could not repair below-floor gaps. Three additional floor-soundness bugs existed: address-pointer branch used max-ignoring-empties (unsafe), silent-relay NEG-OPEN left interest stuck forever, and budget-truncated Etag/Ptag serves advanced past their query stranding the stored tail.

## Trigger

H2 finding from read-path review: a single stray below-floor stored event permanently suppresses an author's backfill in NEG-eligible follow-feed shapes. Recon confirmed all three soundness bugs were still present.

## Decision

Stage A: EligibleFilter::unfloored() drops the since lower bound on the NEG path (keeping the floor on plain REQs), making NEG reconciliation self-healing for below-floor gaps. Stage B: (B1) address-pointer branch now uses min/abort rule matching authors branch; (B2) NEG-OPEN liveness deadline via on_idle_tick (30s wall-clock, re-anchored on NEG-MSG, falls back to plain REQ); (B3) budget-truncated Etag/Ptag serves refuse the floor via session-scoped truncation set.

## Consequences

- NEG-eligible shapes are self-healing for below-floor gaps (TDD oracle proves repair)
- Address-pointer floor alignment eliminates the latent max-ignoring-empties policy divergence
- Silent-relay NEG-OPEN no longer strands subscriptions indefinitely
- Truncated serves no longer block relay re-sends for budget-limited shapes
- Each patch proven RED-without-fix via TDD sabotage checks

## Open Tail

*(none)*

## Evidence

- transcript lines 5997-6115


---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: reversal
status: superseded
subjects:
  - projection-subscription
  - adr-0039
  - kernel-ffi-boundary
supersedes: []
related_claims: []
source_lines:
  - 5345-5360
  - 5435-5447
  - 5508-5525
captured_at: 2026-06-13T20:33:24Z
---

# Episode: Host-declared projection consumption supersedes ADR-0039's blanket prohibition

## Prior State

ADR-0039 explicitly rejected letting hosts declare which projections they consume, forcing every registered projection to ship on every snapshot regardless of whether any UI consumes it. The justification was 'an active-group concept would require a round-trip to set kernel state from the host — a one-way-data-flow violation.' relay_diagnostics (a debug screen almost never opened) was serialized, decoded, and verified 4×/sec on every device forever as a direct consequence.

## Trigger

User found the decision 'obviously completely stupid' and directed a clean redesign with 'no backwards compatibility bullshit — PROPER DESIGN.' Analysis showed ADR-0039 refuted a strawman (the active-group-tracking variant, which would leak view state) and over-generalized into banning all consumer-side selection. The kernel already accepts host-declared relay interests (push_interest), profile claims, event claims, and dynamic feed keys — a declared projection set is the output-side sibling, resource ownership not business logic.

## Decision

Supersede ADR-0039 with host-declared consumed-projection set declared once at init (static, no view-state leak, fully one-way). Kernel only serializes declared projection keys. Opus architect dispatched to produce the ADR and implementation.

## Consequences

- relay_diagnostics and other rarely-consumed projections stop being serialized unless explicitly declared
- Composes with incremental emission (ADR-0053): unchanged declared projections use the unchanged wire token; undeclared projections simply don't appear
- The 'view state in the kernel' concern is explicitly scoped out: static declaration at init, not dynamic per-screen subscription
- codex second opinion confirmed the category error and added: host apply must also become rev-aware to avoid broad SwiftUI invalidation from reassigning every @Published slot

## Open Tail

- ADR and implementation from Opus architect not yet finalized
- Three adjacent problems surfaced by codex may fold in: projection closures running on actor thread, duplicate feed materialization on same tick, host apply churn from @Published reassignment

## Evidence

- transcript lines 5345-5360
- transcript lines 5435-5447
- transcript lines 5508-5525


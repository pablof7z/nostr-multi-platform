---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - cache-serve-fan-out-unification
  - project-accepted-event
  - verify-and-persist-persistence-only
supersedes:
  - 2026-06-15-2-cache-serve-fan-out-unified-with
related_claims: []
source_lines:
  - 3309-3315
  - 3607-3639
  - 3697-3700
  - 3766-3787
captured_at: 2026-06-15T16:55:55Z
---

# Episode: Cache-serve fan-out unified into single project_accepted_event helper

## Prior State

Cache-serve's feed_served_event had its own divergent post-store logic — missing D9 future-created_at clamp, missing profiles_ver bumps, missing NIP-parser dispatch. Live path and cache-serve path were separate code paths with no shared helper, allowing them to drift.

## Trigger

PR1b discovered cache-serve missed D9 clamp; PR2 discovered cache-serve missed profiles_ver transition sweep and dispatcher fan-out. Two gaps confirmed the divergence was systematic, not one-off.

## Decision

Extract one shared Kernel::project_accepted_event helper that owns all three post-store concerns kind-agnostically (NIP-parser dispatch, transition sweep, D9 clamp + observer notify). Both ingest_accepted_event and feed_served_event call it. verify_and_persist is now persistence-only (sig-verify → store.insert → raw-tap → provenance → TTL), returning (InsertOutcome, VerifiedEvent) so the caller gates projection on Inserted|Replaced|Ephemeral. PR1b subsumed.

## Consequences

- Cache-serve remains store.insert-free (grep-proven); it reconstructs VerifiedEvent from already-verified store fields and runs only post-store fan-out
- D4 single-fire invariant preserved: Duplicate is projection-silent, no double-dispatch possible
- Fake inject_profile direct-cache writer deleted; test support routes through genuine dispatcher path
- ADR-0057 and ingest module comments updated to reflect the split (verify_and_persist = persistence, project_accepted_event = projection)
- PR1b branch abandoned; its D9 clamp test ported into the shared helper

## Open Tail

- Monitor whether any future post-store concern (e.g. new transition sweep) is added to only one call site and re-diverges

## Evidence

- transcript lines 3309-3315
- transcript lines 3607-3639
- transcript lines 3697-3700
- transcript lines 3766-3787

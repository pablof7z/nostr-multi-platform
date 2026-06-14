---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: active
subjects:
  - aim-axiom-10
  - doctrine-12-incremental-default
supersedes:
  - 2026-06-14-2-projectioncache-interposer-enables-incremental-apply-on
related_claims: []
source_lines:
  - 9760-9768
  - 9822-9835
captured_at: 2026-06-14T12:34:35Z
---

# Episode: aim.md amended: projections incremental-by-default for rev-aware hosts

## Prior State

aim.md Axiom 10 described granular updates as an optimization to be added 'where profiling demands'; Doctrine #12 said incremental was an optimization, not the default

## Trigger

Rung 3 (incremental projection emission) is now landed and empirically validated — the 'profiled optimization' is realized, proven correct, and shipped; the doctrine should reflect the new architectural reality

## Decision

Amended Axiom 10 and Doctrine #12: projections are now incremental-by-default for hosts that advertise rev-aware apply capability (nmp_app_declare_incremental_apply); full snapshot remains the baseline/resync shape for cold start and recovery

## Consequences

- New projections are expected to register with rev-gating from the start, not added as a later optimization
- Full-snapshot path is the resync/recovery mechanism, not the default emission shape
- Rung 6 (feed gating) extends this same doctrine to Tier-1/host-registered projections

## Open Tail

*(none)*

## Evidence

- transcript lines 9760-9768
- transcript lines 9822-9835


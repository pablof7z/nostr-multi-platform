---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-chirp-config
  - relay-roles
  - nmp-discovery-probe
supersedes: []
related_claims: []
source_lines:
  - 3256-3317
  - 3319-3364
captured_at: 2026-06-15T10:08:20Z
---

# Episode: Primal relay role changed from indexer to app relay

## Prior State

primal.net was configured as `both,indexer` in Chirp's default relay set — used both as a general relay and as a dedicated indexer for discovery queries (kind:10002 NIP-65 probes). purplepag.es was the other dedicated indexer.

## Trigger

User directive: 'make primal an app relay instead of an indexer.' Empirical measurement showed adding a single broad app relay (nos.lol) jumps resolution from 60.3% → 88.8% (+300 follows), and 204 of those are the no-NIP-65 cohort that the outbox model can never reach.

## Decision

Change primal.net from `both,indexer` to `both` (app relay only), leaving purplepag.es as sole dedicated indexer. Requires verification that kind:10002 discovery probes (which target the indexer set) still work when purplepag is the sole indexer and AUTH-walls anonymous clients — may need kernel-side fix to make the 10002 probe additive to app relays.

## Consequences

- Profile queries still reach primal via the app-relay lane (preserved), potentially improving resolution for the no-NIP-65 cohort
- kind:10002 discovery probe that previously targeted both primal and purplepag now falls to purplepag-only — risk of regression if purplepag AUTH-walls and the probe isn't also sent to app relays
- Measured 60.3% → 88.8% with a broad app relay validates the three-tier stacking model: indexers → outbox → app relays

## Open Tail

- PR in progress — need to verify/fix that the 10002 discovery probe doesn't regress when purplepag is the sole indexer
- Kernel may need modification to make the kind:10002 discovery probe additive to app relays, not just indexer-only

## Evidence

- transcript lines 3256-3317
- transcript lines 3319-3364

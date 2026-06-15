---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: active
subjects:
  - profile-resolution-ceiling
  - app-relay-necessity
  - nip-65-coverage
supersedes: []
related_claims: []
source_lines:
  - 3206-3316
captured_at: 2026-06-15T10:28:02Z
---

# Episode: No-NIP-65 cohort is structurally unreachable by the outbox model

## Prior State

Belief that the outbox model (NIP-65) would close the profile-resolution gap once activated. The remaining ~40% unreachability was not quantified or attributed to a specific structural cause.

## Trigger

Measurement of follows' kind:0 resolution with app relays added to the outbox model. Live sweep of 1052 follows showed that adding a single broad app relay (nos.lol) jumps resolution from 60.3% to 88.8% (+300 follows).

## Decision

Research finding (not a code change): ~32% of follows (334 of 1052) publish no kind:10002 at all, making them structurally invisible to the outbox model. 204 of the 300 follows gained via app relay are in this no-NIP-65 cohort — an app relay is the only way to reach them. This is a protocol-level ceiling, not a fixable code gap.

## Consequences

- Confirms the outbox model alone can never exceed ~60% for this user's follow graph
- Validates the architecture of NMP's additive routing: indexers + outbox + app relays stack (indexers ~28%, +outbox ~60%, +app-relay ~89%)
- Directly motivated the decision to reclassify primal as an app relay — app relays are load-bearing for the no-NIP-65 cohort, not optional

## Open Tail

- The remaining ~11% (1052 - 935 = 117 follows) may be resolvable with additional relays or AUTH to relay.nostr.band (which returned 0 to anonymous REQs)
- This measurement is point-in-time and varies with relay availability (e.g., purplepag AUTH-walling dropped the indexer baseline from 28% to 10% across runs)

## Evidence

- transcript lines 3206-3316

---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: active
subjects:
  - adr-0055-rung3
  - tier-2-row-suppression
  - feed-byte-dominance
supersedes: []
related_claims: []
source_lines:
  - 9887-9894
captured_at: 2026-06-14T14:10:09Z
---

# Episode: Rung 3 byte-savings reframed: 18% not 81%

## Prior State

Rung 3 (incremental projection emission) was believed to deliver ~81% waste reduction, based on Tier-2 row-count metrics

## Trigger

S6 capstone empirical measurement, opus-reproduced 5×, showed bytes are dominated by the Tier-1 feed (~6× the rest of the frame) which Rung 3 does not gate; the 81% figure was row-count waste, not byte waste

## Decision

Corrected the framing: Rung 3 actually delivers ~18% frame-byte reduction + 68.8% Tier-2 row suppression, zero data loss. The dominant remaining win is Tier-1 feed gating, not further Tier-2 optimization

## Consequences

- Redirected architectural effort to Tier-1 feed gating (Rung 6) as the real whole-product win
- Rung 3 acknowledged as correct debt-free foundation but insufficient for the byte/jank problem
- Feed's absence from the bare measurement harness meant prior win was systematically understated

## Open Tail

- Option B (feed row-deltas) gated behind release/device measurement to confirm whether jank is serialization or SwiftUI/debug-build cost

## Evidence

- transcript lines 9887-9894


---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: reversal
status: superseded
subjects:
  - rung3-empirical-result
  - tier2-row-suppression
  - metric-swap-waste-ratio-to-row-suppression
supersedes:
  - 2026-06-14-1-adr-0055-r3-capstone-metric-swap
related_claims: []
source_lines:
  - 9448-9467
  - 9552-9624
  - 9633-9654
captured_at: 2026-06-14T12:34:35Z
---

# Episode: Rung 3 delivers ~18% byte reduction, not ~81% — Tier-1 feed dominates frame budget

## Prior State

Rung 3 was expected to eliminate ~81% of frame waste; the standing acceptance gate was waste_ratio < 0.05, measuring byte-identity of always-emitted rows

## Trigger

S6 capstone harness measured Phase B at only 17.8% frame-byte reduction (9640→7928B); Phase B waste_ratio is 40% — composed entirely of two always-Changed Tier-1 projections (claimed_event_embeds, nip46_onboarding) that Rung 3 does not gate by design (D3-7). Opus review reproduced all numbers 5× and confirmed: zero unchanged Tier-2 rows leak through, the 40% is Tier-1 only, and the original gate was unachievable by design

## Decision

Swapped the headline acceptance gate from waste_ratio < 0.05 to row_suppression_ratio ≥ 0.50 (measured 0.6875) — the honest Rung-3 metric that directly measures fraction of rows removed by omission. Rewrote the S6 docstring to state the honest result (~18% byte reduction + 68.8% Tier-2 row suppression, zero data loss). Deferred Tier-1/feed gating to a future rung (Rung 6)

## Consequences

- The bulk byte savings and most device-jank fix live in the deferred Tier-1/feed-gating rung — Rung 3 is the correct debt-free foundation but not the jank solution
- row_suppression_ratio is the correct Rung-3 metric; waste_ratio measures hash-stability of always-emitted rows, which is out of scope this rung
- The ~81% framing was row-count waste, not byte waste — the capstone measurement and adversarial review corrected this
- aim.md and pr-ladder amended to record honest result and incremental-by-default doctrine

## Open Tail

- Rung 6 (feed gating) is where the dominant byte savings land; design is posted at #1415 awaiting implementation

## Evidence

- transcript lines 9448-9467
- transcript lines 9552-9624
- transcript lines 9633-9654


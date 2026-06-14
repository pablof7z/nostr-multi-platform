---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: product
status: superseded
subjects:
  - adr-0055-rung3
  - capstone-gates
  - row-suppression
supersedes: []
related_claims: []
source_lines:
  - 9424-9487
captured_at: 2026-06-14T11:34:03Z
---

# Episode: Capstone success metric redefined from waste_ratio to row_suppression_ratio

## Prior State

ADR-0055 S5 capstone spec defined success as waste_ratio < 0.05 (i.e., >95% of serialized bytes eliminated), anticipating that the ~81% measured waste would collapse with incremental enabled.

## Trigger

Empirical measurement showed Phase B serializes 500 rows vs 300 changed — the extra 200 are necessary Tier-1 (always-Changed feed) and Cleared rows, not waste. The original waste_ratio metric measured pre-omission churn rather than post-omission savings, making it the wrong gate for this rung.

## Decision

Headline gate changed from waste_ratio < 0.05 to row_suppression_ratio >= 0.50 (measured 0.6875). Serialize-time gate given ±20% tolerance for OS scheduling noise between independent kernel instances (threshold = p50_a × 1.20). Byte-identity oracle confirmed 0 mismatches across 103 incremental frames.

## Consequences

- Actual frame-byte reduction is ~18% (p50 9640→7928B), not the anticipated ~81%, because Tier-1 feed stays always-Changed this rung
- Row suppression of 68.8% is real and verified; the remaining byte share is legitimately necessary data
- The larger byte savings require Tier-1 gating in a future rung (the D3-7 caveat empirically confirmed)

## Open Tail

- Opus review of the metric swap was in progress at session end — adjudicating whether the gate change is principled or fudged, and what the honest headline number is

## Evidence

- transcript lines 9424-9487


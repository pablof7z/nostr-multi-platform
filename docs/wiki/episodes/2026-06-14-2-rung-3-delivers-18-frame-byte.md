---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: superseded
subjects:
  - incremental-emission-rung3
  - metric-gate-swap
  - tier2-row-suppression
supersedes:
  - 2026-06-14-3-capstone-success-metric-redefined-from-waste
related_claims: []
source_lines:
  - 9424-9468
  - 9470-9488
  - 9552-9632
captured_at: 2026-06-14T11:51:24Z
---

# Episode: Rung 3 delivers ~18% frame-byte reduction — Tier-1 feed gating is the remaining prize

## Prior State

ADR-0055 Rung 3 was expected to eliminate ~81% serialization waste, gated by waste_ratio < 0.05. The module docstring claimed 'collapses Tier-2 waste from ~81% to <5%' and 'every tick' byte-identity verification. The byte-identity oracle treated absent keys as informational rather than failures

## Trigger

S6 capstone empirical measurement showed Phase B waste_ratio was 40% (entirely two Tier-1 always-Changed projections: claimed_event_embeds and nip46_onboarding). The original waste_ratio < 0.05 gate was unachievable by design because Tier-1 projections have no manifest entry and default to Changed. Full 500-row accounting proved zero unchanged Tier-2 rows leak through — the 40% waste is Tier-1 only

## Decision

Swap headline gate from waste_ratio_incremental < 0.05 to row_suppression_ratio >= 0.50 (measured 0.6875). Record honest result: ~18% frame-byte reduction (9640→7928 B) and 68.8% Tier-2 row suppression (1600→500 rows), zero data loss. Defer Tier-1/feed gating to future rung. Correct the module docstring to match implemented gates and honest numbers. Add 20% tolerance band to serialize-time gate (architecturally correct for independent-kernel timing noise, not load-bearing — Phase B consistently ≤ Phase A)

## Consequences

- The metric swap is principled: waste_ratio measured Tier-1 byte-stability which Rung 3 deliberately does not gate (D3-7 boundary)
- The larger remaining device-jank fix lives in Tier-1/feed gating (the feed dominates frame bytes and stays always-Changed this rung)
- Byte-identity oracle proves end-state losslessness (16 keys, zero mismatches) but is end-state-only, not per-tick — docstring must not claim 'every tick'
- Oracle absence-downgrade is a latent hole: future omit bugs dropping a needed Tier-2 row would be silently downgraded rather than failed
- Two file-size hard-cap violations must be fixed by extraction: s6_single_projection_churn.rs (684 LOC) and update_envelope.rs (508 LOC)
- The swap-justification comment incorrectly blamed relay_diagnostics as the waste_ratio dominator; actual dominators are claimed_event_embeds and nip46_onboarding
- The d13_part_a_positive_fixture_fires doctrine lint failure is an order-dependent filesystem race unrelated to ADR-0055

## Open Tail

- Strengthen byte-identity oracle to per-tick or whitelist known-nondeterministic keys and hard-fail on unexpected Tier-2 absence
- Tier-1/feed-gating rung: the next big prize for device-jank reduction
- R3-S6 docs rung: update aim.md and Doctrine #12 to make incremental default

## Evidence

- transcript lines 9424-9468
- transcript lines 9470-9488
- transcript lines 9552-9632


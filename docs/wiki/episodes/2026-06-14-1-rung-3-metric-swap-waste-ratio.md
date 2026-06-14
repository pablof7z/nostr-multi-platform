---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: active
subjects:
  - adr-0055-rung3-metric-gate
  - incremental-emission-acceptance-criteria
supersedes:
  - 2026-06-14-1-rung-3-delivers-18-byte-reduction
related_claims: []
source_lines:
  - 9552-9693
captured_at: 2026-06-14T13:12:38Z
---

# Episode: Rung 3 metric swap — waste_ratio replaced by row_suppression_ratio as acceptance gate

## Prior State

Rung 3's acceptance gate was waste_ratio_incremental < 0.05 (byte-stability of all emitted rows). The PR body and module docstring claimed Tier-2 waste collapses from ~81% to <5%, and the byte-identity oracle treated absent keys as informational (non-failure).

## Trigger

Opus methodology review (5× reproduction) showed the original gate reads 40% waste because two always-Changed Tier-1 host projections (claimed_event_embeds, nip46_onboarding) dominate the hash-waste metric and are out of scope by D3-7. The gate was mis-specified for the post-omission world — it measures byte-stability of always-emitted rows which Rung 3 by design does not address. Docstring overclaimed '~81%→<5%' when measured Phase-B waste is 40%.

## Decision

Swap acceptance gate from waste_ratio_incremental < 0.05 to row_suppression_ratio >= 0.50 (measured 0.6875). Record honest result: ~18% frame-byte reduction (9640→7928 B) + 68.8% Tier-2 row suppression (1600→500 rows), zero data loss. Rewrite module docstring to match implemented gates. Hardened byte-identity oracle to fail-closed (only whitelisted Tier-1 keys may be absent; any other dropped key hard-fails). Corrected swap-justification comment to name real dominators (the two Tier-1 keys, not relay_diagnostics).

## Consequences

- Rung 3's documented win is honestly bounded: ~18% bytes + 68.8% row suppression, not the inflated 81%→<5% framing
- The larger remaining byte prize (Tier-1/feed gating) is explicitly deferred to a future rung, not hand-waved as done
- Fail-closed oracle means a future omit bug that drops a needed Tier-2 row will hard-fail the capstone instead of silently passing
- The old waste_ratio gate is informational only — it cannot be met while Tier-1 is ungated, which is architectural not a bug

## Open Tail

- Tier-1/feed gating is the dominant remaining win (feed is ~6× the rest of the frame) — Rung 6 initiated
- Option B (feed row-deltas) gated behind release/device measurement confirming feed encode is the jank bottleneck

## Evidence

- transcript lines 9552-9693


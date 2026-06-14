---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - adr0055-r3-capstone
  - incremental-emission-metrics
  - tier-2-row-suppression
supersedes:
  - 2026-06-14-2-rung-3-delivers-18-frame-byte
related_claims: []
source_lines:
  - 9424-9468
  - 9470-9488
  - 9552-9632
  - 9655-9693
captured_at: 2026-06-14T12:04:57Z
---

# Episode: ADR-0055 R3 Capstone: Metric swap from waste_ratio to row_suppression_ratio

## Prior State

The original ADR-0055 R3 capstone specified `waste_ratio_incremental < 0.05` as the headline gate, with the expected result framed as '~81% Tier-2 waste eliminated' and byte-identity checked 'every tick.'

## Trigger

The S6 ffi-stress harness empirically measured Phase B waste_ratio at 40% — entirely composed of two Tier-1 always-Changed projections (claimed_event_embeds, nip46_onboarding) that are out of scope by D3-7. The original gate was unachievable by design because Tier-1 gating is a future rung.

## Decision

Replaced the headline gate with `row_suppression_ratio >= 0.50` (measured 0.6875), which directly measures 'fraction of would-be-serialized rows that omission removed.' The honest empirical result is ~18% frame-byte reduction (9640→7928 B) + 68.8% Tier-2 row suppression, zero data loss. Docstring overclaims ('~81%→<5%', removed gate, 'every tick' oracle) were rewritten to match reality. The byte-identity oracle was hardened to fail-closed (only whitelisted Tier-1 keys may be absent).

## Consequences

- The larger remaining byte savings (Tier-1/feed gating) are correctly deferred to a future rung
- The '81% waste' framing was row-count waste, not byte waste — the capstone measurement surfaced this distinction
- The module docstring, swap-justification comment, and oracle were all corrected to state proven numbers rather than aspirational ones
- The file-size gate `--changed-only` trap was a recurring failure mode; must always use `--from-ref origin/master --to-ref HEAD --baseline-ref origin/master`
- Two file-size hard-cap violations required extraction splits: update_envelope.rs (decode_typed_projections) and s6 harness (oracle + gates modules)

## Open Tail

- Tier-1/feed gating is the larger remaining prize for device-jank reduction
- The byte-identity oracle is end-state-only, not per-tick; a transient mid-window divergence that self-heals would pass
- The d13_part_a doctrine-lint test is a known order-dependent flake

## Evidence

- transcript lines 9424-9468
- transcript lines 9470-9488
- transcript lines 9552-9632
- transcript lines 9655-9693


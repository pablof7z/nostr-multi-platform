---
type: episode-card
date: 2026-06-14
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: architecture
status: superseded
subjects:
  - adr-0055-rung3
  - omit-unchanged
  - incremental-apply
  - drain-projections
supersedes:
  - 2026-06-14-1-adr-0055-omit-unchanged-drops-cleared
related_claims: []
source_lines:
  - 12237-12321
captured_at: 2026-06-14T08:49:50Z
---

# Episode: ADR-0055 omit-Unchanged Cleared-signal defect — flagged as blocker, not self-patched

## Prior State

Rung 3 S1 (#1388) introduced producer omit-Unchanged + incremental-apply capability. The `omit_unchanged` transform only iterates entries already present in the typed sidecar vector; drain/stage projections (action_results, signed_events, action_stages, action_lifecycle) are absent from typed when empty (gated by `!is_null()` in projections.rs), so a non-empty→empty transition produces a manifest entry of `Cleared` with no corresponding typed row.

## Trigger

Re-audit of just-merged ADR-0055 Rung 3 S1 found 9 confirmed findings (5 HIGH), all sharing the same root cause: `omit_unchanged` cannot synthesize a Cleared row for a key absent from typed, so incremental hosts receive no signal and permanently cache stale data (spinners never clear, sign-event continuations replayed, lifecycle overlay stuck).

## Decision

Flag-as-blocker rather than self-patch. `incremental_apply` is off-by-default (latent, zero live impact today). The fix mechanism — Cleared row in typed vs manifest tombstone vs other — is the ADR-0055 owner's active design choice since they are mid-shipping the Rung 3 seam (S2+ coming). Unilateral patching would collide with their ongoing work.

## Consequences

- #1390 filed as hard blocker: incremental_apply must not be enabled until the Cleared-signal path is resolved.
- Incremental hosts will permanently cache stale action_results/signed_events/action_stages/action_lifecycle until the owner fixes the emit path.
- The existing test suite (rung3_baseline_tests.rs) explicitly skips all four drain/stage keys, which is why the regression went uncaught.
- Classified as R7-#1364 playbook (flag-to-owner) rather than R8-K3 playbook (direct fix), because the defect is latent and the owner owns the seam.

## Open Tail

- Owner's S2+ design choice: how should a Cleared drain signal be delivered — typed row with empty payload, manifest-only tombstone, or other?
- Rung 3 S2 (#1389) merged during session (encoder buffer reuse) — lower regression risk, but the S1 blocker remains open.

## Evidence

- transcript lines 12237-12321


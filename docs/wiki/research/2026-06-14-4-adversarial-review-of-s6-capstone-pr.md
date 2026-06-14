---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 9552-9631
agent_attribution: Opus review R3-S5 capstone PR1413 subagent
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Adversarial review of S6 capstone (PR #1413); verdict REQUEST-CHANGES — metric swap is principled but docstring overclaims (~81%→<5% is false, real win is ~18% frame-byte + 68.8% row suppression), file-size violations, oracle only end-state not per-tick"
captured_at: 2026-06-14T11:50:54Z
---

Adversarial review of S6 capstone (PR #1413); verdict REQUEST-CHANGES — metric swap is principled but docstring overclaims (~81%→<5% is false, real win is ~18% frame-byte + 68.8% row suppression), file-size violations, oracle only end-state not per-tick

---

User: [Agent task result: Agent "Opus review R3-S5 capstone PR1413" completed]
(Notify endpoint returns 404 — server not running on 8888; nothing I can do about that.)

---

# Verdict: REQUEST-CHANGES

The empirical core of this PR is **honest and reproducible** — I reproduced every headline number across 5 runs and the metric swap is principled. But the deliverable ships with **two genuine hard-gate failures** (file-size) and **a stale module docstring that contradicts its own implemented gates**, which for a "never hand-wave the numbers" deliverable is itself a fudge-in-the-prose. These are fixable without touching the measurement; the numbers stand.

## RULING #1 (the big one) — Is the metric swap honest? YES, it is principled. Swap approved.

I instrumented the harness and decoded every Phase B row. **Full accounting of the 500 serialized rows** (per ~100 window ticks; deterministic, 1600→500 every run):

Per steady-state Phase B tick, ~5 rows are emitted:
| Row | Tier | Why it's on the wire | Necessary? |
|---|---|---|---|
| `claimed_profiles` | Tier-2 | genuinely churns (claim/release) | YES — Changed |
| `resolved_profiles` | Tier-2 | genuinely churns | YES — Changed |
| `relay_diagnostics` | Tier-2 | payload-fingerprint genuinely changes ~102/103 ticks (live counters) | YES — Changed |
| `claimed_event_embeds` | **Tier-1** (host-registered, no manifest entry) | always-Changed by D3-7; byte-identical 101/102 ticks | Necessary *by ADR boundary*, not by content |
| `nip46_onboarding` | **Tier-1** (host-registered) | always-Changed by D3-7; byte-identical 101/102 ticks | Necessary *by ADR boundary*, not by content |

100 ticks × ~5 ≈ 500. **No unchanged Tier-2 row leaks through.** Every Tier-2 row present is genuinely Changed (verified: the only byte-stable Tier-2 key, `relay_diagnostics`, is gated by a payload fingerprint and is legitimately changing). The two byte-stable always-emitted rows are `claimed_event_embeds` and `nip46_onboarding`, which are **Tier-1 host projections explicitly out of scope this rung (D3-7)** — they have no manifest entry, so the omit transform defaults them to Changed and never omits them. That is the declared boundary, not an omit bug. **No residual Tier-2 omission bug. The rung's Tier-2 mechanism works correctly.**

Now, what would the **original** gate `waste_ratio_incremental < 0.05` actually read on these 500 rows? **40%** (I confirmed: 101+101 = 202 byte-identical rows / 500 ≈ 0.40 — matches the harness's reported `waste_ratio_b = 40.0%`). That 40% is composed *entirely* of the two Tier-1 always-Changed projections. **You cannot drive it below 5% without gating Tier-1 — which is a future rung.** So the original gate was mis-specified for the post-omission world; it measures byte-stability of always-emitted rows, which Rung 3 by design does not address. **`row_suppression_ratio >= 0.50` (measured 0.6875) is the honest Rung-3 metric** — it directly measures "fraction of would-be-serialized rows that omission removed," which is exactly what the rung delivers. The swap reflects reality; it does not dodge a failing number.

**One accuracy nit (not blocking):** the code comment justifying the swap (s6, ~line 576) blames `relay_diagnostics` ("manifest-rev advanced but bytes identical") as the waste_ratio dominator. My probe shows that's the wrong culprit — `relay_diagnostics` is byte-identical only 1/103 ticks. The real dominators are the two Tier-1 keys. The conclusion is right; the named example is wrong. Fix the comment to name the actual cause.

## RULING #2 — The true win, honestly stated? MOSTLY yes in the PR body; NO in the source docstring.

The real win is exactly what you predicted:
- **Frame bytes: 9640 → 7928 B = −17.8%** (deterministic across all runs). NOT 81%, because Tier-1 feed-class rows dominate the byte budget and aren't gated.
- **Tier-2 row suppression: 68.8%** (1600 → 500 rows).
- The larger remaining prize (Tier-1 / feed gating, row-deltas) is a future rung.

The **PR body** states this honestly: it labels waste_ratio "informational," makes row_suppression the headline gate, and carries the D3-7 / codex-Q4 caveat ("Tier-1 feed gating is a later rung"). Good.

**But the harness's own module docstring overclaims and must be fixed** (this is the honesty defect in the deliverable):
- Line 4-5: "collapses Tier-2 serialization waste **from ~81 % to <5 %**" — false; measured Phase-B waste is 40%.
- Line 13: "Tier-2 waste **must drop to <5 %**" — the code does not assert this.
- Line 21: lists hard gate "**`waste_ratio_incremental < 0.05`**" — that gate was removed; the actual gate is `row_suppression_ratio >= 0.50`.

A reader trusting the file header would conclude waste fell to <5%, which the implementation deliberately abandoned because it's unachievable while Tier-1 is ungated. For a deliverable whose mandate is empirical truth, the harness contradicting its own gates is the one place I will not wave through. **Rewrite the docstring to match the implemented gates and the honest ~18% frame-byte / 68.8% row-suppression result.**

## RULING #3 — Serialize-time tolerance band. HONEST; band is not masking anything.

Two findings:
1. **The 20% band is not load-bearing here.** Across 5 runs: Phase B serialize_us p50 ∈ {57,58,59,60,61}, Phase A ∈ {59,60,60,63,65}. **Phase B is ≤ Phase A in every run** — even a strict `B ≤ A` gate would have passed. The band tolerates real cross-instance scheduling noise (±5µs observed) and is architecturally correct for a timing gate; it is honest, not "enough to pass."
2. **Does omission add CPU?** I checked the timer scope: `before_serialize` (update.rs:278) starts *before* `run_typed_projections` + manifest + rung2_stamp + `rung3_omit::omit_unchanged` (line 364) + the encode. So `serialize_us` **does** capture the omit-pass and Cleared-synthesis CPU — the gate is not blind to it. Empirically the omit cost is more than offset by encoding fewer rows, so Phase B is consistently faster. **No hidden encode-time regression. The band is defensible.**

## RULING #4 — Byte-identity oracle. REAL but WEAKER than claimed; two soft spots.

It is **not a tautology**: it merges Phase B's *omitted* stream through the D3-3 algorithm (Changed→insert, Cleared→remove, absent→retain) and compares to Phase A's *full* stream — two genuinely different producer code paths. I verified the end state: **all 16 keys present in both, zero mismatches, zero absences** — the reconstruction of the omitted stream is byte-identical to the full stream. That is a meaningful losslessness proof, and the absence-downgrade escape hatch is **never even exercised** in this run (no absences occur).

Two real weaknesses (both nits, not blockers, but the docstring lies about them):
- **(a) Not actually per-tick.** The module header (line 17-18) and inline comment claim byte-identity "**every tick**," but `run_byte_identity_oracle` compares **only the final reconstructed state vs Phase A's final frame**. The inline comment even admits it ("tick indices won't align... compares against Phase A's final"), and the "we additionally assert per-tick..." sentence describes code that **isn't implemented**. ADR §9 asked for equality "over the whole window." A transient mid-window divergence that self-heals by the last frame would pass. Either implement the per-tick check or correct the docstring to "end-state."
- **(b) The absence-downgrade is a latent future hole.** "Absent key → informational, not a failure" is sound today (two independent kernels, no absences observed), but it means a *future* omit bug that drops a needed Tier-2 row would be silently downgraded to PASS rather than failing, because the oracle can't distinguish "absent due to kernel nondeterminism" from "absent due to a bug." A stronger oracle would drive both phases from the *same* deterministic kernel state, or whitelist the known-nondeterministic keys and hard-fail on any other absence. Note for a follow-on; don't block on it.

## Build/test gates — ONE HARD FAILURE (blocking)

- `cargo test -p nmp-core` rung3 suite: **35/35 PASS** (including the negative `unconditional_key_changed_absent_panics_in_debug` and the §10.6 Cleared-signal regression tests). Gates are real hard failures (harness exits 2 on `--fail-on-gate`; I confirmed EXIT=0 on pass).
- `doctrine_lint_smoke`: 59/60; the one failure (`d13_part_a_positive_fixture_fires`) **passes in isolation** — it's an order-dependent filesystem race on a shared `target/doctrine_lint_d13_pos` temp dir, unrelated to ADR-0055. Not a PR regression, but flag it to the orchestrator's merge gate.
- **`check-file-size.sh --from-ref origin/master --to-ref HEAD --baseline-ref origin/master`: EXIT=1, TWO new hard-cap violations introduced by this PR:**
  - `crates/nmp-testing/bin/ffi-stress/s6_single_projection_churn.rs` — **684 LOC ≥ 500** (PR grew it +592; not in `.file-size-ignore` or baseline).
  - `crates/nmp-core/src/update_envelope.rs` — **508 LOC ≥ 500** (PR added the `serialize_us` field).

  This is squarely the recurring split-tax the repo memory and **ADR-0055 §5 itself** pre-warned about: §5 explicitly states `update_envelope.rs` "is exactly at the 499-LOC line and CANNOT take any net addition" and prescribes extracting `decode_typed_projections` *first*. The PR added the field without the prescribed extraction. The PR body claims "check-file-size.sh → exit 0," which **does not reproduce** — it exits 1. Per repo rule: split the files (never bump baseline, never ignore-list a hand-written harness). The s6 harness has obvious extraction seams (the oracle + `MiniProjectionCache`, the per-frame capture structs).

## Summary for the owner (the honest picture)

This rung's real, reproducible win is **an ~18% frame-byte reduction (9640→7928 B) and 68.8% Tier-2 row suppression (1600→500 rows)** on the claimed_profiles churn workload, with **zero data loss** (byte-identity verified end-state). The metric swap from `waste_ratio<0.05` to `row_suppression>=0.50` is **legitimate** — the old gate was unachievable by design because two always-emitted Tier-1 projections (`claimed_event_embeds`, `nip46_onboarding`) dominate the hash-waste metric, and Tier-1 gating is a later rung. **The larger remaining prize — Tier-1 / feed gating — is correctly deferred.** No omit-bug; the Tier-2 mechanism is correct.

**Do not merge** until: (1) the two file-size hard-cap violations are fixed by extraction (blocking); (2) the s6 module docstring is rewritten to match the implemented gates and the honest ~18%/68.8% result, dropping the false "<5% waste" and "every tick" claims (blocking — it's the deliverable contradicting its own numbers); (3) the swap-justification comment is corrected to name the real dominator (nit); (4) optionally strengthen the oracle to per-tick or fail-closed on unexpected Tier-2 absence (follow-on). Also surface the flaky `d13` doctrine-lint test to the merge gate (not this PR's fault).

Relevant files:
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a6c7622ab3cc18ff2/crates/nmp-testing/bin/ffi-stress/s6_single_projection_churn.rs` (docstring overclaim lines 1-24; oracle end-state-only `run_byte_identity_oracle`; swap comment ~line 576; **684 LOC — needs split**)
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a6c7622ab3cc18ff2/crates/nmp-core/src/update_envelope.rs` (**508 LOC — needs split**, §5 prescribed extracting `decode_typed_projections`)
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a6c7622ab3cc18ff2/crates/nmp-core/src/kernel/update.rs` (line 278 `before_serialize` scope confirms serialize_us captures omit cost; line 364 omit wiring; line 375 churn-measure-after-omit — the correct measurement fix)
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a6c7622ab3cc18ff2/crates/nmp-core/src/kernel/update/helpers.rs` (lines 112-183, the churn counters that drive suppression_ratio)
- `/Users/pablofernandez/Work/nostr-multi-platform/docs/decisions/0055-rung3.md` (§3 D3-7 honesty boundary; §5 file-size pre-plan the PR violated; §9 oracle "over the whole window" the impl narrowed to end-state)

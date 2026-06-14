---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 8290-8341
agent_attribution: Opus review R3-S1 PR1388
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Adversarial review of PR #1388 (R3-S1 producer omit-Unchanged): deviation is over-emit-only safe, but two file-size hard-cap violations block merge — REQUEST-CHANGES"
captured_at: 2026-06-14T09:01:47Z
---

Adversarial review of PR #1388 (R3-S1 producer omit-Unchanged): deviation is over-emit-only safe, but two file-size hard-cap violations block merge — REQUEST-CHANGES

---

User: [Agent task result: Agent "Opus review R3-S1 PR1388" completed]
The voice server returned 404 (notify endpoint not running locally), but I attempted it. Here is my verdict.

---

📋 SUMMARY: Adversarial review of PR #1388 (ADR-0055 Rung-3 S1, producer omit-Unchanged). The central `changed_since_last_emit` deviation is correct and over-emit-only; the PR ships two real file-size hard-cap violations that will fail CI.

🔍 ANALYSIS:

**The deviation is CORRECT and SAFE (over-emit only). This is the headline ruling.**

I traced it end to end:

1. **Claim is true.** `SourceVersions` derives `Default` (all `u64` fields = 0; source_versions.rs:35). `compute_rev` is a `saturating_add` SUM fold over a key's dependency counters (mod.rs:246-248), so an unmutated key returns rev=0. The old code `last_emitted.get(key).unwrap_or(0); current > last` evaluated `0 > 0 = false = Unchanged` for a never-emitted key at rev 0. With omission enabled, that key would have been wrongly dropped on the very first frame → genuine D3-5 violation. The fix (absent → `Changed`) is real and necessary.

2. **Direction of error: over-emission only — proven.** A row is omitted iff `presence == Unchanged`, which (absent a pending drain presence) requires the key to be **present** in `last_emitted` with `current <= last`. Because every counter is monotonic via `saturating_add` and `compute_rev` is a pure SUM, `current` can never decrease and two mutations can only *increase* the sum — there is no subtraction, no wrap (saturates at MAX, never to 0), no offsetting-to-0. Therefore `current <= last` ⟹ `current == last`. The only way a *changed* cache unit coexists with `current == last` is a forgotten source-version co-bump — the pre-existing Rung-1 StaleStamp class, which the oracle already guards and which this PR does not touch. The absent→Changed change can only force an *extra* full row on the first/baseline frame. It can never omit a genuinely-changed key. **No under-emission path exists.**

3. **rev=0 vs "emitted at rev 0" is correctly disambiguated** by `last_emitted` map presence/absence. Absent = never emitted = `Changed`; present-with-value-0 = emitted-at-0 = `Unchanged`. The new test `build_state_after_emit_at_rev0_is_unchanged` (tests_unit.rs) proves a key can sit at rev 0 forever and, after its first baseline emit, settles to `Unchanged`. **No perpetual byte leak** — it is emitted exactly once per baseline, not every frame.

4. **The Rung-1 biconditional oracle still bites.** It runs at update.rs:329 on the full, post-stamp, **pre-omit** typed set, so omission cannot blind it. Its baseline guard already skips the first observation of each key (oracle.rs:99), so absent→Changed doesn't weaken it. I ran the Rung-2 reviewer's bite test: I neutered the real `claimed_event_content` co-bump (claimed_event_stamp.rs:58) and `s1` failed as expected (rev did not advance), confirming the rev-discipline gate is live and load-bearing; restored immediately.

**The deviation does reveal a latent Rung-1 design gap** (rev=0 keys were classified Unchanged-when-never-emitted), but it is fixed correctly *in this PR* and does not need a follow-on — the `last_emitted` presence/absence semantics fully resolve it.

⚡ ACTIONS: Loaded ADR-0055 Rung-3 doc + Rung-1 manifest/oracle/source_versions; read every diff hunk; ran `cargo test -p nmp-core --lib` (1493 pass), `doctrine_lint_smoke` (60 pass), targeted rung3/build_state tests (14+2 pass); ran the oracle bite test; ran the file-size gate; scanned for banned tokens / shims / un-updated implementors.

✅ RESULTS: Correctness of the core change verified. omit transform, capability gate, baseline-on-attach/epoch, and FFI all verified sound. Two CI-blocking file-size regressions found.

📊 STATUS: **REQUEST-CHANGES** — not for correctness (the kernel logic is right) but for two hard-cap file-size violations the PR body falsely reports as "clean."

📁 CAPTURE:

**Finding 1 (BLOCKER — file-size hard cap).** `crates/nmp-core/src/kernel/snapshot_registry.rs` = **528 LOC ≥ 500 hard ceiling** (was 460 on master; PR added 68 lines). The PR body claims "File-size gate: clean (update.rs = 407…)" but only update.rs was checked; the implementer almost certainly used `--changed-only` (which repo memory explicitly warns passes locally, fails CI) or skipped the hard-cap pass. **Fix:** per repo doctrine (never baseline-bump; split god-files), extract the new incremental-apply methods (`declare_incremental_apply` / `is_incremental_apply_enabled` / `take_incremental_apply_baseline_pending`, plus the two fields' doc blocks) into the **existing** `crates/nmp-core/src/kernel/snapshot_registry/` submodule dir — the same pattern `kernel_access.rs` already uses for this file.

**Finding 2 (BLOCKER — baseline expansion).** `crates/nmp-core/src/kernel/mod.rs` = **2798 LOC > baseline 2797** (master is 2795, clean; PR added the 3-line `#[cfg(test)] mod rung3_baseline_tests;` declaration). **Fix:** do not bump `.file-size-baseline`. Declare the integration-test module without growing `kernel/mod.rs` — e.g. nest it under an existing `#[cfg(test)]` test-module aggregator, or fold its declaration into a sibling that already carries Rung-3 test mods. The ADR's own §5 set "keep mod.rs at baseline" as a hard constraint; this violates it by exactly the margin the doc warned about.

**Nit (non-blocking).** `omission_biconditional_oracle_omitted_iff_unchanged` (rung3_baseline_tests.rs:233) tests "all-Changed tick" then "all-Unchanged tick"; it never drives a *mixed* tick (one key changes, others don't) to assert the kernel omits exactly the unchanged ones while keeping the changed one in the same frame. The rung3_omit unit tests (`mixed_sidecar_filters_correctly`) cover the transform in isolation, so coverage exists — but a real-kernel mixed-tick assertion would strengthen the integration oracle. Optional.

**Verified clean:** over-emit-only safety; gate is single-writer, set-before-start (debug_assert on `!started`), lives next to ADR-0053 declared set, idempotent, NOT a shim (required trait method, all 3 implementors updated, hard-break compliant); FFI `nmp_app_declare_incremental_apply` mirrors `nmp_app_declare_consumed_projections` (app_ref lookup, null no-op, `#[no_mangle]`, exported); gate-read fails safe to `false`/full-rows on poison/no-slot (D6); baseline-on-attach AND bump_epoch both clear `last_emitted`; integration tests drive a real kernel via `make_update` + real snapshot slot (not mocked counters); test_helpers extraction is byte-identical zero-behavior; no banned tokens, no D0 violation, substrate-generic naming; non-advertising hosts get byte-identical Rung-2 behavior (omit returns `typed` verbatim when disabled).

➡️ NEXT: Send back to implementer to (1) split snapshot_registry.rs into its existing submodule dir and (2) relocate the rung3_baseline_tests module declaration off kernel/mod.rs, then re-run `./.githooks/check-file-size.sh --from-ref origin/master --to-ref HEAD --baseline-ref origin/master` (NOT `--changed-only`). The kernel logic needs no changes. Did not merge, did not push fixes.

📖 STORY EXPLANATION:
1. The riskiest line in this PR — treating a key absent from `last_emitted` as `Changed` — is correct: it fixes a real D3-5 gap where rev-0 keys would be wrongly omitted on the first frame.
2. I proved the change can only ever cause over-emission (an extra full row), never under-emission (a lost row), because revs are monotonic SUMs with no subtraction, no wrap, and no offsetting path.
3. The only way a changed projection could be wrongly omitted is a forgotten source-version co-bump — a pre-existing Rung-1 concern the completeness oracle already catches, and which this PR leaves fully guarded.
4. I confirmed the oracle still bites by neutering a real co-bump and watching the scenario test fail, then restored it.
5. The `last_emitted` map's presence-vs-absence cleanly distinguishes "never emitted" from "emitted at rev 0," so there is no perpetual byte leak — a stable rev-0 key emits once per baseline and then goes quiet.
6. The capability gate, FFI symbol, baseline-on-attach, and epoch reset are all sound, fail-safe, single-writer, and free of compat shims.
7. But the PR ships two file-size hard-cap violations — snapshot_registry.rs at 528 and kernel/mod.rs one line over baseline — that the PR body wrongly reports as clean and that will fail CI.
8. Verdict: the architecture is right and safe; bounce it only to split the two oversized files per the repo's own no-baseline-bump doctrine, then merge.

🎯 COMPLETED: Deviation is correct and over-emit-only; two file-size hard caps block merge.

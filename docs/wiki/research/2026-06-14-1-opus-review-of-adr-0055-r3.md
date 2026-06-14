---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 8837-8910
agent_attribution: opus-review-agent
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Opus review of ADR-0055 R3-S1b Cleared-signal completeness: found perpetual-Changed re-emission defect in note_copy_emit (presence.rs), verified other findings CORRECT/PASS, verdict REQUEST-CHANGES with 3 required fixes"
captured_at: 2026-06-14T11:33:28Z
---

Opus review of ADR-0055 R3-S1b Cleared-signal completeness: found perpetual-Changed re-emission defect in note_copy_emit (presence.rs), verified other findings CORRECT/PASS, verdict REQUEST-CHANGES with 3 required fixes

---

User: [Agent task result: Agent "Opus review R3-S1b PR1393" completed]
(Voice server not running in this env — non-blocking.)

---

# Adversarial Review — PR #1393 (ADR-0055 R3-S1b, Cleared-signal completeness, fix #1390)

## VERDICT: REQUEST-CHANGES

The core fix is sound and the regression test is genuinely non-vacuous (proven to fail on master). **However, the `note_copy_emit` implementation introduces a new perpetual-`Changed` re-emission byte leak for `action_stages` and `action_lifecycle` that did not exist on master.** It does not cause stale UI (the original blocker is correctly fixed), but it defeats a meaningful slice of the byte savings the entire ADR-0055 ladder exists to deliver, and it is a behavior regression vs. master. It should be fixed before this lands, since the whole point of the rung is to be correct before `incremental_apply` is enabled.

---

## (a) DECISIVE RULING — the regression test genuinely FAILS on master ✅ CONFIRMED with evidence

I checked out master `c6f3486f5` in a scratch worktree, dropped in the exact PR test file (`rung3_cleared_signal_tests.rs`), wired the `#[path]` include, and ran it. **All API signatures the test depends on exist on master, so it compiles and fails at assertion time** (not vacuously skipped):

| Test | Master result | Why |
|---|---|---|
| `action_results_cleared_on_drain_empty` | **FAILED** @ line 130 | master never synthesizes the drain-key Cleared row |
| `signed_events_cleared_on_drain_empty` | **FAILED** @ line 179 | same |
| `action_lifecycle_cleared_after_ttl_expiry` | **FAILED** @ line 298 | no Cleared row after TTL expiry |
| `action_stages_cleared_after_ack_of_last_entry` | **FAILED** (oracle `StaleStamp` panic) | finding-7 bug: ack doesn't bump rev |
| `action_stages_partial_ack_stays_changed` | **FAILED** (oracle `StaleStamp` panic) | partial-ack changes payload, no rev bump |
| `always_empty_key_never_produces_cleared_row` | passed on both | correct — spurious-clear guard is not a regression |

On the PR head all 6 pass. **The test is a real gate.** It exercises the full incremental path (real `make_update`, decode the actual wire frame via `decode_snapshot_typed_projections`), all four conditional keys, exactly-once (frame B Cleared → frame C absent), a simulated D3-3 host cache merge, and both negative cases. This is exactly what finding 9 demanded and the implementer's omission of the fail-on-master proof is now closed.

---

## (b) DECISIVE RULING — synthesis is exactly-once and spurious-free, BUT the steady-state path is NOT ⚠️ DEFECT FOUND

**Exactly-once on the clear edge: CORRECT.** Verified statically and by test: `record_emitted_for_manifest` (rung2_stamp.rs:63) iterates the full manifest and calls `record_emitted` for every key, advancing `last_emitted[cleared_key]` to the post-edge rev and removing the `pending_presence` override — so the next tick resolves to `Unchanged` and is not synthesized. Frame-C-absent assertions pass. The never-present key sits at `was_empty && !nonempty → Unchanged` forever (`always_empty` test passes). Synthesized row uses `projection_rev: ps.rev` (the manifest rev), payload empty, `state=Cleared` — matches §10.2 and the D3-3 merge contract.

**DEFECT (HIGH for the rung's goal, NOT a stale-UI bug): perpetual `Changed` re-emission for the copy-with-TTL keys.**

- **File:line:** `crates/nmp-core/src/kernel/projection_rev/presence.rs:106-107,117-118` (`note_copy_emit`, the `nonempty` arm) → reaches the wire via `presence_for` (line 129) and the forward pass in `rung3_omit.rs:75`.
- **Hazard:** `note_copy_emit` unconditionally parks `pending_presence[key] = Changed` whenever the tracker is non-empty. `presence_for` returns the parked value before the rev-vs-last-emit rule can settle. Because `action_stages`/`action_lifecycle` are *copies* that persist across ticks (unlike the true drains, which empty every tick), a stable in-flight action causes the **full payload to be re-emitted as `Changed` on every 4 Hz tick, forever, even when nothing changed.**
- **Evidence:** I added a steady-state probe (stable non-empty, no mutation, 5 ticks):
  - On the **PR head**: `action_stages` → `Changed` ×5; `action_lifecycle` → `Changed` ×4 (byte leak).
  - On **master**: the same probe passes — stable non-empty correctly settles to `Unchanged`/absent.
  This is a **PR-introduced regression** against master's behavior, on the very path the ADR exists to optimize. While any spinner/lifecycle overlay is showing (a common state), the incremental host gets no omit benefit for these two keys.
- **Why the design intended otherwise:** §10.4 specifies `nonempty → Changed (no extra bump — the rev already moved on enqueue/expiry)`. The intent is that steady-state non-empty is governed by the rev-vs-last-emit rule (Unchanged when the rev didn't move), and `note_copy_emit` only needs to inject the *Cleared edge*. The implementation over-reaches by parking `Changed` in `pending_presence` for the non-empty case, which suppresses settling.
- **Fix:** In `note_copy_emit`, do **not** write `pending_presence` for the `nonempty` arm — return `Changed` to the caller (informational) but leave the key to the rev-vs-last-emit rule so a genuinely-unchanged tick resolves to `Unchanged`. Park `pending_presence` ONLY on the `Cleared` edge (and only that edge needs the override). Then add a steady-state assertion to `rung3_cleared_signal_tests.rs` (stable non-empty → absent on ticks 2..n) so this is gated — that test would currently fail, mirroring the finding-9 discipline.
- **Caveat to validate when fixing:** the oracle `StaleStamp` currently passes for partial-ack *because* `pending_presence=Changed` masks the missing rev bump (action_stages `ack` bumps no source-version). If you stop parking `Changed`, partial-ack will re-trip the oracle. The correct resolution is to make `ack_action_stage` bump a source version (e.g. `settlement_enqueue_ver`) so the rev legitimately advances on a partial ack — which is the honest fix (content changed ⇒ rev must move), rather than masking it via a perpetual override. This keeps the oracle sharp and the partial-ack frame correctly `Changed` exactly once.

---

## Other findings

**Finding 5 (declare_incremental_apply → Result/i32): CORRECT.** Hard error replaces `debug_assert!` (lib.rs:1577 returns `Err(AlreadyStarted)` on `started`), return-code table 0/1/2/-1 matches Rust↔C, `NmpCore.h:355` return type changed to `int`, all callers updated (AppHost trait, nmp-ffi impl, nmp-defaults builder impl, GateSpy), no compat shim. The S6 harness (`s6_single_projection_churn.rs`) does not call it, so no breakage. **NIT (doc only):** `NmpCore.h` says code `1 — AlreadyStarted: called after incremental-apply was already declared` — inaccurate; the Rust trigger is post-`nmp_app_start`, and a repeat declare *before* start returns 0 (idempotent). Reword to "called after nmp_app_start."

**Finding 6 (lock coalesce): CORRECT, no race.** `incremental_apply_state` reads `is_incremental_apply_enabled` + `take_incremental_apply_baseline_pending` under one guard, same order and same consuming semantics as the prior two-lock code. Single actor thread; behavior-identical. Call site at update.rs:333 is clean.

**Finding 7 edge machine: mechanism CORRECT (modulo the (b) defect).** `note_copy_emit` is a faithful Cleared analog of `note_drain_emit`; called exactly once per emit per key from `action_stages_projection` (publish_cmd.rs:358) and `action_lifecycle_projection` (publish_cmd.rs:326). The only other call site (`substrate_adapters.rs:281`) is inside `#[cfg(test)] mod tests` — not a second production invocation. Production call chain ordering verified: accessors run in `build_snapshot_struct` (update.rs:275) → park `pending_presence` → `projection_manifest()` (update.rs:345) reads it → `omit_unchanged` (update.rs:371) synthesizes. Ack-of-last/TTL-expiry → Cleared confirmed by tests; partial-ack and double-ack do not emit Cleared (test `action_stages_partial_ack_stays_changed` passes). The `settlement_enqueue_ver` bump was correctly NOT added to `ack` (edge machine replaces it) — though see (b) caveat, the fix may need to add it for the partial-ack rev correctness. The double-bump of `ttl_expiry_ver` on TTL-expiry-of-last (prune bump + Cleared-edge bump) is harmless (rev is monotonic; distinguishability preserved).

**Synthesis predicate (findings 1/2/3/8): CORRECT.** All four branches verified in `rung3_omit.rs:103-155` and unit-tested in `rung3_omit_tests.rs` (cleared-absent synthesizes for all four keys; conditional Changed-absent belt; unconditional Changed-absent `debug_assert!`+`warn!`+no-synthesize; Tier-1 no-manifest-entry never iterated; unchanged-absent no-op; present-key not double-emitted via the `present_keys` HashSet). The owned-`String` HashSet to avoid the borrow conflict is fine.

**No debt / build / tests: PASS.** `cargo build -p nmp-ffi` OK; `cargo test -p nmp-core` = 1514 lib tests + integration/doc all green; `cargo test -p nmp-testing --test doctrine_lint_smoke` = 60 passed; file-size gate exit 0 (only soft-cap warnings; `projection_rev/mod.rs` 419 < 500 after the `presence.rs` extraction; nmp-ffi/lib.rs reduced vs baseline). The §10 extraction (`presence.rs`) is zero-behavior (verbatim move of `note_drain_emit`/`presence_for` plus the new `note_copy_emit`). Non-advertising hosts get byte-identical behavior — `omit_unchanged` early-returns on `!enabled` and `note_copy_emit` only affects the manifest presence, which only matters when omission is enabled (it changes nothing on the full-row path).

---

## Required before merge
1. Fix the perpetual-`Changed` re-emission in `note_copy_emit` (presence.rs) — don't park `pending_presence` on the non-empty arm; let the rev-vs-last-emit rule settle steady state. Add a steady-state regression assertion.
2. Make `ack_action_stage` bump a source version so partial-ack legitimately advances the rev (keeps the oracle sharp once the override is removed).
3. NIT: correct the `NmpCore.h` return-code-1 description ("after nmp_app_start").

Files central to the rulings (all absolute):
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a07e9a73159d34eac/crates/nmp-core/src/kernel/projection_rev/presence.rs` (the defect, lines 97-120)
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a07e9a73159d34eac/crates/nmp-core/src/kernel/publish_cmd.rs` (ack_action_stage:334, accessors:309/352)
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a07e9a73159d34eac/crates/nmp-core/src/kernel/update/rung3_omit.rs` (synthesis — correct)
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a07e9a73159d34eac/crates/nmp-core/src/kernel/rung3_cleared_signal_tests.rs` (regression gate — proven non-vacuous)

I did not push any fixes; report only. The scratch worktree at `/tmp/nmp-master-scratch` was removed and the PR worktree's test file was restored to its committed state.

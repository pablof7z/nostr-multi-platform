---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 8697-8772
agent_attribution: Design R3-S1b Cleared-signal fix
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Design of Cleared-signal completeness fix for #1390; manifest enumerates full key universe so fix is consumer-side (omit_unchanged), codex overturned finding-7 proposed fix"
captured_at: 2026-06-14T09:34:59Z
---

Design of Cleared-signal completeness fix for #1390; manifest enumerates full key universe so fix is consumer-side (omit_unchanged), codex overturned finding-7 proposed fix

---

User: [Agent task result: Agent "Design R3-S1b Cleared-signal fix" completed]
Voice server is not running (404), but I attempted the notification per protocol. Here is my report.

---

📋 SUMMARY: Designed the complete fix for #1390 (ADR-0055 Rung-3 Cleared-signal completeness), codex-reviewed, written as §10 of `docs/decisions/0055-rung3.md`. No production code written.

🔍 ANALYSIS: The decisive unknown is resolved with file:line evidence, and codex overturned the issue's own proposed fix for finding 7.

⚡ ACTIONS: Read the issue + ADR + the full producer/manifest/FFI/test surface; traced the manifest builder; got an adversarial codex review of the core design; appended the ADR resolution note.

✅ RESULTS / per-finding specs below.

📊 STATUS: Plan complete. ADR note written (uncommitted). No PR opened.

📁 CAPTURE: §10 of `0055-rung3.md` is the durable resolution; the regression test is the rung's correctness gate.

---

## The manifest-universe answer (the decisive unknown)

**The manifest enumerates the FULL key universe, NOT just rows present in `typed`.** Evidence:
- `crates/nmp-core/src/kernel/projection_rev/mod.rs:415` `build_manifest` iterates `KERNEL_BUILTIN_PROJECTION_KEYS` (the 18-key const at `update/projections.rs:27-46`), one `ProjectionState` per key, every tick — independent of `typed`.
- For the two true drain keys, presence flips to `Cleared` independent of typed-row insertion: `take_action_results_projection`/`take_signed_events_projection` (`publish_cmd.rs:228`) call `note_drain_emit(key, nonempty)` *unconditionally*; `note_drain_emit` (`mod.rs:266-290`) parks `pending_presence[key]=Cleared` on the `!nonempty && was_nonempty` edge, which `presence_for` (`mod.rs:333`) reads.

**So the finding's claim "the manifest correctly flips to Cleared" is CONFIRMED for the drain keys, and the fix lives in the consumer (`omit_unchanged`), NOT the manifest builder.** `omit_unchanged` (`rung3_omit.rs:44-72`) iterates `typed.into_iter()` only, so it never sees a manifest-Cleared key that has no typed row.

**Critical caveat that splits the findings:** the manifest flips to `Cleared` ONLY for the two true drain keys (only they run `note_drain_emit`). `action_stages`/`action_lifecycle` use the rev-vs-last-emit rule, so their non-empty→empty edge produces `Changed`/`Unchanged`, NOT `Cleared` — this is exactly why finding 7 needs more than a counter bump.

## The Cleared-synthesis contract (findings 1/2/3/8)

`omit_unchanged` gains an inverse pass: after the present-row filter, for each `manifest.states` entry NOT already in the output, synthesize `TypedProjectionData { key, state: WireProjectionState::Cleared, projection_rev: ps.rev, ..Default::default() }` (empty payload/schema metadata — the host's `Cleared` merge is `cache.remove(key)`, keyed on `key`, never decodes). `TypedProjectionData` derives `Default` and `WireProjectionState::Cleared` exists, so the literal compiles as written.

**Predicate (narrowed per codex):** `Cleared` → always synthesize; `Changed && key ∈ {action_results, signed_events, action_stages, action_lifecycle}` → synthesize (defensive belt, because for these four `Null` accessor ⟺ empty); `Changed`-but-absent for any other Tier-2 key → `debug_assert!`+`warn!`, never synthesize (preserves a sharp invariant so producer bugs aren't masked); Tier-1 keys have no manifest entry so are structurally un-synthesizable.

**Fires exactly once (no perpetual clear):** `record_emitted_for_manifest` (`rung2_stamp.rs:63`, called at `update.rs:381`) iterates the full manifest and advances `last_emitted` for every key including the Cleared one — next tick rev is unchanged → `Unchanged` → not re-synthesized. No change to `record_emitted` needed.

## Finding 7 — codex overturned the naive fix

The issue proposed "bump `settlement_enqueue_ver` in `ack_action_stage`." **Codex proved this is insufficient:** `action_stages` presence is rev-vs-last-emit, so a bump yields `Changed`, not `Cleared`, leaving a `Changed`-but-absent key the pure synthesis misses. The host keeps the stale stage.

**Correct fix:** give `action_stages`/`action_lifecycle` the same Cleared-edge machine the drains have — add `note_copy_emit(key, nonempty)` (analog of `note_drain_emit`) + a `copy_prev_nonempty` edge map, called once per emit inside `action_stages_projection` (`publish_cmd.rs:331`) and `action_lifecycle_projection` (`publish_cmd.rs:302`). Then ack-of-last-entry → next emit, `was_nonempty=true`, snapshot `Null` → `Cleared` presence → synthesis emits the row. The counter bump becomes unnecessary; `changed_since_emit=true` (already set) is enough to trigger the emit. The `Changed`-but-absent belt is the backstop.

## Other findings
- **5 (MED):** `declare_incremental_apply` → `Result`/i32 return-code (not `debug_assert`); update `AppHost` trait (`app_host.rs:134`), impl (`app_host_impl.rs:57`), C-ABI (`snapshot.rs:194`), `NmpCore.h:351`, harness callers. No compat shim.
- **6 (LOW):** coalesce the two `snapshot_projections` locks (`update.rs:323-326`) into one `incremental_apply_state() -> (bool, bool)` acquisition.
- **4 (MED):** resolved by R3-S3 interposer, not here. Contract confirmed compatible — synthesized Cleared rows are byte-identical to natural ones on the wire. Noted codex's host-side clear reorder-guard as a future-async belt for S3.

## Codex verdict
Sound-with-changes. Two material changes folded: finding 7 = edge machine not counter bump; predicate narrowed to `Cleared` always + `Changed&&conditional-key` belt + hard-assert on bad invariant.

## Regression test (finding 9 — fails on master)
New `rung3_cleared_signal_tests.rs`: drives the full incremental path with genuine non-empty→empty transitions on all four conditional keys (drains via `record_action_success`/`record_signed_event_return`; stages via `record_action_stage`+`ack_action_stage`; lifecycle via `FixedClock` TTL expiry), asserts a `Cleared` row appears exactly once, threads rows through a tiny in-test cache-merge stand-in to prove the cleared key leaves the simulated host cache, plus a spurious-clear negative test. **Fails on `c6f3486f5`, passes after the fix** — the existing tests skip exactly these four keys (`rung3_baseline_tests.rs:138/257/289`), which is why the cluster went uncaught.

## Files
- `docs/decisions/0055-rung3.md` — §10 added (the deliverable; uncommitted).
- Implicated production files (for the Sonnet implementation, not edited): `crates/nmp-core/src/kernel/update/rung3_omit.rs`, `crates/nmp-core/src/kernel/projection_rev/mod.rs`, `crates/nmp-core/src/kernel/publish_cmd.rs`, `crates/nmp-core/src/kernel/update.rs`, `crates/nmp-core/src/kernel/snapshot_registry/kernel_access.rs`, `crates/nmp-ffi/src/lib.rs`, `crates/nmp-ffi/src/snapshot.rs`, `crates/nmp-ffi/src/app_host_impl.rs`, `crates/nmp-core/src/substrate/app_host.rs`, `ios/Chirp/Chirp/Bridge/NmpCore.h`.

## File-size pre-plan
- `projection_rev/mod.rs` (453) → extract presence cluster to new `projection_rev/presence.rs` first (S1b-a opening commit).
- `rung3_omit.rs` (242) → move `#[cfg(test)] mod tests` to a sibling test file (crosses 300 warn otherwise; tests are not codegen-exempt).
- `publish_cmd.rs` (406), `update.rs` (430), `nmp-ffi/src/lib.rs` — net-neutral, no extraction.

➡️ NEXT: You commit the ADR note (or I can on request). Then dispatch a Sonnet implementer against the S1b-a…S1b-e ladder in §10.8, each step with its own gate; S1b-c is the fails-on-master regression gate.

📖 STORY EXPLANATION:
1. #1390 says enabling incremental_apply will silently freeze UI because conditionally-present projections that go empty never tell the host to drop their cache.
2. The decisive question was whether the manifest knows about a key after it goes empty — I confirmed via `build_manifest` that the manifest is the full key universe, so the Cleared info already exists; the consumer just ignores it.
3. So the core fix is an inverse pass in `omit_unchanged` that synthesizes a payload-less Cleared row for manifest-Cleared keys absent from the typed vector.
4. Codex confirmed this fires exactly once because `record_emitted` advances over the whole manifest, not just emitted rows.
5. Codex overturned finding 7's proposed counter-bump: `action_stages`/`action_lifecycle` aren't drain keys, so they need their own Cleared-edge state machine to make presence semantically correct.
6. The predicate was narrowed to Cleared-always plus a Changed-and-absent belt for exactly the four conditional keys, with a hard assert on any other anomaly so producer bugs aren't masked.
7. The findings 5/6 fixes harden the FFI gate to a real error and coalesce a double-lock; finding 4 is deferred to the host interposer with a confirmed-compatible contract.
8. The regression test drives the full path with real empty transitions and fails on master, making it the rung's correctness gate before incremental_apply is ever flipped on.

🎯 COMPLETED: Manifest is the full key universe; consumer fix plus stages edge machine, codex-verified.

---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 10435-10486
agent_attribution: Focused review R6-S2 PR1418
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "R6-S2 PR1418 review: four criteria (feed-refactor behavior-preserving, per-key freeze guard, publish ordering, capability-OFF byte-identical) evaluated via diff and test runs — verdict APPROVE-WITH-NITS"
captured_at: 2026-06-14T18:11:42Z
---

R6-S2 PR1418 review: four criteria (feed-refactor behavior-preserving, per-key freeze guard, publish ordering, capability-OFF byte-identical) evaluated via diff and test runs — verdict APPROVE-WITH-NITS

---

User: [Agent task result: Agent "Focused review R6-S2 PR1418" completed]
The voice endpoint returns 404 (server not running on this host) — noted; the review content follows.

---

📋 SUMMARY: Focused verification of PR #1418 (ADR-0055 R6-S2) — gating `claimed_event_embeds` + `nip46_onboarding` on the generalized byte-equality omit helper extracted from S1.

🔍 ANALYSIS:

**(a) Feed refactor is behavior-preserving — YES, proven.** I diffed the OLD `FeedEmissionState` (master `crates/nmp-nip01/src/op_feed/emission_state.rs`) against the new `TypedProjectionEmissionState` (`crates/nmp-core/src/projection_emission.rs`). The struct fields (`last_emitted`, `emit_rev`, `last_identity`, `incremental_apply_enabled`) and the entire `should_emit` body are logic-identical: identity-change reset-first, then `*last == payload` exact memcmp (NOT a hash), capability-OFF always-emit, monotonic `emit_rev`. Only doc comments changed. The nip01 file is now a 43-line thin re-export with `pub type FeedEmissionState = TypedProjectionEmissionState;`. Decisive proof: all **27 original S1 emission tests pass unchanged** through the alias, including the cardinal `c10_reset_new_session_id_forces_baseline_not_omit` and `c11_epoch_change_identical_bytes_forces_baseline_not_omit`. No semantic drift.

**(b) Both keys have a working Reset/session freeze guard — YES, per-key, proven.**
- `claimed_event_embeds` (`crates/nmp-ffi/src/embed_sidecar.rs:126-149`): wraps producer in `Arc<Mutex<TypedProjectionEmissionState>>`, reads `(session_id, snapshot_epoch)` from `app.frame_identity_handles()`. Per-key freeze tests pass: `claimed_event_embeds_c1_freeze_guard_session_id_change_forces_baseline` and `_c2_freeze_guard_epoch_change_identical_bytes_forces_baseline` (`embed_sidecar_emission_tests.rs`). Both drive byte-identical-payload-after-identity-change and assert MUST-emit — they would fail a naive no-rebaseline impl.
- `nip46_onboarding` (`crates/nmp-core/src/actor/mod.rs:1782-1838`): same wrapping, reads handles from the registry lock (`reg.frame_identity_handles()`). Its trap coverage rides the generic `projection_emission_tests.rs` (`c6`/`c7` freeze guards pass). There is no per-key freeze test file for `nip46_onboarding` analogous to the embed one — see NIT 2.

**(c) Q3 fresh-identity ordering — CORRECT.** `crates/nmp-core/src/kernel/update.rs` calls `publish_frame_identity()` at line 273 BEFORE `run_typed_projections()` at line 293 within `make_update`. Both keys register via `register_typed`/`register_typed_snapshot_projection` → the same `run_typed()` path, so both read fresh identity each tick. No stale-identity/freeze path.

**(d) Capability-OFF byte-identical, exact equality, no duplication — CONFIRMED.** Capability-OFF tests pass for both keys (`c4` / `claimed_event_embeds_d1`). Exact `==` memcmp, no hash. Exactly one omit implementation (the re-export proves it). D6 poison fallback is safe: actor builds `AtomicBool::new(false)` (off ⇒ always emit) on poisoned registry lock; both producers degrade to always-emit on poisoned emission-state lock.

**Scope discipline — CLEAN.** Diff touches only 8 Rust files in nmp-core / nmp-ffi / nmp-nip01. No iOS/Android/gallery changes (host decoders + ProjectionCache retain already exist from S1).

⚡ ACTIONS: Diffed old-vs-new emission impl; verified handle APIs exist on both `NmpApp` and the registry; confirmed `publish_frame_identity` ordering; ran `cargo test -p nmp-nip01` (S1 tests), `-p nmp-core`, `-p nmp-ffi`, `doctrine_lint_smoke`, and the file-size gate.

✅ RESULTS:
- `nmp-nip01`: all green; 27 S1 emission tests pass unchanged.
- `nmp-core`: 17 generic emission tests pass (incl. c6/c7 freeze guards).
- `nmp-ffi`: 113 tests pass (incl. per-key embed freeze guards c1/c2).
- `doctrine_lint_smoke`: 60 passed.
- **file-size gate: EXIT 1 — FAILS.**

📊 STATUS: Mechanism is sound and behavior-preserving; one hard CI blocker (file-size) plus two nits.

📁 CAPTURE:
- **BLOCKER (file-size gate, exit 1):** `crates/nmp-core/src/lib.rs` is a HARD-cap expansion — 608 LOC > frozen baseline 602 (`.file-size-baseline:56`). The PR added `pub mod projection_emission;` + the 4-line `pub use projection_emission::{FrameIdentity, TypedProjectionEmissionState};` re-export block, growing an already-over-cap file. Per the owner's standing rule (memory: file-size-gate-ci-command, feedback_always_right_never_smallest), the baseline must NOT be bumped — `lib.rs` must be brought back to ≤602 (e.g., trim/relocate an equivalent number of re-export lines into a submodule). `actor/mod.rs` at 2461 is fine (under its 2482 baseline). Soft-cap warnings on the two test files + embed_sidecar.rs are non-blocking.
- **NIT 1:** Unused import `wire::ContentTreeWire` at `crates/nmp-ffi/src/embed_sidecar_emission_tests.rs:25` (line 87 uses the fully-qualified `nmp_content::wire::ContentTreeWire::default()`). Harmless warning today (no `-D warnings` in CI), but should be removed.
- **NIT 2:** `nip46_onboarding` has no dedicated per-key emission test mirroring `embed_sidecar_emission_tests.rs`. Its freeze behavior is covered generically, but the prompt's "per-key" bar is met by integration test only for the embed key. Optional: add a small `nip46_onboarding` integration freeze test for symmetry.

➡️ NEXT: Dispatch a Sonnet fix agent to (1) shrink `lib.rs` back under 602 without bumping the baseline, and (2) drop the unused import. Re-run the file-size gate to confirm exit 0. Do not merge until the gate is green.

📖 STORY EXPLANATION:
1. The fundamental constraint here is the trap-proof omit: compare the exact bytes the host receives, never a hash, so a missed change-signal is structurally impossible.
2. S2's core move is a generalization — the feed-specific `FeedEmissionState` became `nmp-core::TypedProjectionEmissionState`, with nip01 keeping only a type alias.
3. I verified that generalization is byte-for-byte behavior-preserving by diffing the old and new `should_emit` and by running all 27 S1 tests unchanged through the alias.
4. Both new keys wrap the shared state in `Arc<Mutex<…>>` and read the same `(session_id, snapshot_epoch)` FrameIdentity the feed uses, so they rebaseline in lockstep with the host's cache `removeAll()`.
5. The freeze guard — the cardinal R6-S1 fix — is exercised per key: identical-bytes-after-Reset and after-epoch-change both assert MUST-emit, the exact tests a naive impl fails.
6. The producers read fresh identity because `publish_frame_identity` runs before typed projections within `make_update`; there is no stale-identity path.
7. The one real defect is mundane but binding: adding the module re-export pushed `lib.rs` six lines over its frozen file-size baseline, failing the gate — and the house rule forbids bumping the baseline.
8. Verdict: the architecture is correct and timeless; merge is gated only on a mechanical file-size split plus a trivial unused-import cleanup.

🎯 COMPLETED: APPROVE-WITH-NITS; mechanism sound but lib.rs file-size gate fails, must split.

**Verdict: APPROVE-WITH-NITS** — the S2 generalization is behavior-preserving (S1 tests green, no semantic drift) and both keys have working, per-key-tested freeze guards. **Do not merge until the `crates/nmp-core/src/lib.rs` file-size hard-cap failure (608 > baseline 602) is fixed by splitting, not by raising the baseline.** Also remove the unused import at `crates/nmp-ffi/src/embed_sidecar_emission_tests.rs:25`. Reporting only — not merging.

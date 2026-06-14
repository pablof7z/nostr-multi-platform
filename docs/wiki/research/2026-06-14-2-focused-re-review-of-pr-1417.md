---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 10312-10359
agent_attribution: ab57e89885690a16c
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Focused re-review of PR #1417 freeze fix verifying two pre-registered kill criteria (Reset freeze closure, new missed-emit edges); verdict APPROVE after empirical falsification of c10 test pre-fix"
captured_at: 2026-06-14T15:00:36Z
---

Focused re-review of PR #1417 freeze fix verifying two pre-registered kill criteria (Reset freeze closure, new missed-emit edges); verdict APPROVE after empirical falsification of c10 test pre-fix

---

User: [Agent task result: Agent "Focused re-review R6-S1 freeze fix" completed]
📋 SUMMARY: Focused re-review of the freeze-fix wiring on PR #1417 — the Reset freeze is closed and I found no new freeze/missed-emit edge in the frame-identity publish/read ordering.

🔍 ANALYSIS:

**VERDICT: APPROVE.** Lead answer to the two kill questions:
1. **Is the Reset freeze closed? YES — empirically proven.** I temporarily reverted the identity key to the pre-fix epoch-only logic and ran the suite: `c10` FAILS at the freeze-guard assertion (`emission_state_tests.rs:461`, "byte-identical payload after a session_id change MUST emit"). With the full two-axis `FrameIdentity(session_id, snapshot_epoch)` key restored, c10 passes. The test genuinely guards the Reset freeze the prior review found.
2. **Any NEW freeze/missed-emit edge in publish/read ordering? NO.** Walked the first-tick, first-post-Reset-tick, and dead-handle paths explicitly (below). All are freeze-safe.

**1. Freeze closed + HostCacheSim models both axes.** `HostCacheSim.apply` (`emission_state_tests.rs:52`) resets on `self.identity != Some(identity)` — i.e. session_id OR snapshot_epoch — mirroring `ProjectionCache.generated.swift`'s two-axis `removeAll()`. c10 covers the session_id axis (Reset), c11 covers the epoch axis (account-switch with identical bytes). Pre-fix, c11 already passed (epoch was the old key) but c10 failed — exactly the gap the fix targets.

**2. No publish/read ordering gap.** `Kernel::publish_frame_identity()` is the FIRST mutating call in `make_update` (`update.rs:273`), BEFORE `build_snapshot_struct` (285, generic closures) and `run_typed_projections` (293, the feed producer). Lock-free Release stores; closure reads Acquire. Tick sequences:
   - **First real tick:** publish runs first using `started_unix_ms` (set in `start()`, which runs before the first tick via the Start command arm) → closure reads the REAL `(session_id, epoch)`, not (0,0). `last_identity=None` → establishes identity AND emits (last_emitted=None). Correct baseline. No (0,0)→real double-read within a tick.
   - **Pre-flight `make_update`** (`actor/mod.rs:1509`) uses a bare `Kernel::new` with NO snapshot_projections slot → producer closure never runs, publish is a no-op. Producer state is not poisoned.
   - **Worst-case (make_update before start):** session_id would be 0, then 0→real next tick forces a baseline EMIT — the safe direction, never an omit-into-cleared-cache.

**3. Reset survival — confirmed, no dead handle.** The `Arc<AtomicU64>` handles live in `SnapshotRegistry`. On Reset, `dispatch.rs:1294` calls `take_snapshot_projection_handle_for_reset()` (`= self.snapshot_projections.take()` — moves the SAME Arc) and `dispatch.rs:1343` re-installs that SAME Arc onto the rebuilt kernel via `set_snapshot_projection_handle`. The registry instance (and its frame-identity atomics) is byte-identical before/after Reset. The surviving feed closure captured clones of those exact atomics at registration; the rebuilt kernel's `publish_frame_identity` writes the new session_id into the SAME atomic the closure reads → change detected → forced baseline. No dead handle → no re-freeze.

**4. Arc<AtomicBool> single source of truth — no divergence.** Producer's clone (`incremental_apply_handle` → `Arc::clone(&self.incremental_apply_enabled)`) and the kernel-side `incremental_apply_state()` (`update.rs:343`) read the SAME atomic. Flag is set-before-start, single-writer, so within-tick read order (closure at 293 vs kernel at 343) is irrelevant. Capability-OFF path is byte-identical to today (always emit). Poisoned-registry fallbacks return fresh `false`/zero handles → producer always emits (full rows) — freeze-safe in the safe direction.

⚡ ACTIONS: Read producer logic + tests + kernel publish path + Reset arm + actor wiring; empirically falsified c10 against pre-fix logic then reverted cleanly; ran all gates.

✅ RESULTS:
- Kill criterion proven: c10 FAILS pre-fix, PASSES with fix; c11 covers epoch axis.
- `nmp-nip01` + `nmp-defaults`: all green (emission_state 27/27).
- `nmp-core`: 1532 passed, 0 failed.
- `doctrine_lint_smoke`: 60 passed.
- File-size gate: EXIT 0 (lib.rs reduced 2976→2933 via extract; snapshot_registry.rs 499 under hard cap; no new hard-cap items).
- Engine `nmp-feed/` diff empty; zero iOS/Android/gallery changes.

📊 STATUS: Review complete. PR #1417 fix verified sound. Worktree `/private/tmp/wt-r6s1-feed-signal` left clean (temp edit reverted, HEAD `4f22242e5`). Did NOT merge — report only, per instructions.

📁 CAPTURE: ADR-0055 R6-S1 freeze fix keys producer rebaseline on `FrameIdentity(session_id, snapshot_epoch)` — the same two-axis signal the host ProjectionCache resets on. Reset survival works because `take/set_snapshot_projection_handle_for_reset` move the SAME `Arc<Mutex<SnapshotRegistry>>` (holding the frame-identity atomics) onto the rebuilt kernel (`dispatch.rs:1294/1343`); the surviving closure shares those exact atomics. `publish_frame_identity` at top of `make_update` (`update.rs:273`) guarantees in-tick read freshness.

➡️ NEXT: Safe to land. Optional nits (non-blocking): none material. If desired, a follow-up could add an actor-level integration test driving a real ActorCommand::Reset through the FFI to complement the unit-level HostCacheSim proof, but the wiring is verified by code-trace and the survival contract mirrors the established event-observer/queue-depth handle pattern.

📖 STORY EXPLANATION:
1. The prior review found a frozen-feed-on-Reset bug: the producer omitted unchanged frames into a host cache the Reset had just cleared, leaving a blank timeline.
2. The fix keys the producer's rebaseline decision on a `FrameIdentity(session_id, snapshot_epoch)` tuple — the exact two axes the host's ProjectionCache resets on.
3. I empirically proved the fix matters by reverting to the old epoch-only key: test c10 fails at the freeze-guard line; restoring the fix makes it pass.
4. The kernel publishes the identity at the very top of `make_update`, before any projection closure runs, so every closure reads this tick's real values — never a stale (0,0).
5. The first-tick and worst-case start orderings only ever err toward EMIT (a baseline), never toward an unsafe omit.
6. On Reset, the same registry Arc — holding the frame-identity atomics — is moved onto the rebuilt kernel, so the surviving closure and the new kernel share the same atomics; no dead handle, no re-freeze.
7. The capability flag is a single `Arc<AtomicBool>` read identically by the producer and the kernel; poisoned-registry fallbacks fail safe to full rows.
8. All gates pass (nmp-core 1532, nip01/defaults green, doctrine lint 60, file-size exit 0, zero engine/shell drift) — APPROVE, do not merge.

🎯 COMPLETED: Reset freeze closed, no new freeze path; approve PR 1417.


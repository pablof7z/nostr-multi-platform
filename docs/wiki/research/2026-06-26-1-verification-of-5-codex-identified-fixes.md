---
type: research-record
date: 2026-06-26
session: ae3e7b5b-75e8-4018-8d1a-ce05f7d4654a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ae3e7b5b-75e8-4018-8d1a-ce05f7d4654a.jsonl
source_lines: 1171-1200
agent_attribution: aae5b1e7cd9b97351
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Verification of 5 codex-identified fixes (deadlock/UAF/D6 bugs, residual reframing, file-size splits) with empirical test results"
captured_at: 2026-06-26T12:18:00Z
---

Verification of 5 codex-identified fixes (deadlock/UAF/D6 bugs, residual reframing, file-size splits) with empirical test results

---

User: [Agent task result: Agent "Fix UniFFI app-loop review findings" finished]
All 5 codex-review fixes are applied and the commit amended (now `1934c9ffc`, single clean commit on `feat/2129-android-uniffi-apploop`).

## Summary

**FIX 1 — close_updates() deadlock (BLOCKER):** Replaced the lock-holding `close_updates_locked` with a 3-phase lock-free teardown — extract `app` under a brief `state` lock, release, run the blocking quiescence with `state` unlocked, then reacquire to clean up. A re-entrant in-flight `on_update` can no longer deadlock.

**FIX 2 — lock-free quiescence UAF (BLOCKER):** Added a `callback_mutation_guard: RwLock<()>` to `Session`. Lock-free callers take the read lock before touching the raw `*mut NmpApp`; `free_native` takes the write lock exclusively before `nmp_app_free`, so a free can never run while a set_update_callback/reregister is mid-flight. Kept lock-free w.r.t. `Session.state` (no D8 polling).

**FIX 3 — D6 init (BLOCKER):** Replaced the `debug_assert_eq!` on `nmp_app_chirp_register` with a runtime check that frees `app` and returns `Err`, so `new()` falls back to the inert handle. No panic on any init path.

**FIX 4 — staged residuals:** Removed all "TRANSITIONAL/convenience" framing from `dispatch_action_json`/`dispatch_intent_json`; reworded as LEGACY STAGED RESIDUALS pointing to #2145 (M14-1). Kotlin call sites annotated `// staged: see #2145 (M14-1)`. No verb migration, no codegen-registry changes.

**FIX 5 — file-size splits (all ≤500 LOC):**
- `session.rs` 613→424 (`CallbackState` + tests extracted to submodules)
- `uniffi_app_loop.rs` 654→395 (tests extracted)
- `KernelBridge.kt` 534→468 (8 signer methods → new `KernelBridgeSignerActions.kt`)
- Generated `nmp_android_ffi.kt` (2017) exempted via `.file-size-ignore`

## Verification
- Crate tests: 33/33 pass (5 new concurrency/UAF-guard tests)
- Doctrine lint smoke: 98/98 pass
- UniFFI drift gate: OK (bindings regenerated)
- File-size gate: none of the lane's files flagged
- Commit is a single clean commit; co-author trailer present
- Not pushed, no PR (as instructed)

## One open item to flag
The agent reports the Android Gradle `:app:assembleDebug` fails on a `close()` naming conflict in the generated `nmp_android_ffi.kt`, and verified this same failure exists in the original commit `f26fdd074` before any of these changes — so it is **pre-existing**, not introduced here. Since this is the FFI lane, you may want a follow-up to confirm/track that generated-binding `close()` collision (likely belongs in the #2145 lane or a separate bindgen-template fix) before the Android build can go green.


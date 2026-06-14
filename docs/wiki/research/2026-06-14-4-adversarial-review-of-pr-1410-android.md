---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 9248-9347
agent_attribution: Opus review R3-S4 PR1410
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Adversarial review of PR #1410 Android ProjectionCache interposer — verdict APPROVE-WITH-NITS; D3-4 decode-before-commit parity confirmed, decodeSucceeds divergence illusory"
captured_at: 2026-06-14T10:36:55Z
---

Adversarial review of PR #1410 Android ProjectionCache interposer — verdict APPROVE-WITH-NITS; D3-4 decode-before-commit parity confirmed, decodeSucceeds divergence illusory

---

User: [Agent task result: Agent "Opus review R3-S4 PR1410" completed]
📋 SUMMARY: Adversarial review of PR #1410 (Android ProjectionCache interposer, ADR-0055 R3-S4) — verdict APPROVE-WITH-NITS.

🔍 ANALYSIS: The headline `decodeSucceeds=isNotEmpty()` divergence is NOT a real D3-4 violation; the iOS "real decoder" preflight provides no stronger guarantee against non-empty corrupt payloads, by the iOS test authors' own admission.

⚡ ACTIONS: Ran codegen `--check` (default + kotlin), `--rerun-tasks` gradle JVM tests, doctrine lint, file-size gate; read both platforms' cache + apply + decoder code and traced the corrupt-payload path end-to-end.

✅ RESULTS: All gates green — codegen exit 0 (both), gradle 208 pass / ProjectionCacheTest 13/13 / 0 fail, doctrine 60 pass, file-size exit 0, gallery untouched.

📊 STATUS: Mergeable. Two NITs (one init-time leak, one missing-test ask), zero blockers.

📁 CAPTURE: ADR-0055 R3 Android interposer is the structural twin of iOS #1409; both platforms commit non-empty-but-corrupt payloads (FlatBuffers `getRoot` is unchecked) — the decode-before-commit guarantee is effectively "reject empty payload only" on BOTH, and self-heal via fail-closed re-decode. The iOS `decodeSucceeds` per-key real-decoder switch is theater for the non-empty case.

➡️ NEXT: Optional follow-ups before/after merge (none blocking): add an Android corrupt-non-empty test mirroring iOS test 12; fix the init-leak; cosmetic warning cleanup.

---

## HEADLINE RULING (#1): `decodeSucceeds = isNotEmpty()` — D3-4 IS honored end-to-end on Android. Not a blocker.

The implementer's report framed this as a semantic divergence from iOS. After tracing both platforms, **the divergence is illusory**. Here is the definitive reasoning.

**The iOS "real decoder" preflight does NOT reject non-empty corrupt payloads.** The iOS typed decoders use unchecked FlatBuffers `getRoot` (a trusted in-process FFI boundary). The iOS test file `ios/Chirp/ChirpTests/ProjectionCacheTests.swift:317-327` states this explicitly:

> "arbitrary non-empty garbage bytes do NOT reliably round-trip to `nil` (an out-of-range offset just yields an empty/default-valued struct)... a fuzzed-garbage assertion would be vacuous under unchecked getRoot."

Test 12 (lines 480-489) even constructs `payload: Data([0x00])` — a non-empty garbage byte labeled `corrupt`, state `.changed` — and on iOS `decodeSucceeds` returns **true** for it (non-nil default struct), so iOS *also* commits it and advances the rev. The ONLY deterministic failure both platforms catch is the **empty-payload Changed row**, which `isNotEmpty()` catches identically. So Android's floor is semantically equal to iOS's effective floor; the iOS per-key decoder switch is stronger only in theory, never in the reproducible path.

**Corrupt-payload end-to-end trace (Android), both paths:**
- *Commit path* (`ProjectionCache.kt:200`): non-empty corrupt bytes → `decodeSucceeds` true → committed, rev advanced, `needsResync` NOT latched. (Same as iOS.)
- *Re-decode path* (`KernelUpdateFrameDecoder.kt:204 decodeProjections` → e.g. `TypedWalletDecoder.decode(bytes)` at `TypedWalletDecoder.kt:62`): every Android typed decoder checks the FlatBuffers `file_identifier` (`WalletStatusBufferHasIdentifier`) AND wraps the parse in `try/catch` returning **null** on any throw. Result for that slot: **answer (a) fail-safe** — projection drops to its `?: null/empty` default, **no crash**, and **self-heals** on the next good rev. It is NOT a crash (c). It can transiently default-blank that one slot (a soft (b)), but that is identical to iOS's user-visible outcome for the same non-empty-garbage input.

**Conclusion:** `isNotEmpty()` is an ACCEPTABLE Android realization of D3-4. The no-corrupt-UI / no-stale guarantee holds to the same degree as iOS, and the fail-closed `try/catch` + identifier-check in every typed decoder is the proven-equivalent protective path. The "uniform `decodeBytes()` per-key probe" the generator comment defers (`kotlin_projection_cache.rs:88-90`, `let _ = entries;`) is a legitimate future cleanup, not a merge blocker.

---

## JOB 2 — Build + tests (empirical, in the PR worktree)
- `cargo run -p nmp-codegen -- gen projection-cache --check` → **exit 0**
- `cargo run -p nmp-codegen -- gen projection-cache --platform kotlin --check` → **exit 0** (generated Kotlin matches registry)
- `./gradlew :app:testDebugUnitTest -x cargoNdk --rerun-tasks` → **BUILD SUCCESSFUL** (25 tasks executed, not cached). Kotlin COMPILES. **208 tests pass, 0 fail/0 error**; `ProjectionCacheTest` **13/13**. Matches the implementer's claim.
- `cargo test -p nmp-testing --test doctrine_lint_smoke` → **60 passed, 0 failed**
- `check-file-size.sh --from-ref origin/master --to-ref HEAD --baseline-ref origin/master` → **exit 0**

(Note: my first gradle run reported UP-TO-DATE/cached; I re-ran with `--rerun-tasks` to genuinely execute and confirm.)

---

## JOB 3 — Semantic parity (Android `ProjectionCache.kt` vs iOS `ProjectionCache.generated.swift`)
Line-for-line, the merge algorithm is identical:
- Rebaseline: `ProjectionCache.kt:170-176` clears cache + sets session/epoch + resets `baselined`/`needsResync` BEFORE the row loop — atomic, matches Swift 162-168.
- Changed reorder-guard: `:194` `incomingRev <= cached.rev → continue` = Swift 184.
- Cleared: `:183-188` removes key + adds to changedKeys = Swift 175-179.
- Omitted/Unchanged: absent from list → retained (no-op) = Swift.
- `sessionId == 0` pass-through, no omission trust: `:163-166` = Swift 156-159.
- `baselined` gates UI; `changedKeys` = committed-Changed ∪ Cleared exactly: `:208, :187` = Swift.
- `sessionId`/`snapshotEpoch` threaded from the **single** decode: `KernelUpdateFrameDecoder.kt:130-131` reads `snapshot.sessionId`/`snapshot.snapshotEpoch` off the same `SnapshotFrame` root — **no second buffer parse** (the mistake caught on iOS). Confirmed.

The only divergence is the `decodeSucceeds` floor, ruled acceptable above.

---

## JOB 4 — Wiring, host-apply, tests, debt
- **`KernelModel.decodeUpdate` (`KernelModel.kt:476-513`)**: feeds raw envelopes to `projectionCache.merge`, then re-decodes the **merged** set via `decodeProjections`. Because the merged set reinstates cached bytes for omitted keys, **omitted projections KEEP their prior value — the #1390 finding-4 analog is avoided.** The frame-level monotonic guard `decoded.rev <= _state.value.rev` (`applyFrame:152`) is preserved. `needsResync`/`changedKeys` are surfaced (logged). No always-update slot is wrongly gated — Android rebuilds the whole `projections` object every frame and relies on StateFlow value-equality dedup (a legitimate Compose-vs-SwiftUI structural difference; `changedKeys` is computed but used only for logging on Android, which is fine).
- **`nmp_app_declare_incremental_apply` (`lib.rs:62-77`)**: called after `declare_consumed_projections`, before `nmp_app_start`, int return checked; nonzero returns handle `0`, which is the dead-handle sentinel → all subsequent JNI calls no-op → init effectively aborts (fail-closed). The `eprintln!` is acceptable (no log crate in this crate). **Ruling: nonzero DOES abort init.** ✔ — see NIT-1 below for the leak.
- **`decodeProjections` → `internal`**: minimal visibility widening (same-module reach from `KernelModel`), not a public leak. ✔
- **`ProjectionCacheTest.kt` (13 tests): REAL, not vacuous.** Every test builds genuine FlatBuffers frames via `FlatBufferBuilder` (`accountsBuffer`/`activeAccountBuffer`) and asserts decoded payload bytes, `changedKeys`, `needsResync`, and session-change-with-rows atomicity (`sessionChangeResetsCache`). The empty-payload D3-4 case is covered (test 6). **Gallery untouched** (`git diff --stat apps/nmp-gallery/` empty). Generated Kotlin clean; file-size gate exit 0.

---

## Findings (file:line)

**NIT-1 (real, low severity) — `app` pointer leaked on the declare-incremental-apply error path.** `crates/nmp-android-ffi/src/lib.rs:67-77`: when `nmp_app_declare_incremental_apply` returns nonzero, the function `return 0` *before* `Session::new(app, chirp)` is constructed (line 90). Since `nmp_app_free` is only invoked via `Session::free_native` (`session.rs:196`), the `app` allocated at line 53 is never freed. Practically unreachable (the comment correctly notes only AlreadyStarted/RegistryUnavailable are possible here, neither of which can occur pre-start on a fresh registry), so it's a NIT, not a blocker. Fix: call `nmp_app_free(app)` before the early `return 0`. (iOS has no analog because it has no equivalent raw-pointer early-return.)

**NIT-2 (test-coverage ask, low severity) — corrupt-non-empty case untested on Android.** `ProjectionCacheTest.kt` test 6 covers EMPTY payload only. iOS test 12 (`Data([0x00])`) at least exercises a non-empty corrupt byte through the cache. Recommend adding an Android test that commits a non-empty corrupt payload and asserts the documented fail-closed behavior (slot defaults via `decodeProjections`, no crash, self-heals on next good rev) — this pins the headline guarantee against future regression. Not blocking given the parity ruling.

**NIT-3 (cosmetic) — redundant null-safety warnings.** `KernelUpdateFrameDecoder.kt:288` (unnecessary `?.let` on non-null `ByteBuffer`) and `:292` (Elvis always returns left operand). Pre-existing-style noise, compiler warnings only, harmless. Optional cleanup.

---

📖 STORY EXPLANATION:
1. The reviewer's stated worry was that Android's `decodeSucceeds` uses a bare `isNotEmpty()` check instead of the iOS per-key typed-decoder preflight, risking a D3-4 violation where corrupt UI reaches real users.
2. I traced the corrupt payload through both Android paths: the commit path accepts any non-empty bytes, but the re-decode path runs every typed decoder inside a `try/catch` with a FlatBuffers identifier check, so a corrupt slot fail-closes to its default and self-heals on the next good rev — no crash, no permanent stale.
3. The decisive evidence is in the iOS test file itself: its authors concede that FlatBuffers `getRoot` is unchecked, so non-empty garbage does NOT reliably fail the iOS decoder either — meaning iOS commits the same corrupt bytes Android does.
4. Therefore the only deterministic failure both platforms catch is the empty-payload Changed row, which `isNotEmpty()` catches identically; the divergence collapses and D3-4 is honored to the same degree on both platforms.
5. Empirically, every gate is green: both codegen `--check` runs exit 0, a forced (non-cached) gradle run passes 208 tests with ProjectionCacheTest 13/13, doctrine lint passes 60, and the file-size gate exits 0.
6. The merge algorithm in `ProjectionCache.kt` is line-for-line semantically identical to the Swift twin — rebaseline atomicity, reorder guard, Cleared handling, session-zero pass-through, and changedKeys precision all match, with session/epoch read from the single decode pass (no second buffer parse).
7. Android's whole-`projections`-rebuild model differs structurally from iOS's per-slot `changedKeys` gating, but because the merged envelope set reinstates cached bytes for omitted keys, omitted projections never blank — the finding-4 regression is avoided, and StateFlow value-equality handles dedup.
8. Verdict APPROVE-WITH-NITS: two low-severity NITs (an unreachable init-time `app` leak on the declare-incremental-apply error path, and a missing corrupt-non-empty Android test) plus a cosmetic warning; none block merge. I did not merge or push fixes.

🎯 COMPLETED: PR 1410 approve with nits, D3-4 honored, gates all green.

Relevant file paths:
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a31fe464c81932032/android/app/src/main/java/org/nmp/android/ProjectionCache.kt`
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a31fe464c81932032/android/app/src/main/java/org/nmp/android/KernelUpdateFrameDecoder.kt`
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a31fe464c81932032/android/app/src/main/java/org/nmp/android/KernelModel.kt`
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a31fe464c81932032/crates/nmp-android-ffi/src/lib.rs` (NIT-1 at lines 67-77)
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a31fe464c81932032/crates/nmp-codegen/src/kotlin_projection_cache.rs`
- `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a31fe464c81932032/android/app/src/test/java/org/nmp/android/ProjectionCacheTest.kt`
- `/Users/pablofernandez/Work/nostr-multi-platform/ios/Chirp/ChirpTests/ProjectionCacheTests.swift` (lines 317-327 and 480-489 — the load-bearing concession that iOS does not reject non-empty garbage either)
- `/Users/pablofernandez/Work/nostr-multi-platform/ios/Chirp/Chirp/Bridge/Generated/ProjectionCache.generated.swift`


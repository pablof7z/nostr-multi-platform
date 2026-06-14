---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 9023-9104
agent_attribution: opus
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Adversarial review of PR #1409 empirically built iOS (xcodebuild failed — missing FlatBuffers import), verified merge algorithm D3-4 correctness and apply-gating; verdict REQUEST-CHANGES"
captured_at: 2026-06-14T10:26:22Z
---

Adversarial review of PR #1409 empirically built iOS (xcodebuild failed — missing FlatBuffers import), verified merge algorithm D3-4 correctness and apply-gating; verdict REQUEST-CHANGES

---

User: [Agent task result: Agent "Opus review R3-S3 PR1409" completed]
The HTTP notify endpoint is down (404), but I used the `say` fallback per the user's global instruction. Here is the review.

---

📋 SUMMARY: Adversarial review of PR #1409 (ADR-0055 R3-S3 iOS ProjectionCache interposer) — the empirical gate FAILED.

🔍 ANALYSIS: The iOS app does not compile. The merge algorithm itself is correct, the tests are real, and the host-apply gating is sound — but a missing import in KernelBridge.swift hard-breaks the build, so incremental_apply can never run on Chirp.

⚡ ACTIONS: Ran `cargo run -p nmp-codegen -- gen projection-cache --check` (exit 0); built `nmp-core` + `nmp-app-chirp` for `aarch64-apple-ios-sim` (exit 0); ran `xcodegen generate` + `xcodebuild build-for-testing` against the booted iPhone 16 sim (FAILED); extracted the compiler diagnostics; read all changed Swift + the codegen template + the ADR §3/§4 + the 13 tests.

✅ RESULTS: Found one blocking build break, plus a should-fix architecture defect in the same block, plus nits.

📊 STATUS: REQUEST-CHANGES. Do not merge.

# Verdict: REQUEST-CHANGES

## (a) Empirical result — LEAD finding

- **Generated file in sync:** `cargo run -p nmp-codegen -- gen projection-cache --check` → **exit 0** ("ok"). The committed `ProjectionCache.generated.swift` matches the registry. PASS.
- **Rust iOS-sim build** (`nmp-core` lmdb + `nmp-app-chirp` marmot, `aarch64-apple-ios-sim`): **exit 0**. PASS.
- **iOS app build (`xcodebuild build-for-testing`): FAILED.** Two hard compile errors:
  - `ios/Chirp/Chirp/Bridge/KernelBridge.swift:735:26: error: cannot find 'ByteBuffer' in scope`
  - `ios/Chirp/Chirp/Bridge/KernelBridge.swift:736:55: error: cannot find 'getRoot' in scope`
- **ChirpTests / Typed*DecoderTests: NOT RUN** — the test bundle can't build because the app target doesn't compile.

The implementer's caveat was correct to worry about: this PR was never built on iOS. It cannot have been. This alone is the automatic REQUEST-CHANGES.

### Root cause (file:line)

`KernelBridge.swift` imports only `Darwin`, `Foundation`, `os.log` (lines 1-3) — it has **no `import FlatBuffers`**. The new "lightweight re-read of the buffer" block the implementer added inside `decodeFlatBuffer` uses FlatBuffers symbols:

```
ios/Chirp/Chirp/Bridge/KernelBridge.swift:735   var tmpBuf = ByteBuffer(data: data)
ios/Chirp/Chirp/Bridge/KernelBridge.swift:736   let tmpFrame: nmp_transport_UpdateFrame = getRoot(byteBuffer: &tmpBuf)
```

`ByteBuffer` / `getRoot` live in the `FlatBuffers` module (every other file that touches them — `KernelUpdateFrameDecoder.swift`, the generated decoders — imports it). Adding `import FlatBuffers` to KernelBridge.swift makes it compile, but see the should-fix below for why that's the wrong fix.

## (b) Decode-before-commit correctness (D3-4) — CORRECT

`ProjectionCache.generated.swift:180-204`. On a `.changed` row the typed decoder runs as a preflight via `decodeSuceeds` (line 190); cache + rev advance and `changedKeys.insert` happen **only inside the success branch** (lines 191-198). On failure: `needsResync = true`, prior entry untouched, key NOT in changedKeys (lines 200-201). There is no code path where a decode failure blanks a value or advances rev. Adversarial cases all hold:
- Decode failure on the first/baseline frame → key absent-and-retained-as-default (not a blank of an existing value), per ADR D3-4. Correct.
- Cleared for an uncached key → `removeValue` is a no-op but the key still enters `changedKeys` (line 178-179) so the slot is nilled idempotently. Fine.
- Same session/epoch, backwards rev → reorder guard `incomingRev <= cached.rev` skips (line 184). Correct.
- Rebaseline (D3-5): session/epoch change clears cache + resets `baselined`/`needsResync` **before** the row loop (lines 162-168), atomically. Correct. `session_id == 0` early-returns pass-through without trusting omission (lines 156-159). Correct. `baselined` is exposed read-only and flips true only after a real merge.
- `changedKeys` is exactly the committed-Changed ∪ Cleared set — verified by tests 8 and 11 and by reading the two `.insert` sites. Not over/under-populated.

## (c) apply-only-changedKeys keeps omitted slots without starving always-update slots — CORRECT

`KernelModel.apply` (KernelModel.swift:778-825): every projection-keyed `@Published` slot is now guarded by `ck.contains(...)` (the #1390 finding-4 fix). Frame-level/non-keyed slots are correctly left ungated: `typedEnvelope` (line 822, carries per-tick rev/metrics/logs), `flatFeeds` (825), `lastErrorToast` (828). No always-update slot is wrongly gated behind a projection key. The stores (`marmot`/`groupChat`/`dmInbox`/`followList`/`discoveredGroups`, lines 861-895) are fed `result.typed*` unconditionally — and those values are re-decoded from the **merged** envelope set in KernelBridge, so they retain correct values even when the projection is omitted this frame. Tier-1 `nmp.feed.home` is in `typed_projections` and is always-Changed, so it lands in `changedKeys` every relevant frame and `typedHomeFeed` updates (KernelModel.swift:785) — verified the key is routed through the cache, not a separate path.

## Should-fix (architecture, same block as the break) — `feedback_always_right_never_smallest`

`KernelBridge.swift:730-738`: the double-decode. `decodeFlatBuffer` already decoded the snapshot once via `KernelUpdateFrameDecoder.decode` (which holds `frame.snapshot`), then re-runs `getRoot` on the same `Data` a second time purely to read two scalars. The comment admits this ("we do not have access to the snapshot reader here … a lightweight re-read of the buffer"). This is an O(buffer) extra parse on every 4Hz frame, and the *reason* the FlatBuffers import leaked into a file that was deliberately FlatBuffers-free. The right fix is to thread `sessionId`/`snapshotEpoch` out of the single existing decode: add them to the `.snapshot(...)` case in `KernelUpdateFrameDecoder` (next to `schemaVersion`) and pass them into `cache.merge`. That removes the second parse AND the spurious import in one move. Just adding `import FlatBuffers` would make it compile but bakes in the perf debt and the layering smell — not the right change.

## Nits

- `ProjectionCache.generated.swift:93` / template `swift_projection_cache.rs:175`: `decodeSuceeds` is misspelled ("Succeeds"). It's a `private static`, so harmless, but fix it in the template (it's the single source). Not a copy-paste-elsewhere indicator — I checked; the misspelling exists only in this generator.
- Tests are **13** functions, not 12 (the doc comment lists 10 areas; `testEpochChangeClears` and `testMultiKeyFrameChangedKeysAreExact` are extras). All real, none vacuous — they build genuine KACC/KACT FlatBuffers, drive `merge`, and assert decoded `npub`/`pubkey`, `changedKeys` membership, `baselined`, and `needsResync`. Test 6 directly proves no-blank-on-decode-failure; Test 4 proves the reorder guard. Coverage gap (not blocking): no test exercises a frame that *simultaneously* changes session/epoch AND carries rows (the atomic reset-before-merge ordering is only covered with empty envelopes). Worth adding once the build is fixed.

## Files

- Blocking: `ios/Chirp/Chirp/Bridge/KernelBridge.swift:735-738` (missing FlatBuffers symbols; should be fixed by carrying session/epoch out of the existing decode rather than re-parsing).
- Generated merge (correct): `ios/Chirp/Chirp/Bridge/Generated/ProjectionCache.generated.swift`.
- Template (nit): `crates/nmp-codegen/src/swift_projection_cache.rs:175`.
- Host apply (correct): `ios/Chirp/Chirp/Bridge/KernelModel.swift:778-828`.
- Tests (real): `ios/Chirp/ChirpTests/ProjectionCacheTests.swift`.

I reverted the `xcodegen`-induced churn in `ios/Chirp/Chirp.xcodeproj/project.pbxproj`. I did not merge and did not push any fixes.

📖 STORY EXPLANATION:
1. The gating job was empirical: prove the iOS app builds and the Swift tests pass — the implementer hadn't verified either.
2. The codegen `--check` passed (exit 0) and both Rust iOS-sim crates built cleanly, so the producer side and registry sync are fine.
3. But `xcodebuild build-for-testing` failed with two hard errors in KernelBridge.swift: `ByteBuffer` and `getRoot` cannot be found in scope.
4. The cause is a missing `import FlatBuffers` — the implementer added a "re-read the buffer" block to a file that deliberately had no FlatBuffers dependency.
5. Because the app target can't compile, the test bundle can't build, so ProjectionCacheTests and the existing decoder tests never ran.
6. The merge algorithm itself, read against the ADR, is correct: decode-before-commit never blanks or advances rev on failure, rebaseline is atomic, changedKeys is exact.
7. The host-apply gating correctly keeps omitted slots while leaving frame-level slots (envelope, flatFeeds, error toast) always-updated — the #1390 finding-4 fix is sound.
8. Verdict is REQUEST-CHANGES: fix the build the right way (thread session/epoch out of the single existing decode, not a second parse), plus a one-character template typo nit.

🎯 COMPLETED: Request changes — iOS build fails, KernelBridge missing FlatBuffers import.

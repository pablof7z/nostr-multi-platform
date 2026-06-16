# ADR-0055 Rung 3 — Implementation Surfaces and S6 Gate

Extracted from [`0055-rung3.md`](0055-rung3.md) to keep each hand-authored ADR file under the repository file-size ceiling. This addendum preserves the concrete implementation surface and capstone measurement gate for Rung 3.

## 8. Files touched (enumerated)

**Rust (`nmp-core` / `nmp-ffi` / `nmp-testing`):**
- `crates/nmp-core/src/kernel/update.rs` — wire `rung3_omit` call (net-neutral;
  see §5).
- `crates/nmp-core/src/kernel/update/rung3_omit.rs` — **new**, omission transform.
- `crates/nmp-core/src/kernel/update/test_helpers.rs` — **new (conditional)**,
  extract `make_update_*_for_test` to buy `update.rs` headroom.
- `crates/nmp-core/src/kernel/projection_rev/mod.rs` — clear `last_emitted` on
  `declare_incremental_apply` / `bump_epoch`; expose changed-key iteration.
- `crates/nmp-core/src/kernel/snapshot_registry.rs` (+ `entry.rs`) —
  `incremental_apply_enabled` flag next to declared set.
- `crates/nmp-core/src/substrate/app_host.rs` — `declare_incremental_apply`.
- `crates/nmp-core/src/update_envelope/tier3_frame.rs` — builder reuse.
- `crates/nmp-core/src/update_envelope.rs` — aux encoder builder reuse (net-neutral).
- `crates/nmp-ffi/...` — `nmp_app_declare_incremental_apply` symbol.
- `crates/nmp-codegen/src/...` — Swift + Kotlin `ProjectionCache` generators
  (sourced from the existing projection registry).
- `crates/nmp-testing/bin/ffi-stress/s6_single_projection_churn.rs` — incremental
  on/off measurement + PASS/FAIL gate.

**Swift (`ios/Chirp`):**
- `Bridge/KernelUpdateFrameDecoder.swift` — `TypedProjectionEnvelope` gains
  `projectionRev` + `state`; populate them.
- `Bridge/Generated/ProjectionCache.generated.swift` — **new (generated)**.
- `Bridge/KernelBridge.swift` — run cache-merge before the decoder family; surface
  `needsResync`.
- `Bridge/KernelModel.swift` — assign only changed slots; reset cache on
  `resetAndRestart`.
- `Bridge/KernelUpdateTypes.swift` — `KernelUpdateResult` gains `changedKeys` +
  `needsResync`.
- `ChirpTests/ProjectionCacheTests.swift` — **new**.

**Kotlin (`android/app`):**
- `org/nmp/android/KernelUpdateFrameDecoder.kt` — `TypedProjectionEnvelope` gains
  `projectionRev` + `state`; populate them.
- `org/nmp/android/ProjectionCache.kt` — **new (generated)**.
- `org/nmp/android/KernelModel.kt` — cache reset on session/epoch change; surface
  needsResync.
- `org/nmp/android/ProjectionCacheTest.kt` — **new**.

**Gallery (`apps/nmp-gallery/android`):** **NONE** — see §6. Explicitly do not
regenerate the curated subset.

**Docs:** this file; `0055-pr-ladder.md` (Rung 3 → landed); `aim.md` §10 +
Doctrine #12 (lands with the capstone).

---

## 9. The S6 before/after measurement procedure (the capstone gate)

1. **Before (already measured, baseline):** run `ffi-stress` scenario S6 with the
   kernel in default mode (no `declare_incremental_apply`). Record
   `window_projections_serialized`, `window_projections_changed`,
   `waste_ratio` (~0.81), and `p50/p99` frame bytes. This is the existing Rung-0
   number.
2. **After:** in the same harness run, construct a second `NmpApp`, call
   `nmp_app_declare_incremental_apply()` before the seed phase, then drive the
   identical claim/release churn window. Record the same metrics.
3. **PASS/FAIL:**
   - `waste_ratio_incremental < 0.05` (Tier-2 waste collapsed) — **hard gate**.
   - `p50_frame_bytes_incremental < p50_frame_bytes_baseline` — **hard gate**.
   - `serialize_us` p50 incremental ≤ baseline (no encode-time regression) —
     **hard gate**.
   - Byte-identity oracle: the merged host-side projection set under incremental
     apply == the full-frame projection set, over the whole window — **hard gate**
     (this is the self-healing/correctness proof, run in test-support).
4. The harness prints all four to the report `measurements` JSON; the PR body
   quotes the before/after table. The S6 report `notes` line is updated to label
   the number "Tier-2 / claimed_profiles churn" (Tier-1 feed gating is a later
   rung — codex Q4 honesty).

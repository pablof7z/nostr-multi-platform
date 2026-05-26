# M14 — UniFFI migration

> Part of the [Build & Validation Plan](../plan.md). Arc 3 — WoT + cross-platform + release (M12 Wallet deferred post-v1).

**Demo product:** iOS app, podcast app, and (incoming) Android/Desktop/Web shells all bind to the kernel via generated bindings produced by `nmp gen modules`, not raw C FFI. Runtime update payloads use the canonical FlatBuffers schema; UniFFI owns object lifecycle, callback registration, and capability interfaces.

**Scope.** Replace the current raw C FFI surface in `crates/nmp-core/src/ffi.rs` with the per-app generated `nmp-app-<name>` crate per ADR-0010. The iOS app stops importing `NmpCore.h` and instead imports the generated Swift module. This milestone does **not** make UniFFI the hot payload format: `AppUpdate` frames remain FlatBuffers, and there is no JSON runtime fallback.

**Subsystem deliverables.**

- `nmp-codegen` extended to produce UniFFI scaffolding in the generated per-app crate.
- `apps/chirp/nmp-app-chirp` and `apps/fixture/nmp-app-fixture` as the first real per-app crates.
- `xcframework` build pipeline for each per-app crate.
- Generated Swift wrappers: `useProfile`, `@Profile`, `useTimeline`, `@Wallet`, etc.
- Generated FlatBuffers readers/writers for the canonical `AppUpdate` schema used by Swift/Kotlin/TS shells.
- CI gate: `nmp gen modules --check` fails the build if bindings drift.

**Exit gate.**

- iOS app builds and runs against UniFFI-generated bindings; no raw C FFI in the app target.
- The runtime update stream is FlatBuffers-only for app shells; legacy JSON is limited to documented migration/test tooling and diagnostics.
- Cross-platform consistency test (next milestone) is unblocked because the FFI shape is now identical across platforms.
- Codegen determinism: repeated runs produce byte-identical output.
- Platform debounce dispatch-rate gate (deferred from M1 per T22): with the M14 generated wrapper (ADR-0005 refcounted component wrapper) in place, validate that mount/unmount churn at 1000/sec for 60 s produces dispatch rate (OpenView + CloseView combined) ≤ 60% of mount rate. The grace-period absorption (30 s by default) is what makes this gate green; without it the wrapper is doing pass-through and the dedup belongs purely to the kernel.

**Runnable artifact.** iOS Twitter + iOS Podcast apps both using UniFFI. Report in `docs/perf/m14/uniffi-migration.md`.

# M14 — UniFFI migration

> Part of the [Build & Validation Plan](../plan.md). Arc 3 — WoT + cross-platform + release (M12 Wallet deferred post-v1).

**Demo product:** iOS app, podcast app, and (incoming) Android/Desktop/Web shells all bind to the kernel via generated host bindings (`nmp gen swift` / `nmp gen typed-decoders`), not raw C FFI. Runtime update payloads use the canonical FlatBuffers schema; UniFFI owns object lifecycle, callback registration, and capability interfaces.

> **ADR-0046 update.** The original scope referred to a per-app `nmp-app-<name>` generated
> crate via `nmp gen modules`. That generator was deleted. Composition is now a library call
> (`nmp-defaults::register_defaults`); the M14 milestone is now about host-binding codegen
> (`gen swift` / `gen typed-decoders`) replacing raw C/JNI FFI, not about generating a wiring
> crate. `apps/fixture/nmp-app-fixture` referenced below does not exist.

**Scope.** Replace the current raw C/JNI lifecycle/action FFI surface in `crates/nmp-ffi` with the UniFFI-generated host bindings. The iOS app stops importing `NmpCore.h` and instead imports the generated Swift module. This milestone does **not** make UniFFI the hot payload format: `AppUpdate` frames remain FlatBuffers, and there is no JSON runtime fallback.

**Subsystem deliverables.**

- `nmp-codegen` extended to produce UniFFI scaffolding via `gen swift` / `gen typed-decoders`.
- `apps/chirp/nmp-app-chirp` as the reference per-app crate demonstrating the library composition model.
- `xcframework` build pipeline for `nmp-app-chirp`.
- Generated Swift wrappers: `useProfile`, `@Profile`, `useTimeline`, `@Wallet`, etc.
- Generated FlatBuffers readers/writers for the canonical `AppUpdate` schema used by Swift/Kotlin/TS shells.
- CI gate: `just gen bindings` + diff check fails the build if bindings drift.

**Exit gate.**

- iOS app builds and runs against UniFFI-generated bindings; no raw C FFI in the app target.
- The runtime update stream is FlatBuffers-only for app shells; legacy JSON is limited to documented migration/test tooling and diagnostics.
- Cross-platform consistency test (next milestone) is unblocked because the FFI shape is now identical across platforms.
- Codegen determinism: repeated runs produce byte-identical output.
- Platform debounce dispatch-rate gate (deferred from M1 per T22): with the M14 generated wrapper (ADR-0005 refcounted component wrapper) in place, validate that mount/unmount churn at 1000/sec for 60 s produces dispatch rate (OpenView + CloseView combined) ≤ 60% of mount rate. The grace-period absorption (30 s by default) is what makes this gate green; without it the wrapper is doing pass-through and the dedup belongs purely to the kernel.

**Runnable artifact.** iOS Twitter + iOS Podcast apps both using UniFFI. Report in `docs/perf/m14/uniffi-migration.md`.

# M14 — UniFFI migration

> Part of the [Build & Validation Plan](../plan.md). Arc 3 — WoT + cross-platform + release (M12 Wallet deferred post-v1).

**Demo product:** iOS app, podcast app, and (incoming) Android/Desktop/Web shells all bind to the kernel via UniFFI-generated bindings produced by `nmp gen modules`, not raw C FFI.

**Scope.** Replace the current raw C FFI surface in `crates/nmp-core/src/ffi.rs` with the per-app generated `nmp-app-<name>` crate per ADR-0010. The iOS app stops importing `NmpCore.h` and instead imports the generated Swift module.

**Subsystem deliverables.**

- `nmp-codegen` extended to produce UniFFI scaffolding in the generated per-app crate.
- `apps/twitter/nmp-app-twitter` and `apps/podcast/nmp-app-podcast` as the first two real per-app crates.
- `xcframework` build pipeline for each per-app crate.
- Generated Swift wrappers: `useProfile`, `@Profile`, `useTimeline`, `@Wallet`, etc.
- CI gate: `nmp gen modules --check` fails the build if bindings drift.

**Exit gate.**

- iOS app builds and runs against UniFFI-generated bindings; no raw C FFI in the app target.
- Cross-platform consistency test (next milestone) is unblocked because the FFI shape is now identical across platforms.
- Codegen determinism: repeated runs produce byte-identical output.

**Runnable artifact.** iOS Twitter + iOS Podcast apps both using UniFFI. Report in `docs/perf/m14/uniffi-migration.md`.

// `nmp.toml` app-manifest parser — consumed by `nmp doctor` / `nmp upgrade`
// to read and bump an app's NMP dependency policy. (ADR-0046 deleted the
// Rust-shell module *generator* that also read this manifest; the manifest
// model itself survives because the dependency-policy commands still need it.)
mod manifest;
// Shared `--check` diff-line reporting for the Swift codegen gates — keeps
// `check_swift` / `check_typed_decoders` reporting consistent and ensures a
// length-only mismatch never masquerades as a missing file.
mod diff_report;
// V6 Stage 1 — Swift `Decodable` emitter pilot. Consumes the JSON document
// `nmp-core --features codegen-schema --bin dump_projection_schemas` writes,
// emits one Swift file with one struct per pilot type. See
// `docs/architecture-audit/v6-codegen-plan.md` §6b.
//
// NOTE (ADR-0046 — "composition is a library, not a generator"): the former
// Rust-shell module-scaffolding generator (`generate` / `ffi_gen` /
// `workspace`) and its `apps/fixture` test consumer were deleted. A generated
// FfiApp never called `register_defaults` and was a non-functional Nostr app
// (the create-react-app-eject anti-pattern). Composition is now a library —
// `nmp-defaults::register_defaults` — so `nmp-codegen` emits ONLY the
// consumer-side Swift artifacts (KernelTypes + typed projection decoders) that
// gate live CI.
pub mod swift;
// V6 Stage 2 — dotted-projection-key registry for `SnapshotProjections` +
// `CodingKeys`. Hand-transcribed from the existing Swift declaration in
// `ios/Chirp/Chirp/Bridge/KernelBridge.swift`; the renderer in `swift.rs`
// appends `SnapshotProjections` to the generated file using this slice.
// Lives in `nmp-codegen` (D0-exempt) so the registry can name dotted host
// keys like `"nmp.nip29.group_chat"` without tripping doctrine-lint on
// `nmp-core`. See module doc for the full rationale.
pub mod swift_projections_registry;
// V6 Stage 4 (consumer-side) — generated typed-FlatBuffer-sidecar decoders.
// Reads `SnapshotProjectionEntry::typed_sidecar` and emits, per projection key
// with a checked-in `flatc --swift` reader binding, the mechanical
// lookup+decode scaffold (the reader→Chirp-domain mapping stays the
// hand-written `TypedProjectionGlue` seam). Foundation for switching Chirp's
// consumer off the JSON `payload` path. See module doc for the generated /
// hand-written seam rationale.
pub mod swift_typed_decoders;
// ADR-0055 R3-S3 — generated `ProjectionMergeCache` (iOS). Sourced from the
// SAME projection registry as `swift_typed_decoders` so the cache can never
// drift from the decoder set. Implements the D3-3 merge algorithm +
// decode-before-commit (D3-4) so app code is oblivious to delta mechanics.
pub mod swift_projection_cache;
// ADR-0055 R3-S4 — generated `ProjectionMergeCache` (Android/Kotlin). Sourced
// from the SAME projection registry as `swift_projection_cache` so the cache
// is byte-for-byte semantically identical to the iOS implementation. Generates
// `android/app/src/main/java/org/nmp/android/ProjectionCache.kt`.
pub mod kotlin_projection_cache;
// ADR-0053 / Workstream-E4 — projection-tier classification + the codegen-derived
// kernel built-in projection key set (derived from `swift_projections_registry`).
pub mod projection_tier;
// ADR-0053 / Workstream-E4 — generator for `nmp-core`'s
// `KERNEL_BUILTIN_PROJECTION_KEYS` const. Renders `projection_tier`'s derived
// list so the kernel built-in key set is not hand-maintained and cannot drift
// from what codegen decodes / the kernel emits.
pub mod rust_builtin_keys;
// #1493 P9 — generate the native known-signer detection lists (Kotlin
// `KNOWN_NOSTR_SIGNERS` + Swift `knownSigners`) from the Rust catalog JSON
// (`nmp_core::signer_catalog` via `dump_signer_catalog`). Parses the catalog
// into a LOCAL typed struct so `nmp-codegen` keeps its no-`nmp-core` posture.
pub mod signer_catalog;

pub use manifest::{AppManifest, ModuleSet, NmpDependency};
pub use projection_tier::{
    kernel_builtin_projection_keys, projection_tier, ProjectionTier,
    KERNEL_BUILTINS_WITHOUT_SHELL_DECODER,
};
pub use rust_builtin_keys::{
    check_builtin_keys, generate_builtin_keys, render_builtin_keys, BuiltinKeysCheckOutcome,
};
pub use signer_catalog::{
    check_signer_catalog, generate_signer_catalog, parse_catalog, render_kotlin_known_signers,
    render_swift_known_signers, SignerApp, SignerCatalogCheckOutcome,
};
pub use swift::{check_swift, generate_swift, SwiftCheckOutcome, SwiftEmitError};
pub use swift_projection_cache::{
    check_projection_cache, generate_projection_cache, render_projection_cache,
    ProjectionCacheCheckOutcome,
};
pub use kotlin_projection_cache::{
    check_kotlin_projection_cache, generate_kotlin_projection_cache,
    render_kotlin_projection_cache, KotlinProjectionCacheCheckOutcome,
};
pub use swift_typed_decoders::{
    check_typed_decoders, generate_typed_decoders, render_typed_decoders, TypedDecodersCheckOutcome,
};

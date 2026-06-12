// `nmp.toml` app-manifest parser — consumed by `nmp doctor` / `nmp upgrade`
// to read and bump an app's NMP dependency policy. (ADR-0046 deleted the
// Rust-shell module *generator* that also read this manifest; the manifest
// model itself survives because the dependency-policy commands still need it.)
mod manifest;
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

pub use manifest::{AppManifest, ModuleSet, NmpDependency};
pub use swift::{check_swift, generate_swift, SwiftCheckOutcome, SwiftEmitError};
pub use swift_typed_decoders::{
    check_typed_decoders, generate_typed_decoders, render_typed_decoders, TypedDecodersCheckOutcome,
};

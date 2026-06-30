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
// schema-owner dump binaries write, emits one Swift file
// with one struct per pilot type. See
// `docs/retired/codegen-v6.md` §6b.
//
// NOTE (ADR-0046 — "composition is a library, not a generator"): the former
// Rust-shell module-scaffolding generator (`generate` / `ffi_gen` /
// `workspace`) and its `apps/fixture` test consumer were deleted. A generated
// FfiApp never composed a real Nostr app; it was the create-react-app-eject
// anti-pattern. Composition lives in explicit app/runtime roots, so
// `nmp-codegen` emits ONLY the consumer-side Swift artifacts (KernelTypes +
// typed projection decoders) that gate live CI.
pub mod swift;
// V6 Stage 2 — dotted-projection-key registry for `SnapshotProjections` +
// `CodingKeys`. Hand-transcribed from the existing Swift declaration in
// `apps/chirp/ios/Chirp/Bridge/KernelBridge.swift`; the renderer in `swift.rs`
// appends `SnapshotProjections` to the generated file using this slice.
// Lives in `nmp-codegen` (D0-exempt) so the registry can name dotted host
// keys like `"nmp.nip29.group_events"` without tripping doctrine-lint on
// `nmp-core`. See module doc for the full rationale.
pub mod swift_projections_registry;
// #1723 (epic #1719) — the neutral projection contract manifest. The single
// platform-independent source for each projection's key / tier / schema_id /
// file_identifier / version / declaration policy / source-version dependencies /
// presence policy. The kernel built-in key set, the revision dependency table,
// and the registries' neutral columns are derived FROM this. See module doc.
pub mod projection_contract;
// #1939 (epic #1921) — the neutral typed action contract manifest. The single
// source for default typed action namespace / producer / payload schema /
// schema-version / FlatBuffers file identifier / default tier / generated
// builder posture / public re-export policy / typed-dispatch posture.
pub mod action_contract;
mod crate_ownership_parse;
// #2506 — compiled positive ownership descriptors + CLI report/audit surface.
// Descriptors live in each crate's source via `nmp_ownership`, while this
// module discovers active workspace packages and audits duplicate exclusive
// scopes.
pub mod crate_ownership;
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
// `apps/chirp/android/app/src/main/java/org/nmp/android/ProjectionCache.kt`.
pub mod kotlin_projection_cache;
// ADR-0053 / Workstream-E4 — projection-tier classification + the codegen-derived
// kernel built-in projection key set (derived from `swift_projections_registry`).
pub mod projection_tier;
// ADR-0053 / Workstream-E4 — generator for `nmp-core`'s
// `KERNEL_BUILTIN_PROJECTION_KEYS` const. Renders `projection_tier`'s derived
// list so the kernel built-in key set is not hand-maintained and cannot drift
// from what codegen decodes / the kernel emits.
pub mod rust_builtin_keys;
// #1723 (epic #1719) — generator for the per-projection producer constants
// (`*_SCHEMA_ID` / `*_FILE_IDENTIFIER` / `*_SCHEMA_VERSION`) the `nmp-core`
// kernel + actor `*_fb.rs` codecs `include!` in place of the deleted
// hand-declared blocks, so those wire-identity facts derive from the projection
// contract instead of being re-stated per producer.
pub mod producer_consts;
// #1723 (epic #1719) — fail-closed producer-version drift gate for the Tier-1
// NIP-crate (+ marmot / content) producers that hand-declare their own
// `*_SCHEMA_VERSION` and do NOT depend on `nmp-codegen` (so `producer_consts`
// can't generate their consts). Reads each producer source on disk and asserts
// its schema-version literal equals the contract's `version`, so the contract
// can't drift from those producers until the full producer-const migration
// reaches the NIP crates (a separate slice — see the module doc).
pub mod projection_version_gate;
// #1493 P9 — generate the native known-signer detection lists (Kotlin
// `KNOWN_NOSTR_SIGNERS` + Swift `knownSigners`) from the Rust catalog JSON
// (`nmp_core::signer_catalog` via `dump_signer_catalog`). Parses the catalog
// into a LOCAL typed struct so `nmp-codegen` keeps its no-`nmp-core` posture.
pub mod signer_catalog;
// ADR-0063 Lane A (#1671) — generated per-key (row-keyed) reference caches for
// keyed projections (`refs.profile` / `refs.event`). Sourced from
// `KEYED_PROJECTIONS`; decode `nmp.refs.RefRowDeltaBatch` and merge row deltas
// under the five invariants, semantically identical to
// `nmp_core::refs::RefRowCache` and to each other across platforms.
pub mod kotlin_keyed_cache;
pub mod swift_keyed_cache;

pub use action_contract::{
    canonical_default_action_namespaces, contract_for as action_contract_for, dm_action_namespaces,
    lookup as action_contract_lookup, marmot_action_namespaces, render_action_contract_report,
    social_action_namespaces, substrate_action_namespaces, typed_dispatch_exemption_namespaces,
    wallet_action_namespaces, zap_action_namespaces, ActionContract, ActionDefaultTier,
    BuilderSupport, PublicReExportPolicy, TypedDispatchPolicy, ACTION_CONTRACT,
};
pub use crate_ownership::{
    load_workspace_ownership, render_ownership_human, render_ownership_json, render_ownership_tsv,
    OwnershipAuditIssue, OwnershipClaim, OwnershipDescriptor, OwnershipNote, OwnershipQuery,
    OwnershipWorkspace,
};
pub use kotlin_keyed_cache::{
    check_kotlin_keyed_ref_cache, generate_kotlin_keyed_ref_cache, render_kotlin_keyed_ref_cache,
    KotlinKeyedRefCacheCheckOutcome,
};
pub use kotlin_projection_cache::{
    check_kotlin_projection_cache, generate_kotlin_projection_cache,
    render_kotlin_projection_cache, KotlinProjectionCacheCheckOutcome,
};
pub use manifest::{AppManifest, ModuleSet, NmpDependency};
pub use producer_consts::{
    check_all_producer_consts, generate_all_producer_consts, render_producer_consts,
    ProducerConstTarget, ProducerConstsCheckOutcome, PRODUCER_CONST_TARGETS,
};
pub use projection_contract::{
    contract_for, drain_projection_keys, kernel_builtin_dependencies,
    kernel_builtin_projection_keys, lookup, rev_conditional_presence_keys, DeclarationPolicy,
    PresencePolicy, ProjectionContract, ProjectionTier, PROJECTION_CONTRACT,
};
pub use projection_tier::projection_tier;
pub use projection_version_gate::{
    check_all_producer_versions, parse_const_u32, repo_root as projection_repo_root,
    ProducerVersionCheckOutcome, ProducerVersionSource, PRODUCER_VERSION_SOURCES,
};
pub use rust_builtin_keys::{
    check_builtin_deps, check_builtin_keys, check_presence_keys, generate_builtin_deps,
    generate_builtin_keys, generate_presence_keys, render_builtin_deps, render_builtin_keys,
    render_presence_keys, BuiltinKeysCheckOutcome,
};
pub use signer_catalog::{
    check_signer_catalog, generate_signer_catalog, parse_catalog, render_kotlin_known_signers,
    render_swift_known_signers, SignerApp, SignerCatalogCheckOutcome,
};
pub use swift::{check_swift, generate_swift, SwiftCheckOutcome, SwiftEmitError};
pub use swift_keyed_cache::{
    check_keyed_ref_cache, generate_keyed_ref_cache, render_keyed_ref_cache,
    KeyedRefCacheCheckOutcome,
};
pub use swift_projection_cache::{
    check_projection_cache, generate_projection_cache, render_projection_cache,
    ProjectionCacheCheckOutcome,
};
pub use swift_typed_decoders::{
    check_typed_decoders, generate_typed_decoders, render_typed_decoders, TypedDecodersCheckOutcome,
};
// ADR-0064 §3 (#1783) — generated typed action-builder codegen (Swift + Kotlin).
// Emits the host-facing typed write builders that construct the
// `DispatchEnvelope` bytes for the native byte doorway from typed inputs, so the
// shells never spell an `action_namespace` or hand-assemble FlatBuffers.
pub mod action_builders;
pub use action_builders::{
    check_action_builders, check_action_builders_from_registry, check_app_action_builder_registry,
    generate_action_builders, generate_action_builders_from_registry,
    load_app_action_builder_registry, parse_app_action_builder_registry,
    render as render_action_builders, render_from_registry as render_action_builders_from_registry,
    validate_app_action_builder_schema_files, ActionBuildersCheckOutcome, AppActionBuilderOutputs,
    AppActionBuilderRegistryCheckOutcome, AppActionBuilderSchema, LoadedAppActionBuilderRegistry,
    Platform as ActionBuilderPlatform,
};

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;

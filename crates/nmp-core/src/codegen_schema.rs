//! V6 Stage 1 — projection-type schema export for the Swift `Decodable`
//! emitter (`nmp-codegen gen swift`).
//!
//! This module exists only when the `codegen-schema` Cargo feature is on
//! (off by default, see `Cargo.toml`). Production builds — every shipped
//! iOS / Android / WASM artifact — never compile this file, never link
//! `schemars`, and never reach the projection-type re-exports below.
//!
//! ## Why a schema-dump function (not a re-export module)
//!
//! Most projection types are `pub(super)` / `pub(crate)` in `nmp-core` —
//! that's the kernel-encapsulation contract (D0: nothing inside the kernel
//! leaks across the crate boundary). The Swift emitter has to call
//! `schemars::schema_for!(T)` for each pilot type, but `schema_for!` only
//! works where `T` is nameable.
//!
//! The intuitive fix — `pub use crate::kernel::types::Metrics` from this
//! module — fails because Rust forbids re-exporting a `pub(crate)` type at
//! `pub` visibility (E0365). Widening every projection type to `pub` only
//! when the feature is on would also work but it leaks D0 internals into
//! the published API surface whenever someone enables the feature.
//!
//! Instead, the schema export happens *inside* this crate: the public
//! [`dump_pilot_schemas`] function returns the complete JSON document the
//! Swift emitter consumes. The `dump_projection_schemas` binary
//! (`src/bin/dump_projection_schemas.rs`) is a 3-line stub that calls this
//! function and prints the result. The crate-private types never escape;
//! only their JSON schemas do.
//!
//! ## Pilot scope
//!
//! Eight flat-record projection types (no nested registry-map
//! complication). Each carries `#[derive(JsonSchema)]` in its defining
//! file, gated by the same `codegen-schema` feature:
//!
//! 1. `Metrics` — 42 primitive fields (counters, durations, ratios).
//! 2. `RelayStatus` — relay-health row.
//! 3. `LogicalInterestStatus` — logical-subscription row.
//! 4. `WireSubscriptionStatus` — wire-subscription row.
//! 5. `AccountSummary` — Accounts screen row.
//! 6. `AppRelay` — Relays settings row.
//! 7. `RelayRoleOption` — relay-role picker option.
//! 8. `TimelineItem` — timeline/thread row (V6 Stage 3 partial — F-05).
//!    Last pure flat-record holdout in `KernelBridge.swift`; the remaining
//!    Stage 3 types (`KernelSnapshot`, the tagged-enum `TimelineBlock` family,
//!    `Nip46Onboarding`, etc.) need emitter extensions (host-map override,
//!    tagged enum, legacy-default flag) before they can land.
//!
//! Stage 2 (the dotted-projection-key registry — `SnapshotProjections`) is
//! live; the remaining Stage 3 work is deferred per
//! `docs/architecture-audit/v6-codegen-plan.md` §6d.

use schemars::{schema_for, JsonSchema};
use serde::Serialize;
use serde_json::Value;

// The four projection types come in through `kernel/mod.rs`'s
// `pub(crate) use ... as ...ForCodegen` aliases. The aliasing is what
// breaks the E0252 collision V6 Stage 1 walked into (re-exporting +
// importing the same name in the same module under
// `--features codegen-schema`). See the alias declarations in
// `kernel/mod.rs` for the full rationale. The local `as` rebinds give us
// the same call-site identifiers as before — no change in this file's
// `schema_value::<T>()` invocations.
use crate::actor::RelayRoleOption;
use crate::kernel::{
    AccountSummary, AppRelay, LogicalInterestStatusForCodegen as LogicalInterestStatus,
    MetricsForCodegen as Metrics, RelayStatusForCodegen as RelayStatus,
    TimelineItemForCodegen as TimelineItem,
    WireSubscriptionStatusForCodegen as WireSubscriptionStatus,
};

/// Per-type metadata the JSON schema alone cannot carry (Swift-side type
/// name, conformance set, `Identifiable.id` source field). The emitter
/// reads these alongside the schema to render conformances and the
/// `var id: String { … }` computed property.
#[derive(Serialize)]
pub struct TypeEntry {
    /// Fully-qualified Rust path — provenance comment in the generated
    /// Swift header. Matches the symbol the `schema_for!` macro reflected.
    pub rust_path: &'static str,
    /// Swift type name the emitter renders. Stays distinct from
    /// `rust_path` because the current hand-written Swift has chosen names
    /// that don't 1:1 match Rust (e.g. Rust `Metrics` → Swift
    /// `KernelMetrics`); the generated names match the hand-written names
    /// verbatim so consumer imports don't change.
    pub swift_name: &'static str,
    /// When `Some("<field>")`, the emitted Swift type also conforms to
    /// `Identifiable` and exposes `var id: String { <field> }`. JSON
    /// Schema has no notion of Swift's `Identifiable` contract, so this
    /// stays in registry metadata.
    pub id_field: Option<&'static str>,
    /// Conformance set (e.g. `["Decodable", "Equatable"]`). The emitter
    /// joins these into the struct's `:`-clause; `Identifiable` is added
    /// automatically when `id_field` is `Some`.
    pub conformances: &'static [&'static str],
    /// Host-rendered fields for this row type, in stable declared order.
    /// Empty = not a row type (no `rendersIdentically` emitted). These are
    /// Rust snake_case names; the emitter camelCases them.
    pub render_identity_fields: &'static [&'static str],
    /// The `schemars`-generated JSON Schema for the type. Carries field
    /// shape, optionality, snake_case names, etc.
    pub schema: Value,
}

/// Top-level document the schema-dump binary writes to stdout and the
/// Swift emitter parses.
#[derive(Serialize)]
pub struct ProjectionSchemaDocument {
    /// Bump when the document shape (NOT the per-type schemas) changes.
    /// The Swift emitter refuses unknown versions.
    pub version: u32,
    /// One entry per pilot type. Order is stable (matches the source
    /// vector in [`dump_pilot_schemas`]).
    pub types: Vec<TypeEntry>,
}

/// `schema_for!` thunk — keeps the call sites to one line each and lets
/// `serde_json::to_value` lift each schema into the document without
/// naming `schemars::schema::RootSchema` everywhere.
fn schema_value<T: JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).unwrap_or(serde_json::Value::Null)
}

/// Build the full pilot-set schema document.
///
/// Order is load-bearing: the Swift emitter writes types in this order,
/// the `--check` gate diffs the resulting file byte-for-byte, and the
/// order is also the rendering order in the generated Swift header
/// comment. Add to the end; do not reorder.
#[must_use]
pub fn dump_pilot_schemas() -> ProjectionSchemaDocument {
    // Every generated type opts in to `Sendable` explicitly: the structs
    // are immutable value types whose fields are all themselves Sendable,
    // but public Swift structs do NOT auto-infer Sendable (unlike
    // `internal` ones), so a consumer that composes a generated type into
    // a non-Sendable wrapper inside a `static let` would hard-fail strict
    // concurrency. Declaring it at the source pre-empts that landmine for
    // every present and future consumer; see the Sendable rationale block
    // in `nmp-codegen/src/swift.rs::render_type`.
    let types = vec![
        TypeEntry {
            rust_path: "nmp_core::kernel::types::Metrics",
            swift_name: "KernelMetrics",
            id_field: None,
            conformances: &["Decodable", "Equatable", "Sendable"],
            render_identity_fields: &[],
            schema: schema_value::<Metrics>(),
        },
        TypeEntry {
            rust_path: "nmp_core::kernel::types::RelayStatus",
            swift_name: "RelayStatus",
            // Relay rows are keyed by URL on the iOS side — preserves the
            // existing `var id: String { relayUrl }` pattern.
            id_field: Some("relayUrl"),
            conformances: &["Decodable", "Equatable", "Sendable"],
            render_identity_fields: &[],
            schema: schema_value::<RelayStatus>(),
        },
        TypeEntry {
            rust_path: "nmp_core::kernel::types::LogicalInterestStatus",
            swift_name: "LogicalInterestStatus",
            id_field: Some("key"),
            conformances: &["Decodable", "Equatable", "Sendable"],
            render_identity_fields: &[],
            schema: schema_value::<LogicalInterestStatus>(),
        },
        TypeEntry {
            rust_path: "nmp_core::kernel::types::WireSubscriptionStatus",
            swift_name: "WireSubscriptionStatus",
            id_field: Some("wireId"),
            conformances: &["Decodable", "Equatable", "Sendable"],
            render_identity_fields: &[],
            schema: schema_value::<WireSubscriptionStatus>(),
        },
        TypeEntry {
            rust_path: "nmp_core::kernel::identity_state::AccountSummary",
            swift_name: "AccountSummary",
            id_field: Some("id"),
            conformances: &["Decodable", "Equatable", "Sendable"],
            render_identity_fields: &[],
            schema: schema_value::<AccountSummary>(),
        },
        TypeEntry {
            rust_path: "nmp_core::kernel::identity_state::AppRelay",
            swift_name: "AppRelay",
            id_field: Some("url"),
            conformances: &["Decodable", "Equatable", "Sendable"],
            render_identity_fields: &[],
            schema: schema_value::<AppRelay>(),
        },
        TypeEntry {
            rust_path: "nmp_core::actor::relay_roles::RelayRoleOption",
            swift_name: "RelayRoleOption",
            id_field: Some("value"),
            conformances: &["Decodable", "Equatable", "Sendable"],
            render_identity_fields: &[],
            schema: schema_value::<RelayRoleOption>(),
        },
        TypeEntry {
            // V6 Stage 3 partial (F-05). The Swift hand-written struct in
            // `ios/Chirp/Chirp/Bridge/KernelBridge.swift` carries
            // `Identifiable`, `Equatable`, and `Hashable` plus a custom
            // `init(from:)` with three `decodeIfPresent ?? default`
            // fallbacks (`isRepost`, `navTargetId`, `repostInnerContent`).
            // Those fallbacks are dead — the Rust kernel always emits the
            // fields (D1 contract documented on the type) — so the
            // generated struct drops the custom decoder and the call
            // sites lose nothing. The optional `author_picture_url ??
            // identicon` fallback at `ModularBlockView.swift:175` is at a
            // CHAIN site (`item?.authorPictureUrl`, where `item` is
            // `TimelineItem?`); the chain stays `String?` regardless of
            // the field's optionality, so that consumer is unaffected.
            // The synthetic `ModularBlockView.syntheticItem` call site
            // (line ~285) DOES need a small update — see the PR.
            //
            // `Sendable` is the load-bearing addition for this type
            // specifically: `NoteRenderContext` (in
            // `ios/Chirp/Chirp/Components/NoteEntityViews.swift`) holds
            // `[String: TimelineItem]` and exposes a `static let empty`,
            // which strict-concurrency rejects on a non-Sendable value
            // type. Without explicit `Sendable` on `TimelineItem` the
            // Chirp build fails the moment this struct elevates from
            // `internal` (hand-written) to `public` (generated).
            rust_path: "nmp_core::kernel::types::TimelineItem",
            swift_name: "TimelineItem",
            id_field: Some("id"),
            conformances: &["Decodable", "Equatable", "Hashable", "Sendable"],
            render_identity_fields: &[
                "id",
                "author_pubkey",
                "author_display_name",
                "author_picture_url",
                "author_lnurl",
                "content",
                "content_preview",
                "created_at",
                "is_repost",
                "kind",
                "nav_target_id",
                "repost_inner_content",
                "relay_count",
            ],
            schema: schema_value::<TimelineItem>(),
        },
    ];

    ProjectionSchemaDocument { version: 1, types }
}

/// Serialise the pilot schema document to pretty-printed JSON. The binary
/// uses this directly; the indirection exists so unit tests can assert on
/// the document shape without re-implementing the serialisation step.
#[must_use]
pub fn dump_pilot_schemas_json() -> String {
    serde_json::to_string_pretty(&dump_pilot_schemas()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pilot_document_has_eight_entries_in_stable_order() {
        // The pilot ships these eight types — and only these. The Stage 2
        // dotted-projection-key registry (`SnapshotProjections`) lives in
        // a separate vector in `nmp-codegen`; this test guards the
        // flat-record set from accidental reordering / silent additions,
        // both of which would change the generated Swift byte-for-byte
        // and break the `--check` CI gate.
        //
        // V6 Stage 3 partial (F-05) added `TimelineItem` to the tail.
        let document = dump_pilot_schemas();
        assert_eq!(document.version, 1, "schema document version is stable");
        let swift_names: Vec<_> = document.types.iter().map(|t| t.swift_name).collect();
        assert_eq!(
            swift_names,
            vec![
                "KernelMetrics",
                "RelayStatus",
                "LogicalInterestStatus",
                "WireSubscriptionStatus",
                "AccountSummary",
                "AppRelay",
                "RelayRoleOption",
                "TimelineItem",
            ],
            "pilot type order is load-bearing for the Swift emitter"
        );
    }

    #[test]
    fn each_pilot_entry_has_a_schema() {
        // Every entry must carry a non-empty JSON Schema. A bug in the
        // `schema_value` thunk that returned `Value::Null` would silently
        // emit zero-field Swift structs in CI; this guards against that.
        let document = dump_pilot_schemas();
        for entry in &document.types {
            assert!(
                entry.schema.is_object(),
                "{} schema must be a JSON object (was: {:?})",
                entry.swift_name,
                entry.schema
            );
        }
    }
}

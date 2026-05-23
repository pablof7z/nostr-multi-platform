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
//! ## Stage 1 scope
//!
//! Seven flat-record projection types (no nested registry-map
//! complication). Each carries `#[derive(JsonSchema)]` in its defining
//! file, gated by the same `codegen-schema` feature:
//!
//! 1. `Metrics` — 42 primitive fields (counters, durations, ratios).
//! 2. `RelayStatus` — relay-health row.
//! 3. `LogicalInterestStatus` — logical-subscription row.
//! 4. `WireSubscriptionStatus` — wire-subscription row.
//! 5. `AccountSummary` — Accounts screen row.
//! 6. `RelayEditRow` — Relays settings row.
//! 7. `RelayRoleOption` — relay-role picker option.
//!
//! Stage 2 (the dotted-projection-key registry — `SnapshotProjections`)
//! and Stage 3 (the remaining ~30 hand-written Decodables, plus
//! `TimelineBlock`'s tagged-enum case) are deferred per
//! `docs/architecture-audit/v6-codegen-plan.md` §6c, §6d.

use schemars::{schema_for, JsonSchema};
use serde::Serialize;
use serde_json::Value;

use crate::actor::RelayRoleOption;
use crate::kernel::{
    AccountSummary, LogicalInterestStatus, Metrics, RelayEditRow, RelayStatus,
    WireSubscriptionStatus,
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
    serde_json::to_value(schema_for!(T))
        .expect("schemars-generated schema is always serde_json-serialisable")
}

/// Build the full pilot-set schema document.
///
/// Order is load-bearing: the Swift emitter writes types in this order,
/// the `--check` gate diffs the resulting file byte-for-byte, and the
/// order is also the rendering order in the generated Swift header
/// comment. Add to the end; do not reorder.
#[must_use]
pub fn dump_pilot_schemas() -> ProjectionSchemaDocument {
    let types = vec![
        TypeEntry {
            rust_path: "nmp_core::kernel::types::Metrics",
            swift_name: "KernelMetrics",
            id_field: None,
            conformances: &["Decodable", "Equatable"],
            schema: schema_value::<Metrics>(),
        },
        TypeEntry {
            rust_path: "nmp_core::kernel::types::RelayStatus",
            swift_name: "RelayStatus",
            // Relay rows are keyed by URL on the iOS side — preserves the
            // existing `var id: String { relayUrl }` pattern.
            id_field: Some("relayUrl"),
            conformances: &["Decodable", "Equatable"],
            schema: schema_value::<RelayStatus>(),
        },
        TypeEntry {
            rust_path: "nmp_core::kernel::types::LogicalInterestStatus",
            swift_name: "LogicalInterestStatus",
            id_field: Some("key"),
            conformances: &["Decodable", "Equatable"],
            schema: schema_value::<LogicalInterestStatus>(),
        },
        TypeEntry {
            rust_path: "nmp_core::kernel::types::WireSubscriptionStatus",
            swift_name: "WireSubscriptionStatus",
            id_field: Some("wireId"),
            conformances: &["Decodable", "Equatable"],
            schema: schema_value::<WireSubscriptionStatus>(),
        },
        TypeEntry {
            rust_path: "nmp_core::kernel::identity_state::AccountSummary",
            swift_name: "AccountSummary",
            id_field: Some("id"),
            conformances: &["Decodable", "Equatable"],
            schema: schema_value::<AccountSummary>(),
        },
        TypeEntry {
            rust_path: "nmp_core::kernel::identity_state::RelayEditRow",
            swift_name: "RelayEditRow",
            id_field: Some("url"),
            conformances: &["Decodable", "Equatable"],
            schema: schema_value::<RelayEditRow>(),
        },
        TypeEntry {
            rust_path: "nmp_core::actor::relay_roles::RelayRoleOption",
            swift_name: "RelayRoleOption",
            id_field: Some("value"),
            conformances: &["Decodable", "Equatable"],
            schema: schema_value::<RelayRoleOption>(),
        },
    ];

    ProjectionSchemaDocument { version: 1, types }
}

/// Serialise the pilot schema document to pretty-printed JSON. The binary
/// uses this directly; the indirection exists so unit tests can assert on
/// the document shape without re-implementing the serialisation step.
#[must_use]
pub fn dump_pilot_schemas_json() -> String {
    serde_json::to_string_pretty(&dump_pilot_schemas())
        .expect("ProjectionSchemaDocument is serialisable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pilot_document_has_seven_entries_in_stable_order() {
        // Stage 1 pilot ships these seven types — and only these. Stage 2
        // expansion (the dotted-projection-key registry) goes in a
        // separate vector; this test guards the Stage 1 set from
        // accidental reordering / silent additions, both of which would
        // change the generated Swift byte-for-byte and break the
        // `--check` CI gate.
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
                "RelayEditRow",
                "RelayRoleOption",
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

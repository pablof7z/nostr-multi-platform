//! ADR-0064 §3 (#1783) — Swift typed action-builder emitter.
//!
//! Renders `ActionBuilders.generated.swift`: one `GeneratedActionBuilders` enum
//! whose static functions are the app-facing typed write builders
//! (`GeneratedActionBuilders.react(...)`, `.follow(...)`, …). Each function
//! returns the finished `DispatchEnvelope` bytes (`[UInt8]`) ready for the one
//! native byte doorway `nmp_app_dispatch_action_bytes` (#1752); the host owns
//! the FFI call + the `correlation_id` it stamps in.
//!
//! ## Self-contained encode (no flatc payload class)
//!
//! The builders encode the per-crate FlatBuffers payload table DIRECTLY via the
//! FlatBuffers Swift runtime (`FlatBufferBuilder` low-level
//! `startTable`/`add`/`endTable`), writing each field at its declaration SLOT
//! (`schema_version` at slot 0, then [`ActionBuilder::fields`] from slot 1). No
//! `flatc --swift` binding for the write schemas is needed — the registry is the
//! source of truth and the per-crate Rust `decode` (+ the round-trip test) is the
//! authoritative wire-shape guard. The vtable offset for slot `i` is `4 + i*2`.
//!
//! ## Determinism
//!
//! Byte-deterministic: iterate [`ACTION_BUILDERS`] in declaration order, emit a
//! fixed template per builder. That stability is what the `--check` drift gate
//! relies on (exactly like [`crate::swift`] / [`crate::swift_typed_decoders`]).

use crate::action_builders::registry::{
    ActionBuilder, FieldKind, ACTION_BUILDERS, DISPATCH_ENVELOPE_FILE_IDENTIFIER,
    DISPATCH_ENVELOPE_SCHEMA_VERSION,
};

const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen action-builders --platform swift \\
//       --out ios/Chirp/Chirp/Bridge/Generated/ActionBuilders.generated.swift
//
// Source of truth: `crates/nmp-codegen/src/action_builders/registry.rs`
// (`ACTION_BUILDERS`). The CI gate (`.github/workflows/codegen-drift.yml`) fails
// any PR whose generated Swift differs from a fresh run.
//
// ADR-0064 §3 — typed write builders. Each function below encodes the per-crate
// FlatBuffers payload for one open-registry `action_namespace` and stamps it,
// the namespace, and the envelope schema_version into a `DispatchEnvelope`,
// returning the finished bytes for the native byte doorway
// `nmp_app_dispatch_action_bytes` (#1752). App code NEVER spells a namespace
// string or hand-assembles FlatBuffers — that lives only here, in generated
// code. The host supplies the `correlation_id` (the operation identity end to
// end, ADR-0064 §4) and owns the FFI call.
// ─────────────────────────────────────────────────────────────────────────────

import FlatBuffers
import Foundation
";

/// Render the generated Swift action-builders for the given registry.
#[must_use]
pub fn render(builders: &[ActionBuilder]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    out.push_str("public enum GeneratedActionBuilders {\n");
    out.push_str(&envelope_helper());
    for builder in builders {
        out.push('\n');
        render_one(builder, &mut out);
    }
    out.push_str("}\n");
    out
}

/// Render the shared private `DispatchEnvelope` wrapper. Mirrors
/// `nmp_core::dispatch_envelope::encode_dispatch_envelope`: correlation_id at
/// slot 0, action_namespace at slot 1, schema_version (uint) at slot 2, payload
/// ([ubyte]) at slot 3, finished with the `NMPD` file identifier.
fn envelope_helper() -> String {
    let mut s = String::new();
    s.push_str(
        "    /// The single recognised envelope schema version — mirrors\n\
         \x20   /// `nmp_core::dispatch_envelope::DISPATCH_ENVELOPE_SCHEMA_VERSION`.\n",
    );
    s.push_str(&format!(
        "    public static let dispatchEnvelopeSchemaVersion: UInt32 = {DISPATCH_ENVELOPE_SCHEMA_VERSION}\n\n"
    ));
    s.push_str(
        "    /// Stamp `(correlationId, actionNamespace, schemaVersion, payload)` into a\n\
         \x20   /// `DispatchEnvelope` and return the finished bytes (file identifier `NMPD`).\n\
         \x20   /// The byte-for-byte twin of `encode_dispatch_envelope` in `nmp-core`.\n",
    );
    s.push_str(
        "    private static func encodeDispatchEnvelope(\n\
         \x20       correlationId: String,\n\
         \x20       actionNamespace: String,\n\
         \x20       payload: [UInt8]\n\
         \x20   ) -> [UInt8] {\n",
    );
    s.push_str("        var fbb = FlatBufferBuilder()\n");
    s.push_str("        let correlationOffset = fbb.create(string: correlationId)\n");
    s.push_str("        let namespaceOffset = fbb.create(string: actionNamespace)\n");
    s.push_str("        let payloadOffset = fbb.createVector(payload)\n");
    // DispatchEnvelope: 4 fields → vtable size; slots 0..3.
    s.push_str("        let start = fbb.startTable(with: 4)\n");
    s.push_str("        fbb.add(offset: correlationOffset, at: 4)   // slot 0: correlation_id\n");
    s.push_str("        fbb.add(offset: namespaceOffset, at: 6)     // slot 1: action_namespace\n");
    s.push_str(
        "        fbb.add(element: dispatchEnvelopeSchemaVersion, def: UInt32(0), at: 8) // slot 2: schema_version\n",
    );
    s.push_str("        fbb.add(offset: payloadOffset, at: 10)      // slot 3: payload\n");
    s.push_str("        let root = Offset(offset: fbb.endTable(at: start))\n");
    s.push_str(&format!(
        "        fbb.finish(offset: root, fileId: {DISPATCH_ENVELOPE_FILE_IDENTIFIER:?})\n"
    ));
    s.push_str("        return fbb.sizedByteArray\n");
    s.push_str("    }\n");
    s
}

/// Render one typed builder function.
fn render_one(builder: &ActionBuilder, out: &mut String) {
    let method = builder.method;
    // Doc + signature.
    out.push_str(&format!("    /// {}\n", builder.doc));
    out.push_str(&format!(
        "    /// Builds the `{}` `DispatchEnvelope` bytes for the byte doorway.\n",
        builder.namespace
    ));
    out.push_str(&format!("    public static func {method}(\n"));
    out.push_str("        correlationId: String");
    for field in builder.fields {
        out.push_str(",\n");
        out.push_str(&format!("        {}: {}", field.name, swift_param_type(field)));
    }
    out.push_str("\n    ) -> [UInt8] {\n");

    // Encode the payload table.
    out.push_str("        var fbb = FlatBufferBuilder()\n");
    // Create string/vector offsets first (FlatBuffers requires nested objects
    // be built before the table that references them).
    for field in builder.fields {
        match field.kind {
            FieldKind::Str if field.optional => {
                out.push_str(&format!(
                    "        let {n}Offset: Offset = {n}.map {{ fbb.create(string: $0) }} ?? Offset()\n",
                    n = field.name
                ));
            }
            FieldKind::Str => {
                out.push_str(&format!(
                    "        let {n}Offset = fbb.create(string: {n})\n",
                    n = field.name
                ));
            }
            FieldKind::StrVec => {
                if field.optional {
                    out.push_str(&format!(
                        "        let {n}Offset: Offset = {{\n\
                         \x20           guard let values = {n}, !values.isEmpty else {{ return Offset() }}\n\
                         \x20           let offsets = values.map {{ fbb.create(string: $0) }}\n\
                         \x20           return fbb.createVector(ofOffsets: offsets)\n\
                         \x20       }}()\n",
                        n = field.name
                    ));
                } else {
                    out.push_str(&format!(
                        "        let {n}Offsets = {n}.map {{ fbb.create(string: $0) }}\n\
                         \x20       let {n}Offset = fbb.createVector(ofOffsets: {n}Offsets)\n",
                        n = field.name
                    ));
                }
            }
            FieldKind::Uint => {}
        }
    }
    // Table: 1 (schema_version) + N fields.
    let table_fields = builder.fields.len() + 1;
    out.push_str(&format!(
        "        let payloadStart = fbb.startTable(with: {table_fields})\n"
    ));
    out.push_str(&format!(
        "        fbb.add(element: UInt32({}), def: UInt32(0), at: 4) // slot 0: schema_version\n",
        builder.payload_schema_version
    ));
    for (idx, field) in builder.fields.iter().enumerate() {
        let slot = idx + 1; // slot 0 is schema_version
        let vtoffset = 4 + slot * 2;
        match field.kind {
            FieldKind::Str | FieldKind::StrVec => {
                if field.optional {
                    out.push_str(&format!(
                        "        if {n}Offset.o != 0 {{ fbb.add(offset: {n}Offset, at: {vt}) }} // slot {slot}: {n}\n",
                        n = field.name,
                        vt = vtoffset
                    ));
                } else {
                    out.push_str(&format!(
                        "        fbb.add(offset: {n}Offset, at: {vt}) // slot {slot}: {n}\n",
                        n = field.name,
                        vt = vtoffset
                    ));
                }
            }
            FieldKind::Uint => {
                out.push_str(&format!(
                    "        fbb.add(element: UInt32({n}), def: UInt32(0), at: {vt}) // slot {slot}: {n}\n",
                    n = field.name,
                    vt = vtoffset
                ));
            }
        }
    }
    out.push_str("        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))\n");
    out.push_str(&format!(
        "        fbb.finish(offset: payloadRoot, fileId: {:?})\n",
        builder.payload_file_identifier
    ));
    out.push_str("        let payload = fbb.sizedByteArray\n");
    // Wrap in the envelope.
    out.push_str(&format!(
        "        return encodeDispatchEnvelope(\n\
         \x20           correlationId: correlationId,\n\
         \x20           actionNamespace: {:?},\n\
         \x20           payload: payload\n\
         \x20       )\n",
        builder.namespace
    ));
    out.push_str("    }\n");
}

/// Swift parameter type for a field.
fn swift_param_type(field: &PayloadFieldRef) -> String {
    match (field.kind, field.optional) {
        (FieldKind::Str, false) => "String".to_string(),
        (FieldKind::Str, true) => "String?".to_string(),
        (FieldKind::Uint, false) => "UInt32".to_string(),
        (FieldKind::Uint, true) => "UInt32?".to_string(),
        (FieldKind::StrVec, false) => "[String]".to_string(),
        (FieldKind::StrVec, true) => "[String]?".to_string(),
    }
}

// Local alias so `swift_param_type` can take a `&PayloadField` without importing
// the type name twice.
use crate::action_builders::registry::PayloadField as PayloadFieldRef;

/// Render the full file for the default registry.
#[must_use]
pub fn render_default() -> String {
    render(ACTION_BUILDERS)
}

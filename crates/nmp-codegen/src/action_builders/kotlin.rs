//! ADR-0064 §3 (#1783) — Kotlin typed action-builder emitter.
//!
//! Renders `ActionBuilders.kt`: a `GeneratedActionBuilders` object whose
//! functions are the app-facing typed write builders
//! (`GeneratedActionBuilders.react(...)`, `.follow(...)`, …). Each returns the
//! finished `DispatchEnvelope` bytes (`ByteArray`) ready for the one native byte
//! doorway `nmp_app_dispatch_action_bytes` (#1752) — the host owns the JNI call
//! + the `correlation_id` it stamps in.
//!
//! ## Self-contained encode (no flatc payload class)
//!
//! The builders encode the per-crate FlatBuffers payload table DIRECTLY via the
//! FlatBuffers Kotlin runtime (`FlatBufferBuilder` low-level
//! `startTable`/`addInt`/`addOffset`/`endTable`), writing each field at its
//! declaration SLOT (`schema_version` at slot 0, then [`ActionBuilder::fields`]
//! from slot 1). No `flatc --kotlin` binding for the write schemas is needed —
//! the registry is the source of truth and the per-crate Rust `decode` (+ the
//! round-trip test) is the authoritative wire-shape guard. Kotlin's `add*` takes
//! a 0-indexed SLOT (not a vtable byte offset, unlike Swift).
//!
//! ## Determinism
//!
//! Byte-deterministic: iterate [`ACTION_BUILDERS`] in declaration order, emit a
//! fixed template per builder (same contract as the Swift twin).

use crate::action_builders::registry::{
    ActionBuilder, FieldKind, PayloadField, ACTION_BUILDERS, DISPATCH_ENVELOPE_FILE_IDENTIFIER,
    DISPATCH_ENVELOPE_SCHEMA_VERSION,
};
use crate::action_contract::contract_for;

const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen action-builders --platform kotlin \\
//       --out android/app/src/main/java/org/nmp/android/ActionBuilders.kt
//
// Source of truth: `crates/nmp-codegen/src/action_builders/registry.rs`
// (`ACTION_BUILDERS`). The CI gate (`.github/workflows/codegen-drift.yml`) fails
// any PR whose generated Kotlin differs from a fresh run.
//
// ADR-0064 §3 — typed write builders. Each function below encodes the per-crate
// FlatBuffers payload for one open-registry `action_namespace` and stamps it,
// the namespace, and the envelope schema_version into a `DispatchEnvelope`,
// returning the finished bytes for the native byte doorway
// `nmp_app_dispatch_action_bytes` (#1752). App code NEVER spells a namespace
// string or hand-assembles FlatBuffers — that lives only here, in generated
// code. The host supplies the `correlationId` (the operation identity end to
// end, ADR-0064 §4) and owns the JNI call.
// ─────────────────────────────────────────────────────────────────────────────

package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
";

/// Render the generated Kotlin action-builders for the given registry.
#[must_use]
pub fn render(builders: &[ActionBuilder]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    out.push_str("object GeneratedActionBuilders {\n");
    out.push_str(&envelope_helper());
    for builder in builders {
        out.push('\n');
        render_one(builder, &mut out);
    }
    // The `nmp.publish` UNION builders (separate emitter — different encode
    // shape; see `kotlin_publish`). Only emitted for the default registry, which
    // is what `render_default` (and therefore the CLI + drift gate) uses.
    if std::ptr::eq(builders.as_ptr(), ACTION_BUILDERS.as_ptr()) {
        crate::action_builders::kotlin_publish::render_publish(&mut out);
    }
    out.push_str("}\n");
    out
}

/// The shared private `DispatchEnvelope` wrapper. Mirrors
/// `nmp_core::dispatch_envelope::encode_dispatch_envelope`: correlation_id at
/// slot 0, action_namespace at slot 1, schema_version at slot 2, payload at slot
/// 3, finished with the `NMPD` file identifier.
fn envelope_helper() -> String {
    let mut s = String::new();
    s.push_str(
        "    /// The single recognised envelope schema version — mirrors\n\
         \x20   /// `nmp_core::dispatch_envelope::DISPATCH_ENVELOPE_SCHEMA_VERSION`.\n",
    );
    s.push_str(&format!(
        "    const val DISPATCH_ENVELOPE_SCHEMA_VERSION: Int = {DISPATCH_ENVELOPE_SCHEMA_VERSION}\n\n"
    ));
    s.push_str(
        "    /// Stamp `(correlationId, actionNamespace, schemaVersion, payload)` into a\n\
         \x20   /// `DispatchEnvelope` and return the finished bytes (file identifier `NMPD`).\n\
         \x20   /// The byte-for-byte twin of `encode_dispatch_envelope` in `nmp-core`.\n",
    );
    s.push_str(
        "    private fun encodeDispatchEnvelope(\n\
         \x20       correlationId: String,\n\
         \x20       actionNamespace: String,\n\
         \x20       payload: ByteArray,\n\
         \x20   ): ByteArray {\n",
    );
    s.push_str("        val fbb = FlatBufferBuilder()\n");
    s.push_str("        val correlationOffset = fbb.createString(correlationId)\n");
    s.push_str("        val namespaceOffset = fbb.createString(actionNamespace)\n");
    s.push_str("        val payloadOffset = fbb.createByteVector(payload)\n");
    s.push_str("        fbb.startTable(4)\n");
    s.push_str("        fbb.addOffset(0, correlationOffset, 0)   // slot 0: correlation_id\n");
    s.push_str("        fbb.addOffset(1, namespaceOffset, 0)     // slot 1: action_namespace\n");
    s.push_str(
        "        fbb.addInt(2, DISPATCH_ENVELOPE_SCHEMA_VERSION, 0) // slot 2: schema_version\n",
    );
    s.push_str("        fbb.addOffset(3, payloadOffset, 0)       // slot 3: payload\n");
    s.push_str("        val root = fbb.endTable()\n");
    s.push_str(&format!(
        "        fbb.finish(root, {DISPATCH_ENVELOPE_FILE_IDENTIFIER:?})\n"
    ));
    s.push_str("        return fbb.sizedByteArray()\n");
    s.push_str("    }\n");
    s
}

/// Render one typed builder function.
fn render_one(builder: &ActionBuilder, out: &mut String) {
    let contract = contract_for(builder.namespace);
    out.push_str(&format!("    /// {}\n", builder.doc));
    out.push_str(&format!(
        "    /// Builds the `{}` `DispatchEnvelope` bytes for the byte doorway.\n",
        builder.namespace
    ));
    out.push_str(&format!("    fun {}(\n", builder.method));
    out.push_str("        correlationId: String");
    for field in builder.fields {
        out.push_str(",\n");
        out.push_str(&format!(
            "        {}: {}",
            field.name,
            kotlin_param_type(field)
        ));
    }
    out.push_str(",\n    ): ByteArray {\n");

    out.push_str("        val fbb = FlatBufferBuilder()\n");
    // Build nested string/vector offsets before the table.
    for field in builder.fields {
        match field.kind {
            FieldKind::Str if field.optional => {
                out.push_str(&format!(
                    "        val {n}Offset = {n}?.let {{ fbb.createString(it) }} ?: 0\n",
                    n = field.name
                ));
            }
            FieldKind::Str => {
                out.push_str(&format!(
                    "        val {n}Offset = fbb.createString({n})\n",
                    n = field.name
                ));
            }
            FieldKind::StrVec => {
                if field.optional {
                    out.push_str(&format!(
                        "        val {n}Offset = run {{\n\
                         \x20           val values = {n}\n\
                         \x20           if (values == null || values.isEmpty()) 0 else {{\n\
                         \x20               val offsets = IntArray(values.size) {{ i -> fbb.createString(values[i]) }}\n\
                         \x20               fbb.startVector(4, offsets.size, 4)\n\
                         \x20               for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])\n\
                         \x20               fbb.endVector()\n\
                         \x20           }}\n\
                         \x20       }}\n",
                        n = field.name
                    ));
                } else {
                    out.push_str(&format!(
                        "        val {n}Offset = run {{\n\
                         \x20           val offsets = IntArray({n}.size) {{ i -> fbb.createString({n}[i]) }}\n\
                         \x20           fbb.startVector(4, offsets.size, 4)\n\
                         \x20           for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])\n\
                         \x20           fbb.endVector()\n\
                         \x20       }}\n",
                        n = field.name
                    ));
                }
            }
            FieldKind::Uint => {}
        }
    }
    let table_fields = builder.fields.len() + 1;
    out.push_str(&format!("        fbb.startTable({table_fields})\n"));
    out.push_str(&format!(
        "        fbb.addInt(0, {}, 0) // slot 0: schema_version\n",
        contract.schema_version
    ));
    for (idx, field) in builder.fields.iter().enumerate() {
        let slot = idx + 1;
        match field.kind {
            FieldKind::Str | FieldKind::StrVec => {
                if field.optional {
                    out.push_str(&format!(
                        "        if ({n}Offset != 0) fbb.addOffset({slot}, {n}Offset, 0) // slot {slot}: {n}\n",
                        n = field.name
                    ));
                } else {
                    out.push_str(&format!(
                        "        fbb.addOffset({slot}, {n}Offset, 0) // slot {slot}: {n}\n",
                        n = field.name
                    ));
                }
            }
            FieldKind::Uint => {
                out.push_str(&format!(
                    "        fbb.addInt({slot}, {n}, 0) // slot {slot}: {n}\n",
                    n = field.name
                ));
            }
        }
    }
    out.push_str("        val payloadRoot = fbb.endTable()\n");
    out.push_str(&format!(
        "        fbb.finish(payloadRoot, {:?})\n",
        contract.file_identifier
    ));
    out.push_str("        val payload = fbb.sizedByteArray()\n");
    out.push_str(&format!(
        "        return encodeDispatchEnvelope(\n\
         \x20           correlationId = correlationId,\n\
         \x20           actionNamespace = {:?},\n\
         \x20           payload = payload,\n\
         \x20       )\n",
        builder.namespace
    ));
    out.push_str("    }\n");
}

/// Kotlin parameter type for a field. Note: a `uint` becomes `Int` (the
/// FlatBuffers Kotlin runtime carries u32 inline as a signed `Int`); the S3
/// trio's only uints are the implicit slot-0 `schema_version`, never a builder
/// parameter, so this is here for completeness.
fn kotlin_param_type(field: &PayloadField) -> String {
    match (field.kind, field.optional) {
        (FieldKind::Str, false) => "String".to_string(),
        (FieldKind::Str, true) => "String?".to_string(),
        (FieldKind::Uint, false) => "Int".to_string(),
        (FieldKind::Uint, true) => "Int?".to_string(),
        (FieldKind::StrVec, false) => "List<String>".to_string(),
        (FieldKind::StrVec, true) => "List<String>?".to_string(),
    }
}

/// Render the full file for the default registry.
#[must_use]
pub fn render_default() -> String {
    render(ACTION_BUILDERS)
}

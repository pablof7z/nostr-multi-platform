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
//       --out apps/chirp/android/app/src/main/java/org/nmp/android/ActionBuilders.kt
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
    out.push_str(&relay_marker_byte_helper());
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

/// Render the private `relayMarkerByte` helper — maps a role string to the
/// `RelayMarker` ubyte (Both=0, Read=1, Write=2, Indexer=3), mirroring
/// `RelayMarker::from_role_string` in `nmp-router` EXACTLY — including
/// rejection. Unknown tokens or no-flag cases (e.g. empty string) encode as
/// 255 (out-of-range sentinel) so the Rust decoder (`marker_from_wire`) fails
/// closed (Err) rather than silently becoming Both. Emitted once at the top of
/// `GeneratedActionBuilders`; used by `publishRelayList`.
///
/// SSOT: `RelayMarker::from_role_string` in `crates/nmp-router/src/publish_relay_list.rs`.
/// 255 = deliberate out-of-range sentinel: the Rust decoder (`marker_from_wire`) rejects
/// any ordinal not in {0,1,2,3}, so encoding 255 for invalid/empty roles makes dispatch
/// fail closed (DispatchAck.error) rather than silently publishing a Both relay.
fn relay_marker_byte_helper() -> String {
    String::from(
        "\n\
         \x20   /// Map a relay role string to the RelayMarker ubyte (Both=0, Read=1, Write=2, Indexer=3),\n\
         \x20   /// mirroring `RelayMarker::from_role_string` in `nmp-router` EXACTLY — including rejection.\n\
         \x20   /// Unknown tokens or no-flag input (e.g. empty string) encode as 255 (out-of-range sentinel)\n\
         \x20   /// so the Rust decoder (`marker_from_wire`) fails closed instead of silently becoming Both.\n\
         \x20   /// Role strings may be comma-separated (e.g. `\"both,indexer\"`); comparisons are case-insensitive.\n\
         \x20   private fun relayMarkerByte(role: String): Byte {\n\
         \x20       var hasBoth = false; var hasRead = false; var hasWrite = false; var hasIndexer = false\n\
         \x20       var invalid = false\n\
         \x20       for (part in role.split(\",\").map { it.trim().lowercase() }) {\n\
         \x20           when (part) {\n\
         \x20               \"\" -> {}\n\
         \x20               \"both\" -> hasBoth = true\n\
         \x20               \"read\" -> hasRead = true\n\
         \x20               \"write\" -> hasWrite = true\n\
         \x20               \"indexer\" -> hasIndexer = true\n\
         \x20               else -> invalid = true\n\
         \x20           }\n\
         \x20       }\n\
         \x20       if (invalid) return 255.toByte()\n\
         \x20       return (when {\n\
         \x20           hasBoth || (hasRead && hasWrite) -> 0\n\
         \x20           hasRead -> 1\n\
         \x20           hasWrite -> 2\n\
         \x20           hasIndexer -> 3\n\
         \x20           else -> 255\n\
         \x20       }).toByte()\n\
         \x20   }\n",
    )
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
    // Build nested string/vector/table offsets before the table.
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
            FieldKind::RelayListEntryVec => {
                // Build each RelayListEntry table (url + marker) then a vector
                // of those table offsets.
                out.push_str(&format!(
                    "        val {n}Offset = run {{\n\
                     \x20           val entryOffsets = IntArray({n}.size) {{ i ->\n\
                     \x20               val (url, role) = {n}[i]\n\
                     \x20               val urlOff = fbb.createString(url)\n\
                     \x20               fbb.startTable(2)\n\
                     \x20               fbb.addOffset(0, urlOff, 0) // RelayListEntry slot 0: url\n\
                     \x20               fbb.addByte(1, relayMarkerByte(role), 0) // RelayListEntry slot 1: marker\n\
                     \x20               fbb.endTable()\n\
                     \x20           }}\n\
                     \x20           fbb.startVector(4, entryOffsets.size, 4)\n\
                     \x20           for (i in entryOffsets.size - 1 downTo 0) fbb.addOffset(entryOffsets[i])\n\
                     \x20           fbb.endVector()\n\
                     \x20       }}\n",
                    n = field.name
                ));
            }
            FieldKind::Uint | FieldKind::Ulong | FieldKind::UlongWithPresenceFlag { .. } => {}
        }
    }
    // Table: 1 (schema_version slot) + sum of each field's slot_count.
    let slot_total: usize = 1 + builder.fields.iter().map(|f| f.slot_count()).sum::<usize>();
    out.push_str(&format!("        fbb.startTable({slot_total})\n"));
    out.push_str(&format!(
        "        fbb.addInt(0, {}, 0) // slot 0: schema_version\n",
        contract.schema_version
    ));
    let mut slot = 1usize; // slot 0 = schema_version
    for field in builder.fields {
        match field.kind {
            FieldKind::Str | FieldKind::StrVec | FieldKind::RelayListEntryVec => {
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
            FieldKind::Ulong => {
                if field.optional {
                    out.push_str(&format!(
                        "        if ({n} != null) fbb.addLong({slot}, {n}, 0L) // slot {slot}: {n}\n",
                        n = field.name
                    ));
                } else {
                    out.push_str(&format!(
                        "        fbb.addLong({slot}, {n}, 0L) // slot {slot}: {n}\n",
                        n = field.name
                    ));
                }
            }
            FieldKind::UlongWithPresenceFlag { flag_name } => {
                let slot_flag = slot + 1;
                out.push_str(&format!(
                    "        if ({n} != null) {{\n\
                     \x20           fbb.addLong({slot}, {n}, 0L) // slot {slot}: {n}\n\
                     \x20           fbb.addBoolean({slot_flag}, true, false) // slot {slot_flag}: {flag}\n\
                     \x20       }}\n",
                    n = field.name,
                    slot = slot,
                    slot_flag = slot_flag,
                    flag = flag_name,
                ));
            }
        }
        slot += field.slot_count();
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

/// Kotlin parameter type for a field. `uint` → `Int` (FlatBuffers Kotlin
/// carries u32 as a signed `Int`); `ulong` → `Long` (u64 carried as signed
/// `Long` at the byte level).
fn kotlin_param_type(field: &PayloadField) -> String {
    match (field.kind, field.optional) {
        (FieldKind::Str, false) => "String".to_string(),
        (FieldKind::Str, true) => "String?".to_string(),
        (FieldKind::Uint, false) => "Int".to_string(),
        (FieldKind::Uint, true) => "Int?".to_string(),
        (FieldKind::StrVec, false) => "List<String>".to_string(),
        (FieldKind::StrVec, true) => "List<String>?".to_string(),
        (FieldKind::Ulong, false) => "Long".to_string(),
        (FieldKind::Ulong, true) => "Long?".to_string(),
        // UlongWithPresenceFlag is always presented as optional — the flag
        // encodes Some vs None so the parameter is always `T?`.
        (FieldKind::UlongWithPresenceFlag { .. }, _) => "Long?".to_string(),
        // RelayListEntry vector: list of (url, role) pairs.
        (FieldKind::RelayListEntryVec, _) => "List<Pair<String, String>>".to_string(),
    }
}

/// Render the full file for the default registry.
#[must_use]
pub fn render_default() -> String {
    render(ACTION_BUILDERS)
}

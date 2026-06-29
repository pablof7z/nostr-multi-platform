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
    ActionBuilder, FieldKind, DISPATCH_ENVELOPE_FILE_IDENTIFIER, DISPATCH_ENVELOPE_SCHEMA_VERSION,
};
use crate::action_builders::ActionBuilderRegistry;
// NIP-51 bookmark helpers: render_bookmark_update, is_bookmark_builder,
// is_bookmark_set_builder, swift_param_type. Module declared in parent
// `action_builders.rs` (sibling module) for 500-LOC cap compliance.
use super::swift_nip51::{
    is_bookmark_builder, is_bookmark_set_builder, render_bookmark_update, swift_param_type,
};

const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen action-builders --platform swift \\
//       --out apps/chirp/ios/Chirp/Bridge/Generated/ActionBuilders.generated.swift
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
    render_registry(&ActionBuilderRegistry::builtin_slice(builders))
}

/// Render the generated Swift action-builders for the given registry context.
#[must_use]
pub fn render_registry(registry: &ActionBuilderRegistry<'_>) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    out.push_str("public enum GeneratedActionBuilders {\n");
    out.push_str(&publish_signer_types());
    out.push_str(&envelope_helper());
    out.push_str(&relay_marker_byte_helper());
    for builder in registry.builders {
        out.push('\n');
        render_one(builder, registry, &mut out);
    }
    // The `nmp.publish` UNION builders (separate emitter — different encode
    // shape; see `swift_publish`). Only emitted for the default registry, which
    // is what `render_default` (and therefore the CLI + drift gate) uses.
    if registry.is_full_builtin() {
        crate::action_builders::swift_publish::render_publish(&mut out);
        // The `nmp.marmot` UNION builders (M14-1c / #2169 — 9-arm union).
        crate::action_builders::swift_marmot::render_marmot(&mut out);
    }
    out.push_str("}\n");
    out
}

fn publish_signer_types() -> String {
    "    public enum PublishSignerProvenance: String {\n\
     \x20       case appManaged = \"app_managed\"\n\
     \x20       case userSelected = \"user_selected\"\n\
     \x20       case protocolPinned = \"protocol_pinned\"\n\
     \x20       case diagnostic = \"diagnostic\"\n\
     \x20   }\n\n\
     \x20   public enum PublishSignerSelection {\n\
     \x20       case active\n\
     \x20       case registered(pubkey: String, provenance: PublishSignerProvenance)\n\
     \x20   }\n\n\
     \x20   public enum PublishRouteClass: String {\n\
     \x20       case manualOverride = \"manual_override\"\n\
     \x20       case groupHostPin = \"group_host_pin\"\n\
     \x20       case verifiedPrivateInbox = \"verified_private_inbox\"\n\
     \x20       case importedOrPresigned = \"imported_or_presigned\"\n\
     \x20       case diagnostic = \"diagnostic\"\n\
     \x20   }\n\n\
     \x20   public enum PublishTargetSelection {\n\
     \x20       case auto\n\
     \x20       case explicit(relays: [String], routeClass: PublishRouteClass)\n\
     \x20   }\n\n"
        .to_string()
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
         \x20   private static func relayMarkerByte(_ role: String) -> UInt8 {\n\
         \x20       var hasBoth = false; var hasRead = false; var hasWrite = false; var hasIndexer = false\n\
         \x20       var invalid = false\n\
         \x20       for part in role.split(separator: \",\").map({ $0.trimmingCharacters(in: .whitespaces).lowercased() }) {\n\
         \x20           switch part {\n\
         \x20           case \"\": break\n\
         \x20           case \"both\": hasBoth = true\n\
         \x20           case \"read\": hasRead = true\n\
         \x20           case \"write\": hasWrite = true\n\
         \x20           case \"indexer\": hasIndexer = true\n\
         \x20           default: invalid = true\n\
         \x20           }\n\
         \x20       }\n\
         \x20       if invalid { return 255 }\n\
         \x20       if hasBoth || (hasRead && hasWrite) { return 0 }\n\
         \x20       if hasRead { return 1 }\n\
         \x20       if hasWrite { return 2 }\n\
         \x20       if hasIndexer { return 3 }\n\
         \x20       return 255\n\
         \x20   }\n",
    )
}

/// Render one typed builder function.
fn render_one(builder: &ActionBuilder, registry: &ActionBuilderRegistry<'_>, out: &mut String) {
    if is_bookmark_builder(builder) {
        render_bookmark_update(builder, out);
        return;
    }
    if is_bookmark_set_builder(builder) {
        crate::action_builders::swift_bookmark_set::render_bookmark_set_update(builder, out);
        return;
    }
    let contract = registry.wire_contract_for(builder.namespace);
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
        out.push_str(&format!(
            "        {}: {}",
            field.name,
            swift_param_type(field)
        ));
    }
    out.push_str("\n    ) -> [UInt8] {\n");

    // Encode the payload table.
    out.push_str("        var fbb = FlatBufferBuilder()\n");
    // Create string/vector/table offsets first (FlatBuffers requires nested
    // objects to be finished before the table that references them).
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
            FieldKind::UintVec => {
                if field.optional {
                    out.push_str(&format!(
                        "        let {n}Offset: Offset = {{\n\
                         \x20           guard let values = {n}, !values.isEmpty else {{ return Offset() }}\n\
                         \x20           return fbb.createVector(values)\n\
                         \x20       }}()\n",
                        n = field.name
                    ));
                } else {
                    out.push_str(&format!(
                        "        let {n}Offset = fbb.createVector({n})\n",
                        n = field.name
                    ));
                }
            }
            FieldKind::RelayListEntryVec => {
                // Build each RelayListEntry table (url + marker) then a vector
                // of those entry offsets.
                out.push_str(&format!(
                    "        var {n}EntryOffsets: [Offset] = []\n\
                     \x20       for r in {n} {{\n\
                     \x20           let urlOff = fbb.create(string: r.url)\n\
                     \x20           let entryStart = fbb.startTable(with: 2)\n\
                     \x20           fbb.add(offset: urlOff, at: 4) // RelayListEntry slot 0: url\n\
                     \x20           fbb.add(element: Self.relayMarkerByte(r.role), def: UInt8(0), at: 6) // RelayListEntry slot 1: marker\n\
                     \x20           {n}EntryOffsets.append(Offset(offset: fbb.endTable(at: entryStart)))\n\
                     \x20       }}\n\
                     \x20       let {n}Offset = fbb.createVector(ofOffsets: {n}EntryOffsets)\n",
                    n = field.name
                ));
            }
            FieldKind::GroupRef => {
                // Build the GroupRef nested table (two required string slots) before
                // the parent table — FlatBuffers requires nested objects finished first.
                out.push_str(&format!(
                "        let {n}HostRelayUrlOffset = fbb.create(string: {n}.hostRelayUrl)\n\
                 \x20       let {n}LocalIdOffset = fbb.create(string: {n}.localId)\n\
                 \x20       let {n}TableStart = fbb.startTable(with: 2)\n\
                 \x20       fbb.add(offset: {n}HostRelayUrlOffset, at: 4) // GroupRef slot 0: host_relay_url\n\
                 \x20       fbb.add(offset: {n}LocalIdOffset, at: 6)       // GroupRef slot 1: local_id\n\
                 \x20       let {n}Offset = Offset(offset: fbb.endTable(at: {n}TableStart))\n",
                n = field.name
            ));
            }
            FieldKind::StringTagVec => {
                // Build a vector of StringTag tables (each wrapping a [string] values
                // vector). Always optional — nil/absent → Offset() → slot skipped.
                out.push_str(&format!(
                "        let {n}Offset: Offset = {{\n\
                 \x20           guard let tagRows = {n}, !tagRows.isEmpty else {{ return Offset() }}\n\
                 \x20           var tagOffsets: [Offset] = []\n\
                 \x20           for row in tagRows {{\n\
                 \x20               let valOffsets = row.map {{ fbb.create(string: $0) }}\n\
                 \x20               let valsVec = fbb.createVector(ofOffsets: valOffsets)\n\
                 \x20               let tagStart = fbb.startTable(with: 1)\n\
                 \x20               fbb.add(offset: valsVec, at: 4) // StringTag slot 0: values\n\
                 \x20               tagOffsets.append(Offset(offset: fbb.endTable(at: tagStart)))\n\
                 \x20           }}\n\
                 \x20           return fbb.createVector(ofOffsets: tagOffsets)\n\
                 \x20       }}()\n",
                n = field.name
            ));
            }
            FieldKind::Uint
            | FieldKind::Ulong
            | FieldKind::UlongWithPresenceFlag { .. }
            | FieldKind::Ubyte
            | FieldKind::Sbyte => {}
        }
    }
    // Table: 1 (schema_version slot) + sum of each field's slot_count.
    let slot_total: usize = 1 + builder.fields.iter().map(|f| f.slot_count()).sum::<usize>();
    out.push_str(&format!(
        "        let payloadStart = fbb.startTable(with: {slot_total})\n"
    ));
    out.push_str(&format!(
        "        fbb.add(element: UInt32({}), def: UInt32(0), at: 4) // slot 0: schema_version\n",
        contract.schema_version
    ));
    let mut slot = 1usize; // slot 0 = schema_version
    for field in builder.fields {
        let vtoffset = 4 + slot * 2;
        match field.kind {
            FieldKind::Str
            | FieldKind::StrVec
            | FieldKind::UintVec
            | FieldKind::RelayListEntryVec => {
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
            FieldKind::Ulong => {
                if field.optional {
                    out.push_str(&format!(
                        "        if let {n}Val = {n} {{ fbb.add(element: {n}Val, def: UInt64(0), at: {vt}) }} // slot {slot}: {n}\n",
                        n = field.name,
                        vt = vtoffset
                    ));
                } else {
                    out.push_str(&format!(
                        "        fbb.add(element: {n}, def: UInt64(0), at: {vt}) // slot {slot}: {n}\n",
                        n = field.name,
                        vt = vtoffset
                    ));
                }
            }
            FieldKind::UlongWithPresenceFlag { flag_name } => {
                let vt_flag = vtoffset + 2; // flag is the next slot
                out.push_str(&format!(
                    "        if let {n}Val = {n} {{\n\
                     \x20           fbb.add(element: {n}Val, def: UInt64(0), at: {vt}) // slot {slot}: {n}\n\
                     \x20           fbb.add(element: true, def: false, at: {vt_flag}) // slot {slot_flag}: {flag}\n\
                     \x20       }}\n",
                    n = field.name,
                    vt = vtoffset,
                    vt_flag = vt_flag,
                    slot = slot,
                    slot_flag = slot + 1,
                    flag = flag_name,
                ));
            }
            FieldKind::Ubyte => {
                out.push_str(&format!(
                    "        fbb.add(element: {n}, def: UInt8(0), at: {vt}) // slot {slot}: {n}\n",
                    n = field.name,
                    vt = vtoffset
                ));
            }
            FieldKind::Sbyte => {
                out.push_str(&format!(
                    "        fbb.add(element: {n}, def: Int8(0), at: {vt}) // slot {slot}: {n}\n",
                    n = field.name,
                    vt = vtoffset
                ));
            }
            FieldKind::GroupRef | FieldKind::StringTagVec => {
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
        }
        slot += field.slot_count();
    }
    out.push_str("        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))\n");
    out.push_str(&format!(
        "        fbb.finish(offset: payloadRoot, fileId: {:?})\n",
        contract.file_identifier
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

/// Render the full file for the default registry.
#[must_use]
pub fn render_default() -> String {
    render_registry(&ActionBuilderRegistry::builtin())
}

//! NIP-51 bookmark builder helpers for the Swift action-builder emitter.
//!
//! Extracted from `swift.rs` to keep each file under the 500-LOC hard cap.
//! Contains: `render_bookmark_update`, `is_bookmark_builder`,
//! `is_bookmark_set_builder`, and `swift_param_type`.

use crate::action_builders::registry::{ActionBuilder, FieldKind, PayloadField};
use crate::action_contract::contract_for;

pub(crate) fn is_bookmark_builder(builder: &ActionBuilder) -> bool {
    matches!(
        builder.namespace,
        "nmp.nip51.add_bookmark" | "nmp.nip51.remove_bookmark"
    )
}

pub(crate) fn is_bookmark_set_builder(builder: &ActionBuilder) -> bool {
    matches!(
        builder.namespace,
        "nmp.nip51.add_bookmark_set_item" | "nmp.nip51.remove_bookmark_set_item"
    )
}

pub(crate) fn render_bookmark_update(builder: &ActionBuilder, out: &mut String) {
    let contract = contract_for(builder.namespace);
    let method = builder.method;
    out.push_str(&format!("    /// {}\n", builder.doc));
    out.push_str(&format!(
        "    /// Builds the `{}` `DispatchEnvelope` bytes for the byte doorway.\n",
        builder.namespace
    ));
    out.push_str(&format!(
        "    public static func {method}(\n\
         \x20       correlationId: String,\n\
         \x20       accountPubkey: String,\n\
         \x20       itemKind: UInt8,\n\
         \x20       value: String,\n\
         \x20       relay: String?\n\
         \x20   ) -> [UInt8] {{\n"
    ));
    out.push_str("        var fbb = FlatBufferBuilder()\n");
    out.push_str("        let accountPubkeyOffset = fbb.create(string: accountPubkey)\n");
    out.push_str("        let valueOffset = fbb.create(string: value)\n");
    out.push_str(
        "        let relayOffset: Offset = relay.map { fbb.create(string: $0) } ?? Offset()\n",
    );
    out.push_str("        let itemStart = fbb.startTable(with: 3)\n");
    out.push_str("        fbb.add(element: itemKind, def: UInt8(0), at: 4) // slot 0: kind\n");
    out.push_str("        fbb.add(offset: valueOffset, at: 6) // slot 1: value\n");
    out.push_str(
        "        if relayOffset.o != 0 { fbb.add(offset: relayOffset, at: 8) } // slot 2: relay\n",
    );
    out.push_str("        let itemRoot = Offset(offset: fbb.endTable(at: itemStart))\n");
    out.push_str("        let payloadStart = fbb.startTable(with: 3)\n");
    out.push_str(&format!(
        "        fbb.add(element: UInt32({}), def: UInt32(0), at: 4) // slot 0: schema_version\n",
        contract.schema_version
    ));
    out.push_str("        fbb.add(offset: accountPubkeyOffset, at: 6) // slot 1: account_pubkey\n");
    out.push_str("        fbb.add(offset: itemRoot, at: 8) // slot 2: item\n");
    out.push_str("        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))\n");
    out.push_str(&format!(
        "        fbb.finish(offset: payloadRoot, fileId: {:?})\n",
        contract.file_identifier
    ));
    out.push_str("        let payload = fbb.sizedByteArray\n");
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
pub(crate) fn swift_param_type(field: &PayloadField) -> String {
    match (field.kind, field.optional) {
        (FieldKind::Str, false) => "String".to_string(),
        (FieldKind::Str, true) => "String?".to_string(),
        (FieldKind::Uint, false) => "UInt32".to_string(),
        (FieldKind::Uint, true) => "UInt32?".to_string(),
        (FieldKind::StrVec, false) => "[String]".to_string(),
        (FieldKind::StrVec, true) => "[String]?".to_string(),
        (FieldKind::UintVec, false) => "[UInt32]".to_string(),
        (FieldKind::UintVec, true) => "[UInt32]?".to_string(),
        (FieldKind::Ulong, false) => "UInt64".to_string(),
        (FieldKind::Ulong, true) => "UInt64?".to_string(),
        // UlongWithPresenceFlag is always presented as optional — the flag
        // encodes Some vs None so the parameter is always `T?`.
        (FieldKind::UlongWithPresenceFlag { .. }, _) => "UInt64?".to_string(),
        // RelayListEntry vector: named-tuple array (url + role string).
        (FieldKind::RelayListEntryVec, _) => "[(url: String, role: String)]".to_string(),
        // Ubyte scalar (u8) — used for FlatBuffers ubyte enum discriminants.
        (FieldKind::Ubyte, false) => "UInt8".to_string(),
        (FieldKind::Ubyte, true) => "UInt8?".to_string(),
        // Sbyte scalar (i8) — used for FlatBuffers byte enum discriminants.
        (FieldKind::Sbyte, false) => "Int8".to_string(),
        (FieldKind::Sbyte, true) => "Int8?".to_string(),
        // GroupRef nested table — two required string sub-fields.
        (FieldKind::GroupRef, _) => "(hostRelayUrl: String, localId: String)".to_string(),
        // StringTagVec — always optional array of string arrays.
        (FieldKind::StringTagVec, _) => "[[String]]?".to_string(),
    }
}

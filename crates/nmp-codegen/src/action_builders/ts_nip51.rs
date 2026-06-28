//! NIP-51 bookmark builder helpers for the TypeScript action-builder emitter.
//!
//! Extracted from `ts.rs` to keep each file under the 500-LOC hard cap.
//! Contains: `render_bookmark_set_update`, `is_bookmark_builder`,
//! `is_bookmark_set_builder`, and `ts_param_type`.

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
    out.push_str(&format!("  /** {} */\n", builder.doc));
    out.push_str(&format!(
        "  {}(\n\
         \x20   correlationId: string,\n\
         \x20   accountPubkey: string,\n\
         \x20   itemKind: number,\n\
         \x20   value: string,\n\
         \x20   relay: string | null,\n\
         \x20 ): Uint8Array {{\n",
        builder.method
    ));
    out.push_str("    const fbb = new flatbuffers.Builder(64);\n");
    out.push_str("    const accountPubkeyOffset = fbb.createString(accountPubkey);\n");
    out.push_str("    const valueOffset = fbb.createString(value);\n");
    out.push_str("    const relayOffset = relay === null ? 0 : fbb.createString(relay);\n");
    out.push_str("    fbb.startObject(3);\n");
    out.push_str("    fbb.addFieldInt8(0, itemKind, 0); // slot 0: kind\n");
    out.push_str("    fbb.addFieldOffset(1, valueOffset, 0); // slot 1: value\n");
    out.push_str(
        "    if (relayOffset !== 0) fbb.addFieldOffset(2, relayOffset, 0); // slot 2: relay\n",
    );
    out.push_str("    const itemRoot = fbb.endObject();\n");
    out.push_str("    fbb.startObject(3);\n");
    out.push_str(&format!(
        "    fbb.addFieldInt32(0, {}, 0); // slot 0: schema_version\n",
        contract.schema_version
    ));
    out.push_str("    fbb.addFieldOffset(1, accountPubkeyOffset, 0); // slot 1: account_pubkey\n");
    out.push_str("    fbb.addFieldOffset(2, itemRoot, 0); // slot 2: item\n");
    out.push_str("    const payloadRoot = fbb.endObject();\n");
    out.push_str(&format!(
        "    fbb.finish(payloadRoot, {:?});\n",
        contract.file_identifier
    ));
    out.push_str("    const payload = fbb.asUint8Array();\n");
    out.push_str(&format!(
        "    return encodeDispatchEnvelope(correlationId, {:?}, payload);\n",
        builder.namespace
    ));
    out.push_str("  },\n\n");
}

pub(crate) fn render_bookmark_set_update(builder: &ActionBuilder, out: &mut String) {
    let contract = contract_for(builder.namespace);
    out.push_str(&format!("  /** {} */\n", builder.doc));
    out.push_str(&format!(
        "  {}(\n\
         \x20   correlationId: string,\n\
         \x20   accountPubkey: string,\n\
         \x20   setKind: number,\n\
         \x20   identifier: string,\n\
         \x20   itemKind: number,\n\
         \x20   value: string,\n\
         \x20   relay: string | null,\n\
         \x20 ): Uint8Array {{\n",
        builder.method
    ));
    out.push_str("    const fbb = new flatbuffers.Builder(64);\n");
    out.push_str("    const accountPubkeyOffset = fbb.createString(accountPubkey);\n");
    out.push_str("    const identifierOffset = fbb.createString(identifier);\n");
    out.push_str("    const valueOffset = fbb.createString(value);\n");
    out.push_str("    const relayOffset = relay === null ? 0 : fbb.createString(relay);\n");
    // Build nested BookmarkItem table (3 slots: kind ubyte, value string, relay string)
    out.push_str("    fbb.startObject(3);\n");
    out.push_str("    fbb.addFieldInt8(0, itemKind, 0); // slot 0: kind\n");
    out.push_str("    fbb.addFieldOffset(1, valueOffset, 0); // slot 1: value\n");
    out.push_str(
        "    if (relayOffset !== 0) fbb.addFieldOffset(2, relayOffset, 0); // slot 2: relay\n",
    );
    out.push_str("    const itemRoot = fbb.endObject();\n");
    // Build BookmarkSetUpdatePayload root table (5 slots: schema_version, account_pubkey, set_kind, identifier, item)
    out.push_str("    fbb.startObject(5);\n");
    out.push_str(&format!(
        "    fbb.addFieldInt32(0, {}, 0); // slot 0: schema_version\n",
        contract.schema_version
    ));
    out.push_str("    fbb.addFieldOffset(1, accountPubkeyOffset, 0); // slot 1: account_pubkey\n");
    out.push_str("    fbb.addFieldInt8(2, setKind, 0); // slot 2: set_kind\n");
    out.push_str("    fbb.addFieldOffset(3, identifierOffset, 0); // slot 3: identifier\n");
    out.push_str("    fbb.addFieldOffset(4, itemRoot, 0); // slot 4: item\n");
    out.push_str("    const payloadRoot = fbb.endObject();\n");
    out.push_str(&format!(
        "    fbb.finish(payloadRoot, {:?});\n",
        contract.file_identifier
    ));
    out.push_str("    const payload = fbb.asUint8Array();\n");
    out.push_str(&format!(
        "    return encodeDispatchEnvelope(correlationId, {:?}, payload);\n",
        builder.namespace
    ));
    out.push_str("  },\n\n");
}

/// TypeScript parameter type for a field. An optional field is `T | null`
/// (mirrors the Swift `T?` / Kotlin `T?`); the builder omits the field when it
/// is `null`, so the Rust decoder reads `None`.
pub(crate) fn ts_param_type(field: &PayloadField) -> String {
    match (field.kind, field.optional) {
        (FieldKind::Str, false) => "string".to_string(),
        (FieldKind::Str, true) => "string | null".to_string(),
        (FieldKind::Uint, false) => "number".to_string(),
        (FieldKind::Uint, true) => "number | null".to_string(),
        (FieldKind::StrVec, false) => "string[]".to_string(),
        (FieldKind::StrVec, true) => "string[] | null".to_string(),
        (FieldKind::UintVec, false) => "number[]".to_string(),
        (FieldKind::UintVec, true) => "number[] | null".to_string(),
        (FieldKind::Ulong, false) => "bigint".to_string(),
        (FieldKind::Ulong, true) => "bigint | null".to_string(),
        // UlongWithPresenceFlag is always presented as optional.
        (FieldKind::UlongWithPresenceFlag { .. }, _) => "bigint | null".to_string(),
        // RelayListEntry vector: array of {url, role} objects.
        (FieldKind::RelayListEntryVec, _) => "Array<{ url: string; role: string }>".to_string(),
        // Ubyte scalar (u8) — used for FlatBuffers ubyte enum discriminants.
        (FieldKind::Ubyte, _) => "number".to_string(),
        // Sbyte scalar (i8) — used for FlatBuffers byte enum discriminants.
        (FieldKind::Sbyte, _) => "number".to_string(),
        // GroupRef nested table.
        (FieldKind::GroupRef, _) => "{ hostRelayUrl: string; localId: string }".to_string(),
        // StringTagVec — vector of tag rows.
        (FieldKind::StringTagVec, _) => "string[][] | null".to_string(),
    }
}

//! NIP-51 bookmark builder helpers for the Kotlin action-builder emitter.
//!
//! Extracted from `kotlin.rs` to keep each file under the 500-LOC hard cap.
//! Contains: `render_bookmark_update`, `is_bookmark_builder`,
//! `is_bookmark_set_builder`, and `kotlin_param_type`.

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
    out.push_str(&format!("    /// {}\n", builder.doc));
    out.push_str(&format!(
        "    /// Builds the `{}` `DispatchEnvelope` bytes for the byte doorway.\n",
        builder.namespace
    ));
    out.push_str(&format!(
        "    fun {}(\n\
         \x20       correlationId: String,\n\
         \x20       accountPubkey: String,\n\
         \x20       itemKind: Int,\n\
         \x20       value: String,\n\
         \x20       relay: String?,\n\
         \x20   ): ByteArray {{\n",
        builder.method
    ));
    out.push_str("        val fbb = FlatBufferBuilder()\n");
    out.push_str("        val accountPubkeyOffset = fbb.createString(accountPubkey)\n");
    out.push_str("        val valueOffset = fbb.createString(value)\n");
    out.push_str("        val relayOffset = relay?.let { fbb.createString(it) } ?: 0\n");
    out.push_str("        fbb.startTable(3)\n");
    out.push_str("        fbb.addByte(0, itemKind.toByte(), 0) // slot 0: kind\n");
    out.push_str("        fbb.addOffset(1, valueOffset, 0) // slot 1: value\n");
    out.push_str(
        "        if (relayOffset != 0) fbb.addOffset(2, relayOffset, 0) // slot 2: relay\n",
    );
    out.push_str("        val itemRoot = fbb.endTable()\n");
    out.push_str("        fbb.startTable(3)\n");
    out.push_str(&format!(
        "        fbb.addInt(0, {}, 0) // slot 0: schema_version\n",
        contract.schema_version
    ));
    out.push_str("        fbb.addOffset(1, accountPubkeyOffset, 0) // slot 1: account_pubkey\n");
    out.push_str("        fbb.addOffset(2, itemRoot, 0) // slot 2: item\n");
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
pub(crate) fn kotlin_param_type(field: &PayloadField) -> String {
    match (field.kind, field.optional) {
        (FieldKind::Str, false) => "String".to_string(),
        (FieldKind::Str, true) => "String?".to_string(),
        (FieldKind::Uint, false) => "Int".to_string(),
        (FieldKind::Uint, true) => "Int?".to_string(),
        (FieldKind::StrVec, false) => "List<String>".to_string(),
        (FieldKind::StrVec, true) => "List<String>?".to_string(),
        (FieldKind::UintVec, false) => "List<Int>".to_string(),
        (FieldKind::UintVec, true) => "List<Int>?".to_string(),
        (FieldKind::Ulong, false) => "Long".to_string(),
        (FieldKind::Ulong, true) => "Long?".to_string(),
        // UlongWithPresenceFlag is always presented as optional — the flag
        // encodes Some vs None so the parameter is always `T?`.
        (FieldKind::UlongWithPresenceFlag { .. }, _) => "Long?".to_string(),
        // RelayListEntry vector: list of (url, role) pairs.
        (FieldKind::RelayListEntryVec, _) => "List<Pair<String, String>>".to_string(),
        // Ubyte scalar (u8) — used for FlatBuffers ubyte enum discriminants.
        (FieldKind::Ubyte, false) => "Byte".to_string(),
        (FieldKind::Ubyte, true) => "Byte?".to_string(),
        // Sbyte scalar (i8) — used for FlatBuffers byte enum discriminants.
        (FieldKind::Sbyte, false) => "Byte".to_string(),
        (FieldKind::Sbyte, true) => "Byte?".to_string(),
        // GroupRef nested table — host passes a Pair<hostRelayUrl, localId>.
        (FieldKind::GroupRef, _) => "Pair<String, String>".to_string(),
        // StringTagVec — host passes a list of tag rows (each row = list of strings).
        (FieldKind::StringTagVec, _) => "List<List<String>>?".to_string(),
    }
}

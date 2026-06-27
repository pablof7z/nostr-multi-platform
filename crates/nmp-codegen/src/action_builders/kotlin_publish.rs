//! ADR-0064 §3 (#1783) — Kotlin emitter for the `nmp.publish` UNION builders.
//!
//! Split out of [`crate::action_builders::kotlin`] purely as a size-management
//! seam (AGENTS.md / V-12). This file hand-rolls the `PublishPayload` encode — a
//! nested body table (`PublishRaw` / `PublishProfile`) wrapped in the union root
//! — the byte-for-byte twin of `encode_publish_payload` in
//! `nmp_core::publish::wire`. Kotlin's `add*` takes a 0-indexed SLOT (not a
//! vtable byte offset, unlike Swift).

use crate::action_builders::registry::{BodyShape, PublishBuilder, PUBLISH_BUILDERS};
use crate::action_contract::{contract_for, PUBLISH_NAMESPACE};

/// Render every `nmp.publish` builder into `out`.
pub(crate) fn render_publish(out: &mut String) {
    for builder in PUBLISH_BUILDERS {
        out.push('\n');
        render_one(builder, out);
    }
}

fn render_one(builder: &PublishBuilder, out: &mut String) {
    let contract = contract_for(PUBLISH_NAMESPACE);
    out.push_str(&format!("    /// {}\n", builder.doc));
    out.push_str(&format!(
        "    /// Builds the `{PUBLISH_NAMESPACE}` `DispatchEnvelope` bytes (body \
         `{:?}`) for the byte doorway.\n",
        builder.body
    ));
    out.push_str(&format!("    fun {}(\n", builder.method));
    out.push_str("        correlationId: String,\n");
    match builder.body {
        BodyShape::PublishRaw => {
            out.push_str("        kind: Int,\n");
            out.push_str("        tags: List<List<String>>,\n");
            out.push_str("        content: String,\n");
            out.push_str("        relays: List<String>? = null,\n");
            out.push_str("        signerPubkey: String? = null,\n");
        }
        BodyShape::PublishProfile => {
            out.push_str("        fields: List<Pair<String, String>>,\n");
        }
        BodyShape::PublishReply => {
            out.push_str("        content: String,\n");
            out.push_str("        replyToEventId: String,\n");
            out.push_str("        relays: List<String>? = null,\n");
            out.push_str("        signerPubkey: String? = null,\n");
        }
    }
    out.push_str("    ): ByteArray {\n");
    out.push_str("        val fbb = FlatBufferBuilder()\n");

    match builder.body {
        BodyShape::PublishRaw => render_raw_body(out),
        BodyShape::PublishProfile => render_profile_body(out),
        BodyShape::PublishReply => render_reply_body(out),
    }

    // PublishPayload root: schema_version (slot 0), body_type ubyte (slot 1),
    // body offset (slot 2).
    out.push_str(&format!(
        "        fbb.startTable(3)\n\
         \x20       fbb.addInt(0, {PUBLISH_SCHEMA_VERSION}, 0) // slot 0: schema_version\n\
         \x20       fbb.addByte(1, {body_type}.toByte(), 0) // slot 1: body_type\n\
         \x20       fbb.addOffset(2, bodyOffset, 0) // slot 2: body\n",
        PUBLISH_SCHEMA_VERSION = contract.schema_version,
        body_type = builder.body_type
    ));
    out.push_str("        val payloadRoot = fbb.endTable()\n");
    out.push_str(&format!(
        "        fbb.finish(payloadRoot, {:?})\n",
        contract.file_identifier
    ));
    out.push_str("        val payload = fbb.sizedByteArray()\n");
    out.push_str(&format!(
        "        return encodeDispatchEnvelope(\n\
         \x20           correlationId = correlationId,\n\
         \x20           actionNamespace = {PUBLISH_NAMESPACE:?},\n\
         \x20           payload = payload,\n\
         \x20       )\n"
    ));
    out.push_str("    }\n");
}

/// A helper that builds a `[string]` vector from a `List<String>` and leaves the
/// offset on the named val. FlatBuffers vectors are built last element first.
fn string_vector(out: &mut String, val_name: &str, source: &str) {
    out.push_str(&format!(
        "        val {val_name} = run {{\n\
         \x20           val offsets = IntArray({source}.size) {{ i -> fbb.createString({source}[i]) }}\n\
         \x20           fbb.startVector(4, offsets.size, 4)\n\
         \x20           for (i in offsets.size - 1 downTo 0) fbb.addOffset(offsets[i])\n\
         \x20           fbb.endVector()\n\
         \x20       }}\n"
    ));
}

/// Encode a `PublishTarget`: `null`/empty `relays` → `Auto` (`explicit = false`);
/// a non-empty set → `Explicit`. Leaves the offset on `targetOffset`. Matches
/// `build_target` in `nmp_core::publish::wire`.
fn render_target(out: &mut String) {
    out.push_str(
        "        val targetRelays = relays ?: emptyList()\n\
         \x20       val explicit = targetRelays.isNotEmpty()\n",
    );
    string_vector(out, "targetRelaysVec", "targetRelays");
    out.push_str(
        "        fbb.startTable(2)\n\
         \x20       fbb.addBoolean(0, explicit, false) // slot 0: explicit\n\
         \x20       fbb.addOffset(1, targetRelaysVec, 0) // slot 1: relays\n\
         \x20       val targetOffset = fbb.endTable()\n",
    );
}

fn render_raw_body(out: &mut String) {
    // Build each TagRow table (its own `values:[string]` vector), collect the
    // offsets, then the `[TagRow]` vector — all before the PublishRaw table.
    out.push_str(
        "        val tagRowOffsets = IntArray(tags.size) { r ->\n\
         \x20           val row = tags[r]\n\
         \x20           val valueOffsets = IntArray(row.size) { i -> fbb.createString(row[i]) }\n\
         \x20           fbb.startVector(4, valueOffsets.size, 4)\n\
         \x20           for (i in valueOffsets.size - 1 downTo 0) fbb.addOffset(valueOffsets[i])\n\
         \x20           val valuesVec = fbb.endVector()\n\
         \x20           fbb.startTable(1)\n\
         \x20           fbb.addOffset(0, valuesVec, 0) // slot 0: values\n\
         \x20           fbb.endTable()\n\
         \x20       }\n\
         \x20       val tagsVec = run {\n\
         \x20           fbb.startVector(4, tagRowOffsets.size, 4)\n\
         \x20           for (i in tagRowOffsets.size - 1 downTo 0) fbb.addOffset(tagRowOffsets[i])\n\
         \x20           fbb.endVector()\n\
         \x20       }\n\
         \x20       val contentOffset = fbb.createString(content)\n\
         \x20       val signerPubkeyOffset = signerPubkey?.let { fbb.createString(it) } ?: 0\n",
    );
    render_target(out);
    // PublishRaw: kind (slot 0), tags (slot 1), content (slot 2), target
    // (slot 3), signer_pubkey (slot 4, optional).
    out.push_str(
        "        fbb.startTable(5)\n\
         \x20       fbb.addInt(0, kind, 0) // slot 0: kind\n\
         \x20       fbb.addOffset(1, tagsVec, 0) // slot 1: tags\n\
         \x20       fbb.addOffset(2, contentOffset, 0) // slot 2: content\n\
         \x20       fbb.addOffset(3, targetOffset, 0) // slot 3: target\n\
         \x20       if (signerPubkeyOffset != 0) fbb.addOffset(4, signerPubkeyOffset, 0) // slot 4: signer_pubkey\n\
         \x20       val bodyOffset = fbb.endTable()\n",
    );
}

fn render_profile_body(out: &mut String) {
    out.push_str(
        "        val profileFieldOffsets = IntArray(fields.size) { i ->\n\
         \x20           val keyOffset = fbb.createString(fields[i].first)\n\
         \x20           val valueOffset = fbb.createString(fields[i].second)\n\
         \x20           fbb.startTable(2)\n\
         \x20           fbb.addOffset(0, keyOffset, 0) // slot 0: key\n\
         \x20           fbb.addOffset(1, valueOffset, 0) // slot 1: value\n\
         \x20           fbb.endTable()\n\
         \x20       }\n\
         \x20       val fieldsVec = run {\n\
         \x20           fbb.startVector(4, profileFieldOffsets.size, 4)\n\
         \x20           for (i in profileFieldOffsets.size - 1 downTo 0) fbb.addOffset(profileFieldOffsets[i])\n\
         \x20           fbb.endVector()\n\
         \x20       }\n\
         \x20       fbb.startTable(1)\n\
         \x20       fbb.addOffset(0, fieldsVec, 0) // slot 0: fields\n\
         \x20       val bodyOffset = fbb.endTable()\n",
    );
}

fn render_reply_body(out: &mut String) {
    out.push_str(
        "        val contentOffset = fbb.createString(content)\n\
         \x20       val replyToEventIdOffset = fbb.createString(replyToEventId)\n\
         \x20       val signerPubkeyOffset = signerPubkey?.let { fbb.createString(it) } ?: 0\n",
    );
    render_target(out);
    // PublishReply: content (slot 0), reply_to_event_id (slot 1), target
    // (slot 2), signer_pubkey (slot 3, optional).
    out.push_str(
        "        fbb.startTable(4)\n\
         \x20       fbb.addOffset(0, contentOffset, 0) // slot 0: content\n\
         \x20       fbb.addOffset(1, replyToEventIdOffset, 0) // slot 1: reply_to_event_id\n\
         \x20       fbb.addOffset(2, targetOffset, 0) // slot 2: target\n\
         \x20       if (signerPubkeyOffset != 0) fbb.addOffset(3, signerPubkeyOffset, 0) // slot 3: signer_pubkey\n\
         \x20       val bodyOffset = fbb.endTable()\n",
    );
}

//! ADR-0064 §3 / #2169 (M14-1c) — Kotlin emitter for the `nmp.marmot` UNION
//! builders.
//!
//! Split out of [`crate::action_builders::kotlin`] as a size-management seam
//! (AGENTS.md / V-12). Hand-rolls the `MarmotActionPayload` encode — one arm
//! body table wrapped in the union root — the byte-for-byte twin of
//! `MarmotAction::encode` in `nmp_marmot::wire::action_payload`.
//!
//! Kotlin's `add*` takes a 0-indexed SLOT (not a vtable byte offset, unlike
//! Swift). Slot 0 of `MarmotActionPayload` is `schema_version`, slot 1 is
//! `body_type` (ubyte), slot 2 is `body` offset.
//!
//! `inviteeNpubs: List<String>?` — `null` → absent (Rust `None`); non-null →
//! present vector (even if empty, Rust `Some(vec![])`).

use crate::action_builders::registry::{MarmotBodyShape, MarmotBuilder, MARMOT_BUILDERS, MARMOT_NAMESPACE};
use crate::action_contract::contract_for;

/// Render every `nmp.marmot` builder into `out`.
pub(crate) fn render_marmot(out: &mut String) {
    for builder in MARMOT_BUILDERS {
        out.push('\n');
        render_one(builder, out);
    }
}

fn render_one(builder: &MarmotBuilder, out: &mut String) {
    let contract = contract_for(MARMOT_NAMESPACE);
    out.push_str(&format!("    /// {}\n", builder.doc));
    out.push_str(&format!(
        "    /// Builds the `{MARMOT_NAMESPACE}` `DispatchEnvelope` bytes (body \
         `{:?}`) for the byte doorway.\n",
        builder.body
    ));
    out.push_str(&format!("    fun {}(\n", builder.method));
    out.push_str("        correlationId: String,\n");
    emit_params(builder, out);
    out.push_str("    ): ByteArray {\n");
    out.push_str("        val fbb = FlatBufferBuilder()\n");
    emit_body(builder, out);
    // MarmotActionPayload root: schema_version (slot 0), body_type ubyte
    // (slot 1), body offset (slot 2).
    out.push_str(&format!(
        "        fbb.startTable(3)\n\
         \x20       fbb.addInt(0, {schema_version}, 0) // slot 0: schema_version\n\
         \x20       fbb.addByte(1, {body_type}.toByte(), 0) // slot 1: body_type\n\
         \x20       fbb.addOffset(2, bodyOffset, 0) // slot 2: body\n",
        schema_version = contract.schema_version,
        body_type = builder.body_type,
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
         \x20           actionNamespace = {MARMOT_NAMESPACE:?},\n\
         \x20           payload = payload,\n\
         \x20       )\n"
    ));
    out.push_str("    }\n");
}

/// Helper: build a `[string]` vector from `source` (a `List<String>` val)
/// and assign the offset to `val_name`.
fn str_vec(out: &mut String, val_name: &str, source: &str) {
    out.push_str(&format!(
        "        val {val_name} = run {{\n\
         \x20           val offs = IntArray({source}.size) {{ i -> fbb.createString({source}[i]) }}\n\
         \x20           fbb.startVector(4, offs.size, 4)\n\
         \x20           for (i in offs.size - 1 downTo 0) fbb.addOffset(offs[i])\n\
         \x20           fbb.endVector()\n\
         \x20       }}\n"
    ));
}

fn emit_params(builder: &MarmotBuilder, out: &mut String) {
    match builder.body {
        MarmotBodyShape::PublishKeyPackage => {
            out.push_str("        relays: List<String> = emptyList(),\n");
        }
        MarmotBodyShape::CreateGroup => {
            out.push_str("        name: String,\n");
            out.push_str("        description: String = \"\",\n");
            out.push_str("        inviteeText: String? = null,\n");
            out.push_str("        inviteeNpubs: List<String>? = null,\n");
            out.push_str("        signedKeyPackageEventsJson: List<String> = emptyList(),\n");
            out.push_str("        relays: List<String> = emptyList(),\n");
        }
        MarmotBodyShape::Invite => {
            out.push_str("        groupIdHex: String,\n");
            out.push_str("        inviteeText: String? = null,\n");
            out.push_str("        inviteeNpubs: List<String>? = null,\n");
            out.push_str("        signedKeyPackageEventsJson: List<String> = emptyList(),\n");
        }
        MarmotBodyShape::Send => {
            out.push_str("        groupIdHex: String,\n");
            out.push_str("        text: String,\n");
        }
        MarmotBodyShape::Leave => {
            out.push_str("        groupIdHex: String,\n");
        }
        MarmotBodyShape::Remove => {
            out.push_str("        groupIdHex: String,\n");
            out.push_str("        memberNpubs: List<String> = emptyList(),\n");
        }
        MarmotBodyShape::AcceptWelcome => {
            out.push_str("        welcomeIdHex: String,\n");
        }
        MarmotBodyShape::DeclineWelcome => {
            out.push_str("        welcomeIdHex: String,\n");
        }
        MarmotBodyShape::ClearPending => {
            out.push_str("        groupIdHex: String,\n");
        }
    }
}

fn emit_body(builder: &MarmotBuilder, out: &mut String) {
    match builder.body {
        // ── PublishKeyPackage ────────────────────────────────────────────────
        // { relays:[string] } — slot 0
        MarmotBodyShape::PublishKeyPackage => {
            str_vec(out, "relaysVec", "relays");
            out.push_str(
                "        fbb.startTable(1)\n\
                 \x20       fbb.addOffset(0, relaysVec, 0) // slot 0: relays\n\
                 \x20       val bodyOffset = fbb.endTable()\n",
            );
        }
        // ── CreateGroup ──────────────────────────────────────────────────────
        // { name (req), description, invitee_text, invitee_npubs,
        //   signed_key_package_events_json, relays } — slots 0-5
        MarmotBodyShape::CreateGroup => {
            // relays + signed_key_package_events_json are NON-OPTIONAL [string]:
            // ALWAYS present (even when empty) to match the Rust encoder (golden
            // byte parity — #2169 / nip02 convention). `str_vec` emits present-always.
            str_vec(out, "relaysVec", "relays");
            str_vec(out, "jsonVec", "signedKeyPackageEventsJson");
            out.push_str(
                "        // inviteeNpubs: null → absent (None); non-null → present vector (even if empty)\n\
                 \x20       val npubsVec = inviteeNpubs?.let { npubs ->\n\
                 \x20           val offs = IntArray(npubs.size) { i -> fbb.createString(npubs[i]) }\n\
                 \x20           fbb.startVector(4, offs.size, 4)\n\
                 \x20           for (i in offs.size - 1 downTo 0) fbb.addOffset(offs[i])\n\
                 \x20           fbb.endVector()\n\
                 \x20       } ?: 0\n\
                 \x20       val inviteeTextOffset = inviteeText?.let { fbb.createString(it) } ?: 0\n\
                 \x20       val descOffset = if (description.isEmpty()) 0 else fbb.createString(description)\n\
                 \x20       val nameOffset = fbb.createString(name)\n\
                 \x20       fbb.startTable(6)\n\
                 \x20       fbb.addOffset(0, nameOffset, 0) // slot 0: name (required)\n\
                 \x20       if (descOffset != 0) fbb.addOffset(1, descOffset, 0) // slot 1: description\n\
                 \x20       if (inviteeTextOffset != 0) fbb.addOffset(2, inviteeTextOffset, 0) // slot 2: invitee_text\n\
                 \x20       if (npubsVec != 0) fbb.addOffset(3, npubsVec, 0) // slot 3: invitee_npubs\n\
                 \x20       fbb.addOffset(4, jsonVec, 0) // slot 4: signed_key_package_events_json\n\
                 \x20       fbb.addOffset(5, relaysVec, 0) // slot 5: relays\n\
                 \x20       val bodyOffset = fbb.endTable()\n",
            );
        }
        // ── Invite ───────────────────────────────────────────────────────────
        // { group_id_hex (req), invitee_text, invitee_npubs,
        //   signed_key_package_events_json } — slots 0-3
        MarmotBodyShape::Invite => {
            // signed_key_package_events_json is NON-OPTIONAL [string]: ALWAYS present
            // (even when empty) to match the Rust encoder (golden byte parity — #2169).
            str_vec(out, "jsonVec", "signedKeyPackageEventsJson");
            out.push_str(
                "        val npubsVec = inviteeNpubs?.let { npubs ->\n\
                 \x20           val offs = IntArray(npubs.size) { i -> fbb.createString(npubs[i]) }\n\
                 \x20           fbb.startVector(4, offs.size, 4)\n\
                 \x20           for (i in offs.size - 1 downTo 0) fbb.addOffset(offs[i])\n\
                 \x20           fbb.endVector()\n\
                 \x20       } ?: 0\n\
                 \x20       val inviteeTextOffset = inviteeText?.let { fbb.createString(it) } ?: 0\n\
                 \x20       val gidOffset = fbb.createString(groupIdHex)\n\
                 \x20       fbb.startTable(4)\n\
                 \x20       fbb.addOffset(0, gidOffset, 0) // slot 0: group_id_hex (required)\n\
                 \x20       if (inviteeTextOffset != 0) fbb.addOffset(1, inviteeTextOffset, 0) // slot 1: invitee_text\n\
                 \x20       if (npubsVec != 0) fbb.addOffset(2, npubsVec, 0) // slot 2: invitee_npubs\n\
                 \x20       fbb.addOffset(3, jsonVec, 0) // slot 3: signed_key_package_events_json\n\
                 \x20       val bodyOffset = fbb.endTable()\n",
            );
        }
        // ── Send ─────────────────────────────────────────────────────────────
        // { group_id_hex (req), text (req) } — slots 0-1
        MarmotBodyShape::Send => {
            out.push_str(
                "        val textOffset = fbb.createString(text)\n\
                 \x20       val gidOffset = fbb.createString(groupIdHex)\n\
                 \x20       fbb.startTable(2)\n\
                 \x20       fbb.addOffset(0, gidOffset, 0) // slot 0: group_id_hex (required)\n\
                 \x20       fbb.addOffset(1, textOffset, 0) // slot 1: text (required)\n\
                 \x20       val bodyOffset = fbb.endTable()\n",
            );
        }
        // ── Leave ────────────────────────────────────────────────────────────
        // { group_id_hex (req) } — slot 0
        MarmotBodyShape::Leave => {
            out.push_str(
                "        val gidOffset = fbb.createString(groupIdHex)\n\
                 \x20       fbb.startTable(1)\n\
                 \x20       fbb.addOffset(0, gidOffset, 0) // slot 0: group_id_hex (required)\n\
                 \x20       val bodyOffset = fbb.endTable()\n",
            );
        }
        // ── Remove ───────────────────────────────────────────────────────────
        // { group_id_hex (req), member_npubs:[string] } — slots 0-1
        MarmotBodyShape::Remove => {
            str_vec(out, "npubsVec", "memberNpubs");
            out.push_str(
                "        val gidOffset = fbb.createString(groupIdHex)\n\
                 \x20       fbb.startTable(2)\n\
                 \x20       fbb.addOffset(0, gidOffset, 0) // slot 0: group_id_hex (required)\n\
                 \x20       fbb.addOffset(1, npubsVec, 0) // slot 1: member_npubs\n\
                 \x20       val bodyOffset = fbb.endTable()\n",
            );
        }
        // ── AcceptWelcome ────────────────────────────────────────────────────
        // { welcome_id_hex (req) } — slot 0
        MarmotBodyShape::AcceptWelcome => {
            out.push_str(
                "        val widOffset = fbb.createString(welcomeIdHex)\n\
                 \x20       fbb.startTable(1)\n\
                 \x20       fbb.addOffset(0, widOffset, 0) // slot 0: welcome_id_hex (required)\n\
                 \x20       val bodyOffset = fbb.endTable()\n",
            );
        }
        // ── DeclineWelcome ───────────────────────────────────────────────────
        // { welcome_id_hex (req) } — slot 0
        MarmotBodyShape::DeclineWelcome => {
            out.push_str(
                "        val widOffset = fbb.createString(welcomeIdHex)\n\
                 \x20       fbb.startTable(1)\n\
                 \x20       fbb.addOffset(0, widOffset, 0) // slot 0: welcome_id_hex (required)\n\
                 \x20       val bodyOffset = fbb.endTable()\n",
            );
        }
        // ── ClearPending ─────────────────────────────────────────────────────
        // { group_id_hex (req) } — slot 0
        MarmotBodyShape::ClearPending => {
            out.push_str(
                "        val gidOffset = fbb.createString(groupIdHex)\n\
                 \x20       fbb.startTable(1)\n\
                 \x20       fbb.addOffset(0, gidOffset, 0) // slot 0: group_id_hex (required)\n\
                 \x20       val bodyOffset = fbb.endTable()\n",
            );
        }
    }
}

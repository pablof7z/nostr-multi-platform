//! ADR-0064 §3 / #2169 (M14-1c) — TypeScript emitter for the `nmp.marmot`
//! UNION builders.
//!
//! Split out of [`crate::action_builders::ts`] as a size-management seam
//! (AGENTS.md / V-12). Hand-rolls the `MarmotActionPayload` encode — one arm
//! body table wrapped in the union root — the byte-for-byte twin of
//! `MarmotAction::encode` in `nmp_marmot::wire::action_payload`.
//!
//! The TS `Builder.add*` takes a 0-indexed SLOT (identical to Kotlin).
//! Slot 0 of `MarmotActionPayload` is `schema_version`, slot 1 is
//! `body_type`, slot 2 is `body` offset.
//!
//! `inviteeNpubs: string[] | null` — `null` → absent (Rust `None`); non-null
//! → present vector (even if empty, Rust `Some(vec![])`).

use crate::action_builders::registry::{MarmotBodyShape, MarmotBuilder, MARMOT_BUILDERS, MARMOT_NAMESPACE};
use crate::action_contract::contract_for;

/// Render every `nmp.marmot` builder into `out` (as methods on the
/// `GeneratedActionBuilders` object literal).
pub(crate) fn render_marmot(out: &mut String) {
    for builder in MARMOT_BUILDERS {
        render_one(builder, out);
    }
}

fn render_one(builder: &MarmotBuilder, out: &mut String) {
    let contract = contract_for(MARMOT_NAMESPACE);
    out.push_str(&format!("  /** {} */\n", builder.doc));
    out.push_str(&format!("  {}(\n", builder.method));
    out.push_str("    correlationId: string,\n");
    emit_params(builder, out);
    out.push_str("  ): Uint8Array {\n");
    out.push_str("    const fbb = new flatbuffers.Builder(64);\n");
    emit_body(builder, out);
    // MarmotActionPayload root: schema_version (slot 0), body_type (slot 1),
    // body offset (slot 2).
    out.push_str(&format!(
        "    fbb.startObject(3);\n\
         \x20   fbb.addFieldInt32(0, {schema_version}, 0); // slot 0: schema_version\n\
         \x20   fbb.addFieldInt8(1, {body_type}, 0); // slot 1: body_type\n\
         \x20   fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body\n",
        schema_version = contract.schema_version,
        body_type = builder.body_type,
    ));
    out.push_str("    const payloadRoot = fbb.endObject();\n");
    out.push_str(&format!(
        "    fbb.finish(payloadRoot, {:?});\n",
        contract.file_identifier
    ));
    out.push_str("    const payload = fbb.asUint8Array();\n");
    out.push_str(&format!(
        "    return encodeDispatchEnvelope(correlationId, {MARMOT_NAMESPACE:?}, payload);\n"
    ));
    out.push_str("  },\n\n");
}

fn emit_params(builder: &MarmotBuilder, out: &mut String) {
    match builder.body {
        MarmotBodyShape::PublishKeyPackage => {
            out.push_str("    relays: string[] = [],\n");
        }
        MarmotBodyShape::CreateGroup => {
            out.push_str("    name: string,\n");
            out.push_str("    description: string = \"\",\n");
            out.push_str("    inviteeText: string | null = null,\n");
            out.push_str("    inviteeNpubs: string[] | null = null,\n");
            out.push_str("    signedKeyPackageEventsJson: string[] = [],\n");
            out.push_str("    relays: string[] = [],\n");
        }
        MarmotBodyShape::Invite => {
            out.push_str("    groupIdHex: string,\n");
            out.push_str("    inviteeText: string | null = null,\n");
            out.push_str("    inviteeNpubs: string[] | null = null,\n");
            out.push_str("    signedKeyPackageEventsJson: string[] = [],\n");
        }
        MarmotBodyShape::Send => {
            out.push_str("    groupIdHex: string,\n");
            out.push_str("    text: string,\n");
        }
        MarmotBodyShape::Leave => {
            out.push_str("    groupIdHex: string,\n");
        }
        MarmotBodyShape::Remove => {
            out.push_str("    groupIdHex: string,\n");
            out.push_str("    memberNpubs: string[] = [],\n");
        }
        MarmotBodyShape::AcceptWelcome => {
            out.push_str("    welcomeIdHex: string,\n");
        }
        MarmotBodyShape::DeclineWelcome => {
            out.push_str("    welcomeIdHex: string,\n");
        }
        MarmotBodyShape::ClearPending => {
            out.push_str("    groupIdHex: string,\n");
        }
    }
}

/// Build a `[string]` vector from an array and leave its offset on the stack
/// as a `const`. The caller names the const via `val_name`.
fn str_vec_stmt(out: &mut String, val_name: &str, source: &str) {
    out.push_str(&format!(
        "    const {val_name} = stringVector(fbb, {source});\n"
    ));
}

fn emit_body(builder: &MarmotBuilder, out: &mut String) {
    match builder.body {
        // ── PublishKeyPackage ────────────────────────────────────────────────
        // { relays:[string] } — slot 0
        MarmotBodyShape::PublishKeyPackage => {
            str_vec_stmt(out, "relaysVec", "relays");
            out.push_str(
                "    fbb.startObject(1);\n\
                 \x20   fbb.addFieldOffset(0, relaysVec, 0); // slot 0: relays\n\
                 \x20   const bodyOffset = fbb.endObject();\n",
            );
        }
        // ── CreateGroup ──────────────────────────────────────────────────────
        // { name (req), description, invitee_text, invitee_npubs,
        //   signed_key_package_events_json, relays } — slots 0-5
        MarmotBodyShape::CreateGroup => {
            // relays + signed_key_package_events_json are NON-OPTIONAL [string]:
            // ALWAYS present (even when empty) to match the Rust encoder (golden
            // byte parity — #2169 / nip02 convention). `stringVector` is present-always.
            str_vec_stmt(out, "relaysVec", "relays");
            str_vec_stmt(out, "jsonVec", "signedKeyPackageEventsJson");
            out.push_str(
                "    // inviteeNpubs: null → absent (None); non-null → present vector (even if empty)\n\
                 \x20   const npubsVec = inviteeNpubs === null ? 0 : stringVector(fbb, inviteeNpubs);\n\
                 \x20   const inviteeTextOffset = inviteeText === null ? 0 : fbb.createString(inviteeText);\n\
                 \x20   const descOffset = description === \"\" ? 0 : fbb.createString(description);\n\
                 \x20   const nameOffset = fbb.createString(name);\n\
                 \x20   fbb.startObject(6);\n\
                 \x20   fbb.addFieldOffset(0, nameOffset, 0); // slot 0: name (required)\n\
                 \x20   if (descOffset !== 0) fbb.addFieldOffset(1, descOffset, 0); // slot 1: description\n\
                 \x20   if (inviteeTextOffset !== 0) fbb.addFieldOffset(2, inviteeTextOffset, 0); // slot 2: invitee_text\n\
                 \x20   if (npubsVec !== 0) fbb.addFieldOffset(3, npubsVec, 0); // slot 3: invitee_npubs\n\
                 \x20   fbb.addFieldOffset(4, jsonVec, 0); // slot 4: signed_key_package_events_json\n\
                 \x20   fbb.addFieldOffset(5, relaysVec, 0); // slot 5: relays\n\
                 \x20   const bodyOffset = fbb.endObject();\n",
            );
        }
        // ── Invite ───────────────────────────────────────────────────────────
        // { group_id_hex (req), invitee_text, invitee_npubs,
        //   signed_key_package_events_json } — slots 0-3
        MarmotBodyShape::Invite => {
            // signed_key_package_events_json is NON-OPTIONAL [string]: ALWAYS present
            // (even when empty) to match the Rust encoder (golden byte parity — #2169).
            str_vec_stmt(out, "jsonVec", "signedKeyPackageEventsJson");
            out.push_str(
                "    const npubsVec = inviteeNpubs === null ? 0 : stringVector(fbb, inviteeNpubs);\n\
                 \x20   const inviteeTextOffset = inviteeText === null ? 0 : fbb.createString(inviteeText);\n\
                 \x20   const gidOffset = fbb.createString(groupIdHex);\n\
                 \x20   fbb.startObject(4);\n\
                 \x20   fbb.addFieldOffset(0, gidOffset, 0); // slot 0: group_id_hex (required)\n\
                 \x20   if (inviteeTextOffset !== 0) fbb.addFieldOffset(1, inviteeTextOffset, 0); // slot 1: invitee_text\n\
                 \x20   if (npubsVec !== 0) fbb.addFieldOffset(2, npubsVec, 0); // slot 2: invitee_npubs\n\
                 \x20   fbb.addFieldOffset(3, jsonVec, 0); // slot 3: signed_key_package_events_json\n\
                 \x20   const bodyOffset = fbb.endObject();\n",
            );
        }
        // ── Send ─────────────────────────────────────────────────────────────
        // { group_id_hex (req), text (req) } — slots 0-1
        MarmotBodyShape::Send => {
            out.push_str(
                "    const textOffset = fbb.createString(text);\n\
                 \x20   const gidOffset = fbb.createString(groupIdHex);\n\
                 \x20   fbb.startObject(2);\n\
                 \x20   fbb.addFieldOffset(0, gidOffset, 0); // slot 0: group_id_hex (required)\n\
                 \x20   fbb.addFieldOffset(1, textOffset, 0); // slot 1: text (required)\n\
                 \x20   const bodyOffset = fbb.endObject();\n",
            );
        }
        // ── Leave ────────────────────────────────────────────────────────────
        // { group_id_hex (req) } — slot 0
        MarmotBodyShape::Leave => {
            out.push_str(
                "    const gidOffset = fbb.createString(groupIdHex);\n\
                 \x20   fbb.startObject(1);\n\
                 \x20   fbb.addFieldOffset(0, gidOffset, 0); // slot 0: group_id_hex (required)\n\
                 \x20   const bodyOffset = fbb.endObject();\n",
            );
        }
        // ── Remove ───────────────────────────────────────────────────────────
        // { group_id_hex (req), member_npubs:[string] } — slots 0-1
        MarmotBodyShape::Remove => {
            str_vec_stmt(out, "npubsVec", "memberNpubs");
            out.push_str(
                "    const gidOffset = fbb.createString(groupIdHex);\n\
                 \x20   fbb.startObject(2);\n\
                 \x20   fbb.addFieldOffset(0, gidOffset, 0); // slot 0: group_id_hex (required)\n\
                 \x20   fbb.addFieldOffset(1, npubsVec, 0); // slot 1: member_npubs\n\
                 \x20   const bodyOffset = fbb.endObject();\n",
            );
        }
        // ── AcceptWelcome ────────────────────────────────────────────────────
        // { welcome_id_hex (req) } — slot 0
        MarmotBodyShape::AcceptWelcome => {
            out.push_str(
                "    const widOffset = fbb.createString(welcomeIdHex);\n\
                 \x20   fbb.startObject(1);\n\
                 \x20   fbb.addFieldOffset(0, widOffset, 0); // slot 0: welcome_id_hex (required)\n\
                 \x20   const bodyOffset = fbb.endObject();\n",
            );
        }
        // ── DeclineWelcome ───────────────────────────────────────────────────
        // { welcome_id_hex (req) } — slot 0
        MarmotBodyShape::DeclineWelcome => {
            out.push_str(
                "    const widOffset = fbb.createString(welcomeIdHex);\n\
                 \x20   fbb.startObject(1);\n\
                 \x20   fbb.addFieldOffset(0, widOffset, 0); // slot 0: welcome_id_hex (required)\n\
                 \x20   const bodyOffset = fbb.endObject();\n",
            );
        }
        // ── ClearPending ─────────────────────────────────────────────────────
        // { group_id_hex (req) } — slot 0
        MarmotBodyShape::ClearPending => {
            out.push_str(
                "    const gidOffset = fbb.createString(groupIdHex);\n\
                 \x20   fbb.startObject(1);\n\
                 \x20   fbb.addFieldOffset(0, gidOffset, 0); // slot 0: group_id_hex (required)\n\
                 \x20   const bodyOffset = fbb.endObject();\n",
            );
        }
    }
}

//! ADR-0064 §3 / #2169 (M14-1c) — Swift emitter for the `nmp.marmot` UNION
//! builders.
//!
//! Split out of [`crate::action_builders::swift`] as a size-management seam
//! (AGENTS.md / V-12). This file hand-rolls the `MarmotActionPayload` encode
//! — one nested arm body table wrapped in the union root — the byte-for-byte
//! twin of `MarmotAction::encode` in `nmp_marmot::wire::action_payload`.
//!
//! `schema_version` (slot 0 / vt 4 of the **root** `MarmotActionPayload`)
//! precedes the union discriminant (slot 1 / vt 6) and body offset (slot 2 /
//! vt 8). Each arm's body slots are local to the body table and start at
//! vt 4 (slot 0 within the body table).
//!
//! `invitee_npubs: [String]?` uses Swift's `nil` to distinguish `None` from
//! `Some([])`: the generated method takes an `invitee_npubs: [String]?`
//! parameter; when non-nil an offset is always emitted (even for empty) so
//! the Rust decoder can tell `None` from `Some(vec![])`.

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
    out.push_str(&format!("    public static func {}(\n", builder.method));
    out.push_str("        correlationId: String");
    emit_params(builder, out);
    out.push_str("\n    ) -> [UInt8] {\n");
    out.push_str("        var fbb = FlatBufferBuilder()\n");
    emit_body(builder, out);
    // MarmotActionPayload root: schema_version (slot 0 / vt 4), body_type ubyte
    // (slot 1 / vt 6), body offset (slot 2 / vt 8).
    out.push_str(&format!(
        "        let payloadStart = fbb.startTable(with: 3)\n\
         \x20       fbb.add(element: UInt32({schema_version}), def: UInt32(0), at: 4) // slot 0: schema_version\n\
         \x20       fbb.add(element: UInt8({body_type}), def: UInt8(0), at: 6) // slot 1: body_type\n\
         \x20       fbb.add(offset: bodyOffset, at: 8) // slot 2: body\n",
        schema_version = contract.schema_version,
        body_type = builder.body_type,
    ));
    out.push_str("        let payloadRoot = Offset(offset: fbb.endTable(at: payloadStart))\n");
    out.push_str(&format!(
        "        fbb.finish(offset: payloadRoot, fileId: {:?})\n",
        contract.file_identifier
    ));
    out.push_str("        let payload = fbb.sizedByteArray\n");
    out.push_str(&format!(
        "        return encodeDispatchEnvelope(\n\
         \x20           correlationId: correlationId,\n\
         \x20           actionNamespace: {MARMOT_NAMESPACE:?},\n\
         \x20           payload: payload\n\
         \x20       )\n"
    ));
    out.push_str("    }\n");
}

fn emit_params(builder: &MarmotBuilder, out: &mut String) {
    match builder.body {
        MarmotBodyShape::PublishKeyPackage => {
            out.push_str(",\n        relays: [String] = []");
        }
        MarmotBodyShape::CreateGroup => {
            out.push_str(",\n        name: String");
            out.push_str(",\n        description: String = \"\"");
            out.push_str(",\n        inviteeText: String? = nil");
            out.push_str(",\n        inviteeNpubs: [String]? = nil");
            out.push_str(",\n        signedKeyPackageEventsJson: [String] = []");
            out.push_str(",\n        relays: [String] = []");
        }
        MarmotBodyShape::Invite => {
            out.push_str(",\n        groupIdHex: String");
            out.push_str(",\n        inviteeText: String? = nil");
            out.push_str(",\n        inviteeNpubs: [String]? = nil");
            out.push_str(",\n        signedKeyPackageEventsJson: [String] = []");
        }
        MarmotBodyShape::Send => {
            out.push_str(",\n        groupIdHex: String");
            out.push_str(",\n        text: String");
        }
        MarmotBodyShape::Leave => {
            out.push_str(",\n        groupIdHex: String");
        }
        MarmotBodyShape::Remove => {
            out.push_str(",\n        groupIdHex: String");
            out.push_str(",\n        memberNpubs: [String] = []");
        }
        MarmotBodyShape::AcceptWelcome => {
            out.push_str(",\n        welcomeIdHex: String");
        }
        MarmotBodyShape::DeclineWelcome => {
            out.push_str(",\n        welcomeIdHex: String");
        }
        MarmotBodyShape::ClearPending => {
            out.push_str(",\n        groupIdHex: String");
        }
    }
}

fn emit_body(builder: &MarmotBuilder, out: &mut String) {
    match builder.body {
        // ── PublishKeyPackage ────────────────────────────────────────────────
        // { relays:[string] } — slot 0 / vt 4
        MarmotBodyShape::PublishKeyPackage => {
            out.push_str(
                "        let relayOffsets = relays.map { fbb.create(string: $0) }\n\
                 \x20       let relaysVec = fbb.createVector(ofOffsets: relayOffsets)\n\
                 \x20       let bodyStart = fbb.startTable(with: 1)\n\
                 \x20       fbb.add(offset: relaysVec, at: 4) // slot 0: relays\n\
                 \x20       let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))\n",
            );
        }
        // ── CreateGroup ──────────────────────────────────────────────────────
        // { name (req), description, invitee_text, invitee_npubs, signed_key_package_events_json, relays }
        // slots 0-5 / vt 4-14
        MarmotBodyShape::CreateGroup => {
            out.push_str(
                "        // Build offsets for nested objects FIRST (FlatBuffers bottom-up).\n\
                 \x20       // relays + signed_key_package_events_json are NON-OPTIONAL [string]:\n\
                 \x20       // ALWAYS present (even when empty) to match the Rust encoder (golden\n\
                 \x20       // byte parity — #2169 / nip02 convention).\n\
                 \x20       let relayOffsets = relays.map { fbb.create(string: $0) }\n\
                 \x20       let relaysVec = fbb.createVector(ofOffsets: relayOffsets)\n\
                 \x20       let jsonOffsets = signedKeyPackageEventsJson.map { fbb.create(string: $0) }\n\
                 \x20       let jsonVec = fbb.createVector(ofOffsets: jsonOffsets)\n\
                 \x20       // inviteeNpubs: nil → absent (None); non-nil → present vector (even if empty)\n\
                 \x20       let npubsVec: Offset? = inviteeNpubs.map { npubs in\n\
                 \x20           let offs = npubs.map { fbb.create(string: $0) }\n\
                 \x20           return Offset(offset: fbb.createVector(ofOffsets: offs).o)\n\
                 \x20       }\n\
                 \x20       let inviteeTextOffset: Offset? = inviteeText.map { fbb.create(string: $0) }\n\
                 \x20       let descOffset: Offset? = description.isEmpty ? nil : Optional(fbb.create(string: description))\n\
                 \x20       let nameOffset = fbb.create(string: name)\n\
                 \x20       let bodyStart = fbb.startTable(with: 6)\n\
                 \x20       fbb.add(offset: nameOffset, at: 4) // slot 0: name (required)\n\
                 \x20       if let descOffset { fbb.add(offset: descOffset, at: 6) } // slot 1: description\n\
                 \x20       if let inviteeTextOffset { fbb.add(offset: inviteeTextOffset, at: 8) } // slot 2: invitee_text\n\
                 \x20       if let npubsVec { fbb.add(offset: npubsVec, at: 10) } // slot 3: invitee_npubs\n\
                 \x20       fbb.add(offset: jsonVec, at: 12) // slot 4: signed_key_package_events_json\n\
                 \x20       fbb.add(offset: relaysVec, at: 14) // slot 5: relays\n\
                 \x20       let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))\n",
            );
        }
        // ── Invite ───────────────────────────────────────────────────────────
        // { group_id_hex (req), invitee_text, invitee_npubs, signed_key_package_events_json }
        // slots 0-3 / vt 4-10
        MarmotBodyShape::Invite => {
            out.push_str(
                "        // signed_key_package_events_json is NON-OPTIONAL [string]: ALWAYS present\n\
                 \x20       // (even when empty) to match the Rust encoder (golden byte parity — #2169).\n\
                 \x20       let jsonOffsets = signedKeyPackageEventsJson.map { fbb.create(string: $0) }\n\
                 \x20       let jsonVec = fbb.createVector(ofOffsets: jsonOffsets)\n\
                 \x20       let npubsVec: Offset? = inviteeNpubs.map { npubs in\n\
                 \x20           let offs = npubs.map { fbb.create(string: $0) }\n\
                 \x20           return Offset(offset: fbb.createVector(ofOffsets: offs).o)\n\
                 \x20       }\n\
                 \x20       let inviteeTextOffset: Offset? = inviteeText.map { fbb.create(string: $0) }\n\
                 \x20       let gidOffset = fbb.create(string: groupIdHex)\n\
                 \x20       let bodyStart = fbb.startTable(with: 4)\n\
                 \x20       fbb.add(offset: gidOffset, at: 4) // slot 0: group_id_hex (required)\n\
                 \x20       if let inviteeTextOffset { fbb.add(offset: inviteeTextOffset, at: 6) } // slot 1: invitee_text\n\
                 \x20       if let npubsVec { fbb.add(offset: npubsVec, at: 8) } // slot 2: invitee_npubs\n\
                 \x20       fbb.add(offset: jsonVec, at: 10) // slot 3: signed_key_package_events_json\n\
                 \x20       let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))\n",
            );
        }
        // ── Send ─────────────────────────────────────────────────────────────
        // { group_id_hex (req), text (req) } — slots 0-1 / vt 4-6
        MarmotBodyShape::Send => {
            out.push_str(
                "        let textOffset = fbb.create(string: text)\n\
                 \x20       let gidOffset = fbb.create(string: groupIdHex)\n\
                 \x20       let bodyStart = fbb.startTable(with: 2)\n\
                 \x20       fbb.add(offset: gidOffset, at: 4) // slot 0: group_id_hex (required)\n\
                 \x20       fbb.add(offset: textOffset, at: 6) // slot 1: text (required)\n\
                 \x20       let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))\n",
            );
        }
        // ── Leave ────────────────────────────────────────────────────────────
        // { group_id_hex (req) } — slot 0 / vt 4
        MarmotBodyShape::Leave => {
            out.push_str(
                "        let gidOffset = fbb.create(string: groupIdHex)\n\
                 \x20       let bodyStart = fbb.startTable(with: 1)\n\
                 \x20       fbb.add(offset: gidOffset, at: 4) // slot 0: group_id_hex (required)\n\
                 \x20       let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))\n",
            );
        }
        // ── Remove ───────────────────────────────────────────────────────────
        // { group_id_hex (req), member_npubs:[string] } — slots 0-1 / vt 4-6
        MarmotBodyShape::Remove => {
            out.push_str(
                "        let npubOffsets = memberNpubs.map { fbb.create(string: $0) }\n\
                 \x20       let npubsVec = fbb.createVector(ofOffsets: npubOffsets)\n\
                 \x20       let gidOffset = fbb.create(string: groupIdHex)\n\
                 \x20       let bodyStart = fbb.startTable(with: 2)\n\
                 \x20       fbb.add(offset: gidOffset, at: 4) // slot 0: group_id_hex (required)\n\
                 \x20       fbb.add(offset: npubsVec, at: 6) // slot 1: member_npubs\n\
                 \x20       let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))\n",
            );
        }
        // ── AcceptWelcome ────────────────────────────────────────────────────
        // { welcome_id_hex (req) } — slot 0 / vt 4
        MarmotBodyShape::AcceptWelcome => {
            out.push_str(
                "        let widOffset = fbb.create(string: welcomeIdHex)\n\
                 \x20       let bodyStart = fbb.startTable(with: 1)\n\
                 \x20       fbb.add(offset: widOffset, at: 4) // slot 0: welcome_id_hex (required)\n\
                 \x20       let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))\n",
            );
        }
        // ── DeclineWelcome ───────────────────────────────────────────────────
        // { welcome_id_hex (req) } — slot 0 / vt 4
        MarmotBodyShape::DeclineWelcome => {
            out.push_str(
                "        let widOffset = fbb.create(string: welcomeIdHex)\n\
                 \x20       let bodyStart = fbb.startTable(with: 1)\n\
                 \x20       fbb.add(offset: widOffset, at: 4) // slot 0: welcome_id_hex (required)\n\
                 \x20       let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))\n",
            );
        }
        // ── ClearPending ─────────────────────────────────────────────────────
        // { group_id_hex (req) } — slot 0 / vt 4
        MarmotBodyShape::ClearPending => {
            out.push_str(
                "        let gidOffset = fbb.create(string: groupIdHex)\n\
                 \x20       let bodyStart = fbb.startTable(with: 1)\n\
                 \x20       fbb.add(offset: gidOffset, at: 4) // slot 0: group_id_hex (required)\n\
                 \x20       let bodyOffset = Offset(offset: fbb.endTable(at: bodyStart))\n",
            );
        }
    }
}

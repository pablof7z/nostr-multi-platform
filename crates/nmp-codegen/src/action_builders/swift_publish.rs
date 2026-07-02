//! ADR-0071 §3 (#1783) — Swift emitter for the `nmp.publish` UNION builders.
//!
//! Split out of [`crate::action_builders::swift`] purely as a size-management
//! seam (AGENTS.md / V-12): the flat-table emitter and this union emitter
//! together would exceed the 500-LOC ceiling. This file hand-rolls the
//! `PublishPayload` encode — a nested body table (`PublishRaw` / `PublishProfile`)
//! wrapped in the union root — the byte-for-byte twin of `encode_publish_payload`
//! in `nmp_core::publish::wire`.
//!
//! The body tables nest further tables (`PublishTarget`, `TagRow`,
//! `ProfileField`), so the shape is matched on the closed
//! [`crate::action_builders::registry::BodyShape`] enum rather than the generic
//! flat-table [`PayloadField`] list.

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
    if matches!(builder.body, BodyShape::PublishRaw) {
        out.push_str(
            "    /// Requires typed signer selection and route provenance for explicit targets; not the starter happy path.\n",
        );
    }
    out.push_str(&format!(
        "    /// Builds the `{PUBLISH_NAMESPACE}` `DispatchEnvelope` bytes (body \
         `{:?}`) for the byte doorway.\n",
        builder.body
    ));
    out.push_str(&format!("    public static func {}(\n", builder.method));
    out.push_str("        correlationId: String,\n");
    match builder.body {
        BodyShape::PublishRaw => {
            out.push_str("        kind: UInt32,\n");
            out.push_str("        tags: [[String]],\n");
            out.push_str("        content: String,\n");
            out.push_str("        target: PublishTargetSelection = .auto,\n");
            out.push_str("        signer: PublishSignerSelection = .active\n");
        }
        BodyShape::PublishProfile => {
            out.push_str("        fields: [(String, String)]\n");
        }
        BodyShape::PublishReply => {
            out.push_str("        content: String,\n");
            out.push_str("        replyToEventId: String,\n");
            out.push_str("        target: PublishTargetSelection = .auto,\n");
            out.push_str("        signer: PublishSignerSelection = .active\n");
        }
    }
    out.push_str("    ) -> [UInt8] {\n");
    out.push_str("        var fbb = FlatBufferBuilder()\n");

    match builder.body {
        BodyShape::PublishRaw => render_raw_body(out),
        BodyShape::PublishProfile => render_profile_body(out),
        BodyShape::PublishReply => render_reply_body(out),
    }

    // PublishPayload root: schema_version (slot 0 / vt 4), body_type ubyte
    // (slot 1 / vt 6), body offset (slot 2 / vt 8).
    out.push_str(&format!(
        "        let payloadStart = fbb.startTable(with: 3)\n\
         \x20       fbb.add(element: UInt32({PUBLISH_SCHEMA_VERSION}), def: UInt32(0), at: 4) // slot 0: schema_version\n\
         \x20       fbb.add(element: UInt8({body_type}), def: UInt8(0), at: 6) // slot 1: body_type\n\
         \x20       fbb.add(offset: bodyOffset, at: 8) // slot 2: body\n",
        PUBLISH_SCHEMA_VERSION = contract.schema_version,
        body_type = builder.body_type
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
         \x20           actionNamespace: {PUBLISH_NAMESPACE:?},\n\
         \x20           payload: payload\n\
         \x20       )\n"
    ));
    out.push_str("    }\n");
}

/// Encode a `PublishTarget`. Auto omits route provenance; explicit targets
/// require callers to name both relays and route class in `PublishTargetSelection`.
fn render_target(out: &mut String) {
    out.push_str(
        "        let targetOffset: Offset = {\n\
         \x20           let explicit: Bool\n\
         \x20           let targetRelays: [String]\n\
         \x20           let routeClass: PublishRouteClass?\n\
         \x20           switch target {\n\
         \x20           case .auto:\n\
         \x20               explicit = false\n\
         \x20               targetRelays = []\n\
         \x20               routeClass = nil\n\
         \x20           case .explicit(let relays, let cls):\n\
         \x20               explicit = true\n\
         \x20               targetRelays = relays\n\
         \x20               routeClass = cls\n\
         \x20           }\n\
         \x20           let relayOffsets = targetRelays.map { fbb.create(string: $0) }\n\
         \x20           let relaysVec = fbb.createVector(ofOffsets: relayOffsets)\n\
         \x20           let routeClassOffset = routeClass.map { fbb.create(string: $0.rawValue) } ?? Offset()\n\
         \x20           let start = fbb.startTable(with: 3)\n\
         \x20           fbb.add(element: explicit, def: false, at: 4) // slot 0: explicit\n\
         \x20           fbb.add(offset: relaysVec, at: 6) // slot 1: relays\n\
         \x20           if routeClassOffset.o != 0 { fbb.add(offset: routeClassOffset, at: 8) } // slot 2: route_class\n\
         \x20           return Offset(offset: fbb.endTable(at: start))\n\
         \x20       }()\n",
    );
}

fn render_signer(out: &mut String) {
    out.push_str(
        "        let signerOffset: Offset = {\n\
         \x20           switch signer {\n\
         \x20           case .active:\n\
         \x20               return Offset()\n\
         \x20           case .registered(let pubkey, let provenance):\n\
         \x20               let signerPubkeyOffset = fbb.create(string: pubkey)\n\
         \x20               let signerProvenanceOffset = fbb.create(string: provenance.rawValue)\n\
         \x20               let start = fbb.startTable(with: 3)\n\
         \x20               fbb.add(element: UInt8(1), def: UInt8(0), at: 4) // slot 0: mode (Registered)\n\
         \x20               fbb.add(offset: signerPubkeyOffset, at: 6) // slot 1: pubkey\n\
         \x20               fbb.add(offset: signerProvenanceOffset, at: 8) // slot 2: provenance\n\
         \x20               return Offset(offset: fbb.endTable(at: start))\n\
         \x20           }\n\
         \x20       }()\n",
    );
}

fn render_raw_body(out: &mut String) {
    // Nested objects (tags rows, target, content/signer strings) must be built
    // before the PublishRaw table that references them.
    out.push_str(
        "        let tagRowOffsets: [Offset] = tags.map { row in\n\
         \x20           let valueOffsets = row.map { fbb.create(string: $0) }\n\
         \x20           let valuesVec = fbb.createVector(ofOffsets: valueOffsets)\n\
         \x20           let start = fbb.startTable(with: 1)\n\
         \x20           fbb.add(offset: valuesVec, at: 4) // slot 0: values\n\
         \x20           return Offset(offset: fbb.endTable(at: start))\n\
         \x20       }\n\
         \x20       let tagsVec = fbb.createVector(ofOffsets: tagRowOffsets)\n\
         \x20       let contentOffset = fbb.create(string: content)\n",
    );
    render_signer(out);
    render_target(out);
    // PublishRaw: kind (slot 0 / vt 4), tags (slot 1 / vt 6), content
    // (slot 2 / vt 8), target (slot 3 / vt 10), signer (slot 4 / vt 12).
    out.push_str(
        "        let rawStart = fbb.startTable(with: 5)\n\
         \x20       fbb.add(element: kind, def: UInt32(0), at: 4) // slot 0: kind\n\
         \x20       fbb.add(offset: tagsVec, at: 6) // slot 1: tags\n\
         \x20       fbb.add(offset: contentOffset, at: 8) // slot 2: content\n\
         \x20       fbb.add(offset: targetOffset, at: 10) // slot 3: target\n\
         \x20       if signerOffset.o != 0 { fbb.add(offset: signerOffset, at: 12) } // slot 4: signer\n\
         \x20       let bodyOffset = Offset(offset: fbb.endTable(at: rawStart))\n",
    );
}

fn render_profile_body(out: &mut String) {
    out.push_str(
        "        let profileFieldOffsets: [Offset] = fields.map { (key, value) in\n\
         \x20           let keyOffset = fbb.create(string: key)\n\
         \x20           let valueOffset = fbb.create(string: value)\n\
         \x20           let start = fbb.startTable(with: 2)\n\
         \x20           fbb.add(offset: keyOffset, at: 4) // slot 0: key\n\
         \x20           fbb.add(offset: valueOffset, at: 6) // slot 1: value\n\
         \x20           return Offset(offset: fbb.endTable(at: start))\n\
         \x20       }\n\
         \x20       let fieldsVec = fbb.createVector(ofOffsets: profileFieldOffsets)\n\
         \x20       let profileStart = fbb.startTable(with: 1)\n\
         \x20       fbb.add(offset: fieldsVec, at: 4) // slot 0: fields\n\
         \x20       let bodyOffset = Offset(offset: fbb.endTable(at: profileStart))\n",
    );
}

fn render_reply_body(out: &mut String) {
    out.push_str(
        "        let contentOffset = fbb.create(string: content)\n\
         \x20       let replyToEventIdOffset = fbb.create(string: replyToEventId)\n",
    );
    render_signer(out);
    render_target(out);
    // PublishReply: content (slot 0 / vt 4), reply_to_event_id (slot 1 / vt 6),
    // target (slot 2 / vt 8), signer (slot 3 / vt 10).
    out.push_str(
        "        let replyStart = fbb.startTable(with: 4)\n\
         \x20       fbb.add(offset: contentOffset, at: 4) // slot 0: content\n\
         \x20       fbb.add(offset: replyToEventIdOffset, at: 6) // slot 1: reply_to_event_id\n\
         \x20       fbb.add(offset: targetOffset, at: 8) // slot 2: target\n\
         \x20       if signerOffset.o != 0 { fbb.add(offset: signerOffset, at: 10) } // slot 3: signer\n\
         \x20       let bodyOffset = Offset(offset: fbb.endTable(at: replyStart))\n",
    );
}

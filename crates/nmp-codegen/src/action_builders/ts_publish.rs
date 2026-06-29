//! ADR-0064 §3 (#1776) — TypeScript emitter for the `nmp.publish` UNION builders.
//!
//! Split out of [`crate::action_builders::ts`] purely as a size-management seam
//! (AGENTS.md / V-12). This file hand-rolls the `PublishPayload` encode — a
//! nested body table (`PublishRaw` / `PublishProfile`) wrapped in the union root
//! — the byte-for-byte twin of `encode_publish_payload` in
//! `nmp_core::publish::wire`. The TS `Builder.add*` takes a 0-indexed SLOT (not
//! a vtable byte offset, unlike Swift) — identical to the Kotlin runtime.
//!
//! The body tables nest further tables (`PublishTarget`, `TagRow`,
//! `ProfileField`), so the shape is matched on the closed
//! [`crate::action_builders::registry::BodyShape`] enum rather than the generic
//! flat-table [`PayloadField`] list.

use crate::action_builders::registry::{BodyShape, PublishBuilder, PUBLISH_BUILDERS};
use crate::action_contract::{contract_for, PUBLISH_NAMESPACE};

/// Render every `nmp.publish` builder into `out` (as methods on the
/// `GeneratedActionBuilders` object literal).
pub(crate) fn render_publish(out: &mut String) {
    for builder in PUBLISH_BUILDERS {
        render_one(builder, out);
    }
}

fn render_one(builder: &PublishBuilder, out: &mut String) {
    let contract = contract_for(PUBLISH_NAMESPACE);
    out.push_str(&format!("  /** {} */\n", builder.doc));
    out.push_str(&format!("  {}(\n", builder.method));
    out.push_str("    correlationId: string,\n");
    match builder.body {
        BodyShape::PublishRaw => {
            out.push_str("    kind: number,\n");
            out.push_str("    tags: string[][],\n");
            out.push_str("    content: string,\n");
            out.push_str("    relays: string[] | null = null,\n");
            out.push_str("    signerPubkey: string | null = null,\n");
        }
        BodyShape::PublishProfile => {
            out.push_str("    fields: Array<[string, string]>,\n");
        }
        BodyShape::PublishReply => {
            out.push_str("    content: string,\n");
            out.push_str("    replyToEventId: string,\n");
            out.push_str("    relays: string[] | null = null,\n");
            out.push_str("    signerPubkey: string | null = null,\n");
        }
    }
    out.push_str("  ): Uint8Array {\n");
    out.push_str("    const fbb = new flatbuffers.Builder(64);\n");

    match builder.body {
        BodyShape::PublishRaw => render_raw_body(out),
        BodyShape::PublishProfile => render_profile_body(out),
        BodyShape::PublishReply => render_reply_body(out),
    }

    // PublishPayload root: schema_version (slot 0), body_type ubyte (slot 1),
    // body offset (slot 2).
    out.push_str(&format!(
        "    fbb.startObject(3);\n\
         \x20   fbb.addFieldInt32(0, {PUBLISH_SCHEMA_VERSION}, 0); // slot 0: schema_version\n\
         \x20   fbb.addFieldInt8(1, {body_type}, 0); // slot 1: body_type\n\
         \x20   fbb.addFieldOffset(2, bodyOffset, 0); // slot 2: body\n",
        PUBLISH_SCHEMA_VERSION = contract.schema_version,
        body_type = builder.body_type
    ));
    out.push_str("    const payloadRoot = fbb.endObject();\n");
    out.push_str(&format!(
        "    fbb.finish(payloadRoot, {:?});\n",
        contract.file_identifier
    ));
    out.push_str("    const payload = fbb.asUint8Array();\n");
    out.push_str(&format!(
        "    return encodeDispatchEnvelope(correlationId, {PUBLISH_NAMESPACE:?}, payload);\n"
    ));
    out.push_str("  },\n\n");
}

/// Encode a `PublishTarget`: `null`/empty `relays` → `Auto` (`explicit = false`);
/// a non-empty set → `Explicit` manual override. Leaves the offset on
/// `targetOffset`. Matches `build_target` in `nmp_core::publish::wire`.
fn render_target(out: &mut String) {
    out.push_str(
        "    const targetRelays = relays ?? [];\n\
         \x20   const explicit = targetRelays.length > 0;\n\
         \x20   const targetRelaysVec = stringVector(fbb, targetRelays);\n\
         \x20   const routeClassOffset = fbb.createString(\"manual_override\");\n\
         \x20   fbb.startObject(3);\n\
         \x20   fbb.addFieldInt8(0, explicit ? 1 : 0, 0); // slot 0: explicit\n\
         \x20   fbb.addFieldOffset(1, targetRelaysVec, 0); // slot 1: relays\n\
         \x20   if (explicit) fbb.addFieldOffset(2, routeClassOffset, 0); // slot 2: route_class\n\
         \x20   const targetOffset = fbb.endObject();\n",
    );
}

fn render_raw_body(out: &mut String) {
    // Build each TagRow table (its own `values:[string]` vector), collect the
    // offsets, then the `[TagRow]` vector — all before the PublishRaw table.
    out.push_str(
        "    const tagRowOffsets = tags.map((row) => {\n\
         \x20     const valuesVec = stringVector(fbb, row);\n\
         \x20     fbb.startObject(1);\n\
         \x20     fbb.addFieldOffset(0, valuesVec, 0); // slot 0: values\n\
         \x20     return fbb.endObject();\n\
         \x20   });\n\
         \x20   fbb.startVector(4, tagRowOffsets.length, 4);\n\
         \x20   for (let i = tagRowOffsets.length - 1; i >= 0; i--) fbb.addOffset(tagRowOffsets[i]!);\n\
         \x20   const tagsVec = fbb.endVector();\n\
         \x20   const contentOffset = fbb.createString(content);\n\
         \x20   const signerPubkeyOffset = signerPubkey === null ? 0 : fbb.createString(signerPubkey);\n",
    );
    render_target(out);
    // PublishRaw: kind (slot 0), tags (slot 1), content (slot 2), target
    // (slot 3), signer_pubkey (slot 4, optional).
    out.push_str(
        "    fbb.startObject(5);\n\
         \x20   fbb.addFieldInt32(0, kind, 0); // slot 0: kind\n\
         \x20   fbb.addFieldOffset(1, tagsVec, 0); // slot 1: tags\n\
         \x20   fbb.addFieldOffset(2, contentOffset, 0); // slot 2: content\n\
         \x20   fbb.addFieldOffset(3, targetOffset, 0); // slot 3: target\n\
         \x20   if (signerPubkeyOffset !== 0) fbb.addFieldOffset(4, signerPubkeyOffset, 0); // slot 4: signer_pubkey\n\
         \x20   const bodyOffset = fbb.endObject();\n",
    );
}

fn render_profile_body(out: &mut String) {
    out.push_str(
        "    const profileFieldOffsets = fields.map(([key, value]) => {\n\
         \x20     const keyOffset = fbb.createString(key);\n\
         \x20     const valueOffset = fbb.createString(value);\n\
         \x20     fbb.startObject(2);\n\
         \x20     fbb.addFieldOffset(0, keyOffset, 0); // slot 0: key\n\
         \x20     fbb.addFieldOffset(1, valueOffset, 0); // slot 1: value\n\
         \x20     return fbb.endObject();\n\
         \x20   });\n\
         \x20   fbb.startVector(4, profileFieldOffsets.length, 4);\n\
         \x20   for (let i = profileFieldOffsets.length - 1; i >= 0; i--) fbb.addOffset(profileFieldOffsets[i]!);\n\
         \x20   const fieldsVec = fbb.endVector();\n\
         \x20   fbb.startObject(1);\n\
         \x20   fbb.addFieldOffset(0, fieldsVec, 0); // slot 0: fields\n\
         \x20   const bodyOffset = fbb.endObject();\n",
    );
}

fn render_reply_body(out: &mut String) {
    out.push_str(
        "    const contentOffset = fbb.createString(content);\n\
         \x20   const replyToEventIdOffset = fbb.createString(replyToEventId);\n\
         \x20   const signerPubkeyOffset = signerPubkey === null ? 0 : fbb.createString(signerPubkey);\n",
    );
    render_target(out);
    // PublishReply: content (slot 0), reply_to_event_id (slot 1), target
    // (slot 2), signer_pubkey (slot 3, optional).
    out.push_str(
        "    fbb.startObject(4);\n\
         \x20   fbb.addFieldOffset(0, contentOffset, 0); // slot 0: content\n\
         \x20   fbb.addFieldOffset(1, replyToEventIdOffset, 0); // slot 1: reply_to_event_id\n\
         \x20   fbb.addFieldOffset(2, targetOffset, 0); // slot 2: target\n\
         \x20   if (signerPubkeyOffset !== 0) fbb.addFieldOffset(3, signerPubkeyOffset, 0); // slot 3: signer_pubkey\n\
         \x20   const bodyOffset = fbb.endObject();\n",
    );
}

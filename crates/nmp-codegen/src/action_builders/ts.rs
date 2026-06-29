//! ADR-0064 §3 (#1776) — TypeScript typed action-builder emitter.
//!
//! Renders `actionBuilders.generated.ts`: one `GeneratedActionBuilders` object
//! whose functions are the app-facing typed write builders
//! (`GeneratedActionBuilders.react(...)`, `.follow(...)`, …). Each returns the
//! finished `DispatchEnvelope` bytes (`Uint8Array`) ready for the ONE
//! `dispatch_bytes` wasm doorway (#1750) — the byte-symmetric twin of the native
//! `nmp_app_dispatch_action_bytes` FFI seam. The host owns the boundary call +
//! the `correlationId` it stamps in.
//!
//! ## Self-contained encode (no flatc payload class)
//!
//! The builders encode the per-crate FlatBuffers payload table DIRECTLY via the
//! `flatbuffers` npm runtime's low-level `Builder` API
//! (`startObject`/`addFieldOffset`/`addFieldInt32`/`endObject`), writing each
//! field at its declaration SLOT (`schema_version` at slot 0, then
//! [`ActionBuilder::fields`] from slot 1). No `flatc --ts` binding for the write
//! schemas is needed — the registry is the source of truth and the per-crate
//! Rust `decode` (+ the round-trip test) is the authoritative wire-shape guard.
//! The TS `Builder` is SLOT-indexed (not a vtable byte offset, unlike Swift) —
//! identical in shape to the Kotlin `FlatBufferBuilder`.
//!
//! ## Single web envelope wrapper
//!
//! Unlike the Swift/Kotlin emitters (which emit a private `encodeDispatchEnvelope`
//! per file), the web already ships a hand-written, tested `encodeDispatchEnvelope`
//! in `web/packages/runtime-web/src/dispatchEnvelope.ts` (Cut A #1809). The
//! generated file IMPORTS it rather than re-emitting it, so the envelope wrapper
//! keeps a single web source of truth.
//!
//! ## Determinism
//!
//! Byte-deterministic: iterate [`ACTION_BUILDERS`] in declaration order, emit a
//! fixed template per builder. That stability is what the `--check` drift gate
//! relies on (exactly like [`crate::action_builders::kotlin`]).

use crate::action_builders::registry::{ActionBuilder, FieldKind, ACTION_BUILDERS};
use crate::action_contract::contract_for;
// NIP-51 bookmark helpers: render_bookmark_update, render_bookmark_set_update,
// is_bookmark_builder, is_bookmark_set_builder, ts_param_type. Module declared
// in parent `action_builders.rs` (sibling module) for 500-LOC cap compliance.
use super::ts_nip51::{
    is_bookmark_builder, is_bookmark_set_builder, render_bookmark_set_update,
    render_bookmark_update, ts_param_type,
};

const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen action-builders --platform ts \\
//       --out web/packages/runtime-web/src/actionBuilders.generated.ts
//
// Source of truth: `crates/nmp-codegen/src/action_builders/registry.rs`
// (`ACTION_BUILDERS`). The CI gate (`.github/workflows/codegen-drift.yml`) fails
// any PR whose generated TypeScript differs from a fresh run.
//
// ADR-0064 §3 (#1776) — typed write builders. Each function below encodes the
// per-crate FlatBuffers payload for one open-registry `action_namespace` and
// stamps it, the namespace, and the envelope schema_version into a
// `DispatchEnvelope`, returning the finished bytes for the `dispatch_bytes` wasm
// doorway (#1750). App code NEVER spells a namespace string or hand-assembles
// FlatBuffers — that lives only here, in generated code. The host supplies the
// `correlationId` (the operation identity end to end, ADR-0064 §4) and owns the
// boundary call.
// ─────────────────────────────────────────────────────────────────────────────

import * as flatbuffers from \"flatbuffers\";

import { encodeDispatchEnvelope } from \"./dispatchEnvelope\";
";

/// Render the generated TypeScript action-builders for the given registry.
#[must_use]
pub fn render(builders: &[ActionBuilder]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    out.push_str(&shared_helpers());
    out.push('\n');
    out.push_str("export const GeneratedActionBuilders = {\n");
    for builder in builders {
        render_one(builder, &mut out);
    }
    // The `nmp.publish` UNION builders (separate emitter — different encode
    // shape; see `ts_publish`). Only emitted for the default registry, which is
    // what `render_default` (and therefore the CLI + drift gate) uses.
    if std::ptr::eq(builders.as_ptr(), ACTION_BUILDERS.as_ptr()) {
        crate::action_builders::ts_publish::render_publish(&mut out);
        // The `nmp.marmot` UNION builders (M14-1c / #2169 — 9-arm union).
        crate::action_builders::ts_marmot::render_marmot(&mut out);
    }
    out.push_str("};\n");
    out
}

/// Render the module-level FlatBuffers helpers shared by the flat-table and
/// publish-union builders. `stringVector` encodes a `[string]` vector
/// (last-element-first, the FlatBuffers vector layout) and returns its offset.
/// `relayMarkerByte` maps a role string to the RelayMarker ubyte, mirroring
/// `RelayMarker::from_role_string` in `nmp-router` EXACTLY — including rejection.
///
/// SSOT: `RelayMarker::from_role_string` in `crates/nmp-router/src/publish_relay_list.rs`.
/// 255 = deliberate out-of-range sentinel: the Rust decoder (`marker_from_wire`) rejects
/// any ordinal not in {0,1,2,3}, so encoding 255 for invalid/empty roles makes dispatch
/// fail closed (DispatchAck.error) rather than silently publishing a Both relay.
fn shared_helpers() -> String {
    String::from(
        "export type PublishRouteClass =\n\
         \x20 | \"manual_override\"\n\
         \x20 | \"group_host_pin\"\n\
         \x20 | \"verified_private_inbox\"\n\
         \x20 | \"imported_or_presigned\"\n\
         \x20 | \"diagnostic\";\n\
         \n\
         export type PublishTargetSelection =\n\
         \x20 | { kind: \"auto\" }\n\
         \x20 | { kind: \"explicit\"; relays: string[]; routeClass: PublishRouteClass };\n\
         \n\
         /** Encode a `[string]` FlatBuffers vector (built last element first) and\n\
         \x20* return its offset. Shared by the generated builders below. */\n\
         function stringVector(fbb: flatbuffers.Builder, values: string[]): flatbuffers.Offset {\n\
         \x20 const offsets = values.map((s) => fbb.createString(s));\n\
         \x20 fbb.startVector(4, offsets.length, 4);\n\
         \x20 for (let i = offsets.length - 1; i >= 0; i--) fbb.addOffset(offsets[i]!);\n\
         \x20 return fbb.endVector();\n\
         }\n\
         \n\
         /** Encode a `[uint]` FlatBuffers vector and return its offset. */\n\
         function uintVector(fbb: flatbuffers.Builder, values: number[]): flatbuffers.Offset {\n\
         \x20 fbb.startVector(4, values.length, 4);\n\
         \x20 for (let i = values.length - 1; i >= 0; i--) fbb.addInt32(values[i]!);\n\
         \x20 return fbb.endVector();\n\
         }\n\
         \n\
         /** Map a relay role string to the RelayMarker ubyte (Both=0, Read=1, Write=2, Indexer=3),\n\
          * mirroring `RelayMarker::from_role_string` in `nmp-router` EXACTLY — including rejection.\n\
          * Unknown tokens or no-flag input (e.g. empty string) encode as 255 (out-of-range sentinel)\n\
          * so the Rust decoder (`marker_from_wire`) fails closed instead of silently becoming Both.\n\
          * Role strings may be comma-separated (e.g. `\"both,indexer\"`); comparisons are case-insensitive. */\n\
         function relayMarkerByte(role: string): number {\n\
         \x20 let hasBoth = false, hasRead = false, hasWrite = false, hasIndexer = false;\n\
         \x20 let invalid = false;\n\
         \x20 for (const part of role.split(\",\").map((s) => s.trim().toLowerCase())) {\n\
         \x20   if (part === \"\") { /* no-op: empty part (e.g. trailing comma) matches Rust */ }\n\
         \x20   else if (part === \"both\") hasBoth = true;\n\
         \x20   else if (part === \"read\") hasRead = true;\n\
         \x20   else if (part === \"write\") hasWrite = true;\n\
         \x20   else if (part === \"indexer\") hasIndexer = true;\n\
         \x20   else invalid = true;\n\
         \x20 }\n\
         \x20 if (invalid) return 255;\n\
         \x20 if (hasBoth || (hasRead && hasWrite)) return 0;\n\
         \x20 if (hasRead) return 1;\n\
         \x20 if (hasWrite) return 2;\n\
         \x20 if (hasIndexer) return 3;\n\
         \x20 return 255;\n\
         }\n",
    )
}

/// Render one typed builder method (a property on the `GeneratedActionBuilders`
/// object literal).
fn render_one(builder: &ActionBuilder, out: &mut String) {
    if is_bookmark_builder(builder) {
        render_bookmark_update(builder, out);
        return;
    }
    if is_bookmark_set_builder(builder) {
        render_bookmark_set_update(builder, out);
        return;
    }
    let contract = contract_for(builder.namespace);
    out.push_str(&format!("  /** {} */\n", builder.doc));
    out.push_str(&format!("  {}(\n", builder.method));
    out.push_str("    correlationId: string");
    for field in builder.fields {
        out.push_str(",\n");
        out.push_str(&format!("    {}: {}", field.name, ts_param_type(field)));
    }
    out.push_str(",\n  ): Uint8Array {\n");

    out.push_str("    const fbb = new flatbuffers.Builder(64);\n");
    // Build nested string/vector/table offsets before the table (FlatBuffers
    // requires nested objects be finished before the table that references them).
    for field in builder.fields {
        match field.kind {
            FieldKind::Str if field.optional => {
                out.push_str(&format!(
                    "    const {n}Offset = {n} === null ? 0 : fbb.createString({n});\n",
                    n = field.name
                ));
            }
            FieldKind::Str => {
                out.push_str(&format!(
                    "    const {n}Offset = fbb.createString({n});\n",
                    n = field.name
                ));
            }
            FieldKind::StrVec => {
                if field.optional {
                    out.push_str(&format!(
                        "    const {n}Offset =\n\
                         \x20     {n} === null || {n}.length === 0 ? 0 : stringVector(fbb, {n});\n",
                        n = field.name
                    ));
                } else {
                    out.push_str(&format!(
                        "    const {n}Offset = stringVector(fbb, {n});\n",
                        n = field.name
                    ));
                }
            }
            FieldKind::UintVec => {
                if field.optional {
                    out.push_str(&format!(
                        "    const {n}Offset =\n\
                         \x20     {n} === null || {n}.length === 0 ? 0 : uintVector(fbb, {n});\n",
                        n = field.name
                    ));
                } else {
                    out.push_str(&format!(
                        "    const {n}Offset = uintVector(fbb, {n});\n",
                        n = field.name
                    ));
                }
            }
            FieldKind::RelayListEntryVec => {
                // Build each RelayListEntry table (url + marker) then a vector
                // of those table offsets.
                out.push_str(&format!(
                    "    const {n}Offset = (() => {{\n\
                     \x20     const entryOffsets: number[] = {n}.map((r) => {{\n\
                     \x20       const urlOff = fbb.createString(r.url);\n\
                     \x20       fbb.startObject(2);\n\
                     \x20       fbb.addFieldOffset(0, urlOff, 0); // RelayListEntry slot 0: url\n\
                     \x20       fbb.addFieldInt8(1, relayMarkerByte(r.role), 0); // RelayListEntry slot 1: marker\n\
                     \x20       return fbb.endObject();\n\
                     \x20     }});\n\
                     \x20     fbb.startVector(4, entryOffsets.length, 4);\n\
                     \x20     for (let i = entryOffsets.length - 1; i >= 0; i--) fbb.addOffset(entryOffsets[i]!);\n\
                     \x20     return fbb.endVector();\n\
                     \x20   }})();\n",
                    n = field.name
                ));
            }
            FieldKind::GroupRef => {
                out.push_str(&format!(
                    "    const {n}HostRelayUrlOffset = fbb.createString({n}.hostRelayUrl);\n\
                     \x20   const {n}LocalIdOffset = fbb.createString({n}.localId);\n\
                     \x20   fbb.startObject(2);\n\
                     \x20   fbb.addFieldOffset(0, {n}HostRelayUrlOffset, 0); // GroupRef slot 0: host_relay_url\n\
                     \x20   fbb.addFieldOffset(1, {n}LocalIdOffset, 0); // GroupRef slot 1: local_id\n\
                     \x20   const {n}Offset = fbb.endObject();\n",
                    n = field.name
                ));
            }
            FieldKind::StringTagVec => {
                out.push_str(&format!(
                    "    const {n}Offset = (() => {{\n\
                     \x20     if ({n} == null || {n}.length === 0) return 0;\n\
                     \x20     const tagOffsets = {n}.map((row) => {{\n\
                     \x20       const valOffsets = row.map((s) => fbb.createString(s));\n\
                     \x20       fbb.startVector(4, valOffsets.length, 4);\n\
                     \x20       for (let i = valOffsets.length - 1; i >= 0; i--) fbb.addOffset(valOffsets[i]!);\n\
                     \x20       const valsVec = fbb.endVector();\n\
                     \x20       fbb.startObject(1);\n\
                     \x20       fbb.addFieldOffset(0, valsVec, 0); // StringTag slot 0: values\n\
                     \x20       return fbb.endObject();\n\
                     \x20     }});\n\
                     \x20     fbb.startVector(4, tagOffsets.length, 4);\n\
                     \x20     for (let i = tagOffsets.length - 1; i >= 0; i--) fbb.addOffset(tagOffsets[i]!);\n\
                     \x20     return fbb.endVector();\n\
                     \x20   }})();\n",
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
    out.push_str(&format!("    fbb.startObject({slot_total});\n"));
    out.push_str(&format!(
        "    fbb.addFieldInt32(0, {}, 0); // slot 0: schema_version\n",
        contract.schema_version
    ));
    let mut slot = 1usize; // slot 0 = schema_version
    for field in builder.fields {
        match field.kind {
            FieldKind::Str
            | FieldKind::StrVec
            | FieldKind::UintVec
            | FieldKind::RelayListEntryVec => {
                if field.optional {
                    out.push_str(&format!(
                        "    if ({n}Offset !== 0) fbb.addFieldOffset({slot}, {n}Offset, 0); // slot {slot}: {n}\n",
                        n = field.name
                    ));
                } else {
                    out.push_str(&format!(
                        "    fbb.addFieldOffset({slot}, {n}Offset, 0); // slot {slot}: {n}\n",
                        n = field.name
                    ));
                }
            }
            FieldKind::Uint => {
                out.push_str(&format!(
                    "    fbb.addFieldInt32({slot}, {n}, 0); // slot {slot}: {n}\n",
                    n = field.name
                ));
            }
            FieldKind::Ulong => {
                if field.optional {
                    out.push_str(&format!(
                        "    if ({n} !== null) fbb.addFieldInt64({slot}, {n}, BigInt(0)); // slot {slot}: {n}\n",
                        n = field.name
                    ));
                } else {
                    out.push_str(&format!(
                        "    fbb.addFieldInt64({slot}, {n}, BigInt(0)); // slot {slot}: {n}\n",
                        n = field.name
                    ));
                }
            }
            FieldKind::UlongWithPresenceFlag { flag_name } => {
                let slot_flag = slot + 1;
                out.push_str(&format!(
                    "    if ({n} !== null) {{\n\
                     \x20     fbb.addFieldInt64({slot}, {n}, BigInt(0)); // slot {slot}: {n}\n\
                     \x20     fbb.addFieldInt8({slot_flag}, 1, 0); // slot {slot_flag}: {flag} (bool)\n\
                     \x20   }}\n",
                    n = field.name,
                    slot = slot,
                    slot_flag = slot_flag,
                    flag = flag_name,
                ));
            }
            FieldKind::Ubyte => {
                out.push_str(&format!(
                    "    fbb.addFieldInt8({slot}, {n}, 0); // slot {slot}: {n}\n",
                    n = field.name
                ));
            }
            FieldKind::Sbyte => {
                out.push_str(&format!(
                    "    fbb.addFieldInt8({slot}, {n}, 0); // slot {slot}: {n}\n",
                    n = field.name
                ));
            }
            FieldKind::GroupRef | FieldKind::StringTagVec => {
                if field.optional {
                    out.push_str(&format!(
                        "    if ({n}Offset !== 0) fbb.addFieldOffset({slot}, {n}Offset, 0); // slot {slot}: {n}\n",
                        n = field.name
                    ));
                } else {
                    out.push_str(&format!(
                        "    fbb.addFieldOffset({slot}, {n}Offset, 0); // slot {slot}: {n}\n",
                        n = field.name
                    ));
                }
            }
        }
        slot += field.slot_count();
    }
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

/// Render the full file for the default registry.
#[must_use]
pub fn render_default() -> String {
    render(ACTION_BUILDERS)
}

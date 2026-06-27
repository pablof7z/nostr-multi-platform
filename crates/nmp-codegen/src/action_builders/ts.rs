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

use crate::action_builders::registry::{ActionBuilder, FieldKind, PayloadField, ACTION_BUILDERS};
use crate::action_contract::contract_for;

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
    }
    out.push_str("};\n");
    out
}

/// Render the module-level FlatBuffers helpers shared by the flat-table and
/// publish-union builders. `stringVector` encodes a `[string]` vector
/// (last-element-first, the FlatBuffers vector layout) and returns its offset.
fn shared_helpers() -> String {
    String::from(
        "/** Encode a `[string]` FlatBuffers vector (built last element first) and\n\
         \x20* return its offset. Shared by the generated builders below. */\n\
         function stringVector(fbb: flatbuffers.Builder, values: string[]): flatbuffers.Offset {\n\
         \x20 const offsets = values.map((s) => fbb.createString(s));\n\
         \x20 fbb.startVector(4, offsets.length, 4);\n\
         \x20 for (let i = offsets.length - 1; i >= 0; i--) fbb.addOffset(offsets[i]!);\n\
         \x20 return fbb.endVector();\n\
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
    // Build nested string/vector offsets before the table (FlatBuffers requires
    // nested objects be built before the table that references them).
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
            FieldKind::Uint => {}
        }
    }
    // Table: 1 (schema_version) + N fields.
    out.push_str("    fbb.startObject(");
    out.push_str(&format!("{});\n", builder.fields.len() + 1));
    out.push_str(&format!(
        "    fbb.addFieldInt32(0, {}, 0); // slot 0: schema_version\n",
        contract.schema_version
    ));
    for (idx, field) in builder.fields.iter().enumerate() {
        let slot = idx + 1; // slot 0 is schema_version
        match field.kind {
            FieldKind::Str | FieldKind::StrVec => {
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
        }
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

fn is_bookmark_builder(builder: &ActionBuilder) -> bool {
    matches!(
        builder.namespace,
        "nmp.nip51.add_bookmark" | "nmp.nip51.remove_bookmark"
    )
}

fn render_bookmark_update(builder: &ActionBuilder, out: &mut String) {
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

/// TypeScript parameter type for a field. An optional field is `T | null`
/// (mirrors the Swift `T?` / Kotlin `T?`); the builder omits the field when it
/// is `null`, so the Rust decoder reads `None`.
fn ts_param_type(field: &PayloadField) -> String {
    match (field.kind, field.optional) {
        (FieldKind::Str, false) => "string".to_string(),
        (FieldKind::Str, true) => "string | null".to_string(),
        (FieldKind::Uint, false) => "number".to_string(),
        (FieldKind::Uint, true) => "number | null".to_string(),
        (FieldKind::StrVec, false) => "string[]".to_string(),
        (FieldKind::StrVec, true) => "string[] | null".to_string(),
    }
}

/// Render the full file for the default registry.
#[must_use]
pub fn render_default() -> String {
    render(ACTION_BUILDERS)
}

//! ADR-0070 Lane G twin (#2722) — the TYPED row-payload rendering for the
//! TypeScript `KeyedRefCache` generator. Split out of `ts_keyed_cache.rs` so
//! neither source file exceeds the 500-LOC cap (the Swift
//! `swift_keyed_cache_typed.rs` / Kotlin `kotlin_keyed_cache_typed.rs` twins).
//!
//! Only entries whose [`crate::keyed_projection_row_payload::RefRowPayload::ts`]
//! is `Some` get a typed accessor — the feasibility gate is identical to the
//! Swift/Kotlin generators (the `flatc --ts` reader binding must be checked in
//! before a decoder can reference it by name). An entry with `ts: None` stays
//! reachable only through the raw `payload()` / `snapshot()` merge primitives.

use crate::swift_projections_registry::KeyedProjectionEntry;

/// The name of the generated per-namespace typed-decode function for one keyed
/// projection, e.g. `refs.profile` → `decodeProfileRow`. Both the typed
/// accessor AND the decode-before-commit seam route through this one function,
/// so the read-side decode and the commit-side validation can never diverge —
/// mirrors the Swift/Kotlin `typed_decode_fn_name` twins.
fn typed_decode_fn_name(e: &KeyedProjectionEntry) -> String {
    let mut chars = e.accessor.chars();
    let capitalized = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    format!("decode{capitalized}Row")
}

/// Render the per-namespace typed-decode helpers (ADR-0070 Lane G twin). Each
/// does a checked-enough decode of the row payload buffer (bufferHasIdentifier
/// guard on the row payload's OWN file identifier, NOT the `NRRD` batch id)
/// then hands the reader to the hand-written `refRowDecoders.ts` glue.
pub(super) fn render_typed_decode_fns(entries: &[KeyedProjectionEntry]) -> String {
    let mut s = String::new();
    for e in entries {
        let Some(ts) = e.row_payload.ts.as_ref() else {
            continue;
        };
        let rp = &e.row_payload;
        let fn_name = typed_decode_fn_name(e);
        s.push_str(&format!(
            "/** Decode one `{row_id}` row payload buffer into `{domain}` (ADR-0070 Lane G twin). */\n\
             function {fn_name}(bytes: Uint8Array): {domain} | undefined {{\n\
             \x20\x20if (bytes.length < 8) return undefined;\n\
             \x20\x20try {{\n\
             \x20\x20\x20\x20const bb = new flatbuffers.ByteBuffer(bytes);\n\
             \x20\x20\x20\x20if (!{reader}.bufferHasIdentifier(bb)) return undefined;\n\
             \x20\x20\x20\x20const reader = {reader}.getRootAs{reader}(bb);\n\
             \x20\x20\x20\x20// Hand-written glue (NOT generated): reader -> domain. See `refRowDecoders.{glue}`.\n\
             \x20\x20\x20\x20return {glue}(reader);\n\
             \x20\x20}} catch {{\n\
             \x20\x20\x20\x20return undefined;\n\
             \x20\x20}}\n\
             }}\n\n",
            row_id = rp.row_file_identifier,
            domain = ts.domain_type,
            fn_name = fn_name,
            reader = ts.reader_type,
            glue = ts.glue,
        ));
    }
    s
}

/// Render the decode-before-commit routing table (ADR-0070 invariant #2): maps
/// each PROJECTION KEY (the `KeyedRefCache` class's own row-cache key — see
/// `ts_keyed_cache.rs`'s module doc on why it routes by `projectionKey`, not
/// the wire's self-reported `namespace`) with a typed decoder to its
/// `<fn>(bytes) !== undefined` predicate. A key absent from this table falls
/// back to the non-empty-payload default (mirrors the Swift/Kotlin `rowDecoder`
/// default).
pub(super) fn render_row_decoder_table(entries: &[KeyedProjectionEntry]) -> String {
    let mut s = String::from(
        "  /** ADR-0070 invariant #2: a `Changed` row commits only after its\n\
         \x20\x20 *  payload decodes to the namespace's concrete type. Projection keys\n\
         \x20\x20 *  with no typed decoder accept any non-empty payload (raw-bytes-only\n\
         \x20\x20 *  namespaces, e.g. `refs.event` until #2722 scopes it). */\n\
         \x20\x20private rowDecoder(projectionKey: string, payload: Uint8Array): boolean {\n\
         \x20\x20\x20\x20switch (projectionKey) {\n",
    );
    for e in entries {
        let Some(_ts) = e.row_payload.ts.as_ref() else {
            continue;
        };
        s.push_str(&format!(
            "      case {proj_key:?}:\n        return {fn_name}(payload) !== undefined;\n",
            proj_key = e.projection_key,
            fn_name = typed_decode_fn_name(e),
        ));
    }
    s.push_str(
        "      default:\n        return payload.length > 0;\n\
         \x20\x20\x20\x20}\n\
         \x20\x20}\n\n",
    );
    s
}

/// Render the per-key TYPED accessors (ADR-0070 Lane G twin): a `profile(key)`
/// method PLUS a `profiles()` full-snapshot decode — the latter has no Swift/
/// Kotlin equivalent (mobile apps observe per-row change events; a Solid/React
/// web host derives a keyed store from the full decoded set each frame, the
/// same pattern `RefProfileStore.profiles()` already established by hand).
pub(super) fn render_accessors(entries: &[KeyedProjectionEntry]) -> String {
    let mut s = String::from(
        "  // ADR-0070 Lane G twin (#2722): per-key + full-snapshot TYPED accessors.\n\
         \x20\x20// Each decodes the cached row-payload buffer through the namespace's\n\
         \x20\x20// typed reader (the SAME buffer the kernel's `ref_*_row_payload` encoder\n\
         \x20\x20// emits) into the concrete domain type — never a raw `Uint8Array`\n\
         \x20\x20// surface (invariant #4). A decode miss returns `undefined` / omits the key.\n",
    );
    for e in entries {
        let Some(ts) = e.row_payload.ts.as_ref() else {
            continue;
        };
        let fn_name = typed_decode_fn_name(e);
        s.push_str(&format!(
            "  {accessor}(key: string): {domain} | undefined {{\n\
             \x20\x20\x20\x20const bytes = this.payload({proj_key:?}, key);\n\
             \x20\x20\x20\x20if (!bytes) return undefined;\n\
             \x20\x20\x20\x20return {fn_name}(bytes);\n\
             \x20\x20}}\n\n\
             \x20\x20{accessors_name}(): Map<string, {domain}> {{\n\
             \x20\x20\x20\x20const out = new Map<string, {domain}>();\n\
             \x20\x20\x20\x20for (const [key, bytes] of this.snapshot({proj_key:?})) {{\n\
             \x20\x20\x20\x20\x20\x20const wire = {fn_name}(bytes);\n\
             \x20\x20\x20\x20\x20\x20if (wire !== undefined) out.set(key, wire);\n\
             \x20\x20\x20\x20}}\n\
             \x20\x20\x20\x20return out;\n\
             \x20\x20}}\n\n",
            accessor = e.accessor,
            accessors_name = plural(e.accessor),
            domain = ts.domain_type,
            proj_key = e.projection_key,
            fn_name = fn_name,
        ));
    }
    s
}

/// Naive English pluraliser for accessor names (`profile` → `profiles`). Only
/// ever called on the registry's own `accessor` idents, which are always
/// simple lowerCamelCase nouns — no need for a general inflector.
fn plural(accessor: &str) -> String {
    format!("{accessor}s")
}

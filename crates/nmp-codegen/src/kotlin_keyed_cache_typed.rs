//! ADR-0063 Lane G (#1671) — the TYPED row-payload rendering for the Kotlin
//! `KeyedRefCache` generator. Split out of `kotlin_keyed_cache.rs` so neither
//! source file exceeds the 500-LOC cap (the Swift `swift_keyed_cache_typed.rs`
//! twin).
//!
//! This is the half that turns the raw-bytes merge primitive into a typed per-key
//! accessor: it renders (1) the per-namespace typed-decode helpers (a CHECKED
//! root decode of the ROW payload buffer — KPRF / KCEV — into the namespace's
//! Kotlin reader, then the hand-written `KeyedRefDecoders` glue), (2) the
//! `installTypedRowDecoder()` decode-before-commit seam that routes each namespace
//! to its helper, and (3) the typed accessors `profile(key) -> ProfileCard?` /
//! `event(key) -> ClaimedEventDto?`.

use crate::swift_projections_registry::KeyedProjectionEntry;

/// The name of the generated per-namespace typed-decode helper for one keyed
/// projection, e.g. `refs.profile` → `decodeProfileRow`. Both the typed accessor
/// AND the decode-before-commit `rowDecoder` route through this single function,
/// so the read-side decode and the commit-side validation can never diverge.
/// Mirrors the Swift twin's `typed_decode_fn_name`.
fn typed_decode_fn_name(e: &KeyedProjectionEntry) -> String {
    let mut chars = e.accessor.chars();
    let capitalized = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    format!("decode{capitalized}Row")
}

/// Render the per-key TYPED accessors (ADR-0063 Lane G, #1671). Each decodes the
/// cached row-payload buffer through the namespace's typed Kotlin reader into the
/// concrete domain type — NOT a raw `ByteArray` passthrough. Byte-for-byte
/// semantically identical to the Swift `render_accessors`.
pub(super) fn render_accessors(entries: &[KeyedProjectionEntry]) -> String {
    let mut s = String::from(
        "    // ADR-0063 Lane G (#1671): per-key TYPED accessors — the #1671 host\n\
         \x20\x20\x20\x20// per-key reactive read API (the Swift typed twin). A view reads\n\
         \x20\x20\x20\x20// `model.profile(pubkey)` and observes `addRowChangeListener` filtered\n\
         \x20\x20\x20\x20// on its key, so exactly one avatar re-renders when that pubkey's row\n\
         \x20\x20\x20\x20// updates. Each accessor DECODES the cached row-payload buffer through\n\
         \x20\x20\x20\x20// the namespace's typed reader (the SAME buffer the kernel's\n\
         \x20\x20\x20\x20// `ref_*_row_payload` encoder emits) into the concrete domain type —\n\
         \x20\x20\x20\x20// never a dishonest raw `ByteArray` surface (invariant #4). A decode\n\
         \x20\x20\x20\x20// miss returns null.\n",
    );
    for e in entries {
        let kotlin = e.row_payload.kotlin.as_ref().expect(
            "ADR-0063 Lane G: every keyed projection must carry a Kotlin typed \
             row-payload descriptor (KotlinRefRowPayload)",
        );
        s.push_str(&format!(
            "    fun {}(key: String): {}? {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20val bytes = payload(projectionKey = {:?}, rowKey = key) ?: return null\n\
             \x20\x20\x20\x20\x20\x20\x20\x20return {}(bytes)\n\
             \x20\x20\x20\x20}}\n",
            e.accessor, kotlin.domain_type, e.projection_key, typed_decode_fn_name(e),
        ));
    }
    s
}

/// Render the per-namespace typed-decode helpers + the real, typed `rowDecoder`
/// injection (ADR-0063 invariant #2: decode-before-commit). Each helper does a
/// CHECKED root decode of the row payload buffer (verifying the row-payload
/// `file_identifier` — KPRF / KCEV, NOT the `NRRD` batch id) and hands the reader
/// to the hand-written `KeyedRefDecoders` glue. The `rowDecoder` returns `true`
/// iff the helper produces a non-null domain value, so a malformed row is NEVER
/// committed (prior row retained, `needsResync` latches). Mirrors the Swift twin.
pub(super) fn render_typed_decoders(entries: &[KeyedProjectionEntry]) -> String {
    let mut s = String::from(
        "    // ── Typed row decode (ADR-0063 Lane G, #1671) ───────────────────────\n\
         \x20\x20\x20\x20//\n\
         \x20\x20\x20\x20// The real per-namespace typed decoders. Each does a CHECKED root\n\
         \x20\x20\x20\x20// decode of the row payload buffer (verifying its OWN file_identifier\n\
         \x20\x20\x20\x20// — KPRF / KCEV, NOT the NRRD batch id) then maps the reader to the\n\
         \x20\x20\x20\x20// Chirp domain type via the hand-written `KeyedRefDecoders` glue.\n\
         \x20\x20\x20\x20// Invariant #2: `installTypedRowDecoder()` (called from `init`) wires\n\
         \x20\x20\x20\x20// these into the decode-before-commit seam so a row only commits if it\n\
         \x20\x20\x20\x20// decodes to the concrete type.\n",
    );
    for e in entries {
        let kotlin = e.row_payload.kotlin.as_ref().expect(
            "ADR-0063 Lane G: every keyed projection must carry a Kotlin typed \
             row-payload descriptor (KotlinRefRowPayload)",
        );
        let fn_name = typed_decode_fn_name(e);
        s.push_str(&format!(
            "    /** Decode one `{}` row payload buffer into `{}` (ADR-0063 Lane G). */\n\
             \x20\x20\x20\x20private fun {}(bytes: ByteArray): {}? {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20if (bytes.isEmpty()) return null\n\
             \x20\x20\x20\x20\x20\x20\x20\x20return try {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20val bb = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// CHECKED decode: verify the row-payload file_identifier ({}),\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// NOT the NRRD batch id, before reading any field (fail closed).\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20if (bytes.size < 8 || !{}.{}(bb)) {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20return null\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}}\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20val reader = {}.{}(bb)\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// Hand-written glue (NOT generated): reader -> domain type.\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20// See `KeyedRefDecoders.{}`.\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20KeyedRefDecoders.{}(reader)\n\
             \x20\x20\x20\x20\x20\x20\x20\x20}} catch (e: Exception) {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20null\n\
             \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
             \x20\x20\x20\x20}}\n",
            kotlin.reader_type,
            kotlin.domain_type,
            fn_name,
            kotlin.domain_type,
            e.row_payload.row_file_identifier,
            short_reader(kotlin.reader_type),
            buffer_has_identifier_fn(kotlin.reader_type),
            short_reader(kotlin.reader_type),
            get_root_fn(kotlin.reader_type),
            kotlin.glue,
            kotlin.glue,
        ));
    }
    // The decode-before-commit injection: route each namespace to its typed
    // helper (invariant #2). Called once at construction (see init).
    s.push_str(
        "    /**\n\
         \x20\x20\x20\x20 * Wire the real typed decode-before-commit seam (ADR-0063 #2): a\n\
         \x20\x20\x20\x20 * `Changed` row commits ONLY if its payload decodes to the namespace's\n\
         \x20\x20\x20\x20 * concrete type. Called from `init` so the typed contract holds with no\n\
         \x20\x20\x20\x20 * caller wiring (the Swift `installTypedRowDecoder` twin).\n\
         \x20\x20\x20\x20 */\n\
         \x20\x20\x20\x20private fun installTypedRowDecoder() {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20rowDecoder = { namespace, payload ->\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20when (namespace) {\n",
    );
    for e in entries {
        s.push_str(&format!(
            "                {:?} -> {}(payload) != null\n",
            e.namespace,
            typed_decode_fn_name(e),
        ));
    }
    s.push_str(
        "                else -> false\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20}\n\n",
    );
    s
}

/// The short class name (last `.`-segment) of a fully-qualified Kotlin reader,
/// e.g. `nmp.kernel.ProfileSnapshot` → `ProfileSnapshot`. The generated file
/// imports the reader so the short name is in scope.
fn short_reader(fq: &str) -> &str {
    fq.rsplit('.').next().unwrap_or(fq)
}

/// The flatc-generated `<Reader>BufferHasIdentifier` companion fn name.
fn buffer_has_identifier_fn(fq: &str) -> String {
    format!("{}BufferHasIdentifier", short_reader(fq))
}

/// The flatc-generated `getRootAs<Reader>` companion fn name.
fn get_root_fn(fq: &str) -> String {
    format!("getRootAs{}", short_reader(fq))
}

//! ADR-0063 Lane C (#1671) — the TYPED row-payload rendering for the Swift
//! `KeyedRefCache` generator. Split out of `swift_keyed_cache.rs` so neither
//! source file exceeds the 500-LOC cap.
//!
//! This is the half that turns Lane A's raw-`Data` host surface into a typed
//! per-key accessor: it renders (1) the per-namespace typed-decode helpers (a
//! CHECKED root decode of the ROW payload buffer — KPRF / KCEV — into the
//! namespace's reader, then the hand-written `TypedProjectionGlue`), (2) the
//! `init`-installed decode-before-commit seam that routes each namespace to its
//! helper, and (3) the typed accessors `profile(pubkey) -> ProfileCard?` /
//! `event(primaryId) -> ClaimedEventDto?`.

use crate::swift_projections_registry::KeyedProjectionEntry;

/// The `init` that wires the typed decode-before-commit seam (ADR-0063 Lane C).
/// Kept minimal so the rest of the class stays static.
pub(super) const STATIC_INIT: &str = r#"    /// ADR-0063 Lane C: wire the real typed decoder at construction so the
    /// decode-before-commit seam validates every `Changed` row against the
    /// namespace's concrete type (no caller setup required).
    init() {
        installTypedRowDecoder()
    }

"#;

/// The name of the generated per-namespace typed-decode helper for one keyed
/// projection, e.g. `refs.profile` → `decodeProfileRow`. Both the typed accessor
/// AND the decode-before-commit `rowDecoder` route through this single function,
/// so the read-side decode and the commit-side validation can never diverge.
fn typed_decode_fn_name(e: &KeyedProjectionEntry) -> String {
    let mut chars = e.accessor.chars();
    let capitalized = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    format!("decode{capitalized}Row")
}

/// Render the per-key TYPED accessors (ADR-0063 Lane C, #1671 part-(b)). Each
/// decodes the cached row-payload buffer through the namespace's typed reader
/// into the concrete domain type — NOT the Lane-A raw `Data` passthrough.
pub(super) fn render_accessors(entries: &[KeyedProjectionEntry]) -> String {
    let mut s = String::from(
        "    // MARK: - Per-key TYPED accessors (ADR-0063 Lane C, #1671)\n\
         \x20\x20\x20\x20//\n\
         \x20\x20\x20\x20// One TYPED accessor per keyed namespace — the #1671 part-(b) host\n\
         \x20\x20\x20\x20// per-key reactive read API. A view binds `model.profile(pubkey)` and\n\
         \x20\x20\x20\x20// subscribes to `rowChanged` filtered on its key, so exactly one\n\
         \x20\x20\x20\x20// `AvatarView(pubkey:)` re-renders when that pubkey's row updates. The\n\
         \x20\x20\x20\x20// accessor DECODES the cached row-payload buffer through the\n\
         \x20\x20\x20\x20// namespace's typed reader (the SAME buffer the kernel's\n\
         \x20\x20\x20\x20// `ref_*_row_payload` encoder emits) into the concrete domain type —\n\
         \x20\x20\x20\x20// NOT the Lane-A raw `Data` passthrough. A decode miss returns nil.\n",
    );
    for e in entries {
        let rp = &e.row_payload;
        s.push_str(&format!(
            "    func {}(_ key: String) -> {}? {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20guard let bytes = payload(projectionKey: {:?}, rowKey: key) else {{ return nil }}\n\
             \x20\x20\x20\x20\x20\x20\x20\x20return {}(bytes: bytes)\n\
             \x20\x20\x20\x20}}\n",
            e.accessor, rp.swift_domain_type, e.projection_key, typed_decode_fn_name(e),
        ));
    }
    s
}

/// Render the per-namespace typed-decode helpers + the real, typed `rowDecoder`
/// injection (ADR-0063 invariant #2: decode-before-commit). Each helper does a
/// CHECKED root decode of the row payload buffer (verifying the row-payload
/// `file_identifier`, NOT the `NRRD` batch id) and hands the reader to the
/// hand-written `TypedProjectionGlue`. The `rowDecoder` returns `true` iff the
/// helper produces a non-nil domain value, so a malformed row is NEVER committed
/// (the prior row is retained and `needsResync` latches) — and the host surface
/// is now the TYPED domain type, not raw bytes.
pub(super) fn render_typed_decoders(entries: &[KeyedProjectionEntry]) -> String {
    let mut s = String::from(
        "    // MARK: - Typed row decode (ADR-0063 Lane C, #1671)\n\
         \x20\x20\x20\x20//\n\
         \x20\x20\x20\x20// The real per-namespace typed decoders that replace Lane A's\n\
         \x20\x20\x20\x20// raw-bytes passthrough. Each does a CHECKED root decode of the row\n\
         \x20\x20\x20\x20// payload buffer (verifying its OWN file_identifier — KPRF / KCEV —\n\
         \x20\x20\x20\x20// NOT the NRRD batch id) then maps the reader to the Chirp domain\n\
         \x20\x20\x20\x20// type via the hand-written `TypedProjectionGlue`. Invariant #2:\n\
         \x20\x20\x20\x20// `installTypedRowDecoder()` wires these into the decode-before-commit\n\
         \x20\x20\x20\x20// seam so a row only commits if it decodes to the concrete type.\n",
    );
    // Per-namespace typed-decode helpers (used by BOTH the accessor and the
    // decode-before-commit seam).
    for e in entries {
        let rp = &e.row_payload;
        let fn_name = typed_decode_fn_name(e);
        s.push_str(&format!(
            "    /// Decode one `{}` row payload buffer into `{}` (ADR-0063 Lane C).\n\
             \x20\x20\x20\x20private func {}(bytes: Data) -> {}? {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20guard !bytes.isEmpty else {{ return nil }}\n\
             \x20\x20\x20\x20\x20\x20\x20\x20var buffer = ByteBuffer(data: bytes)\n\
             \x20\x20\x20\x20\x20\x20\x20\x20let reader: {}\n\
             \x20\x20\x20\x20\x20\x20\x20\x20do {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20reader = try getCheckedRoot(byteBuffer: &buffer, fileId: {})\n\
             \x20\x20\x20\x20\x20\x20\x20\x20}} catch {{\n\
             \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20return nil\n\
             \x20\x20\x20\x20\x20\x20\x20\x20}}\n\
             \x20\x20\x20\x20\x20\x20\x20\x20// Hand-written glue (NOT generated): reader → domain type.\n\
             \x20\x20\x20\x20\x20\x20\x20\x20// See `TypedProjectionGlue.{}`.\n\
             \x20\x20\x20\x20\x20\x20\x20\x20return TypedProjectionGlue.{}(reader)\n\
             \x20\x20\x20\x20}}\n",
            rp.row_file_identifier,
            rp.swift_domain_type,
            fn_name,
            rp.swift_domain_type,
            rp.swift_reader_type,
            format!("{}.id", rp.swift_reader_type),
            rp.swift_glue,
            rp.swift_glue,
        ));
    }
    // The decode-before-commit injection: route each namespace to its typed
    // helper (invariant #2). Called once at construction (see init).
    s.push_str(
        "    /// Wire the real typed decode-before-commit seam (ADR-0063 #2): a\n\
         \x20\x20\x20\x20/// `Changed` row commits ONLY if its payload decodes to the\n\
         \x20\x20\x20\x20/// namespace's concrete type. Called from `init` so the typed\n\
         \x20\x20\x20\x20/// contract holds without any caller wiring.\n\
         \x20\x20\x20\x20private func installTypedRowDecoder() {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20rowDecoder = { [weak self] namespace, payload in\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20guard let self else { return false }\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20switch namespace {\n",
    );
    for e in entries {
        s.push_str(&format!(
            "            case {:?}: return self.{}(bytes: payload) != nil\n",
            e.namespace,
            typed_decode_fn_name(e),
        ));
    }
    s.push_str(
        "            default: return false\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20}\n",
    );
    s
}

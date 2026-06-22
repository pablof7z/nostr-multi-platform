//! V6 Stage 4 (consumer-side) — generated typed-FlatBuffer-sidecar decoders.
//!
//! ## What this emits and why
//!
//! Every snapshot projection now ships a typed FlatBuffer entry in the
//! `SnapshotFrame.typed_projections` sidecar (ADR-0037/0044) ALONGSIDE the
//! generic JSON `payload`. Switching Chirp's consumer off the JSON path means
//! decoding those sidecars in Swift. The hand-written precedent is
//! `ios/Chirp/Chirp/Bridge/TypedHomeFeedDecoder.swift`: find the envelope by
//! `key`+`schemaId`, `getCheckedRoot(fileId:)` the bytes into the `flatc
//! --swift` reader struct, map the reader to the Chirp domain type.
//!
//! Hand-writing one such decoder per projection key (35 of them) would recreate
//! the exact hand-written-Decodable debt the V6 codegen sweep eliminates. So
//! this module GENERATES the mechanical part of every decoder.
//!
//! ## The generated / hand-written seam (the FEASIBILITY GATE outcome)
//!
//! The FlatBuffer **wire** shape and the Chirp **domain** type are not
//! field-aligned for most keys — the domain types are field-*subsets* of the
//! wire, carry `has_*` companion-bool optionals, enum-as-string mappings, and
//! sentinel-to-nil conversions (see `TypedHomeFeedDecoder`). A generic that
//! mapped wire→domain across all keys would be leaky. So the altitude of the
//! generated layer is the **`flatc --swift` reader struct**, not the domain
//! type:
//!
//! - **Generated (this module):** the envelope lookup (`key`+`schema_id`) and
//!   unchecked `getRoot(byteBuffer:)` that yields the reader struct. The O(N)
//!   FlatBuffers Verifier is explicitly skipped because all buffers are produced
//!   by our own Rust kernel microseconds before decode across a trusted
//!   in-process FFI boundary — verifying them is pure wasted CPU on the 4 Hz
//!   hot path. This is the first ~10 lines of every `TypedHomeFeedDecoder`-shaped
//!   decoder, replicated per key. That is the debt worth generating away.
//! - **Hand-written glue (per key, NOT generated):** the
//!   `<reader> -> <domain>` mapping, declared as a `TypedProjectionGlue`
//!   static. The generated decoder calls into it. For thin keys the glue is a
//!   few lines (`active_account`: `reader.hasActiveAccount ? reader.pubkey :
//!   nil`); for thick keys (nested sub-buffers) it is the bespoke decoder the
//!   `nmp.feed.home` precedent already hand-writes.
//!
//! ## Only keys with a checked-in `flatc --swift` binding are emitted
//!
//! A generated decoder references the reader struct by name
//! (`nmp_kernel_AccountsSnapshot`). That type only exists if the `flatc
//! --swift` binding for the schema is checked into the Chirp target. Only the
//! proof-key bindings (`accounts`, `active_account`) ship today, so the emitter
//! iterates `SNAPSHOT_PROJECTIONS` and emits a decoder ONLY for entries whose
//! `typed_sidecar.swift_reader_type` is `Some` — referencing an absent type
//! would not compile. The remaining ~29 schemas need their `flatc --swift`
//! binding generated (+ a binding-drift gate) before they can be wired; that is
//! the named follow-up.
//!
//! ## Output determinism
//!
//! Byte-deterministic: iterate the registry in declaration order, emit a fixed
//! template per entry. That stability is what makes the `--check` drift gate
//! possible — exactly like [`crate::swift`].

use std::path::Path;

use crate::swift_projections_registry::{SnapshotProjectionEntry, SNAPSHOT_PROJECTIONS};

/// Header comment emitted at the top of the generated file. Keep the
/// regeneration command accurate — the `--check` gate fails on a stale file
/// and anyone hitting the failure needs the exact command to reproduce.
const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen typed-decoders \\
//       --out ios/Chirp/Chirp/Bridge/Generated/TypedProjectionDecoders.generated.swift
//
// Source of truth: the typed-sidecar identities in
// `crates/nmp-codegen/src/swift_projections_registry.rs`
// (`SnapshotProjectionEntry::typed_sidecar`). The CI gate
// (`.github/workflows/codegen-drift.yml`) fails any PR whose generated Swift
// differs from a fresh run.
//
// V6 Stage 4 (consumer-side). Each enum below is the GENERATED mechanical half
// of one projection's typed-sidecar decoder: the `key`+`schemaId` lookup over
// `[TypedProjectionEnvelope]` and the `getRoot(byteBuffer:)` (unchecked) decode
// into the `flatc --swift` reader struct. Buffers arrive over a trusted
// in-process FFI boundary (Rust kernel → Swift shell, same process/memory);
// running the O(buffer) FlatBuffers Verifier on the 4 Hz hot path is pure waste.
// The reader→Chirp-domain mapping is the HAND-WRITTEN `TypedProjectionGlue` seam
// (see `ios/Chirp/Chirp/Bridge/TypedProjectionGlue.swift`).
//
// Only projection keys whose `flatc --swift` reader binding is checked into the
// Chirp target appear here. The rest need their binding generated first.
// ─────────────────────────────────────────────────────────────────────────────

import FlatBuffers
import Foundation
";

/// Render the generated typed-sidecar decoder Swift for the given registry.
///
/// Emits one decoder enum per entry whose `typed_sidecar.swift_reader_type` is
/// `Some`. Returns the rendered Swift as a `String`; the caller writes it to
/// disk (the indirection lets [`check_typed_decoders`] diff without the
/// filesystem).
#[must_use]
pub fn render_typed_decoders(entries: &[SnapshotProjectionEntry]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');

    let emitted: Vec<&SnapshotProjectionEntry> = entries
        .iter()
        .filter(|e| {
            e.typed_sidecar
                .as_ref()
                .and_then(|s| s.swift_reader_type)
                .is_some()
        })
        .collect();

    if emitted.is_empty() {
        out.push_str(
            "// No projection key has a checked-in `flatc --swift` reader binding yet.\n",
        );
        return out;
    }

    for entry in emitted {
        render_one_decoder(entry, &mut out);
        out.push('\n');
    }
    // Trim the trailing blank line so the file ends with exactly one newline,
    // matching the deterministic-output convention of `crate::swift`.
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// Render one projection key's generated decoder enum.
///
/// `entry.typed_sidecar` and its `swift_reader_type` are guaranteed `Some` by
/// the caller's filter. The glue function name is the projection's
/// `swift_field` (lowerCamelCase, unique by registry invariant), so the
/// generated decoder calls `TypedProjectionGlue.<swift_field>(reader)`.
fn render_one_decoder(entry: &SnapshotProjectionEntry, out: &mut String) {
    let sidecar = entry
        .typed_sidecar
        .as_ref()
        .expect("caller filters to Some sidecars");
    let reader = sidecar
        .swift_reader_type
        .expect("caller filters to Some reader types");
    // #1723: the neutral schema_id / file_identifier are SOURCED from the
    // projection contract (the single source), not from the Swift registry — the
    // `TypedSidecar` no longer redeclares them, nor the producer envelope `key`
    // (the entry's own `key` IS the producer envelope key). Fail-closed: an entry
    // whose key has no contract row panics here.
    let contract = crate::projection_contract::contract_for(entry.key);
    let schema_id = contract.schema_id;
    let file_identifier = contract.file_identifier;
    let enum_name = decoder_enum_name(entry.swift_field);
    let domain = entry.swift_type;
    let glue = entry.swift_field;

    out.push_str(&format!("// MARK: - {enum_name}\n"));
    out.push_str(&format!(
        "// Projection `{}` → typed sidecar `{}` ({}). Domain type: `{}?`.\n",
        entry.key, schema_id, file_identifier, domain
    ));
    out.push_str(&format!("enum {enum_name} {{\n"));
    out.push_str(&format!(
        "    /// `TypedProjection.key` the producer publishes for this projection.\n    static let key = {:?}\n",
        entry.key
    ));
    out.push_str(&format!(
        "    /// `TypedPayload.schema_id` carried on the sidecar buffer.\n    static let schemaId = {:?}\n",
        schema_id
    ));
    out.push_str(&format!(
        "    /// FlatBuffers `file_identifier` for `{reader}`.\n    static let fileIdentifier = {:?}\n",
        file_identifier
    ));
    out.push('\n');

    // Envelope-set entry point — the shape `TypedHomeFeedDecoder.decode(from:)`
    // established. Returns the Chirp domain value, or nil when the sidecar is
    // absent / wrong-schema / malformed (graceful fallback to the JSON path).
    out.push_str(&format!(
        "    /// Decode the typed `{}` sidecar from the snapshot's typed-projection\n",
        entry.key
    ));
    out.push_str(
        "    /// envelopes into the Chirp domain value. Returns `nil` (so the host\n",
    );
    out.push_str(
        "    /// falls back to the generic JSON `payload`) when the sidecar is absent,\n",
    );
    out.push_str("    /// carries the wrong schema, or is not a well-formed buffer.\n");
    out.push_str(&format!(
        "    static func decode(from projections: [TypedProjectionEnvelope]) -> {domain}? {{\n"
    ));
    out.push_str("        guard let projection = projections.first(where: {\n");
    out.push_str("            $0.key == key && $0.schemaId == schemaId\n");
    out.push_str("        }), !projection.payload.isEmpty else {\n");
    out.push_str("            return nil\n");
    out.push_str("        }\n");
    out.push_str("        return decode(bytes: projection.payload)\n");
    out.push_str("    }\n");
    out.push('\n');

    // Raw-bytes entry point — the GENERATED scaffold: unchecked getRoot into
    // the reader struct, then hand into the hand-written glue.  Buffers arrive
    // over a trusted in-process FFI boundary (Rust kernel → Swift shell, same
    // process/memory); the O(N) FlatBuffers Verifier walk is pure wasted CPU
    // on the 4 Hz hot path.  The key+schemaId routing above already selects
    // the right sub-buffer, and any gross wiring error surfaces as nil/empty
    // via the glue rather than a crash.  getRoot is infallible so no try? is
    // needed; the `!bytes.isEmpty` guard above handles the only "no data" case.
    out.push_str(&format!(
        "    /// Decode a raw `{}` FlatBuffers buffer into the Chirp domain value.\n",
        file_identifier
    ));
    out.push_str(&format!(
        "    static func decode(bytes: Data) -> {domain}? {{\n"
    ));
    out.push_str("        guard !bytes.isEmpty else { return nil }\n");
    out.push_str("        var buffer = ByteBuffer(data: bytes)\n");
    out.push_str(&format!(
        "        let reader: {reader} = getRoot(byteBuffer: &buffer)\n"
    ));
    out.push_str(&format!(
        "        // Hand-written glue (NOT generated): map the `flatc --swift` reader\n",
    ));
    out.push_str(&format!(
        "        // struct to the Chirp domain type. See `TypedProjectionGlue.{glue}`.\n"
    ));
    out.push_str(&format!(
        "        return TypedProjectionGlue.{glue}(reader)\n"
    ));
    out.push_str("    }\n");
    out.push_str("}\n");
}

/// The Swift enum name for a projection's generated decoder. `accounts` →
/// `TypedAccountsDecoder`, `activeAccount` → `TypedActiveAccountDecoder`.
fn decoder_enum_name(swift_field: &str) -> String {
    let mut chars = swift_field.chars();
    let capitalized = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    format!("Typed{capitalized}Decoder")
}

/// Outcome of a `--check` run. Mirrors [`crate::swift::SwiftCheckOutcome`].
#[derive(Debug)]
pub struct TypedDecodersCheckOutcome {
    /// `true` when the on-disk file matches the freshly-rendered output.
    pub up_to_date: bool,
    /// First differing line (1-based) when not up-to-date; `None` when
    /// up-to-date OR when the file doesn't exist.
    pub first_diff_line: Option<usize>,
}

/// Write the generated typed-decoder Swift to `out_path`, replacing whatever
/// was there.
///
/// # Errors
/// Filesystem I/O failures.
pub fn generate_typed_decoders(out_path: &Path) -> std::io::Result<()> {
    let rendered = render_typed_decoders(SNAPSHOT_PROJECTIONS);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, rendered)
}

/// Diff a freshly-rendered output against the file at `out_path`. A missing
/// file is reported as stale (`up_to_date = false`), matching the `swift.rs`
/// gate's treatment.
///
/// # Errors
/// Filesystem I/O failures other than NotFound.
pub fn check_typed_decoders(out_path: &Path) -> std::io::Result<TypedDecodersCheckOutcome> {
    let rendered = render_typed_decoders(SNAPSHOT_PROJECTIONS);
    let actual = match std::fs::read_to_string(out_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TypedDecodersCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(err),
    };
    if actual == rendered {
        return Ok(TypedDecodersCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    // Strings already proven to differ above; a length-only mismatch must still
    // report a real diff line, not `None` (which CI reads as "file missing").
    let first_diff_line = crate::diff_report::first_diff_or_length(&actual, &rendered);
    Ok(TypedDecodersCheckOutcome {
        up_to_date: false,
        first_diff_line,
    })
}

#[cfg(test)]
#[path = "swift_typed_decoders/tests.rs"]
mod tests;

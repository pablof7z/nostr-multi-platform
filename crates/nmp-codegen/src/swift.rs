//! V6 Stage 1 — Swift `Decodable` emitter.
//!
//! Reads one or more `ProjectionSchemaDocument` JSON values (the output of each
//! schema-owner crate's `dump_projection_schemas` binary) and renders Swift
//! `struct` declarations conforming to `Decodable` (plus Equatable /
//! Identifiable when registry metadata asks).
//!
//! Stage 1 covers flat-record types only — every pilot schema decodes as a
//! JSON Schema `object` with scalar / nullable-scalar / array-of-scalar
//! properties. Tagged enums (`ActionStage`, `TimelineBlock`) and the
//! dotted-projection-key registry (`SnapshotProjections`) are Stage 2/3 work
//! and are explicitly out of scope here. Any pilot schema that doesn't match
//! the flat-record shape returns a [`SwiftEmitError::Unsupported`] so the
//! CI gate fails loudly rather than emitting silent wrong-shape Swift.
//!
//! ## Output determinism
//!
//! The emitter is byte-deterministic. Type order matches the input
//! document; field order matches the input schema's `properties` object
//! (which `nmp-core::codegen_schema` sorts alphabetically via schemars).
//! That stability is what makes the `--check` CI gate possible — running
//! the emitter twice on the same input produces byte-identical output.
//!
//! ## Module layout
//!
//! This file owns only the top-level orchestration (parsing the document
//! stream, stitching Stage 1 + Stage 2 output together, and the
//! `generate_swift` / `check_swift` file-I/O entry points). The actual
//! per-type rendering lives in dedicated submodules so each stays under the
//! file-size ceiling and each doc-comments what it owns:
//!
//! - [`error`] — `SwiftEmitError`, shared by every entry point below.
//! - [`schema_input`] — the `ProjectionSchemaDocument` / `TypeEntry` wire
//!   parse shape Stage 1 decodes.
//! - [`flat_record_emit`] — Stage 1 flat-record Swift struct emission.
//! - [`snapshot_projections_emit`] — Stage 2 `SnapshotProjections`
//!   registry-wiring emission (struct + `CodingKeys` + the
//!   `.convertFromSnakeCase` transform).

use std::collections::BTreeSet;
use std::path::Path;

use crate::swift_projections_registry::SNAPSHOT_PROJECTIONS;
use schema_input::{parse_schema_documents, SUPPORTED_DOCUMENT_VERSION};

mod error;
mod flat_record_emit;
mod schema_input;
mod snapshot_projections_emit;

pub use error::SwiftEmitError;
use flat_record_emit::render_type;
use snapshot_projections_emit::render_snapshot_projections;

/// Header comment emitted at the top of every generated file. The regeneration
/// command must stay accurate — CI fails on a stale generated file, so anyone
/// hitting the failure needs the exact command to reproduce it locally.
const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-core --features codegen-schema --bin dump_projection_schemas \\
//       | cargo run -p nmp-codegen -- gen swift --out <path>
//
// Source of truth: the Rust projection types listed in the per-struct
// provenance comments below. The CI gate (`.github/workflows/codegen-drift.yml`)
// fails any PR whose generated Swift differs from a fresh run.
//
// Stage 1 pilot — 7 flat-record types (V6, docs/architecture-audit/
// docs/retired/codegen-v6.md §6b). Stage 2 expands to the dotted-projection-key
// registry; Stage 3 sweeps the remaining hand-written Decodables.
// ─────────────────────────────────────────────────────────────────────────────

import Foundation
";

/// Generate the Swift source for the given schema-document JSON stream.
///
/// Returns the rendered Swift as a `String`. Caller is responsible for
/// writing it to disk (the indirection lets [`check_swift`] diff against
/// the committed file without going through the filesystem).
///
/// # Errors
/// - [`SwiftEmitError::ParseFailed`] if `document_json` isn't a valid stream of
///   `ProjectionSchemaDocument` values.
/// - [`SwiftEmitError::UnsupportedDocumentVersion`] if the document version
///   doesn't match this emitter.
/// - [`SwiftEmitError::DuplicateSwiftType`] if two schema owners claim the same
///   Swift type.
/// - [`SwiftEmitError::Unsupported`] if any type has a non-flat-record
///   schema.
pub fn render_swift(document_json: &str) -> Result<String, SwiftEmitError> {
    let documents =
        parse_schema_documents(document_json).map_err(|err| SwiftEmitError::ParseFailed {
            reason: err.to_string(),
        })?;
    if documents.is_empty() {
        return Err(SwiftEmitError::ParseFailed {
            reason: "no schema documents supplied".to_string(),
        });
    }

    for document in &documents {
        if document.version != SUPPORTED_DOCUMENT_VERSION {
            return Err(SwiftEmitError::UnsupportedDocumentVersion {
                found: document.version,
                expected: SUPPORTED_DOCUMENT_VERSION,
            });
        }
    }

    let mut out = String::from(HEADER);
    out.push('\n');
    let mut seen_swift_names = BTreeSet::new();
    for document in &documents {
        for entry in &document.types {
            if !seen_swift_names.insert(entry.swift_name.clone()) {
                return Err(SwiftEmitError::DuplicateSwiftType {
                    swift_name: entry.swift_name.clone(),
                });
            }
            render_type(entry, &mut out)?;
            out.push('\n');
        }
    }
    // V6 Stage 2 — append the `SnapshotProjections` registry struct +
    // `CodingKeys` enum. Driven by the static slice in
    // [`crate::swift_projections_registry`] rather than a schemars schema,
    // because the registry is a list of (json_key, swift_field, swift_type)
    // triples — there is no Rust type to reflect (the projection values
    // come from many different crates, including app-layer ones).
    render_snapshot_projections(SNAPSHOT_PROJECTIONS, &mut out);
    Ok(out)
}

/// Outcome of a `--check` run.
#[derive(Debug)]
pub struct SwiftCheckOutcome {
    /// `true` when the on-disk file matches the freshly-rendered output.
    pub up_to_date: bool,
    /// First differing line (1-based) when not up-to-date; `None` when
    /// up-to-date OR when the file doesn't exist yet.
    pub first_diff_line: Option<usize>,
}

/// Write the rendered Swift to `out_path`, replacing whatever was there.
///
/// # Errors
/// As [`render_swift`], plus filesystem I/O failures.
pub fn generate_swift(document_json: &str, out_path: &Path) -> Result<(), SwiftEmitError> {
    let rendered = render_swift(document_json)?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, rendered)?;
    Ok(())
}

/// Diff a freshly-rendered output against the file at `out_path`.
///
/// # Errors
/// As [`render_swift`]. A missing file returns `up_to_date = false` with
/// `first_diff_line = None`, not an error — the CI gate treats "file
/// doesn't exist" the same as "file is stale".
pub fn check_swift(
    document_json: &str,
    out_path: &Path,
) -> Result<SwiftCheckOutcome, SwiftEmitError> {
    let rendered = render_swift(document_json)?;
    let actual = match std::fs::read_to_string(out_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SwiftCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(SwiftEmitError::Io(err)),
    };
    if actual == rendered {
        return Ok(SwiftCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    // Strings already proven to differ above; see `diff_report` for why a
    // length-only mismatch must still yield a `Some` line, not `None`.
    let first_diff_line = crate::diff_report::first_diff_or_length(&actual, &rendered);
    Ok(SwiftCheckOutcome {
        up_to_date: false,
        first_diff_line,
    })
}

#[cfg(test)]
#[path = "swift/tests.rs"]
mod tests;

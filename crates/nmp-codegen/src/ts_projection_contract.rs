//! #2722 — generated TypeScript projection-contract table. The TS read side
//! needs the SAME `key -> (schema_id, file_identifier)` facts the Swift typed
//! decoders pull from [`crate::projection_contract::PROJECTION_CONTRACT`] via
//! `projection_contract::contract_for` (see `swift_typed_decoders.rs`'s
//! `render_one_decoder`), so a generic web `findTypedProjection(frame, key)`
//! helper can verify a sidecar's `schemaId`/`fileIdentifier` without every
//! consumer hand-copying those wire constants per key (the drift every
//! hand-rolled `NRRD_FILE_IDENTIFIER = "NRRD"` / `REFS_PROFILE_PROJECTION_KEY =
//! "refs.profile"` pair in hl/gallery risked).
//!
//! Generates `web/packages/runtime-web/src/projectionContract.generated.ts`
//! from the FULL [`PROJECTION_CONTRACT`] — every projection, not a
//! feasibility-gated subset (there is no per-key TS type dependency here,
//! only string/number constants, so nothing gates emission).

use std::path::Path;

use crate::projection_contract::PROJECTION_CONTRACT;

const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen projection-contract --platform ts \\
//       --out web/packages/runtime-web/src/projectionContract.generated.ts
//
// Source of truth: PROJECTION_CONTRACT in
// `crates/nmp-codegen/src/projection_contract/table.rs`.
// The CI gate (`.github/workflows/codegen-drift.yml`) fails any PR whose
// generated TypeScript differs.
//
// #2722 — the neutral wire identity (schema_id + file_identifier) of every
// projection the kernel/host can emit, keyed by the `TypedProjection.key` a
// `SnapshotFrame.typed_projections` entry carries. Consumed by
// `updateFrameDecoder.ts`'s `findTypedProjection` to verify a sidecar's
// identity before handing its bytes to a decoder — never hand-copied per call
// site (the historical hl/nmp-gallery pattern this replaces).
// ─────────────────────────────────────────────────────────────────────────────

/** One projection's neutral wire identity. */
export type ProjectionContractEntry = {
  /** `TypedPayload.schema_id` carried on the sidecar buffer. */
  schemaId: string;
  /** FlatBuffers `file_identifier` for the sidecar's root table. */
  fileIdentifier: string;
};

/** `TypedProjection.key` -> neutral wire identity, for every projection the
 *  system emits (kernel built-ins + host-registered + keyed row-delta
 *  carriers). */
export const PROJECTION_CONTRACT: Readonly<Record<string, ProjectionContractEntry>> = {
";

/// Outcome of a `--check` run.
#[derive(Debug)]
pub struct TsProjectionContractCheckOutcome {
    pub up_to_date: bool,
    pub first_diff_line: Option<usize>,
}

/// Render the TypeScript projection-contract table.
#[must_use]
pub fn render_ts_projection_contract() -> String {
    let mut out = String::from(HEADER);
    for entry in PROJECTION_CONTRACT {
        out.push_str(&format!(
            "  {:?}: {{ schemaId: {:?}, fileIdentifier: {:?} }},\n",
            entry.key, entry.schema_id, entry.file_identifier
        ));
    }
    out.push_str("};\n");
    out
}

/// Write the generated TypeScript file to `out_path`.
///
/// # Errors
/// Filesystem I/O failures.
pub fn generate_ts_projection_contract(out_path: &Path) -> std::io::Result<()> {
    let rendered = render_ts_projection_contract();
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, rendered)
}

/// Diff a freshly-rendered output against the file at `out_path`.
///
/// # Errors
/// Filesystem I/O failures other than NotFound.
pub fn check_ts_projection_contract(
    out_path: &Path,
) -> std::io::Result<TsProjectionContractCheckOutcome> {
    let rendered = render_ts_projection_contract();
    let actual = match std::fs::read_to_string(out_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TsProjectionContractCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(err),
    };
    if actual == rendered {
        return Ok(TsProjectionContractCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    let first_diff_line = crate::diff_report::first_diff_or_length(&actual, &rendered);
    Ok(TsProjectionContractCheckOutcome {
        up_to_date: false,
        first_diff_line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_marked_generated() {
        assert!(render_ts_projection_contract()
            .contains("THIS FILE IS GENERATED. DO NOT EDIT BY HAND."));
    }

    #[test]
    fn emits_every_contract_entry() {
        let out = render_ts_projection_contract();
        for entry in PROJECTION_CONTRACT {
            assert!(
                out.contains(&format!(
                    "{:?}: {{ schemaId: {:?}, fileIdentifier: {:?} }},",
                    entry.key, entry.schema_id, entry.file_identifier
                )),
                "missing contract row for {}",
                entry.key
            );
        }
    }

    #[test]
    fn covers_the_refs_profile_key_used_by_the_keyed_ref_cache() {
        let out = render_ts_projection_contract();
        assert!(out.contains("\"refs.profile\": { schemaId:"));
    }
}

//! Fail-closed wire-identity drift gate for concept-owned reads (#2899, Part D
//! — added atop the B+C emitter that landed in #2905/#2909). Mirrors
//! [`crate::projection_version_gate`].
//!
//! `nmp-codegen` must not depend on any concept crate (`nmp-replies` /
//! `nmp-reactions` / `nmp-reposts` / `nmp-zaps` — crate-boundaries §10), so the
//! [`super::registry::CONCEPT_READS`] table names each read's `schema_id` as
//! plain text. Nothing otherwise stops that hardcoded fact from drifting away
//! from the concept crate's real `*_SUMMARY_SCHEMA_ID` / `*_SUMMARY_VERSION` /
//! `*_SUMMARY_FILE_IDENTIFIER` consts. This gate reads each concept crate's
//! `summary.rs` source TEXT (via `CARGO_MANIFEST_DIR` at test time — the crates
//! are reachable on disk even though they are not linked) and asserts:
//!
//! 1. the three source consts equal this gate's own hardcoded expectations
//!    (fail-closed: a schema bump in the concept crate that forgets to update
//!    the expectation fails here), and
//! 2. the [`super::registry::CONCEPT_READS`] registry agrees with the same
//!    expectations — so the codegen table can never silently disagree with the
//!    concept-crate source either.

use std::path::PathBuf;

use super::registry::concept_read_for;

/// One concept read's expected wire identity + where its consts are declared.
struct WireIdentityExpectation {
    /// Registry id (`super::registry::ConceptRead::id`).
    concept: &'static str,
    /// Repo-root-relative source declaring the three consts below.
    source_path: &'static str,
    schema_id_const: &'static str,
    schema_version_const: &'static str,
    file_identifier_const: &'static str,
    expected_schema_id: &'static str,
    expected_schema_version: u32,
    expected_file_identifier: &'static str,
}

/// The wire identities the codegen registry + concept-crate source must agree
/// on. Hardcoded here (not pulled from either side) so the gate is a true
/// third-party contract both must satisfy.
const WIRE_IDENTITIES: &[WireIdentityExpectation] = &[
    WireIdentityExpectation {
        concept: "replies",
        source_path: "crates/nmp-replies/src/summary.rs",
        schema_id_const: "REPLY_SUMMARY_SCHEMA_ID",
        schema_version_const: "REPLY_SUMMARY_SCHEMA_VERSION",
        file_identifier_const: "REPLY_SUMMARY_FILE_IDENTIFIER",
        expected_schema_id: "nmp.replies.summary",
        expected_schema_version: 1,
        expected_file_identifier: "NRSM",
    },
    WireIdentityExpectation {
        concept: "reactions",
        source_path: "crates/nmp-reactions/src/summary.rs",
        schema_id_const: "REACTION_SUMMARY_SCHEMA_ID",
        schema_version_const: "REACTION_SUMMARY_SCHEMA_VERSION",
        file_identifier_const: "REACTION_SUMMARY_FILE_IDENTIFIER",
        expected_schema_id: "nmp.reactions.summary",
        expected_schema_version: 1,
        expected_file_identifier: "NRCS",
    },
    WireIdentityExpectation {
        concept: "reposts",
        source_path: "crates/nmp-reposts/src/summary.rs",
        schema_id_const: "REPOST_SUMMARY_SCHEMA_ID",
        schema_version_const: "REPOST_SUMMARY_SCHEMA_VERSION",
        file_identifier_const: "REPOST_SUMMARY_FILE_IDENTIFIER",
        expected_schema_id: "nmp.reposts.summary",
        expected_schema_version: 1,
        expected_file_identifier: "NRPS",
    },
    WireIdentityExpectation {
        concept: "zaps",
        source_path: "crates/nmp-zaps/src/summary.rs",
        schema_id_const: "ZAP_SUMMARY_SCHEMA_ID",
        schema_version_const: "ZAP_SUMMARY_SCHEMA_VERSION",
        file_identifier_const: "ZAP_SUMMARY_FILE_IDENTIFIER",
        expected_schema_id: "nmp.zaps.summary",
        expected_schema_version: 1,
        expected_file_identifier: "NZSM",
    },
];

/// Resolve the repo root (`<repo>/crates/nmp-codegen`).
///
/// # Panics
/// When `CARGO_MANIFEST_DIR` does not have the expected shape.
#[must_use]
pub fn repo_root() -> PathBuf {
    crate::projection_version_gate::repo_root()
}

/// Outcome of checking one concept read's wire identity.
#[derive(Debug)]
pub struct WireIdentityCheckOutcome {
    pub concept: &'static str,
    /// Source `*_SCHEMA_ID` equals the expectation.
    pub source_schema_id_matches: bool,
    /// Source `*_SCHEMA_VERSION` equals the expectation.
    pub source_schema_version_matches: bool,
    /// Source `*_FILE_IDENTIFIER` equals the expectation.
    pub source_file_identifier_matches: bool,
    /// The codegen `CONCEPT_READS` registry row exists and its `schema_id`
    /// equals the expectation.
    pub registry_schema_id_matches: bool,
}

impl WireIdentityCheckOutcome {
    #[must_use]
    pub fn matches(&self) -> bool {
        self.source_schema_id_matches
            && self.source_schema_version_matches
            && self.source_file_identifier_matches
            && self.registry_schema_id_matches
    }
}

/// Check every wire identity against both the concept-crate source and the
/// codegen registry, under `repo_root`.
#[must_use]
pub fn check_all_wire_identities(repo_root: &std::path::Path) -> Vec<WireIdentityCheckOutcome> {
    WIRE_IDENTITIES
        .iter()
        .map(|e| check_one(repo_root, e))
        .collect()
}

fn check_one(
    repo_root: &std::path::Path,
    e: &WireIdentityExpectation,
) -> WireIdentityCheckOutcome {
    let source = std::fs::read_to_string(repo_root.join(e.source_path)).ok();
    let source_schema_id_matches = source
        .as_deref()
        .and_then(|s| parse_const_str(s, e.schema_id_const))
        .as_deref()
        == Some(e.expected_schema_id);
    let source_schema_version_matches = source
        .as_deref()
        .and_then(|s| crate::projection_version_gate::parse_const_u32(s, e.schema_version_const))
        == Some(e.expected_schema_version);
    let source_file_identifier_matches = source
        .as_deref()
        .and_then(|s| parse_byte_string_const(s, e.file_identifier_const))
        .as_deref()
        == Some(e.expected_file_identifier);
    // Cross-check the codegen registry: its row must exist and carry the same
    // schema_id text this gate expects, so the table can't drift either.
    let registry_schema_id_matches = concept_read_for(e.concept)
        .map(|row| row.summary.schema_id == e.expected_schema_id)
        .unwrap_or(false);
    WireIdentityCheckOutcome {
        concept: e.concept,
        source_schema_id_matches,
        source_schema_version_matches,
        source_file_identifier_matches,
        registry_schema_id_matches,
    }
}

/// Parse a `<vis> const <name>: &str = "<value>";` declaration. Fail-closed
/// (mirrors [`crate::projection_version_gate::parse_const_u32`]): a comment,
/// wrong type annotation, or non-terminated literal yields `None`.
#[must_use]
pub fn parse_const_str(source: &str, const_name: &str) -> Option<String> {
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        let after_vis = strip_visibility(trimmed);
        let Some(after_const) = after_vis.strip_prefix("const ") else {
            continue;
        };
        let after_const = after_const.trim_start();
        let Some(after_name) = after_const.strip_prefix(const_name) else {
            continue;
        };
        let after_name = after_name.trim_start();
        let after_colon = match after_name.strip_prefix(':') {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        let after_ty = match after_colon.strip_prefix("&str") {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        let after_eq = match after_ty.strip_prefix('=') {
            Some(rest) => rest.trim(),
            None => continue,
        };
        let Some(value) = after_eq.strip_suffix(';') else {
            continue;
        };
        let value = value.trim();
        if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            return Some(inner.to_string());
        }
    }
    None
}

/// Parse a `<vis> const <name>: &[u8] = b"<value>";` declaration, returning the
/// ASCII string content of the byte-string literal. Fail-closed.
#[must_use]
pub fn parse_byte_string_const(source: &str, const_name: &str) -> Option<String> {
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        let after_vis = strip_visibility(trimmed);
        let Some(after_const) = after_vis.strip_prefix("const ") else {
            continue;
        };
        let after_const = after_const.trim_start();
        let Some(after_name) = after_const.strip_prefix(const_name) else {
            continue;
        };
        let after_name = after_name.trim_start();
        let after_colon = match after_name.strip_prefix(':') {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        let after_ty = match after_colon.strip_prefix("&[u8]") {
            Some(rest) => rest.trim_start(),
            None => continue,
        };
        let after_eq = match after_ty.strip_prefix('=') {
            Some(rest) => rest.trim(),
            None => continue,
        };
        let Some(value) = after_eq.strip_suffix(';') else {
            continue;
        };
        let value = value.trim();
        if let Some(inner) = value.strip_prefix("b\"").and_then(|v| v.strip_suffix('"')) {
            return Some(inner.to_string());
        }
    }
    None
}

fn strip_visibility(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix("pub(crate) ") {
        rest
    } else if let Some(rest) = line.strip_prefix("pub ") {
        rest
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_str_and_byte_string_consts() {
        assert_eq!(
            parse_const_str(
                r#"pub const REPLY_SUMMARY_SCHEMA_ID: &str = "nmp.replies.summary";"#,
                "REPLY_SUMMARY_SCHEMA_ID"
            ),
            Some("nmp.replies.summary".to_string())
        );
        assert_eq!(
            parse_byte_string_const(
                r#"pub const REPLY_SUMMARY_FILE_IDENTIFIER: &[u8] = b"NRSM";"#,
                "REPLY_SUMMARY_FILE_IDENTIFIER"
            ),
            Some("NRSM".to_string())
        );
    }

    #[test]
    fn fails_closed_on_comment_or_wrong_type() {
        assert_eq!(parse_const_str(r#"// pub const X: &str = "y";"#, "X"), None);
        assert_eq!(parse_const_str(r#"pub const X: u32 = 1;"#, "X"), None);
        assert_eq!(parse_byte_string_const(r#"pub const X: &str = "NRSM";"#, "X"), None);
    }

    /// Every wire identity in the gate table has a matching codegen registry
    /// row (guards against the table naming a concept the registry dropped).
    #[test]
    fn every_expectation_has_a_registry_row() {
        for e in WIRE_IDENTITIES {
            assert!(
                concept_read_for(e.concept).is_some(),
                "wire-identity table names concept {:?} with no CONCEPT_READS row",
                e.concept
            );
        }
    }

    /// FAIL-CLOSED: every concept read's wire identity agrees across the gate's
    /// expectation, the concept-crate source consts, AND the codegen registry.
    /// A schema bump in a concept crate (or a registry edit) that forgets the
    /// others fails here.
    #[test]
    fn concept_read_wire_identities_agree_source_and_registry() {
        let root = repo_root();
        let outcomes = check_all_wire_identities(&root);
        let mut failures = Vec::new();
        for o in &outcomes {
            if !o.matches() {
                failures.push(format!(
                    "{}: source_schema_id={} source_schema_version={} \
                     source_file_identifier={} registry_schema_id={}",
                    o.concept,
                    o.source_schema_id_matches,
                    o.source_schema_version_matches,
                    o.source_file_identifier_matches,
                    o.registry_schema_id_matches,
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "concept-read wire-identity drift (source consts / codegen registry):\n{}",
            failures.join("\n")
        );
    }
}

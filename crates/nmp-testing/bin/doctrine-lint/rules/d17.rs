//! D17 — social-timeline kind policy (`{1,6}`) must not be hardcoded in
//! `nmp-core` substrate.
//!
//! V-68 (Stages 1+2) removed the last `{1,6}` social-timeline literal from
//! `nmp-core`; it now flows from the `nmp-ffi` host shim. D17 is the
//! regression guard: it fires whenever the discriminating shape
//! `"kinds":[1,6]` (with the `"kinds":` prefix) reappears in non-comment,
//! non-test nmp-core production code.
//!
//! ## What this catches
//!
//! The **discriminating shape** is `"kinds":` followed (with optional
//! whitespace) by `[`, optional whitespace, `1`, optional whitespace, `,`,
//! optional whitespace, `6`, optional whitespace, `]`. The `"kinds":` prefix
//! is what makes the token unambiguously a social-timeline policy literal;
//! bare `[1, 6]` or `[1,6]` without the prefix is NOT flagged (would be too
//! noisy — 1 and 6 are common integer values).
//!
//! Whitespace variants covered: `"kinds":[1,6]`, `"kinds":[1, 6]`,
//! `"kinds": [1, 6]`, `"kinds": [1,6]`.
//!
//! ## Exemptions
//!
//! - Doc-comment lines (`///`, `//!`, `//`, inside `/* */`) — skipped via the
//!   `is_comment` flag passed by the walker.
//! - `#[cfg(test)]` module bodies — the caller's `in_test_cfg` flag gates the
//!   firing site in `main.rs` (mirrors D14).
//! - Test-only files (`tests.rs`, `*_tests.rs`, …) — handled via
//!   `d6::file_is_test_only` in the `main.rs` driver block.
//! - Files outside `crates/nmp-core/src/` (the substrate scope) — gated by
//!   `file_in_scope`; `--d17-extra-scope` opts a fixture path in for the
//!   smoke test.
//!
//! ## Per-line opt-out
//!
//! `// doctrine-allow: D17 — reason` on the offending line suppresses the
//! finding (the standard `allow::line_allows` mechanism).

use std::path::Path;

pub const ID: &str = "D17";

/// True iff the file lives under `crates/nmp-core/src/`. Other crates and
/// the doctrine-lint binary's own source tree are out of scope.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    let in_nmp_core_src =
        s.contains("/crates/nmp-core/src/") || s.starts_with("crates/nmp-core/src/");
    if !in_nmp_core_src {
        return false;
    }
    // Exempt the doctrine-lint binary's source tree (its string constants
    // contain the banned pattern — meta-false-positives on broad sweeps).
    // Fixtures under `/fixtures/` are intentionally NOT exempted so smoke
    // tests remain effective.
    if s.contains("/bin/doctrine-lint/") {
        return false;
    }
    true
}

/// Returns `(col, message, suggested)` for each occurrence of the social-kind
/// filter shape on `line`. `is_comment` short-circuits the scan.
pub fn check(line: &str, is_comment: bool) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let mut start = 0;
    while let Some(col) = find_social_kinds_filter(&line[start..]) {
        let abs_col = start + col;
        hits.push((
            abs_col + 1, // 1-indexed columns (clippy-parseable)
            "social-timeline kind policy (`{1,6}`) must not be hardcoded in \
             nmp-core substrate (D0 / V-68); declare kinds at the nmp-ffi host \
             shim — see V-68"
                .to_string(),
            "pass the kind set as a parameter from the nmp-ffi shim instead of \
             embedding a literal `[1,6]` filter in nmp-core"
                .to_string(),
        ));
        // Advance past this match to find any further hits on the same line.
        start = abs_col + "\"kinds\"".len();
    }
    hits
}

/// Find the byte offset of the next `"kinds":` prefix immediately followed
/// (with optional whitespace) by `[` `1` `,` `6` `]` in `haystack`.
///
/// Returns `Some(offset)` where `offset` is the index of the leading `"` in
/// `"kinds"`. Returns `None` if no such pattern exists.
///
/// Uses `str::find` for scanning to avoid any byte-boundary panics on
/// UTF-8 source lines containing multi-byte characters (em dashes in
/// `// doctrine-allow` comments, for example).
fn find_social_kinds_filter(haystack: &str) -> Option<usize> {
    let needle = "\"kinds\"";
    let mut search_from = 0;
    while let Some(rel) = haystack[search_from..].find(needle) {
        let abs = search_from + rel;
        let rest = &haystack[abs + needle.len()..];
        if matches_1_6_array(rest) {
            return Some(abs);
        }
        // Advance past this `"kinds"` token to look for further occurrences.
        search_from = abs + needle.len();
    }
    None
}

/// Returns true iff `s` (starting right after `"kinds"`) matches the pattern
/// `\s*:\s*\[\s*1\s*,\s*6\s*\]`.
fn matches_1_6_array(s: &str) -> bool {
    let s = s.trim_start();
    let s = match s.strip_prefix(':') {
        Some(r) => r,
        None => return false,
    };
    let s = s.trim_start();
    let s = match s.strip_prefix('[') {
        Some(r) => r,
        None => return false,
    };
    let s = s.trim_start();
    // Must be exactly `1` (not `10`, `11`, `16`, `100`, ...).
    let s = match s.strip_prefix('1') {
        Some(r) => r,
        None => return false,
    };
    // After `1` the next significant char must be `,` (ruling out `10`,
    // `11`, `16`, `100`, ...).
    let s = s.trim_start();
    let s = match s.strip_prefix(',') {
        Some(r) => r,
        None => return false,
    };
    let s = s.trim_start();
    // Must be exactly `6` (not `60`, `61`, `600`, ...).
    let s = match s.strip_prefix('6') {
        Some(r) => r,
        None => return false,
    };
    // After `6` the next significant char must be `]` (ruling out `60`,
    // `61`, `600`, ...).
    let s = s.trim_start();
    s.starts_with(']')
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- matches_1_6_array unit tests -----------------------------------------

    #[test]
    fn array_exact_no_spaces() {
        assert!(matches_1_6_array(":[1,6]"));
    }

    #[test]
    fn array_space_after_colon() {
        assert!(matches_1_6_array(": [1,6]"));
    }

    #[test]
    fn array_space_after_comma() {
        assert!(matches_1_6_array(":[1, 6]"));
    }

    #[test]
    fn array_all_spaces() {
        assert!(matches_1_6_array(": [ 1 , 6 ]"));
    }

    #[test]
    fn array_rejects_1_60() {
        assert!(!matches_1_6_array(":[1,60]"));
    }

    #[test]
    fn array_rejects_11_6() {
        assert!(!matches_1_6_array(":[11,6]"));
    }

    #[test]
    fn array_rejects_1_6_7() {
        assert!(!matches_1_6_array(":[1,6,7]"));
    }

    #[test]
    fn array_rejects_no_colon() {
        assert!(!matches_1_6_array("[1,6]"));
    }

    #[test]
    fn array_rejects_other_pair() {
        assert!(!matches_1_6_array(":[3,10000]"));
    }

    // -- check() unit tests ---------------------------------------------------

    #[test]
    fn flags_bare_kinds_1_6() {
        let hits = check(r#"json!({"kinds":[1,6],"limit":10})"#, false);
        assert_eq!(hits.len(), 1, "must flag kinds:[1,6]");
        assert!(
            hits[0].1.contains("V-68"),
            "message must reference V-68; got: {}",
            hits[0].1
        );
    }

    #[test]
    fn flags_kinds_1_space_6() {
        let hits = check(r#"json!({"kinds":[1, 6],"limit":10})"#, false);
        assert_eq!(hits.len(), 1, "must flag kinds:[1, 6]");
    }

    #[test]
    fn flags_kinds_colon_space_bracket() {
        let hits = check(r#"json!({"kinds": [1, 6]})"#, false);
        assert_eq!(hits.len(), 1, "must flag kinds: [1, 6]");
    }

    #[test]
    fn does_not_flag_comment_line() {
        let hits = check(r#"/// see `"kinds":[1,6]` example"#, true);
        assert!(hits.is_empty(), "doc-comment lines must not be flagged");
    }

    #[test]
    fn does_not_flag_bare_array_without_prefix() {
        let hits = check("let arr = [1, 6];", false);
        assert!(
            hits.is_empty(),
            "bare [1,6] without \"kinds\": prefix must not be flagged"
        );
    }

    #[test]
    fn does_not_flag_kinds_1_only() {
        let hits = check(r#"json!({"kinds":[1],"limit":5})"#, false);
        assert!(hits.is_empty(), "kinds:[1] alone must not be flagged");
    }

    #[test]
    fn does_not_flag_kinds_1_6_7() {
        let hits = check(r#"json!({"kinds":[1,6,7]})"#, false);
        assert!(hits.is_empty(), "kinds:[1,6,7] must not be flagged");
    }

    #[test]
    fn does_not_flag_kinds_3_10000() {
        let hits = check(r#"json!({"kinds":[3,10000]})"#, false);
        assert!(hits.is_empty(), "unrelated kind pairs must not be flagged");
    }

    #[test]
    fn col_is_1_indexed_at_kinds_prefix() {
        let hits = check(r#""kinds":[1,6]"#, false);
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].0, 1,
            "column must be 1-indexed at the '\"' of \"kinds\""
        );
    }

    #[test]
    fn flags_two_occurrences_on_same_line() {
        let hits = check(r#"["kinds":[1,6],"kinds":[1,6]]"#, false);
        assert_eq!(hits.len(), 2, "must flag each occurrence");
    }

    // -- file_in_scope unit tests ---------------------------------------------

    #[test]
    fn scope_nmp_core_src_is_in_scope() {
        assert!(file_in_scope(&std::path::PathBuf::from(
            "crates/nmp-core/src/kernel/requests/thread.rs"
        )));
        assert!(file_in_scope(&std::path::PathBuf::from(
            "/abs/path/crates/nmp-core/src/actor/outbound.rs"
        )));
    }

    #[test]
    fn scope_non_nmp_core_is_out_of_scope() {
        assert!(!file_in_scope(&std::path::PathBuf::from(
            "crates/nmp-nip17/src/lib.rs"
        )));
        assert!(!file_in_scope(&std::path::PathBuf::from(
            "apps/chirp/nmp-app-chirp/src/ffi.rs"
        )));
        assert!(!file_in_scope(&std::path::PathBuf::from(
            "crates/nmp-marmot/src/projection/mod.rs"
        )));
    }

    #[test]
    fn scope_doctrine_lint_binary_is_out_of_scope() {
        assert!(!file_in_scope(&std::path::PathBuf::from(
            "crates/nmp-testing/bin/doctrine-lint/rules/d17.rs"
        )));
    }
}

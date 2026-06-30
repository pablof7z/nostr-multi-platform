//! D17 — social-timeline kind policy (`{1,6}`) must not be hardcoded in
//! `nmp-core` or `nmp-ffi` substrate.
//!
//! V-68 (Stages 1+2) removed the last `{1,6}` social-timeline literal from
//! `nmp-core`; it now flows from the app layer (`nmp-app-chirp`). D17 is the
//! regression guard: it fires whenever the discriminating shape
//! `"kinds":[1,6]` (with the `"kinds":` prefix) reappears in non-comment,
//! non-test nmp-core **or nmp-ffi** production code, or whenever a Rust
//! kind-set literal of the form `[1u32, 6u32]` / `BTreeSet::from([1`...`6`
//! appears in those same files.
//!
//! ## What this catches
//!
//! ### JSON filter shape
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
//! ### Rust kind-set literals
//!
//! The shape that the deleted `nmp_app_open_timeline` used — and that any
//! future regression in `nmp-ffi` would reintroduce — is a Rust array or
//! `BTreeSet` literal containing exactly `1u32` and `6u32`. Two sub-patterns
//! are checked:
//!
//! - `[1u32` (the opening of `[1u32, 6u32]`)
//! - `BTreeSet::from([1` (the opening of `BTreeSet::from([1u32, 6u32])`)
//!
//! The `u32` suffix is what disambiguates a social-kind literal from a
//! generic integer; bare `[1, 6]` is NOT flagged (too noisy).
//!
//! ## Exemptions
//!
//! - Doc-comment lines (`///`, `//!`, `//`, inside `/* */`) — skipped via the
//!   `is_comment` flag passed by the walker.
//! - `#[cfg(test)]` module bodies — the caller's `in_test_cfg` flag gates the
//!   firing site in `main.rs` (mirrors D14).
//! - Test-only files (`tests.rs`, `*_tests.rs`, …) — handled via
//!   `d6::file_is_test_only` in the `main.rs` driver block.
//! - `apps/chirp/crates/nmp-app-chirp/` — this is the **legitimate home** of the
//!   kind policy literal; it must not fire there.
//! - Files outside `crates/nmp-core/src/` and `crates/nmp-ffi/src/` (the
//!   substrate scope) — gated by `file_in_scope`; `--d17-extra-scope` opts a
//!   fixture path in for the smoke test.
//!
//! ## Per-line opt-out
//!
//! `// doctrine-allow: D17 — reason` on the offending line suppresses the
//! finding (the standard `allow::line_allows` mechanism).

use std::path::Path;

pub const ID: &str = "D17";

/// True iff the file lives under `crates/nmp-core/src/` or
/// `crates/nmp-ffi/src/`. `apps/chirp/crates/nmp-app-chirp/` is the **legitimate
/// home** of the kind-policy literal and is explicitly excluded. The
/// doctrine-lint binary's own source tree is also excluded to avoid
/// meta-false-positives from the string constants in these rule files.
/// Fixtures under `/fixtures/` are intentionally NOT exempted so smoke tests
/// remain effective.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");

    // Chirp app is the legitimate home of the kind policy — always out of scope.
    if s.contains("/apps/chirp/crates/nmp-app-chirp/")
        || s.starts_with("apps/chirp/crates/nmp-app-chirp/")
    {
        return false;
    }

    // Exempt the doctrine-lint binary's source tree.
    if s.contains("/bin/doctrine-lint/") {
        return false;
    }

    let in_nmp_core_src =
        s.contains("/crates/nmp-core/src/") || s.starts_with("crates/nmp-core/src/");
    let in_nmp_ffi_src = s.contains("/crates/nmp-ffi/src/") || s.starts_with("crates/nmp-ffi/src/");

    in_nmp_core_src || in_nmp_ffi_src
}

/// Returns `(col, message, suggested)` for each occurrence of the social-kind
/// filter shape on `line`. `is_comment` short-circuits the scan.
///
/// Two pattern families are checked:
///
/// 1. JSON filter shape: `"kinds":[1,6]` (with optional whitespace).
/// 2. Rust kind-set literal: `[1u32` followed (with optional whitespace and
///    comma) by `6`. The `u32` suffix is the discriminating token — it makes
///    the expression unambiguously a typed Rust kind constant rather than a
///    generic integer array. This pattern catches both `[1u32, 6u32]` (plain
///    array) and `BTreeSet::from([1u32, 6u32])` (set constructor).
pub fn check(line: &str, is_comment: bool) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let mut hits = Vec::new();

    // ── Pattern 1: JSON filter shape `"kinds":[1,6]` ────────────────────────
    let mut start = 0;
    while let Some(col) = find_social_kinds_filter(&line[start..]) {
        let abs_col = start + col;
        hits.push((
            abs_col + 1, // 1-indexed columns (clippy-parseable)
            "social-timeline kind policy (`{1,6}`) must not be hardcoded in \
             nmp-core or nmp-ffi substrate (D0 / V-68); declare kinds at the \
             app layer — see V-68"
                .to_string(),
            "pass the kind set as a parameter from the app layer instead of \
             embedding a literal `[1,6]` filter in the substrate"
                .to_string(),
        ));
        // Advance past this match to find any further hits on the same line.
        start = abs_col + "\"kinds\"".len();
    }

    // ── Pattern 2: Rust kind-set literal `[1u32 … , … 6 …]` ────────────────
    // The `u32` suffix is what makes this token unambiguously a typed Nostr
    // kind constant. Catches both `[1u32, 6u32]` and
    // `BTreeSet::from([1u32, 6u32])`. The `6` check (via `matches_rust_6`)
    // mirrors the precision of `find_social_kinds_filter` — it rules out
    // `[1u32, 60u32]`, `[1u32, 61u32]`, etc.
    {
        let needle = "[1u32";
        let mut from = 0;
        while let Some(rel) = line[from..].find(needle) {
            let abs = from + rel;
            let rest = &line[abs + needle.len()..];
            if matches_rust_6(rest) {
                hits.push((
                    abs + 1,
                    "social-timeline kind-set literal (`[1u32, 6…]`) must not \
                     be hardcoded in nmp-core or nmp-ffi substrate (D0 / V-68); \
                     the kind set must originate at the app layer — see V-68"
                        .to_string(),
                    "move the kind-set literal to the app layer (e.g. \
                     nmp-app-chirp) and pass it through the FFI boundary"
                        .to_string(),
                ));
            }
            from = abs + needle.len();
        }
    }

    hits
}

/// Returns true iff `s` (starting right after `[1u32`) contains `, 6`
/// (with optional whitespace around the comma) before any `]`. This rules
/// out `[1u32, 60u32]`, `[1u32, 61u32]`, etc. — mirrors `matches_1_6_array`.
fn matches_rust_6(s: &str) -> bool {
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
    // After `6` the next significant character must not be a digit (so
    // `60`, `61`, `600` are excluded). Valid continuations: `u`, `,`, `]`,
    // ` `, `\t`.
    s.starts_with(|c: char| !c.is_ascii_digit())
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

    // -- Rust kind-set literal check() unit tests ----------------------------

    #[test]
    fn flags_rust_kind_set_literal_array() {
        let hits = check("BTreeSet::from([1u32, 6u32])", false);
        assert!(
            !hits.is_empty(),
            "must flag the Rust kind-set literal [1u32, 6u32]"
        );
        assert!(
            hits[0].1.contains("V-68"),
            "message must reference V-68; got: {}",
            hits[0].1
        );
    }

    #[test]
    fn flags_rust_kind_set_literal_btreeset_from() {
        // BTreeSet::from([1u32, 6u32]) — the exact form the deleted
        // nmp_app_open_timeline used.
        let hits = check("    let kinds = BTreeSet::from([1u32, 6u32]);", false);
        assert!(!hits.is_empty(), "must flag BTreeSet::from([1u32, 6u32])");
    }

    #[test]
    fn does_not_flag_rust_kind_1u32_60() {
        // [1u32, 60u32] must NOT fire — `60` is not `6`.
        let hits = check("let arr = [1u32, 60u32];", false);
        assert!(
            hits.is_empty(),
            "[1u32, 60u32] must not be flagged (60 != 6)"
        );
    }

    #[test]
    fn does_not_flag_rust_kind_1u32_61() {
        let hits = check("let arr = [1u32, 61u32];", false);
        assert!(
            hits.is_empty(),
            "[1u32, 61u32] must not be flagged (61 != 6)"
        );
    }

    #[test]
    fn does_not_flag_plain_integer_array() {
        // [1, 6] without the u32 suffix is NOT flagged (too noisy — 1 and 6
        // are common values).
        let hits = check("let arr = [1, 6];", false);
        assert!(
            hits.is_empty(),
            "bare [1, 6] without u32 suffix must not be flagged"
        );
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
    fn scope_nmp_ffi_src_is_in_scope() {
        assert!(
            file_in_scope(&std::path::PathBuf::from("crates/nmp-ffi/src/timeline.rs")),
            "nmp-ffi/src must be in scope after N2 extension"
        );
        assert!(file_in_scope(&std::path::PathBuf::from(
            "/abs/path/crates/nmp-ffi/src/lib.rs"
        )));
    }

    #[test]
    fn scope_chirp_app_is_out_of_scope() {
        // apps/chirp/crates/nmp-app-chirp is the legitimate home of the kind-policy
        // literal — must always be out of scope.
        assert!(!file_in_scope(&std::path::PathBuf::from(
            "apps/chirp/crates/nmp-app-chirp/src/ffi.rs"
        )));
        assert!(!file_in_scope(&std::path::PathBuf::from(
            "/abs/path/apps/chirp/crates/nmp-app-chirp/src/ffi/mod.rs"
        )));
    }

    #[test]
    fn scope_non_nmp_core_is_out_of_scope() {
        assert!(!file_in_scope(&std::path::PathBuf::from(
            "crates/nmp-nip17/src/lib.rs"
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

//! A5 — raw-tap registrations inside the repo must be verbatim-forwarding
//! consumers only; state-derivation belongs on `register_ingest_parser`.
//!
//! The raw-event tap (`register_raw_event_observer`) survived the four-PR
//! retirement ladder as a narrowed seam for **verbatim signed-frame
//! forwarding** to external stores and relay bridges (e.g. the `hl` app's
//! nostrdb mirror). All IN-REPO state-derivation consumers (NIP-17 DM inbox,
//! Marmot, chirp-tui debug cache) migrated to `register_ingest_parser` via
//! PRs #1137/#1145/#1148.
//!
//! Rule A5 pins that state. It flags any IN-REPO call to
//! `register_raw_event_observer` that does NOT live in:
//! - the seam's own definition files (the nmp-ffi `raw_event_tap.rs`, the
//!   kernel raw-observer module, `app_host.rs`, `builder.rs`); or
//! - test code (`#[cfg(test)]` bodies, `_tests.rs` / `tests.rs` files).
//!
//! There are zero such in-repo production callers today — this rule pins that.
//!
//! ## What this catches
//!
//! Any occurrence of `register_raw_event_observer(` on a non-comment,
//! non-test line outside the seam's own definition files.
//!
//! ## Exemptions
//!
//! - Doc-comment lines (`///`, `//!`, `//`, inside `/* */`) — skipped via
//!   the `is_comment` flag passed by the walker.
//! - `#[cfg(test)]` module bodies — the caller's `in_test_cfg` flag.
//! - Test-only files (`*_tests.rs`, `tests.rs`, …) — handled via
//!   `d6::file_is_test_only` in the `main.rs` driver block.
//! - The seam's own definition files — `file_is_exempt` returns `true`
//!   for these (see below).
//! - The doctrine-lint binary's own source tree — meta-false-positives.
//! - External apps (outside `crates/` and `apps/`) — out of scope.
//!
//! ## Per-line opt-out
//!
//! `// doctrine-allow: A5 — reason` on the offending line suppresses the
//! finding (the standard `allow::line_allows` mechanism).

use std::path::Path;

pub const ID: &str = "A5";

/// Token that triggers the rule when found on a production non-comment line
/// outside the seam definition files.
const BANNED_TOKEN: &str = "register_raw_event_observer(";

/// True iff `path` is one of the seam's own definition files — the places
/// where `register_raw_event_observer` is legitimately DEFINED (not called
/// as a client). These are excluded so the rule does not flag its own seam
/// implementation.
///
/// Definition files:
/// - `crates/nmp-ffi/src/raw_event_tap.rs` — the C-ABI registration surface.
/// - `crates/nmp-ffi/src/lib.rs` — `NmpApp::register_raw_event_observer` impl.
/// - `crates/nmp-core/src/substrate/app_host.rs` — `AppHost` trait definition.
/// - `crates/nmp-core/src/substrate/` — the kernel raw-observer module.
/// - `crates/nmp-defaults/src/builder.rs` — `AppHost` delegation impl.
/// - The doctrine-lint binary's own source tree.
pub fn file_is_exempt(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");

    // Doctrine-lint's own source tree — meta-false-positives.
    if s.contains("/bin/doctrine-lint/") || s.starts_with("doctrine-lint/") {
        return true;
    }

    // Seam definition files.
    let is_definition_file = s.ends_with("/crates/nmp-ffi/src/raw_event_tap.rs")
        || s.contains("crates/nmp-ffi/src/raw_event_tap.rs")
        || s.ends_with("/crates/nmp-ffi/src/lib.rs")
        || s.contains("crates/nmp-ffi/src/lib.rs")
        || s.contains("/crates/nmp-core/src/substrate/")
        || s.contains("crates/nmp-core/src/substrate/")
        || s.ends_with("/crates/nmp-defaults/src/builder.rs")
        || s.contains("crates/nmp-defaults/src/builder.rs");

    is_definition_file
}

/// True iff the file is in the A5 scan scope: within the `crates/` or `apps/`
/// subtree of the monorepo, but not one of the seam definition files and not
/// the doctrine-lint binary itself.
///
/// The rule is workspace-wide (any in-repo production code that registers the
/// raw tap for state-derivation is a violation), so scope is broad: all Rust
/// source under `crates/` or `apps/`. The narrowing is done via `file_is_exempt`.
///
/// External consumers (the `hl` app, etc.) live outside the monorepo and are
/// never scanned, so they are automatically out of scope.
pub fn file_in_scope(path: &Path) -> bool {
    if file_is_exempt(path) {
        return false;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    s.contains("/crates/") || s.contains("/apps/") || s.starts_with("crates/") || s.starts_with("apps/")
}

/// Returns `(col, message, suggested)` for each occurrence of
/// `register_raw_event_observer(` on a non-comment, non-test line.
pub fn check(
    line: &str,
    is_comment: bool,
    in_test_cfg: bool,
) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let mut start = 0;
    while let Some(rel) = line[start..].find(BANNED_TOKEN) {
        let col = start + rel + 1; // 1-indexed
        hits.push((
            col,
            "in-repo call to `register_raw_event_observer` outside the seam definition files \
             violates rule A5: derive state via `register_ingest_parser` (fires on cache-served \
             replay + slot-keyed replace); the raw tap is verbatim-forwarding only"
                .to_string(),
            "replace with `register_ingest_parser` or `replace_ingest_parser` (slot-keyed) \
             for state-derivation; reserve the raw tap for verbatim signed-frame forwarding \
             to external stores/bridges only"
                .to_string(),
        ));
        start += rel + BANNED_TOKEN.len();
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // -- check() unit tests ---------------------------------------------------

    #[test]
    fn flags_register_raw_event_observer_in_production() {
        let hits = check(
            "    let id = app.register_raw_event_observer(filter, observer);",
            false,
            false,
        );
        assert_eq!(hits.len(), 1, "must flag register_raw_event_observer");
        assert!(
            hits[0].1.contains("rule A5"),
            "message must reference rule A5; got: {}",
            hits[0].1
        );
        assert!(
            hits[0].1.contains("register_ingest_parser"),
            "message must name register_ingest_parser; got: {}",
            hits[0].1
        );
    }

    #[test]
    fn does_not_flag_in_comment() {
        let hits = check(
            "// call register_raw_event_observer(filter, observer)",
            true,
            false,
        );
        assert!(hits.is_empty(), "comment lines must not be flagged");
    }

    #[test]
    fn does_not_flag_in_test_cfg() {
        let hits = check(
            "    app.register_raw_event_observer(filter, observer);",
            false,
            true,
        );
        assert!(
            hits.is_empty(),
            "#[cfg(test)] bodies must not be flagged by A5"
        );
    }

    #[test]
    fn flags_two_occurrences_same_line() {
        let hits = check(
            "let a = x.register_raw_event_observer(f, o); let b = y.register_raw_event_observer(f, o);",
            false,
            false,
        );
        assert_eq!(
            hits.len(),
            2,
            "both occurrences on same line must be flagged"
        );
    }

    #[test]
    fn col_is_1_indexed() {
        let line = "    app.register_raw_event_observer(filter, obs);";
        let hits = check(line, false, false);
        assert_eq!(hits.len(), 1);
        // "register_raw_event_observer(" starts at "    app." (8 chars), so col = 9.
        assert_eq!(
            hits[0].0,
            line.find("register_raw_event_observer").unwrap() + 1,
            "column must be 1-indexed at the token start"
        );
    }

    // -- file_is_exempt() unit tests ------------------------------------------

    #[test]
    fn raw_event_tap_is_exempt() {
        assert!(file_is_exempt(&PathBuf::from(
            "crates/nmp-ffi/src/raw_event_tap.rs"
        )));
        assert!(file_is_exempt(&PathBuf::from(
            "/abs/path/crates/nmp-ffi/src/raw_event_tap.rs"
        )));
    }

    #[test]
    fn lib_rs_is_exempt() {
        assert!(file_is_exempt(&PathBuf::from("crates/nmp-ffi/src/lib.rs")));
    }

    #[test]
    fn substrate_app_host_is_exempt() {
        assert!(file_is_exempt(&PathBuf::from(
            "crates/nmp-core/src/substrate/app_host.rs"
        )));
    }

    #[test]
    fn builder_is_exempt() {
        assert!(file_is_exempt(&PathBuf::from(
            "crates/nmp-defaults/src/builder.rs"
        )));
    }

    #[test]
    fn doctrine_lint_source_is_exempt() {
        assert!(file_is_exempt(&PathBuf::from(
            "crates/nmp-testing/bin/doctrine-lint/rules/a5.rs"
        )));
    }

    // -- file_in_scope() unit tests -------------------------------------------

    #[test]
    fn in_repo_production_crate_is_in_scope() {
        // Any production crate that is NOT a definition file is in scope.
        assert!(file_in_scope(&PathBuf::from(
            "crates/nmp-nip17/src/lib.rs"
        )));
        assert!(file_in_scope(&PathBuf::from(
            "apps/chirp/nmp-app-chirp/src/ffi/register.rs"
        )));
        assert!(file_in_scope(&PathBuf::from(
            "crates/nmp-marmot/src/projection/mod.rs"
        )));
    }

    #[test]
    fn definition_files_are_not_in_scope() {
        // file_in_scope delegates to file_is_exempt — definition files are
        // excluded before the path match runs.
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-ffi/src/raw_event_tap.rs"
        )));
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-core/src/substrate/app_host.rs"
        )));
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-defaults/src/builder.rs"
        )));
    }

    #[test]
    fn files_outside_monorepo_are_not_in_scope() {
        // e.g. /tmp/hl/src/nostrdb_mirror.rs — not in `crates/` or `apps/`
        assert!(!file_in_scope(&PathBuf::from(
            "/tmp/hl/src/nostrdb_mirror.rs"
        )));
    }
}

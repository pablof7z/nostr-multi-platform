//! A6 — in-repo use of the schema-less JSON snapshot-projection lane is banned.
//!
//! The generic (`serde_json::Value`) projection lane
//! (`register_snapshot_projection` / `ProjectionFn` / `SnapshotRegistry::register` /
//! `KernelSnapshot::projections`) was eliminated in PR #1525 (escape hatch #2).
//! The FlatBuffers wire schema deleted `payload:Value` from `SnapshotFrame` so
//! the lane was never wire-encoded; the Rust-internal producer code is now gone.
//! Only the typed FlatBuffers lane (`register_typed_snapshot_projection` /
//! `TypedProjectionFn` / `SnapshotRegistry::register_typed`) remains.
//!
//! Rule A6 pins that state. It flags any IN-REPO use of the deleted generic
//! lane symbols outside the doctrine-lint source tree itself.
//!
//! ## What this catches
//!
//! - `register_snapshot_projection(` — anchored so the typed variant
//!   `register_typed_snapshot_projection(` is NOT flagged.
//! - `register_snapshot_projection_gated(` — same anchor.
//! - `nmp_app_register_snapshot_projection` — the deleted C-ABI symbol.
//! - `SnapshotRegistry::register(` — the deleted `register()` method.
//! - `.register_gated(` — anchored so a leading ident char suppresses the match.
//! - `ProjectionFn` — the deleted type alias (anchored; `TypedProjectionFn` is NOT flagged).
//!
//! ## Exemptions
//!
//! - Doc-comment lines (`///`, `//!`, `//`) — skipped via `is_comment`.
//! - `#[cfg(test)]` module bodies — the caller's `in_test_cfg` flag.
//! - Test-only files (`*_tests.rs`, `tests.rs`, …) — handled via
//!   `d6::file_is_test_only` in the `main.rs` driver block.
//! - The doctrine-lint binary's own source tree — meta-false-positives.
//!
//! ## Per-line opt-out
//!
//! `// doctrine-allow: A6 — reason` on the offending line suppresses the
//! finding (the standard `allow::line_allows` mechanism).

use std::path::Path;

pub const ID: &str = "A6";

struct BannedToken {
    token: &'static str,
    /// When `true`, reject the match if the char immediately before the token
    /// start is an ASCII identifier char (`a–z`, `A–Z`, `0–9`, `_`). This
    /// prevents e.g. `register_typed_snapshot_projection(` from triggering
    /// on the `register_snapshot_projection(` sub-token.
    anchor: bool,
}

const BANNED_TOKENS: &[BannedToken] = &[
    BannedToken {
        token: "register_snapshot_projection(",
        anchor: true,
    },
    BannedToken {
        token: "register_snapshot_projection_gated(",
        anchor: true,
    },
    BannedToken {
        token: "nmp_app_register_snapshot_projection",
        anchor: false,
    },
    BannedToken {
        token: "SnapshotRegistry::register(",
        anchor: false,
    },
    // `anchor: false` — the leading `.` already disambiguates (no `register_typed_*`
    // method contains `.register_gated(`); an anchor would wrongly reject every real
    // `receiver.register_gated(` call since the char before `.` is always an identifier.
    BannedToken {
        token: ".register_gated(",
        anchor: false,
    },
    BannedToken {
        token: "ProjectionFn",
        anchor: true,
    },
];

/// True iff `path` is the doctrine-lint binary's own source tree.
pub fn file_is_exempt(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.contains("/bin/doctrine-lint/") || s.starts_with("doctrine-lint/")
}

/// True iff the file is in the A6 scan scope.
///
/// `.rs` files: within `crates/` or `apps/`, not the doctrine-lint tree.
/// `.h` header files: within `ios/` — catches C-ABI symbol reappearance in
/// native bridge headers (e.g. `apps/chirp/ios/Chirp/Bridge/NmpCore.h`) that the
/// `.rs`-only walker would silently miss.
pub fn file_in_scope(path: &Path) -> bool {
    if file_is_exempt(path) {
        return false;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    let is_h = path.extension().and_then(|e| e.to_str()) == Some("h");
    if is_h {
        return s.contains("/ios/") || s.starts_with("ios/");
    }
    s.contains("/crates/")
        || s.contains("/apps/")
        || s.starts_with("crates/")
        || s.starts_with("apps/")
}

/// Returns `(col, message, suggested)` for each banned A6 token found on a
/// non-comment, non-test line.
pub fn check(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for bt in BANNED_TOKENS {
        let mut start = 0;
        while let Some(rel) = line[start..].find(bt.token) {
            let abs = start + rel;
            if bt.anchor {
                let preceded_by_ident = abs > 0 && {
                    let prev = line.as_bytes()[abs - 1];
                    prev.is_ascii_alphanumeric() || prev == b'_'
                };
                if preceded_by_ident {
                    start += rel + bt.token.len();
                    continue;
                }
            }
            let col = abs + 1;
            hits.push((
                col,
                "in-repo use of the schema-less JSON snapshot-projection lane violates rule A6 \
                 (escape hatch #2 eliminated); KernelSnapshot has no generic projections map"
                    .to_string(),
                "register a typed FlatBuffers sidecar via register_typed_snapshot_projection \
                 (ADR-0037)"
                    .to_string(),
            ));
            start += rel + bt.token.len();
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn flags_register_snapshot_projection() {
        let hits = check(
            "    app.register_snapshot_projection(\"wallet\", || Value::Null);",
            false,
            false,
        );
        assert_eq!(hits.len(), 1, "must flag register_snapshot_projection");
        assert!(
            hits[0].1.contains("rule A6"),
            "message must reference rule A6; got: {}",
            hits[0].1
        );
        assert!(
            hits[0].2.contains("register_typed_snapshot_projection"),
            "suggestion must name register_typed_snapshot_projection; got: {}",
            hits[0].2
        );
    }

    #[test]
    fn does_not_flag_register_typed_snapshot_projection() {
        let hits = check(
            "    app.register_typed_snapshot_projection(\"wallet\", || None);",
            false,
            false,
        );
        assert!(
            hits.is_empty(),
            "register_typed_snapshot_projection must NOT be flagged; got: {:?}",
            hits
        );
    }

    #[test]
    fn flags_register_snapshot_projection_gated() {
        let hits = check(
            "    app.register_snapshot_projection_gated(\"key\", gate, || Value::Null);",
            false,
            false,
        );
        assert_eq!(
            hits.len(),
            1,
            "must flag register_snapshot_projection_gated"
        );
        assert!(hits[0].1.contains("rule A6"));
    }

    #[test]
    fn flags_nmp_app_register_snapshot_projection() {
        let hits = check(
            "unsafe { nmp_app_register_snapshot_projection(app, key, projector); }",
            false,
            false,
        );
        assert_eq!(
            hits.len(),
            1,
            "must flag nmp_app_register_snapshot_projection"
        );
        assert!(hits[0].1.contains("rule A6"));
    }

    #[test]
    fn flags_snapshot_registry_qualified_register() {
        let hits = check(
            "    SnapshotRegistry::register(&mut self, \"key\", f);",
            false,
            false,
        );
        assert_eq!(hits.len(), 1, "SnapshotRegistry::register( must be flagged");
        assert!(hits[0].1.contains("rule A6"));
    }

    #[test]
    fn flags_register_gated() {
        let hits = check(
            "    registry.register_gated(\"key\", gate, || Value::Null);",
            false,
            false,
        );
        assert_eq!(hits.len(), 1, "`.register_gated(` must be flagged");
        assert!(hits[0].1.contains("rule A6"));
    }

    #[test]
    fn flags_projection_fn_type() {
        let hits = check(
            "    let f: ProjectionFn = Box::new(|| Value::Null);",
            false,
            false,
        );
        assert_eq!(hits.len(), 1, "ProjectionFn must be flagged");
        assert!(hits[0].1.contains("rule A6"));
    }

    #[test]
    fn does_not_flag_typed_projection_fn() {
        let hits = check(
            "    let f: TypedProjectionFn = Box::new(|| None);",
            false,
            false,
        );
        assert!(
            hits.is_empty(),
            "TypedProjectionFn must NOT be flagged; got: {:?}",
            hits
        );
    }

    #[test]
    fn does_not_flag_tick_observer_fn() {
        let hits = check("    let f: TickObserverFn = Box::new(|| {});", false, false);
        assert!(
            hits.is_empty(),
            "TickObserverFn must NOT be flagged; got: {:?}",
            hits
        );
    }

    #[test]
    fn does_not_flag_in_comment() {
        let hits = check(
            "// call register_snapshot_projection(\"key\", closure)",
            true,
            false,
        );
        assert!(hits.is_empty(), "comment lines must not be flagged");
    }

    #[test]
    fn does_not_flag_in_test_cfg() {
        let hits = check(
            "    app.register_snapshot_projection(\"key\", || Value::Null);",
            false,
            true,
        );
        assert!(
            hits.is_empty(),
            "#[cfg(test)] bodies must not be flagged by A6"
        );
    }

    #[test]
    fn col_is_1_indexed() {
        let line = "    app.register_snapshot_projection(\"key\", f);";
        let hits = check(line, false, false);
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].0,
            line.find("register_snapshot_projection").unwrap() + 1,
            "column must be 1-indexed at the token start"
        );
    }

    #[test]
    fn doctrine_lint_source_is_exempt() {
        assert!(file_is_exempt(&PathBuf::from(
            "crates/nmp-testing/bin/doctrine-lint/rules/a6.rs"
        )));
    }

    #[test]
    fn in_repo_production_crate_is_in_scope() {
        assert!(file_in_scope(&PathBuf::from(
            "crates/nmp-nip29/src/register.rs"
        )));
        assert!(file_in_scope(&PathBuf::from(
            "apps/chirp/crates/nmp-app-chirp/src/ffi/register.rs"
        )));
    }

    #[test]
    fn doctrine_lint_not_in_scope() {
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-testing/bin/doctrine-lint/rules/a6.rs"
        )));
    }

    #[test]
    fn files_outside_monorepo_are_not_in_scope() {
        assert!(!file_in_scope(&PathBuf::from("/tmp/external/src/lib.rs")));
    }

    /// BLOCKER 3 — header files inside `ios/` are in scope for A6.
    ///
    /// This is the coverage-gap fix: before this change, `file_in_scope` only
    /// matched `crates/` and `apps/` paths and would silently skip
    /// `apps/chirp/ios/Chirp/Bridge/NmpCore.h`. A reappearance of the banned
    /// C-ABI symbol in the header would pass the lint undetected.
    #[test]
    fn ios_header_is_in_scope_for_a6() {
        assert!(
            file_in_scope(&PathBuf::from("apps/chirp/ios/Chirp/Bridge/NmpCore.h")),
            "ios/.h files must be in A6 scope (C-ABI symbol reappearance guard)"
        );
        // The specific file the blocker names.
        assert!(
            file_in_scope(&PathBuf::from(
                "/repo/apps/chirp/ios/Chirp/Bridge/NmpCore.h"
            )),
            "absolute path to NmpCore.h must be in scope"
        );
    }

    /// A `.h` file outside `ios/` is NOT in scope (no false positives on
    /// third-party headers under `crates/` or other non-ios trees).
    #[test]
    fn h_file_outside_ios_is_not_in_scope() {
        assert!(
            !file_in_scope(&PathBuf::from("crates/nmp-ffi/include/nmp_ffi.h")),
            ".h files under crates/ must NOT be in A6 scope (only ios/ headers)"
        );
        assert!(
            !file_in_scope(&PathBuf::from("/tmp/external.h")),
            ".h files outside the monorepo must not be in scope"
        );
    }

    /// The banned C-ABI token is flagged in a header declaration line.
    ///
    /// `a6::check` already handles raw token matching; this test confirms that
    /// a realistic C function declaration containing
    /// `nmp_app_register_snapshot_projection` is flagged by the same rule
    /// that fires on `.rs` files.
    #[test]
    fn c_header_declaration_of_banned_symbol_is_flagged() {
        let line = "void nmp_app_register_snapshot_projection(NmpApp *app, const char *key, void (*fn)(void));";
        let hits = check(line, false, false);
        assert_eq!(
            hits.len(),
            1,
            "the banned C-ABI symbol in a header declaration must be flagged"
        );
        assert!(
            hits[0].1.contains("rule A6"),
            "finding message must reference rule A6; got: {}",
            hits[0].1
        );
    }

    /// The typed-registration C symbol must NOT be flagged.
    #[test]
    fn c_header_typed_symbol_is_not_flagged() {
        let line = "void nmp_app_register_typed_snapshot_projection(NmpApp *app, const char *key, void (*fn)(void));";
        let hits = check(line, false, false);
        assert!(
            hits.is_empty(),
            "nmp_app_register_TYPED_snapshot_projection must NOT be flagged; got: {:?}",
            hits
        );
    }
}

//! D16 — snapshot-projection keys in `apps/chirp/` must use the `nmp.` prefix.
//!
//! Bare `"nip17.*"` / `"nip29.*"` string literals used as snapshot-projection
//! keys collide with the `nmp.` namespace convention every action namespace
//! already follows. After the rename landing in this PR, all projection keys
//! in `apps/chirp/` use `"nmp.nip17.*"` / `"nmp.nip29.*"`. D16 prevents
//! regressions — a new bare `"nip29.foo"` key registration would be caught
//! at lint time rather than surfacing as a silent wire-format mismatch between
//! Rust and Swift.
//!
//! ## What this catches
//!
//! Any string literal in a Rust source file that starts with `nip17.` or
//! `nip29.` (i.e. matches `"nip17.` or `"nip29.` — the opening double-quote
//! and bare prefix, NOT preceded by `nmp.`). The check is a simple substring
//! match: it looks for `"nip17.` or `"nip29.` not immediately preceded by
//! `nmp.` (which would make it `"nmp.nip17.` / `"nmp.nip29.`).
//!
//! ## Scope
//!
//! **Rust only — `apps/chirp/` tree** (`apps/chirp/nmp-app-chirp/src/`).
//! The matching Swift side (`ios/Chirp/`) is covered by the existing
//! `GroupChatDecodeTests.swift` round-trip tests and the typed
//! `SnapshotProjections.CodingKeys` enum — a Swift scanner is out of scope
//! for doctrine-lint (Rust binary, no Swift AST). Protocol crates under
//! `crates/nmp-nip*/src/` are already covered by D9 (action-namespace prefix)
//! and have no projection-key registrations.
//!
//! ## Explicit allowlist
//!
//! `crates/nmp-nip29/src/interest.rs` — `"nip29.discover"` and any other
//! `"nip29.*"` literals in that file are **stable hash seeds** for
//! [`InterestId`] de-duplication, NOT projection keys. Renaming them would
//! silently break interest de-duplication across upgrades. The file is
//! whitelisted by path; add a `// doctrine-allow: D16 — <reason>` per-line
//! opt-out if a future file needs a one-off exemption.
//!
//! Similarly, `crates/nmp-nip17/src/inbox.rs` contains `"nip17.giftwrap"` and
//! `"nip17.giftwrap.active"` as stable [`InterestId`] hash seeds — that file
//! is also path-whitelisted.
//!
//! ## Allowed exemptions
//!
//! - Comment lines (`//`, `///`, `//!`, inside `/* */`).
//! - Per-line `// doctrine-allow: D16 — reason` opt-out.
//! - Whitelisted paths: `nmp-nip29/src/interest.rs` and `nmp-nip17/src/inbox.rs`.

use std::path::Path;

pub const ID: &str = "D16";

/// Bare NIP namespace prefixes that must be preceded by `nmp.` in
/// projection-key context. The check looks for `"<prefix>` NOT immediately
/// preceded by `nmp.` (which would make it already-conformant).
const BARE_PREFIXES: &[&str] = &["\"nip17.", "\"nip29."];

/// Path suffixes that are explicitly whitelisted — these files contain stable
/// hash seeds (NOT projection keys) that share the bare `nip*.` shape.
const WHITELISTED_PATH_SUFFIXES: &[&str] = &[
    "nmp-nip29/src/interest.rs",
    "nmp-nip17/src/inbox.rs",
];

/// True iff the file is in scope for D16 — i.e. it is a Rust source under
/// `apps/chirp/`. Protocol crates and the `nmp-testing` crate (which hosts
/// fixture + negative-example strings) are out of scope.
///
/// The doctrine-lint binary scans Rust files only; Swift coverage is handled
/// by the `GroupChatDecodeTests.swift` round-trip tests in `ios/Chirp/`.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    // Only the apps/chirp/ tree.
    let in_chirp = s.contains("/apps/chirp/") || s.starts_with("apps/chirp/");
    if !in_chirp {
        return false;
    }
    // Exempt nmp-testing — it hosts fixture / negative-example strings.
    if s.contains("/crates/nmp-testing/") || s.starts_with("crates/nmp-testing/") {
        return false;
    }
    true
}

/// True iff the file is on the explicit allowlist (stable hash seeds, NOT
/// projection keys). Called before `file_in_scope` so the whitelisted
/// protocol-crate paths are excluded even if a future scope extension
/// accidentally includes them.
pub fn file_is_allowlisted(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    WHITELISTED_PATH_SUFFIXES
        .iter()
        .any(|suffix| s.ends_with(suffix) || s.contains(suffix))
}

/// Per-line check. Returns `(col, message, suggested)` per finding.
///
/// The check scans for any occurrence of `"nip17.` or `"nip29.` that is NOT
/// immediately preceded by `nmp.` (making it `"nmp.nip17.` / `"nmp.nip29.`
/// — the conformant form). Comment lines and `// doctrine-allow: D16`
/// opt-outs are suppressed by the caller.
pub fn check(line: &str, is_comment: bool) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for prefix in BARE_PREFIXES {
        let mut search_from = 0usize;
        while let Some(rel) = line[search_from..].find(prefix) {
            let abs = search_from + rel;
            search_from = abs + prefix.len();
            // Check if this is already prefixed with `nmp.`: the 4 bytes
            // immediately before the opening quote would be `nmp.`.
            if abs >= 4 && &line[abs - 4..abs] == "nmp." {
                // Already conformant — `"nmp.nip17."` / `"nmp.nip29."`.
                continue;
            }
            // Extract the bare literal value up to the closing quote (for a
            // readable error message). If there is no closing quote on the
            // same line, we still emit the finding — the opening quote is
            // enough to identify the problem.
            let after_quote = &line[abs + 1..]; // skip the opening `"`
            let value_end = after_quote.find('"').unwrap_or(after_quote.len());
            let value = &after_quote[..value_end];
            let col = abs + 1; // 1-indexed, pointing at the `"`
            hits.push((
                col,
                format!(
                    "snapshot-projection key `\"{}\"` uses a bare `nip` prefix — \
                     D16 requires `apps/chirp/` projection keys to start with `nmp.` \
                     (e.g. `\"nmp.{}\"`) so the namespace convention is consistent \
                     with action namespaces",
                    value, value,
                ),
                format!(
                    "rename to `\"nmp.{}\"` and update the matching Swift \
                     `SnapshotProjections.CodingKeys` raw value to the \
                     `.convertFromSnakeCase` post-transform form; add \
                     `// doctrine-allow: D16 — <reason>` if this is a \
                     stable hash seed (like `interest.rs:80`), NOT a \
                     projection key",
                    value,
                ),
            ));
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn chirp_path() -> PathBuf {
        PathBuf::from("apps/chirp/nmp-app-chirp/src/ffi.rs")
    }

    #[test]
    fn flags_bare_nip29_projection_key() {
        let hits = check(
            r#"    app.register_snapshot_projection("nip29.group_chat", move || snap.snapshot_json());"#,
            false,
        );
        assert_eq!(hits.len(), 1, "bare nip29. key must fire D16");
        assert!(hits[0].1.contains("nip29.group_chat"), "message must name the key");
        assert!(hits[0].1.contains("D16"), "message must name the rule");
    }

    #[test]
    fn flags_bare_nip17_projection_key() {
        let hits = check(
            r#"    app.register_snapshot_projection("nip17.dm_inbox", move || snap.snapshot_json());"#,
            false,
        );
        assert_eq!(hits.len(), 1, "bare nip17. key must fire D16");
        assert!(hits[0].1.contains("nip17.dm_inbox"), "message must name the key");
    }

    #[test]
    fn allows_nmp_prefixed_nip29_key() {
        let hits = check(
            r#"    app.register_snapshot_projection("nmp.nip29.group_chat", move || snap.snapshot_json());"#,
            false,
        );
        assert!(
            hits.is_empty(),
            "nmp.-prefixed key must not fire D16; got: {hits:?}"
        );
    }

    #[test]
    fn allows_nmp_prefixed_nip17_key() {
        let hits = check(
            r#"    app.register_snapshot_projection("nmp.nip17.dm_inbox", move || snap.snapshot_json());"#,
            false,
        );
        assert!(hits.is_empty(), "nmp.-prefixed key must not fire D16");
    }

    #[test]
    fn ignores_comment_lines() {
        let hits = check(
            r#"    // app.register_snapshot_projection("nip29.group_chat", ...)"#,
            true,
        );
        assert!(hits.is_empty(), "comment lines must not fire D16");
    }

    #[test]
    fn file_in_scope_includes_chirp_app() {
        assert!(file_in_scope(&PathBuf::from(
            "apps/chirp/nmp-app-chirp/src/ffi.rs"
        )));
        assert!(file_in_scope(&PathBuf::from(
            "/abs/path/apps/chirp/nmp-app-chirp/src/dm_runtime.rs"
        )));
    }

    #[test]
    fn file_in_scope_excludes_protocol_crates() {
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-nip29/src/projection/group_chat.rs"
        )));
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-nip17/src/inbox.rs"
        )));
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-core/src/kernel/mod.rs"
        )));
    }

    #[test]
    fn file_in_scope_excludes_nmp_testing() {
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-testing/bin/doctrine-lint/fixtures/d14/pos.rs"
        )));
    }

    #[test]
    fn allowlist_covers_interest_rs() {
        assert!(file_is_allowlisted(&PathBuf::from(
            "crates/nmp-nip29/src/interest.rs"
        )));
        assert!(file_is_allowlisted(&PathBuf::from(
            "/abs/path/crates/nmp-nip29/src/interest.rs"
        )));
    }

    #[test]
    fn allowlist_covers_inbox_rs() {
        assert!(file_is_allowlisted(&PathBuf::from(
            "crates/nmp-nip17/src/inbox.rs"
        )));
        assert!(file_is_allowlisted(&PathBuf::from(
            "/abs/path/crates/nmp-nip17/src/inbox.rs"
        )));
    }

    #[test]
    fn allowlist_does_not_cover_ordinary_chirp_files() {
        assert!(!file_is_allowlisted(&chirp_path()));
    }

    #[test]
    fn reports_column_at_opening_quote() {
        let line = r#"    app.register_snapshot_projection("nip29.group_chat", ...);"#;
        let hits = check(line, false);
        assert_eq!(hits.len(), 1);
        let expected_col = line.find('"').unwrap() + 1; // 1-indexed
        assert_eq!(hits[0].0, expected_col, "column must point at the opening quote");
    }

    #[test]
    fn flags_nip29_in_doc_comment_only_when_not_is_comment() {
        // The caller passes `is_comment = false` for non-comment lines;
        // a doc comment in code (as a string literal) still fires.
        let hits = check(
            r#"    let key = "nip29.my_projection";"#,
            false,
        );
        assert_eq!(hits.len(), 1, "bare nip29. in a string literal must fire");
    }
}

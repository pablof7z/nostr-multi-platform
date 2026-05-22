//! D9 — protocol-crate action namespaces use the `nmp.` prefix.
//!
//! Every action namespace registered through the kernel's action seam is
//! visible on the dispatch wire (`nmp_app_dispatch_action`'s `namespace`
//! argument). For external hosts to identify substrate-provided verbs at a
//! glance — and for protocol/substrate crates to never collide with an app
//! crate's own action vocabulary — protocol-crate namespaces MUST use the
//! `nmp.<nip>.<verb>` shape (e.g. `nmp.nip29.post_chat_message`).
//!
//! ## What this catches
//!
//! `ActionModule` impls declare their wire namespace via a
//! `const NAMESPACE: &'static str = "..."` item. If the string literal does
//! not begin with `nmp.`, D9 fires.
//!
//! ## Scope
//!
//! Protocol/substrate crates only — every `crates/nmp-*/src/` tree. App-layer
//! crates under `apps/<app>/` legitimately use app-local vocabulary
//! (`chirp.react`, ...) so they are exempt. Note: `nmp.follow` /
//! `nmp.unfollow` were renamed from `chirp.follow` / `chirp.unfollow` because
//! NIP-02 follow/unfollow are protocol primitives, not Chirp-specific verbs —
//! they now live under the substrate `nmp.*` namespace and are not exempt.
//!
//! The `nmp-testing` crate is also exempt — it hosts this rule's own fixtures
//! and test harnesses that intentionally include negative examples.
//!
//! ## Allowed exemptions
//!
//! - Comment lines (any of `//`, `///`, `//!`, inside `/* */`).
//! - Per-line `// doctrine-allow: D9 — reason` opt-out.

use std::path::Path;

pub const ID: &str = "D9";

/// True iff the file lives under a `crates/nmp-*/src/` tree (a protocol or
/// substrate crate), EXCEPT for `crates/nmp-testing/src/` and the doctrine-lint
/// binary's own source — those host fixture / negative-example strings that
/// would otherwise be false positives.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    // Only the workspace `crates/` tree is scoped — app-layer crates under
    // `apps/<app>/` legitimately use app-local vocabulary.
    let in_crates =
        s.contains("/crates/nmp-") || s.starts_with("crates/nmp-");
    if !in_crates {
        return false;
    }
    // Exempt the test-infrastructure crate (this rule's host and fixtures).
    if s.contains("/crates/nmp-testing/") || s.starts_with("crates/nmp-testing/") {
        return false;
    }
    true
}

/// Detect `const NAMESPACE: &'static str = "..."` and emit a finding when the
/// quoted value does not start with `nmp.`.
///
/// The match is deliberately precise — it requires the `NAMESPACE` identifier
/// and the `&'static str` type so it does NOT trip on unrelated `const` items
/// or on `pub const NS_FOO: &str = "chirp.foo"` style aliases (those live in
/// app crates and are out of scope anyway).
pub fn check(line: &str, is_comment: bool) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let Some((value_start, value)) = parse_namespace_literal(line) else {
        return Vec::new();
    };
    if value.starts_with("nmp.") {
        return Vec::new();
    }
    let col = value_start + 1; // 1-indexed columns for clippy compatibility
    vec![(
        col,
        format!(
            "action namespace `\"{}\"` does not start with `nmp.` — D9 requires \
             protocol-crate namespaces to use the `nmp.<nip>.<verb>` shape",
            value
        ),
        format!(
            "rename to `\"nmp.{}\"` (or otherwise prefix with `nmp.`); update every \
             string-literal call site in lockstep",
            value
        ),
    )]
}

/// If `line` declares `const NAMESPACE: &str = "<value>";` or
/// `const NAMESPACE: &'static str = "<value>";`, return the byte offset of
/// the opening quote and the literal value. Returns `None` for any other line.
///
/// Tolerates surrounding whitespace and an optional visibility modifier
/// (`pub`, `pub(crate)`, `pub(super)`) — matches the codebase convention used
/// by every `ActionModule` impl. Both the `&str` (module-level constant) and
/// `&'static str` (trait-associated constant) type ascriptions are accepted.
fn parse_namespace_literal(line: &str) -> Option<(usize, String)> {
    // Cheap reject — skip any line that obviously can't be a NAMESPACE
    // string-literal declaration.
    if !line.contains("NAMESPACE")
        || (!line.contains("&'static str") && !line.contains("&str"))
    {
        return None;
    }
    // Find the position of `NAMESPACE`.
    let ns_pos = line.find("NAMESPACE")?;
    // The fragment before `NAMESPACE` must end in something that makes it a
    // `const` item declaration (we accept `const ` immediately before, or
    // after a visibility modifier).
    let before = &line[..ns_pos];
    let trimmed_before = before.trim_end();
    if !trimmed_before.ends_with("const") {
        return None;
    }

    // Find the `=` between the type ascription and the value.
    let eq_pos = line[ns_pos..].find('=').map(|i| ns_pos + i)?;
    // Find the opening quote of the string literal.
    let after_eq = &line[eq_pos + 1..];
    let quote_rel = after_eq.find('"')?;
    let value_start = eq_pos + 1 + quote_rel;
    let after_quote = &line[value_start + 1..];
    let close_rel = after_quote.find('"')?;
    let value = after_quote[..close_rel].to_string();
    Some((value_start, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn flags_nip29_prefixed_namespace() {
        let hits = check(
            "    const NAMESPACE: &'static str = \"nip29.post_chat_message\";",
            false,
        );
        assert_eq!(hits.len(), 1, "expected one D9 finding");
        assert!(
            hits[0].1.contains("nip29.post_chat_message"),
            "message must name the offending literal; got: {}",
            hits[0].1
        );
        assert!(hits[0].1.contains("D9"), "rule id must appear; got: {}", hits[0].1);
    }

    #[test]
    fn allows_nmp_prefixed_namespace() {
        let hits = check(
            "    const NAMESPACE: &'static str = \"nmp.nip29.post_chat_message\";",
            false,
        );
        assert!(hits.is_empty(), "nmp.-prefixed namespace must not fire");
    }

    #[test]
    fn allows_nmp_publish_namespace() {
        let hits = check(
            "    const NAMESPACE: &'static str = \"nmp.publish\";",
            false,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn allows_nmp_nip17_send_namespace() {
        let hits = check(
            "    const NAMESPACE: &'static str = \"nmp.nip17.send\";",
            false,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn flags_pub_const_namespace() {
        // Tolerates `pub const NAMESPACE: ...` too.
        let hits = check(
            "    pub const NAMESPACE: &'static str = \"foo.bar\";",
            false,
        );
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn flags_module_level_str_namespace() {
        // Module-level constants use `&str` (not `&'static str`) — e.g. the
        // pattern in `crates/nmp-nipNN/src/domain.rs`. Both forms must trip
        // the rule when the literal lacks the `nmp.` prefix.
        let hits = check("pub const NAMESPACE: &str = \"legacy.namespace\";", false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("legacy.namespace"));
    }

    #[test]
    fn allows_module_level_str_namespace_with_prefix() {
        let hits = check("pub const NAMESPACE: &str = \"nmp.nip22.comments\";", false);
        assert!(hits.is_empty());
    }

    #[test]
    fn ignores_comment_line() {
        let hits = check(
            "    /// const NAMESPACE: &'static str = \"nip29.post_chat_message\";",
            true,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn ignores_unrelated_const_with_namespace_in_value() {
        // A `const FOO` whose value happens to contain the word `NAMESPACE`
        // is not a NAMESPACE declaration — D9 must not fire.
        let hits = check(
            "    const FOO: &str = \"reserved.NAMESPACE.token\";",
            false,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn ignores_non_const_namespace_lines() {
        // A method or field named `namespace` is not the NAMESPACE constant
        // and must not be flagged.
        let hits = check("        let namespace = action.namespace();", false);
        assert!(hits.is_empty());
    }

    #[test]
    fn reports_column_at_opening_quote() {
        // Column points at the offending string literal so a developer can
        // jump straight to the value to fix.
        let line = "const NAMESPACE: &'static str = \"foo.bar\";";
        let hits = check(line, false);
        assert_eq!(hits.len(), 1);
        let expected_col = line.find('"').unwrap() + 1; // 1-indexed
        assert_eq!(hits[0].0, expected_col, "column must point at the opening quote");
    }

    #[test]
    fn file_in_scope_includes_protocol_crates() {
        assert!(file_in_scope(&PathBuf::from(
            "crates/nmp-nip29/src/action/content.rs"
        )));
        assert!(file_in_scope(&PathBuf::from(
            "/abs/path/crates/nmp-nip17/src/lib.rs"
        )));
        assert!(file_in_scope(&PathBuf::from("crates/nmp-core/src/publish.rs")));
    }

    #[test]
    fn file_in_scope_excludes_apps() {
        assert!(!file_in_scope(&PathBuf::from(
            "apps/chirp/nmp-app-chirp/src/ffi.rs"
        )));
        assert!(!file_in_scope(&PathBuf::from(
            "/abs/path/apps/chirp/nmp-app-chirp/src/lib.rs"
        )));
    }

    #[test]
    fn file_in_scope_excludes_nmp_testing() {
        // The lint's own host crate carries fixtures with negative examples;
        // scanning itself would create spurious findings.
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-testing/bin/doctrine-lint/fixtures/d9/pos.rs"
        )));
        assert!(!file_in_scope(&PathBuf::from(
            "/abs/path/crates/nmp-testing/src/lib.rs"
        )));
    }
}

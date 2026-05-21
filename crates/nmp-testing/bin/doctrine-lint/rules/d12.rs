//! D12 — async-completing action modules must record stages.
//!
//! Per PR-G, `ActionModule::is_async_completing()` is a registry marker
//! returning `false` by default. A module that overrides it to `true` is
//! declaring a contract with the host: the action will produce a
//! lifecycle observable through `projections["action_stages"]`, and the
//! module MUST record stage transitions via
//! [`Kernel::record_action_stage`] (or any of its callers) so the mirror
//! reflects reality. A module that flips the marker without ever
//! recording a stage ships an empty stage seam — a silent host-visible
//! bug: the host's progress indicator never updates.
//!
//! ## What this catches
//!
//! Per-file scan. A file declaring a `fn is_async_completing(` body that
//! returns `true` (i.e. the line contains both `is_async_completing` and a
//! `true` literal) MUST also contain at least one of:
//!
//!   * `record_action_stage(` — the kernel-side recorder
//!   * `record_action_terminal_failure(` — the engine-side sibling already
//!     covered by the publish path
//!   * `record_action_failure(` — the kernel wrapper that fans both
//!
//! If none appears in the same file the rule fires on the
//! `is_async_completing` line. This is intentionally grep-level (matching
//! D8/D9): a richer AST check is a future-work item documented inline.
//!
//! ## Scope
//!
//! Protocol/substrate crates only — every `crates/nmp-*/src/` tree, plus
//! app-layer crates under `apps/<app>/`. The `nmp-testing` crate is
//! exempt (it hosts negative-example fixtures for this rule).
//!
//! ## Allowed exemptions
//!
//! - Comment lines (any of `//`, `///`, `//!`, inside `/* */`).
//! - Per-line `// doctrine-allow: D12 — reason` opt-out on the
//!   `is_async_completing` declaration.
//!
//! ## Known false-negative: cross-file declaration / recording
//!
//! The grep-level scan is per-FILE. A module whose `is_async_completing`
//! declaration lives in one file (the `impl ActionModule for FooModule`
//! block) but whose `record_action_stage` calls live in a sibling
//! (engine, executor, actor dispatch handler) slips through — exactly
//! the shape `nmp-core`'s `PublishModule` has. Tightening the rule to
//! span files would require crate-wide cross-file scanning, the
//! AST-level check the PR-G spec explicitly deferred. The runtime
//! coverage is by `kernel/action_stages_tests.rs` — a recording-missing
//! regression is caught there, just not at lint time. Documented on
//! `PublishModule::is_async_completing` so a reader sees the pointer
//! to the real call sites.

use std::path::Path;

pub const ID: &str = "D12";

/// True iff D12 should scan `path`. Production protocol + app crates
/// scoped; `nmp-testing` is exempt (fixture host with negative examples).
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    let in_crates = s.contains("/crates/nmp-") || s.starts_with("crates/nmp-");
    let in_apps = s.contains("/apps/") || s.starts_with("apps/");
    if !(in_crates || in_apps) {
        return false;
    }
    if s.contains("/crates/nmp-testing/") || s.starts_with("crates/nmp-testing/") {
        return false;
    }
    true
}

/// Per-file finding type: the line number of the offending
/// `is_async_completing` declaration plus a rendered message. The driver
/// re-uses this to emit a standard `Finding`.
#[derive(Debug)]
pub struct AsyncMarkerHit {
    pub line: usize,
    pub col: usize,
    pub message: String,
    pub suggested: String,
}

/// Scan a whole file's contents in one pass: collect every
/// `is_async_completing` → `true` declaration line and every stage-recording
/// call site; emit a finding for each marker that has no recording
/// sibling. Returns an empty `Vec` for files that either don't declare the
/// marker or do declare it AND record stages.
///
/// Two cheap loops over the file lines:
///   1. find every line that BOTH declares `fn is_async_completing(` and
///      returns `true` (declaration + literal must coexist on the same
///      line — the grep-level heuristic).
///   2. find any line that contains a recording call.
///
/// `is_comment` is computed by the caller (the walker tracks comment
/// state across multi-line `/* */` blocks). The driver also honours
/// `// doctrine-allow: D12 — ...` on the marker line.
pub fn scan_file(text: &str, line_is_comment: &[bool]) -> Vec<AsyncMarkerHit> {
    let mut markers: Vec<(usize, usize)> = Vec::new();
    let mut has_recording_call = false;

    for (idx, line) in text.lines().enumerate() {
        let is_comment = line_is_comment.get(idx).copied().unwrap_or(false);
        if is_comment {
            continue;
        }
        if line_has_async_marker_returning_true(line) {
            // Column of the `is_async_completing` identifier — 1-indexed.
            let col = line.find("is_async_completing").unwrap_or(0) + 1;
            markers.push((idx + 1, col));
        }
        if line_records_action_stage(line) {
            has_recording_call = true;
        }
    }

    if has_recording_call {
        return Vec::new();
    }
    markers
        .into_iter()
        .map(|(line, col)| AsyncMarkerHit {
            line,
            col,
            message: format!(
                "`is_async_completing` returns `true` but this file never calls \
                 `record_action_stage` — D12 requires async-completing modules \
                 to record stage transitions so the `action_stages` mirror \
                 reflects reality"
            ),
            suggested:
                "call `kernel.record_action_stage(correlation_id, stage, detail)` \
                 from the module's executor or actor handler — see \
                 `crates/nmp-core/src/kernel/publish_engine.rs` for the canonical \
                 publish-path consumer"
                    .to_string(),
        })
        .collect()
}

/// True iff `line` declares an `is_async_completing` function body that
/// returns `true`. We accept three shapes the codebase actually uses:
///
///   1. one-liner returning a literal:
///      `fn is_async_completing() -> bool { true }`
///   2. block opener whose body includes `true` on the same line:
///      `fn is_async_completing() -> bool { return true; }`
///   3. the same with a trailing comment.
///
/// We do NOT match a declaration that spans multiple lines (rare in this
/// codebase). The cost is a deferred lint: a multi-line definition slips
/// through this version; the trait marker still drives behaviour and the
/// missing recording-call is observable via the runtime stage mirror in
/// production. Promoted to a multi-line scan once a real callsite needs it.
fn line_has_async_marker_returning_true(line: &str) -> bool {
    // Require both the fn name and a `true` literal on the same line. The
    // `fn` prefix prevents false positives from a doc-comment or method
    // CALL — `is_async_completing()` invocations don't have `fn` before
    // them.
    if !line.contains("is_async_completing") {
        return false;
    }
    if !line.contains("fn ") {
        return false;
    }
    contains_word_true(line)
}

/// True iff `line` contains a `record_action_stage(`, `record_action_failure(`,
/// or `record_action_terminal_failure(` call. The match is plain substring —
/// false positives (a doc comment naming one of these symbols) are filtered
/// by the `is_comment` gate, and a recording call inside a real `#[cfg(test)]`
/// fn still counts (a test exercising the stage seam IS proof the module
/// records).
fn line_records_action_stage(line: &str) -> bool {
    line.contains("record_action_stage(")
        || line.contains("record_action_failure(")
        || line.contains("record_action_terminal_failure(")
}

/// True iff `line` contains the keyword `true` as a standalone token (not
/// e.g. `truely` or `intrue`). Cheap left-and-right boundary check — Rust
/// keywords cannot abut identifier characters in a code-position context.
fn contains_word_true(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut start = 0;
    while let Some(rel) = line[start..].find("true") {
        let pos = start + rel;
        let left_ok = pos == 0 || !is_ident_char(bytes[pos - 1]);
        let right_ok = pos + 4 >= bytes.len() || !is_ident_char(bytes[pos + 4]);
        if left_ok && right_ok {
            return true;
        }
        start = pos + 4;
    }
    false
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: scan a string source as if every line were non-comment.
    fn scan(src: &str) -> Vec<AsyncMarkerHit> {
        let n = src.lines().count();
        let flags = vec![false; n];
        scan_file(src, &flags)
    }

    #[test]
    fn flags_async_marker_without_recording_call() {
        let src = "\
struct M;
impl ActionModule for M {
    fn is_async_completing() -> bool { true }
}
";
        let hits = scan(src);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].message.contains("D12"));
        assert!(hits[0].message.contains("is_async_completing"));
        assert_eq!(hits[0].line, 3, "finding points at the marker declaration line");
    }

    #[test]
    fn passes_when_recording_call_is_present() {
        let src = "\
fn drive(k: &mut Kernel) {
    k.record_action_stage(\"x\", ActionStage::Publishing, None);
}
impl ActionModule for M {
    fn is_async_completing() -> bool { true }
}
";
        let hits = scan(src);
        assert!(
            hits.is_empty(),
            "a sibling `record_action_stage` call satisfies the rule"
        );
    }

    #[test]
    fn passes_when_record_action_failure_is_present() {
        // `record_action_failure` fans into the stage mirror — it's a
        // legitimate recording call.
        let src = "\
impl ActionModule for M {
    fn is_async_completing() -> bool { true }
}
fn fail(k: &mut Kernel) {
    k.record_action_failure(id, msg);
}
";
        let hits = scan(src);
        assert!(hits.is_empty());
    }

    #[test]
    fn default_false_marker_is_ignored() {
        // The trait's default returns `false`. A module that doesn't override
        // it (or that explicitly writes `false`) is synchronous-by-declaration
        // — D12 does not fire.
        let src = "\
impl ActionModule for M {
    fn is_async_completing() -> bool { false }
}
";
        let hits = scan(src);
        assert!(hits.is_empty(), "a `false` marker is synchronous; no recording required");
    }

    #[test]
    fn comment_lines_are_skipped_by_the_caller() {
        // The walker masks comment lines via the parallel `line_is_comment`
        // vec. A doc-comment naming the function must not fire the rule.
        let src = "\
/// Modules with `fn is_async_completing() -> bool { true }` ...
";
        let flags = vec![true];
        let hits = scan_file(src, &flags);
        assert!(hits.is_empty());
    }

    #[test]
    fn method_call_without_fn_is_not_a_declaration() {
        // A call site `M::is_async_completing()` is not a declaration — no
        // `fn` keyword on the same line.
        let src = "\
fn observe(_: &M) {
    let _ = M::is_async_completing();
}
";
        let hits = scan(src);
        assert!(hits.is_empty());
    }

    #[test]
    fn contains_word_true_rejects_truncated_identifiers() {
        // The `true` in `truely` is not a literal; the boundary check filters it.
        assert!(contains_word_true("    fn f() -> bool { true }"));
        assert!(!contains_word_true("    fn f() -> Truely { return Truely; }"));
        assert!(!contains_word_true("    let x = intrue;"));
    }

    #[test]
    fn file_in_scope_includes_protocol_and_app_crates() {
        assert!(file_in_scope(&Path::new(
            "crates/nmp-nip29/src/action/content.rs"
        )));
        assert!(file_in_scope(&Path::new(
            "apps/chirp/nmp-app-chirp/src/lib.rs"
        )));
        assert!(file_in_scope(&Path::new("crates/nmp-core/src/publish.rs")));
    }

    #[test]
    fn file_in_scope_excludes_nmp_testing() {
        assert!(!file_in_scope(&Path::new(
            "crates/nmp-testing/bin/doctrine-lint/fixtures/d12/neg.rs"
        )));
        assert!(!file_in_scope(&Path::new("crates/nmp-testing/src/lib.rs")));
    }
}

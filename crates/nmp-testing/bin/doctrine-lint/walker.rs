//! Source file walker with `#[cfg(test)]` module-block tracking.
//!
//! Rust modules guarded by `#[cfg(test)]` (or `#[cfg(any(test, feature =
//! "test-support"))]`, etc., where any disjunct includes `test`) are excluded
//! from D6 / D8 scans — their unwraps, panics, and allocations are valid in
//! the test build. Without this discrimination the lint produces a storm of
//! false positives on every codebase with healthy in-source tests.
//!
//! The tracker walks each file line by line, maintaining a brace-depth
//! counter and a vector of depths at which test-scope modules opened. It is
//! *not* a full Rust parser. Accuracy bar: "zero false positives on current
//! `nmp-core/`," not "AST-correct on adversarial input."

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::braces::count_braces_ignoring_strings;

/// One scanned source line plus the bookkeeping needed to decide whether it
/// sits inside a `#[cfg(test)]` module.
pub struct ScannedLine<'a> {
    pub line_no: usize,
    pub text: &'a str,
    /// Whether the line is logically inside a `#[cfg(test)]`-gated module
    /// (transitively — nested mods inherit).
    pub in_test_cfg: bool,
    /// Whether the line *starts* inside a block comment OR its first
    /// non-whitespace token is `//`. Rules consume this to skip comments.
    pub is_comment: bool,
}

/// Walk a directory tree under `root`, returning every `.rs` file path,
/// sorted for deterministic output. Skips `target/`, `.git/`, and any
/// `fixtures/` subtree (the lint must not scan its own positive fixtures).
pub fn collect_rs_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == "target" || name_str == ".git" || name_str == "fixtures" {
            continue;
        }
        if file_type.is_dir() {
            walk(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Scan a single file: read it, classify each line, and invoke `visit` per
/// line. The callback receives a [`ScannedLine`] without the path — the
/// caller's closure captures the path itself.
pub fn scan_file<F>(path: &Path, mut visit: F) -> io::Result<()>
where
    F: FnMut(&ScannedLine<'_>),
{
    let body = fs::read_to_string(path)?;
    let mut tracker = CfgTestTracker::default();
    let mut in_block_comment = false;
    let _ = path;

    for (idx, raw_line) in body.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw_line.trim_start();
        let starts_in_block_comment = in_block_comment;
        let is_comment = starts_in_block_comment || trimmed.starts_with("//");

        // The walker reports `in_test_cfg` as the state at the *start* of
        // the line. That correctly excludes the `mod tests {` declaration
        // line (which is outside the scope it opens) and includes every
        // line of the body up to and including the closing `}`.
        let in_test_cfg = tracker.in_test_cfg();
        tracker.observe_line(raw_line, in_block_comment);
        update_block_comment(raw_line, &mut in_block_comment);

        visit(&ScannedLine {
            line_no,
            text: raw_line,
            in_test_cfg,
            is_comment,
        });
    }
    Ok(())
}

fn update_block_comment(line: &str, state: &mut bool) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if *state {
            if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                *state = false;
                i += 2;
                continue;
            }
        } else if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            *state = true;
            i += 2;
            continue;
        }
        i += 1;
    }
}

// ────────────────────────────────────────────────────────────────────────────
// CfgTestTracker
// ────────────────────────────────────────────────────────────────────────────

/// Tracks whether the current source position lies inside a `#[cfg(test)]`-
/// gated module body. Maintains a brace-depth counter (`cur_depth`) plus a
/// vector of depths at which a test-scope module opened (`test_scope_depths`).
/// A line is "in test cfg" iff `test_scope_depths` is non-empty.
#[derive(Default)]
struct CfgTestTracker {
    /// Set when a `#[cfg(test)]`-shaped attribute was just observed.
    /// Consumed by the next item declaration (a `mod ... {` line in our
    /// case; anything else clears it).
    pending_test_attr: bool,
    /// Running brace depth across the file (all `{` minus all `}`).
    cur_depth: i32,
    /// Depths at which a `#[cfg(test)] mod ... {` block opened. Pop when
    /// `cur_depth` falls back to that depth.
    test_scope_depths: Vec<i32>,
}

impl CfgTestTracker {
    fn in_test_cfg(&self) -> bool {
        !self.test_scope_depths.is_empty()
    }

    fn observe_line(&mut self, line: &str, starts_in_block_comment: bool) {
        let trimmed = line.trim_start();
        let is_line_comment = trimmed.starts_with("//");
        let is_attr_or_decl_candidate = !starts_in_block_comment && !is_line_comment;

        if is_attr_or_decl_candidate
            && trimmed.starts_with("#[")
            && line.contains("cfg")
            && contains_test_pred(line)
        {
            self.pending_test_attr = true;
        } else if is_attr_or_decl_candidate && is_mod_decl_line(trimmed) {
            if line.contains('{') {
                // A `mod ... {` opens a new scope at the current depth. If
                // the pending attr says "test" OR we're already in a test
                // scope, the new scope is also test-scoped.
                let inherits = self.in_test_cfg();
                if self.pending_test_attr || inherits {
                    self.test_scope_depths.push(self.cur_depth);
                }
            }
            self.pending_test_attr = false;
        } else if is_attr_or_decl_candidate && !trimmed.is_empty() && !trimmed.starts_with('#') {
            // Any other non-comment, non-attr item line consumes the pending
            // attr (it belonged to the prior item — fn, struct, impl, etc.).
            self.pending_test_attr = false;
        }

        if !starts_in_block_comment {
            let (opens, closes) = count_braces_ignoring_strings(line);
            self.cur_depth += opens as i32;
            self.cur_depth -= closes as i32;
            while let Some(&top) = self.test_scope_depths.last() {
                if self.cur_depth <= top {
                    self.test_scope_depths.pop();
                } else {
                    break;
                }
            }
        }
    }
}

fn is_mod_decl_line(trimmed: &str) -> bool {
    let s = trimmed
        .trim_start_matches("pub(crate)")
        .trim_start_matches("pub(super)")
        .trim_start_matches("pub(in crate)")
        .trim_start_matches("pub")
        .trim_start();
    s.starts_with("mod ")
}

fn contains_test_pred(line: &str) -> bool {
    // Matches `#[cfg(test)]`, `#[cfg(any(test, ...))]`, and `feature =
    // "test-support"` variants. Conservative fallback uses comma-anchored
    // matches to avoid false positives like `cfg(any(testnet))`.
    if line.contains("cfg(test)") {
        return true;
    }
    if line.contains("test-support") && line.contains("cfg(") {
        return true;
    }
    if line.contains("cfg(any(")
        && (line.contains("(test,")
            || line.contains(", test,")
            || line.contains(", test)")
            || line.contains("(test)"))
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope_states(src: &str) -> Vec<(usize, bool)> {
        let mut out = Vec::new();
        let mut tracker = CfgTestTracker::default();
        let mut in_block = false;
        for (idx, line) in src.lines().enumerate() {
            out.push((idx + 1, tracker.in_test_cfg()));
            tracker.observe_line(line, in_block);
            update_block_comment(line, &mut in_block);
        }
        out
    }

    #[test]
    fn flat_test_mod_is_inside() {
        let src = "fn outside() {}\n\
                   #[cfg(test)]\n\
                   mod tests {\n\
                       fn inside() { panic!() }\n\
                   }\n\
                   fn outside_again() {}\n";
        let s = scope_states(src);
        assert!(!s[0].1);
        assert!(!s[1].1);
        assert!(!s[2].1, "mod decl line itself reads as outside");
        assert!(s[3].1, "body must be inside");
        assert!(s[4].1, "closing brace line (start-of-line state) still inside");
        assert!(!s[5].1);
    }

    #[test]
    fn cfg_any_with_test_and_feature_is_inside() {
        let src = "#[cfg(any(test, feature = \"test-support\"))]\n\
                   mod helpers {\n\
                       fn x() { unimplemented!() }\n\
                   }\n";
        assert!(scope_states(src)[2].1);
    }

    #[test]
    fn non_test_mod_does_not_count() {
        let src = "mod real {\n    fn x() { panic!() }\n}\n";
        assert!(!scope_states(src)[1].1);
    }

    #[test]
    fn nested_mod_inherits_test_scope() {
        let src = "#[cfg(test)]\n\
                   mod outer {\n\
                       mod inner {\n\
                           fn x() { panic!() }\n\
                       }\n\
                   }\n";
        assert!(scope_states(src)[3].1);
    }
}

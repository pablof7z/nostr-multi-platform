//! D8 — hot-path: no per-event allocation.
//!
//! Per ADRs 0001–0004, the reactivity hot path (ingest → reverse-index
//! lookup → view recompute → delta-buffer emit) must allocate in proportion
//! to *active view count*, not *event volume*. Per-event `format!` /
//! `Vec::new()` / `Box::new()` calls inside the dispatch loop are silent
//! perf regressions.
//!
//! ## v1 scope (deliberately narrow — the brief flags this as "fuzzy")
//!
//! We refuse to flag every allocation in every `handle_*` function — that
//! way lies false-positive Armageddon (every error-formatting `format!` is
//! a legitimate cold-path allocation). Instead, D8 v1 lints **only lines
//! inside a function whose body contains a `// hot path` marker comment**.
//!
//! The convention is:
//!
//! ```ignore
//! fn ingest_event(&mut self, evt: &Event) {
//!     // hot path
//!     let kinds_match = self.index.lookup(evt.kind);
//!     // …all subsequent lines in this fn are D8-scoped…
//! }
//! ```
//!
//! Adding the marker is an explicit opt-in: hot-path authors take on the
//! discipline. Existing prod code without the marker is unaffected.
//!
//! ## Scope (file allow-list)
//!
//! - `crates/nmp-core/src/kernel/ingest/` — every `.rs` file
//! - `crates/nmp-testing/bin/reactivity-bench/` — the bench itself
//!
//! ## Banned allocations (inside a marked hot-path function)
//!
//! - `Vec::new()`, `Vec::with_capacity(`
//! - `String::new()`, `String::with_capacity(`
//! - `Box::new(`, `Arc::new(`, `Rc::new(`
//! - `format!(`, `vec![`
//!
//! ## Future work
//!
//! Once the hot-path scope is well-marked across `nmp-core`, the marker
//! can be promoted to a real `#[hot_path]` proc-macro attribute (item #24
//! in the brainstorm: dhat-rs-backed allocation-count gate).

use std::path::Path;

pub const ID: &str = "D8";

const SCOPED_PATH_FRAGMENTS: &[&str] = &[
    "crates/nmp-core/src/kernel/ingest/",
    "crates/nmp-testing/bin/reactivity-bench/",
];

const BANNED_ALLOCATIONS: &[(&str, &str)] = &[
    ("Vec::new()", "preallocate at startup; reuse a thread-local scratch `Vec` cleared per event"),
    ("Vec::with_capacity(", "preallocate at startup; reuse a thread-local scratch `Vec` cleared per event"),
    ("String::new()", "preallocate at startup; reuse a `String` cleared per event"),
    ("String::with_capacity(", "preallocate at startup; reuse a `String` cleared per event"),
    ("Box::new(", "consider stack allocation or a pool; D8 forbids per-event Box allocation"),
    ("Arc::new(", "share an `Arc` constructed once at startup; D8 forbids per-event Arc allocation"),
    ("Rc::new(", "share an `Rc` constructed once at startup; D8 forbids per-event Rc allocation"),
    ("format!(", "use `write!` into a reused `String` buffer; per-event `format!` allocates"),
    ("vec![", "preallocate; the `vec![]` macro allocates every call"),
];

pub fn file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    SCOPED_PATH_FRAGMENTS.iter().any(|frag| s.contains(frag))
        || extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// D8 needs context (`is the cursor inside a fn marked // hot path?`). The
/// driver builds that context with [`HotPathTracker`] then calls
/// [`check_in_scope`] per line.
pub fn check_in_scope(
    line: &str,
    is_comment: bool,
    in_hot_path_fn: bool,
) -> Vec<(usize, String, String)> {
    if is_comment || !in_hot_path_fn {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (token, suggested) in BANNED_ALLOCATIONS {
        if let Some(rel) = line.find(token) {
            hits.push((
                rel + 1,
                format!(
                    "`{}` inside a `// hot path`-marked function violates D8 — no per-event allocation",
                    token.trim_end_matches('(')
                ),
                (*suggested).to_string(),
            ));
        }
    }
    hits
}

/// Per-file tracker: walks the body of each function, watches for a
/// `// hot path` marker comment within the function, and reports whether
/// the current line is inside a marked function body.
#[derive(Default)]
pub struct HotPathTracker {
    /// Brace depth across the file (all `{` minus all `}`).
    cur_depth: i32,
    /// Stack: one entry per open `fn ... {`. Tracks the (open_depth,
    /// is_marked_hot) pair. When `cur_depth` drops to `open_depth`, pop.
    fn_stack: Vec<(i32, bool)>,
}

impl HotPathTracker {
    /// Returns whether the *current* line is inside a function whose body
    /// contains a `// hot path` marker. Caller invokes [`observe_line`]
    /// after reading this value to advance the tracker.
    pub fn in_marked_fn(&self) -> bool {
        self.fn_stack.iter().any(|(_, marked)| *marked)
    }

    pub fn observe_line(&mut self, line: &str, starts_in_block_comment: bool) {
        if starts_in_block_comment {
            return;
        }
        // Update brace depth using a string-aware counter (shared with
        // walker::count_braces_ignoring_strings — duplicated here to keep
        // the rule module self-contained for the budget).
        let (opens, closes) = count_braces(line);

        // Pre-process the line for a fn opening: `fn foo(...) {` (the brace
        // may be on the same line or a later line). For simplicity we detect
        // ONLY same-line opener; multi-line fn signatures are uncommon for
        // the hot-path files we scope to.
        let fn_opens_here = is_fn_opener_with_brace(line);
        if fn_opens_here {
            // Push BEFORE applying the brace delta so the open_depth is the
            // pre-open depth.
            self.fn_stack.push((self.cur_depth, false));
        }

        // If a `// hot path` marker appears as a standalone comment (i.e.
        // the trimmed line is exactly `// hot path`, optionally followed
        // by trailing punctuation), flip the innermost fn's flag. We
        // require strict equality to avoid false-positives from doc text
        // referencing the marker (e.g. "// No `// hot path` marker here").
        if is_hot_path_marker(line) {
            if let Some(top) = self.fn_stack.last_mut() {
                top.1 = true;
            }
        }

        // Apply the brace delta.
        self.cur_depth += opens as i32;
        self.cur_depth -= closes as i32;

        // Pop any fns whose open_depth is now ≥ cur_depth.
        while let Some(&(open_depth, _)) = self.fn_stack.last() {
            if self.cur_depth <= open_depth {
                self.fn_stack.pop();
            } else {
                break;
            }
        }
    }
}

/// True if the trimmed line is the standalone hot-path marker comment.
/// Accepts `// hot path`, `// hot path!`, `// hot-path`, `//hot path` (no
/// space after slashes), case-insensitive. Rejects prose that *contains*
/// the phrase, like ``// No `// hot path` marker here``.
fn is_hot_path_marker(line: &str) -> bool {
    let trimmed = line.trim();
    let without_slash = trimmed
        .trim_start_matches('/')
        .trim_start()
        .to_ascii_lowercase();
    // Drop trailing punctuation/whitespace so `// hot path!` matches.
    let normalised = without_slash.trim_end_matches(|c: char| c == '!' || c == '.' || c.is_whitespace());
    matches!(normalised, "hot path" | "hot-path" | "hot_path")
}

fn is_fn_opener_with_brace(line: &str) -> bool {
    // True iff the line contains a `fn NAME(` declaration AND a `{`. We
    // tolerate visibility / `async` / `extern "C"` prefixes. The brace may
    // be the trailing `{` of the signature.
    let trimmed = line.trim_start();
    let has_fn = trimmed.starts_with("fn ")
        || trimmed.starts_with("pub fn ")
        || trimmed.starts_with("pub(crate) fn ")
        || trimmed.starts_with("pub(super) fn ")
        || trimmed.starts_with("async fn ")
        || trimmed.starts_with("pub async fn ")
        || trimmed.starts_with("extern \"C\" fn ")
        || trimmed.contains(" fn ");
    has_fn && trimmed.contains('{')
}

fn count_braces(line: &str) -> (usize, usize) {
    // Use the shared string-aware counter so brace-bearing string literals
    // (e.g. `"{}"` in a `format!` arg) don't perturb fn scope tracking.
    crate::braces::count_braces_ignoring_strings(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn flags_format_in_marked_fn() {
        let hits = check_in_scope("    let s = format!(\"{}\", x);", false, true);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn ignores_format_in_unmarked_fn() {
        let hits = check_in_scope("    let s = format!(\"{}\", x);", false, false);
        assert!(hits.is_empty());
    }

    #[test]
    fn flags_vec_new() {
        let hits = check_in_scope("    let v: Vec<u8> = Vec::new();", false, true);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn tracker_marks_fn_with_hot_path_marker() {
        let src = "\
fn ingest_event(&mut self, e: &E) {\n\
    // hot path\n\
    let v = Vec::new();\n\
}\n\
fn cold_path() {\n\
    let v = Vec::new();\n\
}\n";
        let mut tracker = HotPathTracker::default();
        let mut marked_per_line = Vec::new();
        for line in src.lines() {
            tracker.observe_line(line, false);
            marked_per_line.push(tracker.in_marked_fn());
        }
        // Lines: 1=fn opener, 2=marker (now marked), 3=Vec::new (marked),
        // 4=closing brace (still inside the scope until brace closes — but
        // ordering matters: closing brace is on the *same* line as the pop,
        // so after observe_line the stack is popped → in_marked_fn = false).
        // For our usage we only need: line 3 (Vec::new) reports marked.
        assert!(marked_per_line[2], "Vec::new() line must report as in_marked_fn");
        assert!(!marked_per_line[5], "cold path's Vec::new() must not");
    }

    #[test]
    fn hot_path_marker_recognises_standalone_comment() {
        assert!(is_hot_path_marker("    // hot path"));
        assert!(is_hot_path_marker("// hot path"));
        assert!(is_hot_path_marker("// hot path!"));
        assert!(is_hot_path_marker("//hot path"));
        assert!(is_hot_path_marker("    // HOT PATH"));
    }

    #[test]
    fn hot_path_marker_rejects_prose_containing_marker() {
        // The marker phrase appearing inside a longer prose comment must NOT
        // mark the fn (this was the d8_negative fixture failure mode).
        assert!(!is_hot_path_marker(
            "    // No `// hot path` marker → D8 doesn't fire here"
        ));
        assert!(!is_hot_path_marker(
            "    /// Authors mark hot paths with `// hot path` above the first line"
        ));
    }

    #[test]
    fn scope_check() {
        let no_extra: Vec<String> = Vec::new();
        assert!(file_in_scope(
            &PathBuf::from("/abs/path/crates/nmp-core/src/kernel/ingest/timeline.rs"),
            &no_extra,
        ));
        assert!(file_in_scope(
            &PathBuf::from("/abs/path/crates/nmp-testing/bin/reactivity-bench/main.rs"),
            &no_extra,
        ));
        assert!(!file_in_scope(
            &PathBuf::from("/abs/path/crates/nmp-core/src/relay.rs"),
            &no_extra,
        ));
        // extra_scopes opt-in (smoke test uses this).
        let extra = vec!["fixtures/d8".to_string()];
        assert!(file_in_scope(
            &PathBuf::from("/abs/path/fixtures/d8/pos.rs"),
            &extra,
        ));
    }
}

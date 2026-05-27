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
//!
//! ## D8 — no polling (sleep+check loops banned)
//!
//! Separate from the hot-path-allocation check above, D8's reactivity
//! contract forbids *polling*: `sleep+check` loops are banned at every
//! layer (see `AGENTS.md` §reactivity-contract and the
//! `feedback_no_polling` memory note). The canonical violation is a
//! `std::thread::sleep(...)` call in production code — it busy-waits the
//! kernel instead of using a blocking `recv`, an OS callback, or a
//! wall-clock-gated observer. The async equivalents
//! `tokio::time::sleep(...)` and `tokio::time::sleep_until(...)` are
//! equally forbidden — an awaited sleep+check loop polls just as surely as
//! a blocking one.
//!
//! Unlike the hot-path check this is **not** path-scoped: any
//! `thread::sleep(`, `tokio::time::sleep(`, or `tokio::time::sleep_until(`
//! in non-test code anywhere under `crates/nmp-core/src/` is a D8
//! violation. Test code is exempt (test timing helpers legitimately
//! sleep) via the same two-layer test detection D6 uses:
//!
//! 1. inline `#[cfg(test)]` modules (the walker's `in_test_cfg` flag), and
//! 2. test-only files by name (`*_tests.rs`, `tests_*.rs`, `/tests/`, …)
//!    — handled by the driver before calling [`check_no_polling`].
//!
//! Authors with a genuine need keep the escape hatch:
//! `// doctrine-allow: D8 — reason` on the same line.

use std::path::Path;

pub const ID: &str = "D8";

/// Tokens that flag a polling violation. Each is a plain substring:
///
/// - `thread::sleep(` — matches both fully-qualified `std::thread::sleep(`
///   and the bare `thread::sleep(` form used after a `use std::thread;`
///   import.
/// - `tokio::time::sleep(` — the async equivalent; an awaited sleep+check
///   loop polls just as surely as a blocking one.
/// - `tokio::time::sleep_until(` — the deadline-based async sleep.
///
/// `tokio::time::sleep_until(` does NOT contain `tokio::time::sleep(` (the
/// char after `sleep` is `_`, not `(`), so the two never double-fire on the
/// same call site.
const POLLING_TOKENS: &[&str] = &[
    "thread::sleep(",
    "tokio::time::sleep(",
    "tokio::time::sleep_until(",
];

const SCOPED_PATH_FRAGMENTS: &[&str] = &[
    "crates/nmp-core/src/kernel/ingest/",
    "crates/nmp-testing/bin/reactivity-bench/",
];

const BANNED_ALLOCATIONS: &[(&str, &str)] = &[
    (
        "Vec::new()",
        "preallocate at startup; reuse a thread-local scratch `Vec` cleared per event",
    ),
    (
        "Vec::with_capacity(",
        "preallocate at startup; reuse a thread-local scratch `Vec` cleared per event",
    ),
    (
        "String::new()",
        "preallocate at startup; reuse a `String` cleared per event",
    ),
    (
        "String::with_capacity(",
        "preallocate at startup; reuse a `String` cleared per event",
    ),
    (
        "Box::new(",
        "consider stack allocation or a pool; D8 forbids per-event Box allocation",
    ),
    (
        "Arc::new(",
        "share an `Arc` constructed once at startup; D8 forbids per-event Arc allocation",
    ),
    (
        "Rc::new(",
        "share an `Rc` constructed once at startup; D8 forbids per-event Rc allocation",
    ),
    (
        "format!(",
        "use `write!` into a reused `String` buffer; per-event `format!` allocates",
    ),
    (
        "vec![",
        "preallocate; the `vec![]` macro allocates every call",
    ),
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

/// D8 — no polling. Flags `thread::sleep(`, `tokio::time::sleep(`, and
/// `tokio::time::sleep_until(` calls in production code.
///
/// Unlike [`check_in_scope`] this is **not** path-scoped — it applies to
/// every non-test file under `crates/nmp-core/src/`. `is_comment` skips
/// comment lines; `in_test_cfg` skips lines inside an inline
/// `#[cfg(test)]` module (test timing helpers legitimately sleep). The
/// driver additionally skips whole test-only files by name before calling
/// this. The `// doctrine-allow: D8` escape hatch is honoured by the
/// driver, as for every other rule.
pub fn check_no_polling(
    line: &str,
    is_comment: bool,
    in_test_cfg: bool,
) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for token in POLLING_TOKENS {
        let mut start = 0;
        while let Some(rel) = line[start..].find(token) {
            let col = start + rel;
            hits.push((
                col + 1, // 1-indexed columns for clippy compatibility
                format!(
                    "`{}` violates D8 — no polling; sleep+check loops are banned",
                    token.trim_end_matches('('),
                ),
                "block on `Receiver::recv`, an OS callback, or a wall-clock-gated \
                 observer instead of busy-waiting"
                    .to_string(),
            ));
            start = col + token.len();
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
    let normalised =
        without_slash.trim_end_matches(|c: char| c == '!' || c == '.' || c.is_whitespace());
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
        assert!(
            marked_per_line[2],
            "Vec::new() line must report as in_marked_fn"
        );
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
    fn no_polling_flags_qualified_thread_sleep() {
        let hits = check_no_polling(
            "    std::thread::sleep(Duration::from_millis(30));",
            false,
            false,
        );
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("D8"));
        assert!(hits[0].1.contains("polling"));
    }

    #[test]
    fn no_polling_flags_bare_thread_sleep() {
        // `use std::thread;` then bare `thread::sleep(...)`.
        let hits = check_no_polling("        thread::sleep(backoff);", false, false);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn no_polling_ignores_comment_line() {
        let hits = check_no_polling("// avoid thread::sleep(...) here", true, false);
        assert!(hits.is_empty());
    }

    #[test]
    fn no_polling_ignores_test_cfg() {
        // Test timing helpers legitimately sleep — the in_test_cfg gate
        // (and the driver's test-file-name gate) exempt them.
        let hits = check_no_polling(
            "    thread::sleep(Duration::from_millis(1_000));",
            false,
            true,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn no_polling_reports_one_indexed_column() {
        let hits = check_no_polling("thread::sleep(d);", false, false);
        assert_eq!(hits[0].0, 1, "column is 1-indexed for clippy parity");
    }

    #[test]
    fn no_polling_flags_tokio_sleep() {
        // The async equivalent of `thread::sleep` — equally a poll.
        let hits = check_no_polling(
            "    tokio::time::sleep(Duration::from_millis(10)).await;",
            false,
            false,
        );
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("D8"));
        assert!(hits[0].1.contains("polling"));
        assert!(
            hits[0].1.contains("tokio::time::sleep"),
            "message must name the offending token; got: {}",
            hits[0].1
        );
    }

    #[test]
    fn no_polling_flags_tokio_sleep_until() {
        // The deadline-based async sleep — also a poll.
        let hits = check_no_polling(
            "    tokio::time::sleep_until(deadline).await;",
            false,
            false,
        );
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("D8"));
        assert!(hits[0].1.contains("polling"));
        assert!(
            hits[0].1.contains("tokio::time::sleep_until"),
            "message must name the offending token; got: {}",
            hits[0].1
        );
    }

    #[test]
    fn no_polling_does_not_double_match_sleep_inside_sleep_until() {
        // `tokio::time::sleep_until(` must NOT also trip the
        // `tokio::time::sleep(` token — the char after `sleep` is `_`, not
        // `(`, so the substrings are disjoint. Exactly one finding.
        let hits = check_no_polling(
            "    tokio::time::sleep_until(deadline).await;",
            false,
            false,
        );
        assert_eq!(
            hits.len(),
            1,
            "sleep_until must fire exactly once, not double-count as sleep"
        );
    }

    #[test]
    fn no_polling_ignores_tokio_sleep_in_test_cfg() {
        // Test timing helpers legitimately await a sleep — the in_test_cfg
        // gate exempts them, exactly as for `thread::sleep`.
        let hits = check_no_polling(
            "    tokio::time::sleep(Duration::from_millis(1)).await;",
            false,
            true,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn no_polling_ignores_tokio_sleep_comment_line() {
        let hits = check_no_polling("// avoid tokio::time::sleep(...) here", true, false);
        assert!(hits.is_empty());
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

//! D6 — errors never cross FFI as exceptions; no panics outside test.
//!
//! Operational failures must surface as `toast` / `busy` state fields, not
//! as Rust panics or `Result` types crossing the FFI seam. Inside `nmp-core`
//! production code:
//!
//! - `panic!`, `unreachable!`, `unimplemented!`, `todo!` are banned
//! - `.unwrap()` and `.expect(...)` are banned
//!
//! ## Allowed exemptions (do not flag)
//!
//! 1. **Comment lines** (line, block, `///`, `//!`).
//! 2. **`#[cfg(test)]` modules** detected inline via the walker's tracker.
//! 3. **Test-only files by filename**: `tests.rs`, any file whose name ends
//!    in `_tests.rs` (e.g. `auth_tests.rs`, `discovery_tests.rs`) or
//!    `_support.rs` (e.g. `test_support.rs`, `conformance_support.rs`), any
//!    file beginning `tests_` (e.g. `tests_kind5.rs`), and anything under a
//!    `/tests/` directory. Such files are declared as `#[cfg(test)] mod
//!    foo;` in their parent and the `cfg(test)` gate lives there, not inside
//!    the file — so the walker cannot see it. Filename exemption is the
//!    brief-mandated workaround.
//! 4. **`Mutex::lock().unwrap()` / `RwLock::*().unwrap()`** — lock poisoning
//!    is fatal-by-design; unwinding here is correct behaviour. This rule
//!    detects the immediate `.lock().unwrap()` / `.read().unwrap()` /
//!    `.write().unwrap()` chains.
//! 5. **Per-line `// doctrine-allow: D6 — reason`** opt-out.
//! 6. **Lines containing `// SAFETY:` comment** — author has justified the
//!    invariant; SAFETY-commented unsafe blocks pair with this exemption.
//!
//! `assert!` / `assert_eq!` / `debug_assert*!` are *not* flagged at all
//! (they panic on failure but encode invariants that are typically lifted
//! out by the optimiser in release builds and never reach production users
//! via FFI).

use std::path::Path;

pub const ID: &str = "D6";

const TEST_FILE_NAMES: &[&str] = &["tests.rs", "test_fixtures.rs"];

pub fn file_is_test_only(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        if TEST_FILE_NAMES.contains(&name) {
            return true;
        }
        // `*_tests.rs` is the codebase convention for a test-only file whose
        // `#[cfg(test)] mod <name>;` declaration (and thus the `cfg(test)`
        // gate) lives in the parent module, invisible to the line walker.
        // Match the suffix — but require a real `_` separator so a file
        // literally named `tests.rs` is still caught by the exact list
        // above and an unrelated name like `tests.rs`'s prefix can't slip.
        if name.ends_with("_tests.rs") {
            return true;
        }
        // `*_support.rs` is the codebase convention for a test-support
        // facade — `test_support.rs`, `conformance_support.rs`, etc. — whose
        // `#[cfg(any(test, feature = "test-support"))] mod <name>;`
        // declaration lives in the parent module, invisible to the line
        // walker. The `_support` suffix (with its `_` separator) is the
        // discriminating marker; a production file named `transport.rs`
        // would NOT match. `test_support.rs` was previously an exact-name
        // entry; the suffix subsumes it.
        if name.ends_with("_support.rs") {
            return true;
        }
        // `tests_*.rs` is the mirror convention: files like `tests_kind5.rs`
        // are declared as `#[cfg(all(test, ...))] mod tests_kind5;` in the
        // parent, so the cfg gate is invisible to the line walker here too.
        if name.starts_with("tests_") && name.ends_with(".rs") {
            return true;
        }
    }
    let s = path.to_string_lossy().replace('\\', "/");
    if s.contains("/tests/") || s.contains("/examples/") {
        return true;
    }
    // Test-only crate source: `nmp-testing` (test harnesses, stress binaries)
    // and `nmp-content-fixtures` (fixture builders) are never linked into
    // production artifacts. Using `.expect()` there is appropriate.
    // The doctrine-lint tool's own source contains banned patterns as string
    // constants — exempt non-fixture paths to avoid meta-false-positives.
    let in_test_infra = (s.contains("/nmp-testing/") || s.starts_with("crates/nmp-testing/"))
        && !s.contains("/doctrine-lint/fixtures/");
    if in_test_infra {
        return true;
    }
    s.contains("/nmp-content-fixtures/") || s.starts_with("crates/nmp-content-fixtures/")
}

const BANNED_PATTERNS: &[(&str, &str)] = &[
    ("panic!(", "return `Err` or set a toast field; bubble up via `Result`"),
    ("unreachable!(", "return `Err(KernelError::Invariant(...))` and document the supposedly-impossible state"),
    ("unimplemented!(", "stub with `Err(KernelError::NotImplemented)` or guard behind a feature flag"),
    ("todo!(", "delete or implement before merging; D6 forbids reachable `todo!`"),
];

/// Per-file scanner state. Tracks the previous non-comment line's trailing
/// method-chain element so we can recognise multi-line lock chains:
///
/// ```ignore
/// self.scripts
///     .lock()
///     .unwrap()
/// ```
///
/// without context, the `.unwrap()` line would false-positive.
#[derive(Default)]
pub struct State {
    /// Trimmed tail (last token) of the previous non-comment line. We look
    /// for `.lock()`, `.read()`, `.write()` here.
    prev_trail: String,
}

pub fn check(
    state: &mut State,
    line: &str,
    is_comment: bool,
    in_test_cfg: bool,
) -> Vec<(usize, String, String)> {
    let trimmed = line.trim();
    // Comment / cfg-test gates short-circuit but still update prev_trail
    // so a comment between `.lock()` and `.unwrap()` doesn't trip the
    // multi-line chain detector. (The pathological case `lock() //comment
    // unwrap()` is uncommon enough that we accept it.)
    if is_comment {
        return Vec::new();
    }
    let prev_trail_was_lock = is_lock_chain_tail(&state.prev_trail);
    // Update state for the *next* line. We do this BEFORE the early-return
    // for in_test_cfg so the chain detector still has accurate context if
    // the same chain straddles a cfg boundary (rare).
    state.prev_trail = trimmed.to_string();

    if in_test_cfg {
        return Vec::new();
    }
    // SAFETY-commented lines are author-justified; skip.
    if line.contains("// SAFETY:") || line.contains("// SAFETY ") {
        return Vec::new();
    }

    let mut hits = Vec::new();

    for (token, suggested) in BANNED_PATTERNS {
        if let Some(rel) = line.find(token) {
            hits.push((
                rel + 1,
                format!("`{}` violates D6 — errors must not cross FFI as exceptions", token.trim_end_matches('(')),
                (*suggested).to_string(),
            ));
        }
    }

    // .unwrap() — except Mutex/RwLock poisoning idiom (same-line OR multi-
    // line method chain).
    if let Some(rel) = line.find(".unwrap()") {
        let same_line_lock = is_lock_chain_tail(&line[..rel]);
        let multi_line_lock = prev_trail_was_lock && trimmed.starts_with(".unwrap()");
        if !same_line_lock && !multi_line_lock {
            hits.push((
                rel + 1,
                "`.unwrap()` violates D6 — return `Result` or default and toast".to_string(),
                "use `?` to propagate `Result`, or `.unwrap_or(default)` for fallible defaults".to_string(),
            ));
        }
    }

    // .expect("…") — except Mutex/RwLock poisoning idiom.
    if let Some(rel) = line.find(".expect(") {
        let same_line_lock = is_lock_chain_tail(&line[..rel]);
        let multi_line_lock = prev_trail_was_lock && trimmed.starts_with(".expect(");
        if !same_line_lock && !multi_line_lock {
            hits.push((
                rel + 1,
                "`.expect(...)` violates D6 — return `Result` or default and toast".to_string(),
                "use `?` with a meaningful `KernelError` variant; if invariant, prefer `assert!`".to_string(),
            ));
        }
    }

    hits
}

/// True if `s` ends with one of the lock-acquiring method calls. Used to
/// recognise both same-line (`self.x.lock().unwrap()`) and prior-line
/// (`self.x\n    .lock()\n    .unwrap()`) variants of the poisoning idiom.
fn is_lock_chain_tail(s: &str) -> bool {
    let t = s.trim_end();
    t.ends_with(".lock()") || t.ends_with(".read()") || t.ends_with(".write()")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_one(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
        let mut state = State::default();
        check(&mut state, line, is_comment, in_test_cfg)
    }

    #[test]
    fn flags_panic_in_prod() {
        let hits = check_one("    panic!(\"oops\");", false, false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("panic"));
    }

    #[test]
    fn ignores_panic_in_test_cfg() {
        let hits = check_one("    panic!(\"oops\");", false, true);
        assert!(hits.is_empty());
    }

    #[test]
    fn ignores_panic_in_comment() {
        let hits = check_one("// don't panic!(...)", true, false);
        assert!(hits.is_empty());
    }

    #[test]
    fn flags_unwrap_in_prod() {
        let hits = check_one("    let x = thing.unwrap();", false, false);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn exempts_mutex_lock_unwrap_same_line() {
        let hits = check_one("    let g = self.state.lock().unwrap();", false, false);
        assert!(hits.is_empty(), "Mutex lock-poisoning unwrap is exempt");
    }

    #[test]
    fn exempts_rwlock_read_unwrap_same_line() {
        let hits = check_one("    let g = self.rw.read().unwrap();", false, false);
        assert!(hits.is_empty());
    }

    #[test]
    fn exempts_multi_line_lock_chain() {
        // Replicates the `traits.rs` shape: `.lock()` on one line, `.unwrap()`
        // on the next. The State persists between calls.
        let mut state = State::default();
        let _ = check(&mut state, "        self.scripts", false, false);
        let _ = check(&mut state, "            .lock()", false, false);
        let hits = check(&mut state, "            .unwrap()", false, false);
        assert!(hits.is_empty(), "multi-line lock chain is exempt");
    }

    #[test]
    fn flags_multi_line_unwrap_not_after_lock() {
        let mut state = State::default();
        let _ = check(&mut state, "        self.optional_thing", false, false);
        let _ = check(&mut state, "            .as_ref()", false, false);
        let hits = check(&mut state, "            .unwrap()", false, false);
        assert_eq!(hits.len(), 1, "multi-line chain not via lock() is still a hit");
    }

    #[test]
    fn flags_expect() {
        let hits = check_one("    let x = thing.expect(\"oops\");", false, false);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn exempts_safety_commented_unwrap() {
        let hits = check_one(
            "    unsafe { *p.unwrap() } // SAFETY: invariant docs above",
            false,
            false,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn test_only_filename_exemption_matches_suffix_prefix_and_exacts() {
        use std::path::Path;
        // Exact-name list.
        assert!(file_is_test_only(Path::new("crates/nmp-core/src/kernel/tests.rs")));
        // `test_fixtures.rs` — exact-name exemption added by T154.
        assert!(file_is_test_only(Path::new(
            "crates/nmp-core/src/store/lmdb/test_fixtures.rs"
        )));
        // `*_support.rs` suffix — test-support facades whose
        // `#[cfg(any(test, feature = "test-support"))] mod ...;` declaration
        // lives in the parent. `test_support.rs` (formerly an exact-name
        // entry) and `conformance_support.rs` both match.
        assert!(file_is_test_only(Path::new(
            "crates/nmp-core/src/kernel/test_support.rs"
        )));
        assert!(file_is_test_only(Path::new(
            "crates/nmp-core/src/actor/commands/conformance_support.rs"
        )));
        // `*_tests.rs` suffix — the bug T106 fixed: these were NOT exempt
        // although they are `#[cfg(test)] mod ...;` in the parent.
        assert!(file_is_test_only(Path::new(
            "crates/nmp-core/src/kernel/discovery_tests.rs"
        )));
        assert!(file_is_test_only(Path::new(
            "crates/nmp-core/src/kernel/auth_tests.rs"
        )));
        assert!(file_is_test_only(Path::new("foo/bar/some_feature_tests.rs")));
        // `tests_*.rs` prefix — T154: files like `tests_kind5.rs` whose
        // `#[cfg(all(test, ...))] mod tests_kind5;` declaration lives in the
        // parent and is invisible to the line walker.
        assert!(file_is_test_only(Path::new(
            "crates/nmp-core/src/store/lmdb/tests_kind5.rs"
        )));
        assert!(file_is_test_only(Path::new("foo/bar/tests_scenario.rs")));
        // `/tests/` directory.
        assert!(file_is_test_only(Path::new("crates/x/tests/integration.rs")));
        // `/examples/` directory — standalone binary examples use `.expect()` by idiom.
        assert!(file_is_test_only(Path::new("crates/nmp-core/examples/outbox_perf/main.rs")));
        // Negatives: production files must NOT be exempted.
        assert!(!file_is_test_only(Path::new(
            "crates/nmp-core/src/ffi/capability.rs"
        )));
        // `contests.rs` contains "tests" but matches no exemption pattern.
        assert!(!file_is_test_only(Path::new("crates/x/src/contests.rs")));
        // A file that merely ends in `tests` without `_tests.rs` is not exempt.
        assert!(!file_is_test_only(Path::new("crates/x/src/run_tests.txt")));
        // `transport.rs` ends in `support`'s letters but lacks the `_support`
        // suffix — a production file must NOT be exempted by the new rule.
        assert!(!file_is_test_only(Path::new("crates/x/src/transport.rs")));
        // `support.rs` with no `_` separator is not the `*_support.rs`
        // convention — left un-exempt (production code is the safe default).
        assert!(!file_is_test_only(Path::new("crates/x/src/support.rs")));
    }

    #[test]
    fn exempts_test_infra_crates() {
        // `nmp-testing` and `nmp-content-fixtures` are never linked into
        // production artifacts — using `.expect()` in their source is fine.
        assert!(file_is_test_only(Path::new(
            "crates/nmp-testing/src/store_harness.rs"
        )));
        assert!(file_is_test_only(Path::new(
            "crates/nmp-testing/bin/ffi-stress/s4_reconciler_backpressure.rs"
        )));
        assert!(file_is_test_only(Path::new(
            "crates/nmp-testing/bin/doctrine-lint/rules/d6.rs"
        )));
        assert!(file_is_test_only(Path::new(
            "crates/nmp-content-fixtures/src/identities.rs"
        )));
        // Fixtures in doctrine-lint ARE intentional negative examples and must
        // NOT be exempted, otherwise the linter's own conformance tests break.
        assert!(!file_is_test_only(Path::new(
            "crates/nmp-testing/bin/doctrine-lint/fixtures/d6/violates_panic.rs"
        )));
    }

    #[test]
    fn flags_todo_and_unimplemented() {
        let hits = check_one("    todo!();", false, false);
        assert_eq!(hits.len(), 1);
        let hits = check_one("    unimplemented!();", false, false);
        assert_eq!(hits.len(), 1);
    }
}

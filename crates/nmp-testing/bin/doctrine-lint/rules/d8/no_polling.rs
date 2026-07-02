//! D8 — no polling (sleep+check loops banned).
//!
//! Separate from the hot-path-allocation check ([`super::hot_path_allocation`]),
//! D8's reactivity contract forbids *polling*: `sleep+check` loops are
//! banned at every layer (see `AGENTS.md` §reactivity-contract and the
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

/// D8 — no polling. Flags `thread::sleep(`, `tokio::time::sleep(`, and
/// `tokio::time::sleep_until(` calls in production code.
///
/// Unlike [`super::hot_path_allocation::check_in_scope`] this is **not**
/// path-scoped — it applies to every non-test file under
/// `crates/nmp-core/src/`. `is_comment` skips comment lines; `in_test_cfg`
/// skips lines inside an inline `#[cfg(test)]` module (test timing helpers
/// legitimately sleep). The driver additionally skips whole test-only
/// files by name before calling this. The `// doctrine-allow: D8` escape
/// hatch is honoured by the driver, as for every other rule.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

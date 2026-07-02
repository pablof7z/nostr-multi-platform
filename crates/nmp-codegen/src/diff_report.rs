//! Shared `--check` diff-line reporting for codegen gates.
//!
//! Generated-file checks report the first differing line of a stale file. They distinguish
//! three states for the caller / CI reporting code: up-to-date (`None` line,
//! `up_to_date = true`), file-missing (`None` line, `up_to_date = false`), and
//! stale-but-present (`Some(line)`). A naive `lines().zip()` walk collapses the
//! last state into the second whenever the only difference is a length mismatch
//! (one file is a strict prefix of the other) — it returns `None`, which the CI
//! gate misreports as "file missing". This helper closes that gap.

/// First 1-based line where `actual` and `rendered` differ.
///
/// Callers invoke this only after establishing the two strings differ. When
/// every common line matches, the difference is purely a length mismatch, so
/// we return the first line past the shorter side's end — never `None`, which
/// callers reserve for "file does not exist".
pub(crate) fn first_diff_or_length(actual: &str, rendered: &str) -> Option<usize> {
    actual
        .lines()
        .zip(rendered.lines())
        .position(|(a, b)| a != b)
        .map(|p| p + 1)
        .or_else(|| {
            let a = actual.lines().count();
            let r = rendered.lines().count();
            (a != r).then(|| a.min(r) + 1)
        })
}

#[cfg(test)]
mod tests {
    use super::first_diff_or_length;

    #[test]
    fn reports_mismatched_line() {
        assert_eq!(first_diff_or_length("a\nb\nc\n", "a\nX\nc\n"), Some(2));
    }

    #[test]
    fn length_mismatch_actual_shorter() {
        // actual is a strict prefix of rendered — no mismatched common line.
        assert_eq!(first_diff_or_length("a\nb\n", "a\nb\nc\n"), Some(3));
    }

    #[test]
    fn length_mismatch_actual_longer() {
        assert_eq!(first_diff_or_length("a\nb\nc\n", "a\nb\n"), Some(3));
    }
}

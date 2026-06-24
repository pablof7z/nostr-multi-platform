//! D9 — kernel-owned time.
//!
//! Reducer, replay, and kernel-policy paths must not read wall-clock time
//! directly. They thread time through the kernel's injected `Clock`
//! (`Kernel::now_secs` / `Kernel::now_ms`) or accept a caller-supplied
//! timestamp/instant that already came from that seam. This keeps replay and
//! fixed-clock tests deterministic.
//!
//! ## What this catches
//!
//! - `SystemTime::now()` in D9-scoped paths.
//! - `Instant::now()` in D9-scoped paths. Diagnostic/perf timing needs an
//!   exact-line reasoned allow, not a heuristic bypass.
//! - `now_epoch_ms()` helper use in those same paths.
//!
//! ## Scope
//!
//! `crates/nmp-core/src/kernel/**` and `crates/nmp-core/src/kernel_reducer.rs`.
//! The injected clock implementation itself (`kernel/clock.rs`) is exempt.
//! The main driver excludes test-only files and `#[cfg(test)]` bodies.
//!
//! ## Exemptions
//!
//! A production escape must be on the exact line and carry a reason:
//! `// doctrine-allow: D9 — reason`.

use std::path::Path;

pub const ID: &str = "D9";

const SYSTEM_TIME_TOKENS: &[&str] = &[
    "std::time::SystemTime::now",
    "crate::time::SystemTime::now",
    "SystemTime::now",
];

const INSTANT_TOKENS: &[&str] = &[
    "std::time::Instant::now",
    "crate::time::Instant::now",
    "Instant::now",
];

pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.contains("/bin/doctrine-lint/") {
        return false;
    }
    if s.ends_with("/crates/nmp-core/src/kernel/clock.rs")
        || s.ends_with("crates/nmp-core/src/kernel/clock.rs")
    {
        return false;
    }
    s.ends_with("/crates/nmp-core/src/kernel_reducer.rs")
        || s.ends_with("crates/nmp-core/src/kernel_reducer.rs")
        || s.contains("/crates/nmp-core/src/kernel/")
        || s.contains("crates/nmp-core/src/kernel/")
}

pub fn check(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }

    let mut hits = Vec::new();
    push_system_time_hits(line, &mut hits);
    push_instant_hits(line, &mut hits);
    push_helper_hits(line, &mut hits);
    hits.sort_by_key(|hit| hit.0);
    hits.dedup_by_key(|hit| hit.0);
    hits
}

fn push_system_time_hits(line: &str, hits: &mut Vec<(usize, String, String)>) {
    push_tokens(line, SYSTEM_TIME_TOKENS, hits, |token| {
        (
            format!(
                "`{}` violates D9: reducer/replay/kernel policy paths must not \
                 read wall-clock time directly",
                token
            ),
            "thread time from the kernel's injected `Clock` via `now_ms()` / \
             `now_secs()`, or accept a caller-supplied timestamp sourced from \
             that seam"
                .to_string(),
        )
    });
}

fn push_instant_hits(line: &str, hits: &mut Vec<(usize, String, String)>) {
    push_tokens(line, INSTANT_TOKENS, hits, |token| {
        (
            format!(
                "`{}` violates D9 in reducer/replay/kernel policy paths: \
                 kernel time must be injected for replayable decisions",
                token
            ),
            "pass the relevant instant/timestamp in from the actor/kernel clock \
             seam instead of reading time inside reducer or kernel-policy code"
                .to_string(),
        )
    });
}

fn push_helper_hits(line: &str, hits: &mut Vec<(usize, String, String)>) {
    let needle = "now_epoch_ms(";
    if line.trim_start().contains("fn now_epoch_ms(") {
        return;
    }
    let mut start = 0;
    while let Some(rel) = line[start..].find(needle) {
        let abs = start + rel;
        hits.push((
            abs + 1,
            "`now_epoch_ms()` violates D9 in reducer/replay/kernel policy paths: \
             it hides a raw wall-clock read behind a helper"
                .to_string(),
            "use `self.now_ms()` / `self.now_secs()` or pass an injected timestamp \
             into this path"
                .to_string(),
        ));
        start = abs + needle.len();
    }
}

fn push_tokens<F>(line: &str, tokens: &[&str], hits: &mut Vec<(usize, String, String)>, message: F)
where
    F: Fn(&str) -> (String, String),
{
    for token in tokens {
        let mut start = 0;
        while let Some(rel) = line[start..].find(token) {
            let abs = start + rel;
            if token.starts_with("SystemTime") || token.starts_with("Instant") {
                if abs > 0
                    && matches!(line.as_bytes()[abs - 1], b':' | b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9')
                {
                    start = abs + token.len();
                    continue;
                }
            }
            let (msg, suggested) = message(token);
            hits.push((abs + 1, msg, suggested));
            start = abs + token.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_system_time_now() {
        let hits = check("let now = SystemTime::now();", false, false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("D9"));
        assert!(hits[0].2.contains("now_ms"));
    }

    #[test]
    fn flags_deadline_instant_now() {
        let hits = check(
            "self.contacts_deadline = Some(Instant::now() + ttl);",
            false,
            false,
        );
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("Instant::now"));
    }

    #[test]
    fn flags_bare_local_instant_now() {
        let hits = check("let started = Instant::now();", false, false);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn flags_multiline_argument_instant_now() {
        let hits = check("            Instant::now(),", false, false);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn flags_epoch_helper_call() {
        let hits = check("let now_ms = now_epoch_ms();", false, false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("now_epoch_ms"));
    }

    #[test]
    fn ignores_comments_and_test_cfg() {
        assert!(check("// SystemTime::now()", true, false).is_empty());
        assert!(check("SystemTime::now()", false, true).is_empty());
    }

    #[test]
    fn scope_is_kernel_policy_only() {
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/kernel_reducer.rs"
        )));
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/routing_trace.rs"
        )));
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/clock.rs"
        )));
        assert!(!file_in_scope(Path::new(
            "crates/nmp-testing/bin/doctrine-lint/rules/d9.rs"
        )));
        assert!(!file_in_scope(Path::new("crates/nmp-nip17/src/lib.rs")));
    }
}

//! D24 — single post-store observer fan-out seam (event-flow lock).
//!
//! After an event is accepted and persisted, the kernel notifies app-facing
//! event observers from ONE place: the shared `project_accepted_event`
//! (`crates/nmp-core/src/kernel/ingest/projection.rs`), which calls
//! `notify_event_observers` exactly once per accepted event. The cache-serve
//! replay seam (`crates/nmp-core/src/kernel/cache_serve/`) reaches observers
//! through that same `project_accepted_event` door, so store-first and
//! network-first events fan out identically (ADR-0070 single-mechanism
//! cache-serve).
//!
//! A NEW scattered `notify_event_observers` call — a feature path that decides
//! on its own to re-notify observers — would re-fragment the fan-out: observers
//! would fire from two unsynchronized sites, breaking the "every accepted event
//! reaches the host exactly once, through the post-store seam" invariant. D24
//! makes that permanent: only the fan-out seam, its definition, and the
//! cache-serve replay module may name the call.
//!
//! ## What this bans
//!
//! A `notify_event_observers(` token outside the allowlisted seam files. The
//! match is **left-boundary anchored**: the character before
//! `notify_event_observers` must not be an identifier character, so a longer
//! identifier ending in that name (e.g. a hypothetical
//! `force_notify_event_observers(`) does NOT false-positive. (The `un`-prefixed
//! `unregister_raw_event_observer` substring class the prior reviews flagged is
//! a different token and never matches this needle.)
//! Because the method-name token + `(` is atomic on its own line, a
//! rustfmt-split chained call (`self\n    .notify_event_observers(&ev)`) is
//! caught with no extra state: the `.notify_event_observers(` line still
//! carries the whole token (the `.` before it is a non-identifier left bound).
//!
//! ## Scope (`file_in_scope`)
//!
//! `crates/nmp-core/src/` only — `notify_event_observers` is `pub(in
//! crate::kernel)`, so only kernel code can call it. The allowlisted seam
//! files (excluded from scope) are EXACTLY two:
//! - `kernel/ingest/projection.rs` — the `project_accepted_event` post-store fan-out,
//! - `kernel/event_observer.rs` — the `notify_event_observers` definition.
//!
//! The cache-serve replay seam (`kernel/cache_serve/`) is deliberately NOT
//! exempted: post-PR2/PR3 it routes observers through `project_accepted_event`
//! (`cache_serve/continuation.rs`) and has ZERO direct `notify_event_observers`
//! call. A blanket directory exemption would let a future direct notify regrow
//! there unnoticed, so cache-serve is held to the same one-door rule. (If a
//! cache-serve line ever legitimately needs the call, narrow the allow to that
//! exact line with `// doctrine-allow: D24 — reason`, not the whole dir.)
//!
//! ## Exemptions
//!
//! - The two allowlisted seam files above.
//! - Doc/line comments (`is_comment`) — skipped.
//! - `#[cfg(test)]` bodies (`in_test_cfg`) and test-only files (`d6_test_file`,
//!   handled in the driver) — e.g. `test_support.rs` fires the seam directly to
//!   stage observer state.
//! - Per-line `// doctrine-allow: D24 — reason` opt-out, REASON-REQUIRED (the
//!   D10/D21 tightened idiom; a bare `// doctrine-allow: D24` does not silence).
//! - The doctrine-lint binary's own source tree (string constants contain the
//!   banned token — meta-false-positives on broad sweeps).
//!
//! ## Heuristic scope (regression backstop, NOT a formal proof)
//!
//! D24 is a formatting-heuristic regression BACKSTOP, not a soundness proof. Via
//! the shared [`crate::rules::split_call`] matcher it catches the normal,
//! whitespace-before-paren, trailing-comment, and rustfmt method/paren-split
//! (`…notify_event_observers` / `(`) forms of the call. A deliberately-
//! obfuscated invocation — built through a macro or aliased through a re-export
//! — is OUT OF SCOPE and is a code-review concern, not something a line-based
//! lint chases.

use std::path::Path;

pub const ID: &str = "D24";

/// Seam files where `notify_event_observers` is legal: the post-store fan-out
/// (`project_accepted_event`) and the call's own definition. Matched as path
/// suffixes / fragments. The cache-serve dir is intentionally NOT here.
const SEAM_FILES: &[&str] = &[
    "crates/nmp-core/src/kernel/ingest/projection.rs",
    "crates/nmp-core/src/kernel/event_observer.rs",
];

/// True iff D24 should scan `path`: it lives inside `crates/nmp-core/src/`, is
/// not one of the two allowlisted seam files, and is not the doctrine-lint
/// binary itself.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.contains("/bin/doctrine-lint/") {
        return false;
    }
    if SEAM_FILES.iter().any(|f| s.ends_with(f) || s == *f) {
        return false;
    }
    s.contains("crates/nmp-core/src/")
}

/// The banned call name. The matcher ([`crate::rules::split_call`]) appends the
/// `(` tolerance, so this is just the method identifier.
const CALL_NAME: &str = "notify_event_observers";

/// Cross-line tracker (method/paren split detection) — re-export of the shared
/// [`crate::rules::split_call::State`].
pub type State = crate::rules::split_call::State;

fn message() -> String {
    "notify_event_observers call outside the post-store fan-out seam violates \
     D24 (event-flow lock). The kernel fans out to app-facing observers from \
     ONE place — `project_accepted_event` (kernel/ingest/projection.rs) — and every \
     other path (incl. cache-serve replay) reaches observers through that same \
     door (ADR-0070 single-mechanism cache-serve). A scattered observer notify \
     fires the host twice and breaks the once-per-accepted-event invariant"
        .to_string()
}

fn suggested() -> String {
    "let the event reach observers through `project_accepted_event` (the single \
     post-store fan-out) — do not call `notify_event_observers` from a feature \
     path or the cache-serve replay seam directly"
        .to_string()
}

/// Returns `(col, message, suggested)` for each banned `notify_event_observers`
/// call on `line` — contiguous, whitespace-before-paren, trailing-comment, and
/// rustfmt method/paren-split forms (via the shared matcher). `state` carries
/// the cross-line tracker; `is_comment` / `in_test_cfg` suppress the scan.
pub fn check(
    state: &mut State,
    line: &str,
    is_comment: bool,
    in_test_cfg: bool,
) -> Vec<(usize, String, String)> {
    crate::rules::split_call::columns(state, CALL_NAME, line, is_comment, in_test_cfg)
        .into_iter()
        .map(|col| (col, message(), suggested()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(line: &str) -> Vec<(usize, String, String)> {
        check(&mut State::default(), line, false, false)
    }

    fn run(lines: &[&str]) -> usize {
        let mut s = State::default();
        let mut n = 0;
        for l in lines {
            n += check(&mut s, l, false, false).len();
        }
        n
    }

    #[test]
    fn flags_scattered_notify() {
        let hits = one("        self.notify_event_observers(&ev);");
        assert_eq!(hits.len(), 1, "scattered notify must fire");
        assert!(hits[0].1.contains("D24"));
        assert!(hits[0].1.contains("project_accepted_event"));
    }

    #[test]
    fn flags_bareword_notify() {
        let hits = one("    notify_event_observers(e);");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 5, "column 1-indexed at the token");
    }

    #[test]
    fn flags_split_chained_call() {
        // rustfmt-split chained call: receiver on the previous line, so this
        // line is just `.notify_event_observers(&ev)` — token + `(` atomic.
        let hits = one("            .notify_event_observers(&ev);");
        assert_eq!(hits.len(), 1, "split chained notify must fire");
    }

    #[test]
    fn flags_method_paren_split() {
        // The residual evasion: method NAME on one line, `(` on the next.
        let n = run(&["            .notify_event_observers", "            (&ev);"]);
        assert_eq!(n, 1, "method/paren split must fire");
    }

    #[test]
    fn flags_trailing_comment_then_split_paren() {
        let n = run(&[
            "        self.notify_event_observers // fan out",
            "            (&ev);",
        ]);
        assert_eq!(n, 1, "trailing comment + split paren must fire");
    }

    #[test]
    fn does_not_flag_longer_identifier() {
        // A longer identifier ending in the token must not false-positive.
        assert!(one("    force_notify_event_observers(e);").is_empty());
    }

    #[test]
    fn does_not_flag_unregister_observer_class() {
        // The substring-false-positive class prior reviews flagged: a different
        // observer token must never match this needle.
        assert!(one("    self.unregister_raw_event_observer(id);").is_empty());
    }

    #[test]
    fn does_not_flag_comment() {
        let hits = check(
            &mut State::default(),
            "// calls notify_event_observers(&ev) once",
            true,
            false,
        );
        assert!(hits.is_empty(), "comment lines must not fire");
    }

    #[test]
    fn does_not_flag_in_test_cfg() {
        let hits = check(
            &mut State::default(),
            "self.notify_event_observers(&ev);",
            false,
            true,
        );
        assert!(hits.is_empty(), "#[cfg(test)] bodies must not fire");
    }

    #[test]
    fn seam_files_out_of_scope() {
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/ingest/projection.rs"
        )));
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/event_observer.rs"
        )));
    }

    #[test]
    fn cache_serve_is_in_scope() {
        // cache-serve is NOT exempted — it routes through project_accepted_event
        // and must stay free of direct notify calls (the blanket exemption was
        // removed so a future direct notify there is caught).
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/cache_serve/queries.rs"
        )));
        assert!(file_in_scope(Path::new(
            "/abs/crates/nmp-core/src/kernel/cache_serve/mod.rs"
        )));
    }

    #[test]
    fn other_nmp_core_files_in_scope() {
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/actor/commands/dm.rs"
        )));
    }

    #[test]
    fn non_nmp_core_out_of_scope() {
        assert!(!file_in_scope(Path::new("crates/nmp-nip17/src/inbox.rs")));
    }

    #[test]
    fn doctrine_lint_source_out_of_scope() {
        assert!(!file_in_scope(Path::new(
            "crates/nmp-testing/bin/doctrine-lint/rules/d24.rs"
        )));
    }
}

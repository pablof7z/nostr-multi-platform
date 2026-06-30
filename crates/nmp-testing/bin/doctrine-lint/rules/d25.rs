//! D25 — single REQ-build door / acquisition one-door (Workstream B4).
//!
//! Workstream B collapsed event acquisition to ONE door: every subscription is
//! a `LogicalInterest` compiled by the planner-owned subscription compiler /
//! `SubscriptionLifecycle`, which builds the actual relay `REQ` frame via the
//! kernel's `req_for_relay` (`crates/nmp-core/src/kernel/requests/`). Claim and
//! reverify no longer hand-build REQs — they register interests and let the
//! compiler emit the wire frame.
//!
//! A NEW direct `req_for_relay` call from a feature/request helper would
//! re-open the bypass door: a second REQ-build site outside the compiler means
//! acquisition is no longer planner-owned, and the LogicalInterest accounting
//! (dedup, watermark, lifecycle) is skipped. Master currently has ZERO such
//! call sites; D25 keeps it that way.
//!
//! ## What this bans
//!
//! A `req_for_relay(` token outside the compiler / lifecycle / replay files.
//! The match is **left-boundary anchored**: the character before
//! `req_for_relay` must not be an identifier character, so a longer identifier
//! ending in that name does NOT false-positive.
//!
//! ## Scope (`file_in_scope`)
//!
//! `crates/nmp-core/src/` only — `req_for_relay` is a kernel method (`pub(crate)`),
//! so only kernel code can reach it. The allowlisted REQ-build owners (excluded
//! from scope) are:
//! - `kernel/requests/**` — where `req_for_relay` is defined (the planner-owned
//!   REQ builder), and
//! - `kernel/replay.rs` — the `SubscriptionLifecycle` wire-frame replay
//!   re-emission, which re-issues the lifecycle's own REQ frames.
//!
//! ## Exemptions
//!
//! - The two allowlisted owner paths above.
//! - Doc/line comments (`is_comment`) — skipped.
//! - `#[cfg(test)]` bodies (`in_test_cfg`) and test-only files (`d6_test_file`,
//!   handled in the driver) — kernel unit tests (`*_tests.rs`, inline
//!   `#[cfg(test)]` blocks in `actor/relay_idle.rs`) drive `req_for_relay`
//!   directly to assemble fixtures.
//! - Per-line `// doctrine-allow: D25 — reason` opt-out, REASON-REQUIRED (the
//!   D10/D21 tightened idiom; a bare `// doctrine-allow: D25` does not silence).
//! - The doctrine-lint binary's own source tree (string constants contain the
//!   banned token — meta-false-positives on broad sweeps).
//!
//! ## Heuristic scope (regression backstop, NOT a formal proof)
//!
//! D25 is a formatting-heuristic regression BACKSTOP, not a soundness proof. Via
//! the shared [`crate::rules::split_call`] matcher it catches the normal,
//! whitespace-before-paren, trailing-comment, and rustfmt method/paren-split
//! (`…req_for_relay` / `(`) forms of the call. A deliberately-obfuscated
//! invocation — built through a macro or aliased through a re-export — is OUT OF
//! SCOPE and is a code-review concern, not something a line-based lint chases.

use std::path::Path;

pub const ID: &str = "D25";

/// Path fragment for the planner-owned REQ-builder module (`req_for_relay` is
/// defined here). Every file under it is a legal REQ-build owner.
const REQUESTS_DIR: &str = "crates/nmp-core/src/kernel/requests/";

/// The lifecycle wire-frame replay re-emission file — re-issues the
/// `SubscriptionLifecycle`'s own REQ frames, so it legitimately calls
/// `req_for_relay`.
const REPLAY_FILE: &str = "crates/nmp-core/src/kernel/replay.rs";

/// True iff D25 should scan `path`: it lives inside `crates/nmp-core/src/`, is
/// not one of the allowlisted REQ-build owners, and is not the doctrine-lint
/// binary itself.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.contains("/bin/doctrine-lint/") {
        return false;
    }
    if s.contains(REQUESTS_DIR) {
        return false;
    }
    if s.ends_with(REPLAY_FILE) || s == REPLAY_FILE {
        return false;
    }
    s.contains("crates/nmp-core/src/")
}

/// The banned call name. The matcher ([`crate::rules::split_call`]) appends the
/// `(` tolerance, so this is just the method identifier.
const CALL_NAME: &str = "req_for_relay";

/// Cross-line tracker (method/paren split detection) — re-export of the shared
/// [`crate::rules::split_call::State`].
pub type State = crate::rules::split_call::State;

fn message() -> String {
    "req_for_relay call outside the subscription compiler / lifecycle violates \
     D25 (acquisition one-door, Workstream B4). Event acquisition has ONE door: \
     register a `LogicalInterest` and let the planner-owned subscription \
     compiler / `SubscriptionLifecycle` build the relay REQ (kernel/requests/). \
     A direct REQ build from a feature helper bypasses LogicalInterest dedup, \
     watermark, and lifecycle accounting"
        .to_string()
}

fn suggested() -> String {
    "register a `LogicalInterest` (claim/reverify are LogicalInterests, not \
     direct REQ calls) and let the subscription compiler emit the wire frame; \
     the REQ builder lives in `kernel/requests/`"
        .to_string()
}

/// Returns `(col, message, suggested)` for each banned `req_for_relay` call on
/// `line` — contiguous, whitespace-before-paren, trailing-comment, and rustfmt
/// method/paren-split forms (via the shared matcher). `state` carries the
/// cross-line tracker; `is_comment` / `in_test_cfg` suppress the scan.
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
    fn flags_direct_req_for_relay() {
        let hits = one("        let r = kernel.req_for_relay(role, url, id);");
        assert_eq!(hits.len(), 1, "direct req_for_relay must fire");
        assert!(hits[0].1.contains("D25"));
        assert!(hits[0].1.contains("LogicalInterest"));
    }

    #[test]
    fn flags_bareword_req_for_relay() {
        let hits = one("    req_for_relay(a, b, c);");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 5, "column 1-indexed at the token");
    }

    #[test]
    fn flags_split_chained_call() {
        // rustfmt-split chained call: receiver on the previous line, so this
        // line is just `.req_for_relay(...)` — token + `(` atomic.
        let hits = one("            .req_for_relay(role, url, id);");
        assert_eq!(hits.len(), 1, "split chained req_for_relay must fire");
    }

    #[test]
    fn flags_method_paren_split() {
        // The residual evasion: method NAME on one line, `(` on the next.
        let n = run(&["            .req_for_relay", "            (role, url, id);"]);
        assert_eq!(n, 1, "method/paren split must fire");
    }

    #[test]
    fn flags_trailing_comment_then_split_paren() {
        let n = run(&[
            "        kernel.req_for_relay // build req",
            "            (role, url);",
        ]);
        assert_eq!(n, 1, "trailing comment + split paren must fire");
    }

    #[test]
    fn does_not_flag_longer_identifier() {
        assert!(one("    build_req_for_relay(role);").is_empty());
    }

    #[test]
    fn does_not_flag_comment() {
        let hits = check(
            &mut State::default(),
            "// calls req_for_relay() only in the compiler",
            true,
            false,
        );
        assert!(hits.is_empty(), "comment lines must not fire");
    }

    #[test]
    fn does_not_flag_in_test_cfg() {
        let hits = check(
            &mut State::default(),
            "kernel.req_for_relay(role, url, id);",
            false,
            true,
        );
        assert!(hits.is_empty(), "#[cfg(test)] bodies must not fire");
    }

    #[test]
    fn requests_dir_out_of_scope() {
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/requests/mod.rs"
        )));
        assert!(!file_in_scope(Path::new(
            "/abs/crates/nmp-core/src/kernel/requests/build.rs"
        )));
    }

    #[test]
    fn replay_file_out_of_scope() {
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/replay.rs"
        )));
    }

    #[test]
    fn other_nmp_core_files_in_scope() {
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/actor/commands/feed.rs"
        )));
    }

    #[test]
    fn non_nmp_core_out_of_scope() {
        assert!(!file_in_scope(Path::new("crates/nmp-planner/src/lib.rs")));
    }

    #[test]
    fn doctrine_lint_source_out_of_scope() {
        assert!(!file_in_scope(Path::new(
            "crates/nmp-testing/bin/doctrine-lint/rules/d25.rs"
        )));
    }
}

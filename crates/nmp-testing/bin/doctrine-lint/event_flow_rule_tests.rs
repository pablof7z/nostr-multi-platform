//! Smoke tests for the event-flow spine doctrine gates — D23 (single
//! accepted-event store-insert chokepoint), D24 (single post-store observer
//! fan-out seam), and D25 (single REQ-build door / acquisition one-door).
//! These lints make the just-landed event-flow architecture permanent: they
//! prevent a second ingest ladder, a scattered observer-notify, or a direct
//! REQ-build from regrowing outside the unified spine.
//!
//! Split out of `tests.rs` (file-size hard cap); the shared
//! `run_lint`/`workspace_root`/`fixture_path` helpers live in the parent
//! integration-test module and are imported via `super`.

use super::{fixture_path, run_lint, workspace_root};

/// Stage `fixtures/<rule>/<which>.rs` in an isolated `target/<label>/` dir so
/// the sibling fixture (pos vs neg) cannot pollute the assertion, run the lint
/// with the matching `--<flag> <label>` extra-scope, and return its
/// `(exit_code, stdout, stderr)`.
fn run_isolated(rule: &str, which: &str, flag: &str, label: &str) -> (i32, String, String) {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join(label);
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let src = workspace.join(fixture_path(&format!("{}/{}.rs", rule, which)));
    std::fs::copy(&src, tmp.join(format!("{}.rs", which))).expect("copy fixture");
    let tmp_str = tmp.to_string_lossy().into_owned();
    run_lint(&["--path", &tmp_str, flag, label])
}

// ─── D23 (single accepted-event store-insert chokepoint) ──────────────────────

#[test]
fn d23_positive_fixture_fires() {
    let (code, stdout, stderr) =
        run_isolated("d23", "pos", "--d23-extra-scope", "doctrine_lint_d23_pos");
    assert_eq!(
        code, 1,
        "d23 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D23]"),
        "d23 positive must emit >=1 D23 finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("verify_and_persist"),
        "d23 finding must name the chokepoint; stdout:\n{}",
        stdout
    );
    // The fixture plants 5 store-insert sites: 3 contiguous (self / kernel /
    // bareword) + 1 rustfmt-SPLIT `.store` / `.insert(` chain + 1 with a
    // TRAILING COMMENT on the `.store` line. The last two prove the split-call
    // and trailing-comment evasion holes are closed.
    let n = stdout.matches("error[D23]").count();
    assert!(
        n >= 5,
        "d23 must flag all 5 planted store-insert sites incl. the split chain \
         and the trailing-comment shape; got {}; stdout:\n{}",
        n,
        stdout
    );
}

#[test]
fn d23_negative_fixture_clean() {
    let (code, stdout, stderr) =
        run_isolated("d23", "neg", "--d23-extra-scope", "doctrine_lint_d23_neg");
    assert_eq!(
        code, 0,
        "d23 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D23]"),
        "d23 negative must produce zero D23 findings; stdout:\n{}",
        stdout
    );
}

// ─── D24 (single post-store observer fan-out seam) ────────────────────────────

#[test]
fn d24_positive_fixture_fires() {
    let (code, stdout, stderr) =
        run_isolated("d24", "pos", "--d24-extra-scope", "doctrine_lint_d24_pos");
    assert_eq!(
        code, 1,
        "d24 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D24]"),
        "d24 positive must emit >=1 D24 finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("project_accepted_event"),
        "d24 finding must name the post-store fan-out seam; stdout:\n{}",
        stdout
    );
    // 2 single-line notify sites + 1 chained call + 1 method/paren split.
    let n = stdout.matches("error[D24]").count();
    assert!(
        n >= 4,
        "d24 must flag all 4 planted notify sites incl. the chained and \
         method/paren-split shapes; got {}; stdout:\n{}",
        n,
        stdout
    );
}

#[test]
fn d24_negative_fixture_clean() {
    let (code, stdout, stderr) =
        run_isolated("d24", "neg", "--d24-extra-scope", "doctrine_lint_d24_neg");
    assert_eq!(
        code, 0,
        "d24 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D24]"),
        "d24 negative must produce zero D24 findings; stdout:\n{}",
        stdout
    );
}

// ─── D25 (single REQ-build door / acquisition one-door) ───────────────────────

#[test]
fn d25_positive_fixture_fires() {
    let (code, stdout, stderr) =
        run_isolated("d25", "pos", "--d25-extra-scope", "doctrine_lint_d25_pos");
    assert_eq!(
        code, 1,
        "d25 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D25]"),
        "d25 positive must emit >=1 D25 finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("LogicalInterest"),
        "d25 finding must point at the LogicalInterest one-door; stdout:\n{}",
        stdout
    );
    // 2 single-line req_for_relay sites + 1 chained call + 1 method/paren split.
    let n = stdout.matches("error[D25]").count();
    assert!(
        n >= 4,
        "d25 must flag all 4 planted req_for_relay sites incl. the chained and \
         method/paren-split shapes; got {}; stdout:\n{}",
        n,
        stdout
    );
}

#[test]
fn d25_negative_fixture_clean() {
    let (code, stdout, stderr) =
        run_isolated("d25", "neg", "--d25-extra-scope", "doctrine_lint_d25_neg");
    assert_eq!(
        code, 0,
        "d25 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D25]"),
        "d25 negative must produce zero D25 findings; stdout:\n{}",
        stdout
    );
}

// ─── Integration guards: nmp-core must be clean on current master ─────────────

/// Run the real lint against `crates/nmp-core/src` and return the lines tagged
/// with `rule_tag` (e.g. `error[D23]`).
fn nmp_core_findings_for(rule_tag: &str) -> Vec<String> {
    let (_code, stdout, _stderr) = run_lint(&["--path", "crates/nmp-core/src"]);
    stdout
        .lines()
        .filter(|l| l.contains(rule_tag))
        .map(|l| l.to_string())
        .collect()
}

/// D23: the only legal `store.insert` site in `nmp-core` is the chokepoint
/// file (`kernel/ingest/mod.rs`, allowlisted out of scope). Every other
/// `nmp-core/src` file must be D23-clean. This is the production-facing teeth
/// of the event-flow PR1 lock.
#[test]
fn nmp_core_is_d23_clean() {
    let findings = nmp_core_findings_for("error[D23]");
    assert!(
        findings.is_empty(),
        "nmp-core must be D23-clean — route events through `verify_and_persist`, \
         not a second store-insert. D23 findings:\n{}",
        findings.join("\n")
    );
}

/// D24: the only legal `notify_event_observers` sites are the fan-out seam
/// (`project_accepted_event`), its definition, and the cache-serve replay seam
/// (all allowlisted out of scope). Every other `nmp-core/src` file must be
/// D24-clean.
#[test]
fn nmp_core_is_d24_clean() {
    let findings = nmp_core_findings_for("error[D24]");
    assert!(
        findings.is_empty(),
        "nmp-core must be D24-clean — fan out to observers only through the \
         post-store seam. D24 findings:\n{}",
        findings.join("\n")
    );
}

/// D25: the only legal `req_for_relay` sites are the planner-owned REQ builder
/// (`kernel/requests/`) and the lifecycle replay re-emission
/// (`kernel/replay.rs`), both allowlisted out of scope. Every other
/// `nmp-core/src` file must be D25-clean (master has zero direct-REQ helpers).
#[test]
fn nmp_core_is_d25_clean() {
    let findings = nmp_core_findings_for("error[D25]");
    assert!(
        findings.is_empty(),
        "nmp-core must be D25-clean — acquire events by registering a \
         LogicalInterest, not by building a direct REQ. D25 findings:\n{}",
        findings.join("\n")
    );
}

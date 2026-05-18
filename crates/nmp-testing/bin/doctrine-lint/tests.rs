//! Doctrine-lint smoke test — runs the binary against the per-rule fixture
//! directories and asserts:
//!   - positive fixtures produce ≥1 finding tagged with the expected rule id
//!   - negative fixtures produce zero findings
//!
//! Run via `cargo test -p nmp-testing --test doctrine_lint_smoke`. The
//! GitHub Action `.github/workflows/doctrine-lint.yml` runs both this test
//! AND the binary directly against `nmp-core`.

use std::path::PathBuf;
use std::process::Command;

const FIXTURE_ROOT: &str = "crates/nmp-testing/bin/doctrine-lint/fixtures";

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at the nmp-testing crate; the workspace
    // root is two levels up.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .expect("workspace root must exist two levels above CARGO_MANIFEST_DIR")
}

/// Returns (exit_code, stdout, stderr) for `cargo run --quiet -p nmp-testing
/// --bin doctrine-lint -- <args>` invoked from the workspace root.
fn run_lint(args: &[&str]) -> (i32, String, String) {
    let root = workspace_root();
    let output = Command::new(env!("CARGO"))
        .current_dir(&root)
        .args([
            "run",
            "--quiet",
            "-p",
            "nmp-testing",
            "--bin",
            "doctrine-lint",
            "--",
        ])
        .args(args)
        .output()
        .expect("cargo run must succeed in spawning");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn fixture_path(sub: &str) -> String {
    format!("{}/{}", FIXTURE_ROOT, sub)
}

// ─── D0 ─────────────────────────────────────────────────────────────────────

#[test]
fn d0_positive_fixture_fires() {
    // fixtures/d0/ contains both pos.rs (fires D0) and neg.rs (clean) —
    // the assertion looks for D0 findings, which guarantees pos.rs hit.
    let (code, stdout, stderr) = run_lint(&["--path", &fixture_path("d0")]);
    assert_eq!(
        code, 1,
        "d0 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D0]"),
        "d0 positive must emit a D0 finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("pos.rs"),
        "d0 finding must point at pos.rs; stdout:\n{}",
        stdout
    );
}

#[test]
fn d0_negative_fixture_clean() {
    // Point the lint at a temp dir containing only the neg fixture, to
    // avoid the pos fixture also under fixtures/d0/ polluting the result.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d0_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d0/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 0,
        "d0 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D0]"),
        "d0 negative must produce zero D0 findings; stdout:\n{}",
        stdout
    );
}

// ─── D6 ─────────────────────────────────────────────────────────────────────

#[test]
fn d6_positive_fixture_fires() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d6_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d6/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, _stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(code, 1, "d6 positive must exit 1; stdout:\n{}", stdout);
    assert!(
        stdout.contains("error[D6]"),
        "d6 positive must emit ≥1 D6 finding; stdout:\n{}",
        stdout
    );
}

#[test]
fn d6_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d6_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d6/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 0,
        "d6 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D6]"),
        "d6 negative must produce zero D6 findings; stdout:\n{}",
        stdout
    );
}

// ─── D7 ─────────────────────────────────────────────────────────────────────

#[test]
fn d7_positive_fixture_fires() {
    // The fixture lives under fixtures/d7/substrate/capability.rs — the path
    // ending matches the D7 in-scope check.
    let (code, stdout, _stderr) = run_lint(&["--path", &fixture_path("d7")]);
    assert_eq!(code, 1, "d7 positive must exit 1; stdout:\n{}", stdout);
    assert!(
        stdout.contains("error[D7]"),
        "d7 positive must emit a D7 finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("retry_authentication") || stdout.contains("select_relay"),
        "d7 finding must name the offending method; stdout:\n{}",
        stdout
    );
}

#[test]
fn d7_negative_fixture_clean() {
    let (code, stdout, stderr) = run_lint(&["--path", &fixture_path("d7_neg")]);
    assert_eq!(
        code, 0,
        "d7 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D7]"),
        "d7 negative must produce zero D7 findings; stdout:\n{}",
        stdout
    );
}

// ─── D8 ─────────────────────────────────────────────────────────────────────

#[test]
fn d8_positive_fixture_fires() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d8_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d8/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // D8 is path-scoped; the smoke test uses --d8-extra-scope to open the
    // gate on the temp dir.
    let (code, stdout, _stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d8-extra-scope",
        "doctrine_lint_d8_pos",
    ]);
    assert_eq!(code, 1, "d8 positive must exit 1; stdout:\n{}", stdout);
    assert!(
        stdout.contains("error[D8]"),
        "d8 positive must emit ≥1 D8 finding; stdout:\n{}",
        stdout
    );
}

#[test]
fn d8_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d8_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d8/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d8-extra-scope",
        "doctrine_lint_d8_neg",
    ]);
    assert_eq!(
        code, 0,
        "d8 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D8]"),
        "d8 negative must produce zero D8 findings; stdout:\n{}",
        stdout
    );
}

// ─── Authoritative end-to-end ───────────────────────────────────────────────

/// The current `nmp-core` tree MUST be lint-clean. If a real D0/D6/D7/D8
/// regression lands, this test fails — exactly the intent.
#[test]
fn nmp_core_is_doctrine_clean() {
    let (code, stdout, stderr) = run_lint(&["--crate", "nmp-core"]);
    assert_eq!(
        code, 0,
        "nmp-core must be doctrine-lint clean; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
}

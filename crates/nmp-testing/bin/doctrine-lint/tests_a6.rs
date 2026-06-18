//! Smoke tests for A6 (schema-less JSON snapshot-projection lane banned).
//! Split out of `tests.rs` to keep that file within the file-size hard cap;
//! the shared `run_lint`/`workspace_root`/`fixture_path` helpers live in the
//! parent integration-test module and are imported via `super`.

use super::{fixture_path, run_lint, workspace_root};

// ─── A6 (schema-less JSON snapshot-projection lane banned) ───────────────────

#[test]
fn a6_positive_fixture_fires() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_a6_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("a6/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // A6 is workspace-wide but self-gates via file_in_scope (crates/ + apps/ only).
    // The staged fixture under `target/` falls outside that scope, so
    // `--a6-extra-scope` opts it in (mirrors `--a5-extra-scope`).
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--a6-extra-scope",
        "doctrine_lint_a6_pos",
    ]);
    assert_eq!(
        code, 1,
        "a6 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[A6]"),
        "a6 positive must emit >=1 A6 finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("rule A6"),
        "a6 finding message must reference rule A6; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("register_typed_snapshot_projection"),
        "a6 suggestion must name register_typed_snapshot_projection; stdout:\n{}",
        stdout
    );
}

#[test]
fn a6_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_a6_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("a6/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--a6-extra-scope",
        "doctrine_lint_a6_neg",
    ]);
    assert_eq!(
        code, 0,
        "a6 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[A6]"),
        "a6 negative must produce zero A6 findings; stdout:\n{}",
        stdout
    );
}

/// BLOCKER 3 — the A6 scanner catches the banned C-ABI symbol in a `.h` header
/// file (the coverage gap that the `.rs`-only walker would have missed).
///
/// Stages `fixtures/a6/pos.h` under `target/` in a subdirectory whose name
/// matches `--a6-extra-scope`, then scans it. The fixture declares
/// `nmp_app_register_snapshot_projection` in a C function prototype — exactly
/// the reappearance the gap guard is designed to catch.
#[test]
fn a6_header_file_positive_fixture_fires() {
    let workspace = workspace_root();
    // Stage inside ios/ so `a6::file_in_scope` accepts it without extra flags,
    // but we also pass --a6-extra-scope so the staged-under-target path is
    // accepted regardless of the ios/ check.
    let tmp = workspace.join("target").join("doctrine_lint_a6_h_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("a6/pos.h"));
    std::fs::copy(&pos_src, tmp.join("pos.h")).expect("copy pos.h fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--a6-extra-scope",
        "doctrine_lint_a6_h_pos",
    ]);
    assert_eq!(
        code, 1,
        "a6 header positive must exit 1 (banned C-ABI symbol in .h); stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[A6]"),
        "a6 header positive must emit an A6 finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("pos.h"),
        "a6 finding must point at the .h file; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("rule A6"),
        "a6 finding message must reference rule A6; stdout:\n{}",
        stdout
    );
}

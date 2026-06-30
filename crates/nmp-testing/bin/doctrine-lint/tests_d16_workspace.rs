//! Smoke tests for D16 (snapshot-projection key prefix in `apps/chirp/`),
//! the `--workspace-d8` workspace-wide no-polling scan, and the authoritative
//! end-to-end clean-workspace assertions.
//!
//! Split out of `tests.rs` (file-size cap). Shared helpers imported from
//! parent module via `super`.

use super::{fixture_path, run_lint, workspace_root};
use std::path::PathBuf;

// ─── D16 (snapshot-projection key prefix — apps/chirp/) ─────────────────────

#[test]
fn d16_positive_fixture_fires() {
    // Stage pos.rs in isolation so neg.rs cannot pollute the assertion.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d16_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d16/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // D16 is path-scoped to `apps/chirp/` — the staged fixture under
    // `target/` falls outside that scope, so `--d16-extra-scope` opts it in.
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d16-extra-scope",
        "doctrine_lint_d16_pos",
    ]);
    assert_eq!(
        code, 1,
        "d16 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D16]"),
        "d16 positive must emit ≥1 D16 finding; stdout:\n{}",
        stdout
    );
    // Both banned bare-prefix literals in the fixture must surface so a
    // regression that silently swallows one cannot pass this test.
    for token in ["nip29.group_events", "nip17.dm_inbox"] {
        assert!(
            stdout.contains(token),
            "d16 positive must name `{}`; stdout:\n{}",
            token,
            stdout
        );
    }
}

#[test]
fn d16_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d16_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d16/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d16-extra-scope",
        "doctrine_lint_d16_neg",
    ]);
    assert_eq!(
        code, 0,
        "d16 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D16]"),
        "d16 negative must produce zero D16 findings; stdout:\n{}",
        stdout
    );
}

/// The live `apps/chirp/` tree MUST be D16-clean after the rename.
/// This test confirms no bare `nip17.` / `nip29.` projection keys remain.
#[test]
fn chirp_app_crate_is_d16_clean() {
    let (code, stdout, stderr) = run_lint(&["--path", "apps/chirp/crates/nmp-app-chirp/src"]);
    assert!(
        !stdout.contains("error[D16]"),
        "apps/chirp/crates/nmp-app-chirp/src must be D16 clean after rename; \
         stdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
    let _ = (code, stderr); // exit code may be non-zero if other rules fire; D16 is the load-bearing check
}

// ─── --workspace-d8 (workspace-wide no-polling scan) ─────────────────────────

/// Builds a throwaway `crates/<name>/src/<file>.rs` tree under `target/` and
/// returns the workspace-root path to hand to `--workspace-d8-root`.
fn build_fake_workspace(label: &str, files: &[(&str, &str, &str)]) -> PathBuf {
    let root = workspace_root().join("target").join(label);
    let _ = std::fs::remove_dir_all(&root);
    for (crate_name, file_name, body) in files {
        let src = root.join("crates").join(crate_name).join("src");
        std::fs::create_dir_all(&src).expect("create fake crate src dir");
        std::fs::write(src.join(file_name), body).expect("write fake source file");
    }
    root
}

#[test]
fn workspace_d8_flags_production_sleep_in_any_crate() {
    // A bare `thread::sleep` in production (non-test) code anywhere in the
    // workspace is a D8 violation — even in a crate that is NOT nmp-core.
    let root = build_fake_workspace(
        "doctrine_lint_ws_d8_pos",
        &[(
            "nmp-fake-crate",
            "poller.rs",
            "use std::thread;\nuse std::time::Duration;\n\
             pub fn busy_wait() {\n    thread::sleep(Duration::from_millis(10));\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--workspace-d8", "--workspace-d8-root", &root_str]);
    assert_eq!(
        code, 1,
        "workspace-d8 must exit 1 on a production sleep; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D8]") && stdout.contains("polling"),
        "must emit a D8 no-polling finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("poller.rs"),
        "finding must point at poller.rs; stdout:\n{}",
        stdout
    );
}

#[test]
fn workspace_d8_flags_production_tokio_sleep_in_any_crate() {
    // The async `tokio::time::sleep` is a poll just like `thread::sleep` —
    // a production (non-test) call anywhere in the workspace is a D8
    // violation, even in a crate that is NOT nmp-core.
    let root = build_fake_workspace(
        "doctrine_lint_ws_d8_tokio_pos",
        &[(
            "nmp-fake-crate",
            "async_poller.rs",
            "use std::time::Duration;\n\
             pub async fn busy_wait() {\n    \
             tokio::time::sleep(Duration::from_millis(10)).await;\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--workspace-d8", "--workspace-d8-root", &root_str]);
    assert_eq!(
        code, 1,
        "workspace-d8 must exit 1 on a production tokio::time::sleep; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D8]") && stdout.contains("polling"),
        "must emit a D8 no-polling finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("async_poller.rs"),
        "finding must point at async_poller.rs; stdout:\n{}",
        stdout
    );
}

#[test]
fn workspace_d8_flags_production_tokio_sleep_until_in_any_crate() {
    // `tokio::time::sleep_until` is the deadline-based async sleep — also a
    // poll, also a D8 violation in production code.
    let root = build_fake_workspace(
        "doctrine_lint_ws_d8_tokio_until_pos",
        &[(
            "nmp-fake-crate",
            "deadline_poller.rs",
            "pub async fn busy_wait(deadline: tokio::time::Instant) {\n    \
             tokio::time::sleep_until(deadline).await;\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--workspace-d8", "--workspace-d8-root", &root_str]);
    assert_eq!(
        code, 1,
        "workspace-d8 must exit 1 on a production tokio::time::sleep_until; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D8]") && stdout.contains("polling"),
        "must emit a D8 no-polling finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("deadline_poller.rs"),
        "finding must point at deadline_poller.rs; stdout:\n{}",
        stdout
    );
}

#[test]
fn workspace_d8_exempts_cfg_test_tokio_sleep() {
    // A `tokio::time::sleep` inside a `#[cfg(test)]` module is a legitimate
    // test timing helper — workspace-d8 must exempt the async form too.
    let root = build_fake_workspace(
        "doctrine_lint_ws_d8_tokio_test_exempt",
        &[(
            "nmp-fake-crate",
            "async_lib.rs",
            "pub fn prod() {}\n\n#[cfg(test)]\nmod tests {\n    use std::time::Duration;\n\
             \n    #[tokio::test]\n    async fn t() {\n        \
             tokio::time::sleep(Duration::from_millis(1)).await;\n    }\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--workspace-d8", "--workspace-d8-root", &root_str]);
    assert_eq!(
        code, 0,
        "workspace-d8 must exempt cfg(test) tokio sleeps; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
}

#[test]
fn workspace_d8_runs_only_d8_not_d0_d6_d7() {
    // The workspace scan must NOT flood D0/D6/D7 findings for legitimate
    // app-crate code. This fixture has an `.unwrap()` (a D6 violation in
    // nmp-core, but D6 is intentionally nmp-core-scoped) and no sleep —
    // workspace-d8 must report it clean.
    let root = build_fake_workspace(
        "doctrine_lint_ws_d8_only",
        &[(
            "nmp-app-crate",
            "logic.rs",
            "pub fn parse(s: &str) -> i32 {\n    s.parse().unwrap()\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--workspace-d8", "--workspace-d8-root", &root_str]);
    assert_eq!(
        code, 0,
        "workspace-d8 must not flag a D6 .unwrap(); stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D6]"),
        "workspace-d8 must not emit D6 findings; stdout:\n{}",
        stdout
    );
}

#[test]
fn workspace_d8_exempts_cfg_test_sleeps() {
    // A `thread::sleep` inside a `#[cfg(test)]` module is a legitimate test
    // timing helper — workspace-d8 must exempt it, same as the nmp-core scan.
    let root = build_fake_workspace(
        "doctrine_lint_ws_d8_test_exempt",
        &[(
            "nmp-tested-crate",
            "svc.rs",
            "pub fn run() {}\n\
             #[cfg(test)]\nmod tests {\n    use std::thread;\n    use std::time::Duration;\n\
             \n    #[test]\n    fn t() {\n        thread::sleep(Duration::from_millis(1));\n    }\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--workspace-d8", "--workspace-d8-root", &root_str]);
    assert_eq!(
        code, 0,
        "workspace-d8 must exempt cfg(test) sleeps; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
}

#[test]
fn workspace_d8_scans_nmp_testing_harness() {
    // nmp-testing bin/ (harnesses) are now under the D8 no-polling lint. An
    // un-annotated sleep in a fake `crates/nmp-testing/bin/ffi-stress/` file
    // must be flagged; the same sleep with a doctrine-allow annotation must not.
    let root = build_fake_workspace(
        "doctrine_lint_ws_d8_scan_testing",
        &[
            // Unannotated sleep in the harness bin — must be flagged.
            (
                "nmp-testing/bin/ffi-stress",
                "scenario.rs",
                "use std::thread;\nuse std::time::Duration;\n\
                 pub fn settle() {\n    thread::sleep(Duration::from_millis(5));\n}\n",
            ),
        ],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--workspace-d8", "--workspace-d8-root", &root_str]);
    assert_eq!(
        code, 1,
        "workspace-d8 must flag unannotated sleeps in nmp-testing harness bin/; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D8]"),
        "must emit a D8 finding; stdout:\n{}",
        stdout
    );

    // Same sleep with a doctrine-allow annotation must be clean.
    let root_ok = build_fake_workspace(
        "doctrine_lint_ws_d8_scan_testing_annotated",
        &[(
            "nmp-testing/bin/ffi-stress",
            "scenario.rs",
            "use std::thread;\nuse std::time::Duration;\n\
             pub fn settle() {\n    thread::sleep(Duration::from_millis(5)); // doctrine-allow: D8 — soak window\n}\n",
        )],
    );
    let root_ok_str = root_ok.to_string_lossy().into_owned();
    let (code_ok, stdout_ok, stderr_ok) =
        run_lint(&["--workspace-d8", "--workspace-d8-root", &root_ok_str]);
    assert_eq!(
        code_ok, 0,
        "workspace-d8 must not flag annotated sleeps in nmp-testing harness; stdout:\n{}\nstderr:\n{}",
        stdout_ok, stderr_ok
    );
}

#[test]
fn workspace_d8_rejects_combination_with_crate_flag() {
    // --workspace-d8 owns root resolution; combining it with --crate is a
    // usage error (exit 2).
    let (code, _stdout, stderr) = run_lint(&["--workspace-d8", "--crate", "nmp-core"]);
    assert_eq!(
        code, 2,
        "combining --workspace-d8 with --crate must be a usage error; stderr:\n{}",
        stderr
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

/// Every production crate in the real workspace MUST be free of
/// `thread::sleep` busy-waits. If a polling regression lands in any crate,
/// this test fails — the whole point of the `--workspace-d8` mode.
#[test]
fn workspace_is_d8_no_polling_clean() {
    let (code, stdout, stderr) = run_lint(&["--workspace-d8"]);
    assert_eq!(
        code, 0,
        "workspace must be D8 no-polling clean; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
}

//! Smoke tests for D13 (DM-path raw-key isolation, ADR-0026),
//! D14 (typed snapshot-projection slots), and D15 (host-closure
//! invocations must be panic-guarded).
//!
//! Split out of `tests.rs` (file-size cap). Shared helpers imported from
//! parent module via `super`.

use super::{fixture_path, run_lint, workspace_root};

// ─── D14 (typed snapshot-projection slot) ──────────────────────────────────

#[test]
fn d14_positive_fixture_fires() {
    // Stage pos.rs in isolation so neg.rs (also under fixtures/d14/) cannot
    // confuse the assertion — mirrors the d6/d8/d9 positive fixture pattern.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d14_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d14/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // D14 is path-scoped to `crates/nmp-core/src/` — the smoke fixture
    // staged under `target/` falls outside that scope, so
    // `--d14-extra-scope` opts it in (mirrors `--d8-extra-scope` /
    // `--d9-extra-scope`).
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d14-extra-scope",
        "doctrine_lint_d14_pos",
    ]);
    assert_eq!(
        code, 1,
        "d14 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D14]"),
        "d14 positive must emit >=1 D14 finding; stdout:\n{}",
        stdout
    );
    // The fixture defines four offending fields, one per in-scope struct
    // (`Kernel`, `NmpApp`, `ActorRuntime`, `Nip65OutboxResolver`). All four
    // must surface — a regression that silently swallows one fails this test.
    for token in [
        "indexer_relays",
        "pending_outbound",
        "queued_commands",
        "local_write_relays",
    ] {
        assert!(
            stdout.contains(token),
            "d14 positive must name `{}`; stdout:\n{}",
            token,
            stdout
        );
    }
    for struct_name in ["Kernel", "NmpApp", "ActorRuntime", "Nip65OutboxResolver"] {
        assert!(
            stdout.contains(struct_name),
            "d14 finding must name the enclosing struct `{}`; stdout:\n{}",
            struct_name,
            stdout
        );
    }
}

#[test]
fn d14_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d14_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d14/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d14-extra-scope",
        "doctrine_lint_d14_neg",
    ]);
    assert_eq!(
        code, 0,
        "d14 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D14]"),
        "d14 negative must produce zero D14 findings; stdout:\n{}",
        stdout
    );
}

#[test]
fn d14_skips_out_of_scope_crates() {
    // Even with a bare `Arc<Mutex<Vec<…>>>` field on a `Kernel`-named
    // struct, a file outside `crates/nmp-core/src/` (and outside the
    // explicit `--d14-extra-scope`) must NOT trigger — the rule is
    // substrate-scoped, not workspace-wide.
    let workspace = workspace_root();
    let tmp = workspace
        .join("target")
        .join("doctrine_lint_d14_out_of_scope");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    std::fs::write(
        tmp.join("out_of_scope.rs"),
        "use std::sync::{Arc, Mutex};\n\
         pub struct Kernel {\n    \
            indexer_relays: Arc<Mutex<Vec<String>>>,\n\
         }\n",
    )
    .expect("write fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // NOTE: no --d14-extra-scope here — the rule must self-gate itself
    // away because the path lacks the `crates/nmp-core/src/` fragment.
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 0,
        "d14 out-of-scope must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D14]"),
        "d14 must not fire on out-of-scope paths; stdout:\n{}",
        stdout
    );
}

// ─── D13 (DM-path raw-key isolation, ADR-0026) ───────────────────────────────

#[test]
fn d13_part_a_positive_fixture_fires() {
    // Stage pos.rs in isolation so neg.rs cannot pollute the assertion.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d13_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d13/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // D13 Part A is path-scoped to `dm.rs` by default + marker-driven —
    // the staged fixture under `target/` is opted in via `--d13-extra-scope`
    // (mirrors `--d9-extra-scope`).
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d13-extra-scope",
        "doctrine_lint_d13_pos",
    ]);
    assert_eq!(
        code, 1,
        "d13 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D13]"),
        "d13 positive must emit ≥1 D13 finding; stdout:\n{}",
        stdout
    );
    // Each banned shape in the fixture must surface so a regression that
    // silently swallows one cannot pass this test.
    for token in [
        "active_local_keys",
        ".secret_key()",
        "Keys::parse",
        "mls_local_nsec",
    ] {
        assert!(
            stdout.contains(token),
            "d13 positive must name banned token `{}`; stdout:\n{}",
            token,
            stdout
        );
    }
}

#[test]
fn d13_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d13_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d13/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d13-extra-scope",
        "doctrine_lint_d13_neg",
    ]);
    assert_eq!(
        code, 0,
        "d13 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D13]"),
        "d13 negative must produce zero D13 findings; stdout:\n{}",
        stdout
    );
}

#[test]
fn d13_part_b_positive_fixture_fires_outside_marmot() {
    // Part B is path-derived: any non-marmot, non-testing, non-actor,
    // non-ffi file that reads `mls_local_nsec` is a violation. Stage
    // the fixture under `target/` (outside the carve-outs) — but path
    // matching uses contains("/crates/") etc., so staging at
    // `target/doctrine_lint_d13_part_b_pos/` puts the file outside the
    // exemption set entirely, which is the in-scope condition.
    let workspace = workspace_root();
    let tmp = workspace
        .join("target")
        .join("doctrine_lint_d13_part_b_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d13/part_b_pos.rs"));
    std::fs::copy(&pos_src, tmp.join("part_b_pos.rs")).expect("copy fixture");

    // Stage it inside a fake `crates/some-app-crate/src/` tree so Part B's
    // scope check (`contains("/crates/")` + outside-marmot/testing/ffi/actor)
    // resolves to in-scope.
    let crate_src = tmp.join("crates").join("some-app-crate").join("src");
    std::fs::create_dir_all(&crate_src).expect("create fake crate src");
    std::fs::copy(&pos_src, crate_src.join("part_b_pos.rs")).expect("copy fixture into fake crate");

    let crate_src_str = crate_src.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &crate_src_str]);
    assert_eq!(
        code, 1,
        "d13 Part B positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D13]"),
        "d13 Part B positive must emit a D13 finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("mls_local_nsec"),
        "d13 Part B finding must name `mls_local_nsec`; stdout:\n{}",
        stdout
    );
}

// ─── D15 (host-closure invocations must be panic-guarded) ────────────────────

#[test]
fn d15_positive_fixture_fires() {
    // Stage pos.rs in isolation under `target/` so neg.rs (also under
    // fixtures/d15/) cannot pollute the assertion — mirrors the d6/d8/d9
    // positive fixture pattern.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d15_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d15/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d15-extra-scope",
        "doctrine_lint_d15_pos",
    ]);
    assert_eq!(
        code, 1,
        "d15 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D15]"),
        "d15 positive must emit ≥1 D15 finding; stdout:\n{}",
        stdout
    );
    // Each banned invocation shape in the fixture must surface so a
    // regression that silently swallows one shape cannot pass.
    for token in ["observer(", "(self.callback)(", "callback("] {
        assert!(
            stdout.contains(token),
            "d15 positive must name `{}`; stdout:\n{}",
            token,
            stdout
        );
    }
}

#[test]
fn d15_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d15_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d15/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d15-extra-scope",
        "doctrine_lint_d15_neg",
    ]);
    assert_eq!(
        code, 0,
        "d15 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D15]"),
        "d15 negative must produce zero D15 findings; stdout:\n{}",
        stdout
    );
}

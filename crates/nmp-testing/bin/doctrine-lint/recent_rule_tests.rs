//! Smoke tests for the two newest doctrine rules — D19 (display-formatting
//! banned from kernel projection builders) and D20 (no raw `std::time` on the
//! wasm-compiled path, #1173/#1161). Split out of `tests.rs` to keep that file
//! within the file-size hard cap; the shared
//! `run_lint`/`workspace_root`/`fixture_path` helpers live in the parent
//! integration-test module and are imported via `super`.

use super::{fixture_path, run_lint, workspace_root};

// ─── D19 (display formatting banned from kernel projection builders) ──────────

#[test]
fn d19_positive_fixture_fires() {
    // Stage pos.rs in isolation so neg.rs cannot pollute the assertion.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d19_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d19/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // D19 is path-scoped to kernel projection builder files — the staged
    // fixture under `target/` falls outside that scope, so
    // `--d19-extra-scope` opts it in (mirrors `--d17-extra-scope`).
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d19-extra-scope",
        "doctrine_lint_d19_pos",
    ]);
    assert_eq!(
        code, 1,
        "d19 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D19]"),
        "d19 positive must emit >=1 D19 finding; stdout:\n{}",
        stdout
    );
    // Both banned tokens in pos.rs must surface.
    assert!(
        stdout.contains("crate::display::"),
        "d19 finding must name crate::display::; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("ADR-0032"),
        "d19 finding message must reference ADR-0032; stdout:\n{}",
        stdout
    );
}

#[test]
fn d19_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d19_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d19/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d19-extra-scope",
        "doctrine_lint_d19_neg",
    ]);
    assert_eq!(
        code, 0,
        "d19 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D19]"),
        "d19 negative must produce zero D19 findings; stdout:\n{}",
        stdout
    );
}

// ─── D20 (no raw std::time on the wasm-compiled path) ─────────────────────────

#[test]
fn d20_positive_fixture_fires() {
    // Stage pos.rs in isolation so neg.rs cannot pollute the assertion.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d20_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d20/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // D20 is path-scoped to wasm-reachable crate `src/` trees — the staged
    // fixture under `target/` falls outside that scope, so `--d20-extra-scope`
    // opts it in (mirrors `--d19-extra-scope`).
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d20-extra-scope",
        "doctrine_lint_d20_pos",
    ]);
    assert_eq!(
        code, 1,
        "d20 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D20]"),
        "d20 positive must emit >=1 D20 finding; stdout:\n{}",
        stdout
    );
    // The grouped-import case (`use std::time::{Duration, Instant};`) and the
    // inline call sites must all surface — assert the shim is named in the fix.
    assert!(
        stdout.contains("crate::time"),
        "d20 finding must point at the crate::time shim; stdout:\n{}",
        stdout
    );
    // Both the grouped Instant import AND the grouped SystemTime import must
    // fire — count D20 findings to confirm the fixture's 4 banned sites surface.
    let d20_count = stdout.matches("error[D20]").count();
    assert!(
        d20_count >= 4,
        "d20 must flag all 4 banned sites in pos.rs (2 imports + 2 inline now() calls); \
         got {} findings; stdout:\n{}",
        d20_count,
        stdout
    );
}

#[test]
fn d20_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d20_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d20/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d20-extra-scope",
        "doctrine_lint_d20_neg",
    ]);
    assert_eq!(
        code, 0,
        "d20 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D20]"),
        "d20 negative must produce zero D20 findings; stdout:\n{}",
        stdout
    );
}

/// Integration guard (#1173, #1161): every wasm-reachable crate must be D20
/// clean. This runs the real lint against each crate's `src/` tree — if a
/// future change reintroduces a bare `std::time::Instant`/`SystemTime` on a
/// wasm-reachable path (outside the native-only actor/relay_worker/lmdb
/// subtrees, the two shims, tests, or an explicit `// doctrine-allow: D20`),
/// this test fails. It is the production-facing teeth of the rule, distinct
/// from the synthetic fixture tests above.
#[test]
fn wasm_reachable_crates_are_d20_clean() {
    const WASM_REACHABLE_CRATES: &[&str] = &[
        "nmp-core",
        "nmp-store",
        "nmp-network",
        "nmp-signers",
        "nmp-wasm",
        "nmp-planner",
        "nmp-chirp-config",
        "nmp-signer-iface",
    ];
    for c in WASM_REACHABLE_CRATES {
        let path = format!("crates/{}/src", c);
        let (code, stdout, stderr) = run_lint(&["--path", &path]);
        let d20_findings: Vec<&str> = stdout
            .lines()
            .filter(|l| l.contains("error[D20]"))
            .collect();
        assert!(
            d20_findings.is_empty(),
            "{} must be D20-clean (route std::time through the crate::time shim, \
             or gate native-only sites with // doctrine-allow: D20). \
             D20 findings:\n{}\nfull stdout:\n{}\nstderr:\n{}",
            c,
            d20_findings.join("\n"),
            stdout,
            stderr
        );
        // The lint may exit 1 because of OTHER unrelated rule findings in a
        // crate; we only assert D20-cleanliness here, not a clean exit code.
        let _ = code;
    }
}

//! Smoke tests for D0, D6, D7, D8 (hot-path allocation + no-polling), and D9
//! (protocol-crate action-namespace prefix).
//!
//! Split out of `tests.rs` (file-size cap). Shared helpers imported from
//! parent module via `super`.

use super::{fixture_path, run_lint, workspace_root};

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

// ─── D8 — no polling (thread::sleep / tokio::time::sleep) ────────────────────

#[test]
fn d8_sleep_positive_fixture_fires() {
    // The no-polling check is NOT path-scoped, so no --d8-extra-scope is
    // needed — pointing --path at the fixture dir is enough.
    let (code, stdout, stderr) = run_lint(&["--path", &fixture_path("d8_sleep")]);
    assert_eq!(
        code, 1,
        "d8 no-polling positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D8]"),
        "d8 no-polling positive must emit a D8 finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("pos.rs") && stdout.contains("polling"),
        "d8 no-polling finding must point at pos.rs and mention polling; stdout:\n{}",
        stdout
    );
    // The fixture exercises all four banned forms — assert each is named so
    // a regression that silently drops one token cannot pass this test.
    for token in [
        "thread::sleep",
        "tokio::time::sleep",
        "tokio::time::sleep_until",
    ] {
        assert!(
            stdout.contains(token),
            "d8 no-polling positive must flag `{}`; stdout:\n{}",
            token,
            stdout
        );
    }
}

#[test]
fn d8_sleep_negative_fixture_clean() {
    // Isolate neg.rs in a temp dir so the sibling pos.rs cannot pollute the
    // result. The neg fixture exercises the cfg(test) and doctrine-allow
    // exemptions — both must keep it finding-free.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d8_sleep_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d8_sleep/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 0,
        "d8 no-polling negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D8]"),
        "d8 no-polling negative must produce zero D8 findings; stdout:\n{}",
        stdout
    );
}

// ─── D9 (protocol-crate action-namespace prefix) ────────────────────────────

#[test]
fn d9_positive_fixture_fires() {
    // Stage pos.rs in isolation so neg.rs (also under fixtures/d9/) cannot
    // confuse the assertion — mirrors the d6/d8 positive fixture pattern.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d9_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d9/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // D9 is path-scoped to `crates/nmp-*/` — the smoke fixture staged under
    // `target/` falls outside that scope, so `--d9-extra-scope` opts it in
    // (mirrors `--d8-extra-scope`).
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d9-extra-scope",
        "doctrine_lint_d9_pos",
    ]);
    assert_eq!(
        code, 1,
        "d9 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D9]"),
        "d9 positive must emit ≥1 D9 finding; stdout:\n{}",
        stdout
    );
    // Both bad namespaces in the fixture must surface in the report so a
    // regression that silently swallows one cannot pass this test.
    for token in ["nip29.post_chat_message", "nip29.react_in_group"] {
        assert!(
            stdout.contains(token),
            "d9 positive must name `{}`; stdout:\n{}",
            token,
            stdout
        );
    }
}

#[test]
fn d9_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d9_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d9/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d9-extra-scope",
        "doctrine_lint_d9_neg",
    ]);
    assert_eq!(
        code, 0,
        "d9 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D9]"),
        "d9 negative must produce zero D9 findings; stdout:\n{}",
        stdout
    );
}

/// THE LIVE GUARD: every protocol crate on master must already satisfy
/// D9 — AND, because `scan_one_file` runs every applicable rule per file,
/// every other applicable rule too.
///
/// SCOPE — this is a full-doctrine scan (D0/D6/D7/D8/D9), not D9-only.
/// `scan_one_file` has no "only this rule" mode; opening the scan to
/// every protocol crate's `src/` means D6 (`.unwrap()` / `panic!` outside
/// `#[cfg(test)]`) and D8's no-polling check now apply to every
/// `nmp-nipNN` crate as well, not just `nmp-core`.
///
/// That is intentional, with one caveat: D6's `.unwrap()` and `panic!`
/// bans are universal correctness rules, NOT nmp-core-scoped doctrine.
/// The same goes for D8 no-polling. D0 doesn't fire here because
/// `d0::file_is_exempt` exempts every non-`nmp-core` crate under
/// `crates/nmp-*/src/...` (its mandate is the kernel substrate only;
/// fixing this exemption was part of this PR — see `rules/d0.rs`).
/// D7 is file-scoped to `nmp-core/src/substrate/capability.rs` and
/// likewise doesn't reach NIP crates.
///
/// Net: a future D6 regression in `nmp-nipNN` will fail THIS test even
/// though the test name promises D9 cleanliness. That is the right
/// trade-off — `.unwrap()` in production code is a bug everywhere,
/// not just in `nmp-core` — but reviewers should know the scope is
/// broader than the name suggests.
#[test]
fn protocol_crates_are_doctrine_clean() {
    // Scan every protocol crate. The default mode targets `nmp-core`; we
    // explicitly add the NIP crates by path so the workspace's whole
    // protocol surface is covered. (App crates under `apps/` are out of
    // scope by D9 design — `d9::file_in_scope` excludes them — and by D0
    // design — `d0::file_is_exempt` exempts them.)
    let nip_crates = [
        "nmp-nip01",
        "nmp-nip17",
        "nmp-nip23",
        "nmp-nip29",
        "nmp-nip42",
        "nmp-nip57",
        "nmp-nip59",
    ];
    let mut args: Vec<String> = vec!["--crate".into(), "nmp-core".into()];
    for c in &nip_crates {
        args.push("--path".into());
        args.push(format!("crates/{}/src", c));
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (code, stdout, stderr) = run_lint(&arg_refs);
    assert_eq!(
        code, 0,
        "protocol crates must be doctrine-lint clean; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    // Spell out D9 specifically — the rule this PR adds. A D6 / D8 hit
    // would already fail the `code == 0` check above; an explicit D9
    // assertion makes the intent obvious in the test name.
    assert!(
        !stdout.contains("error[D9]"),
        "protocol crates must not contain D9 findings; stdout:\n{}",
        stdout
    );
}

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
        "nmp-nip22",
        "nmp-nip23",
        "nmp-nip29",
        "nmp-nip42",
        "nmp-nip57",
        "nmp-nip59",
        "nmp-nip77",
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

// ─── D10 (provenance: gift-wrap publish never escapes to public relays) ────

#[test]
fn d10_positive_fixture_fires() {
    // Stage pos.rs in isolation so neg.rs (also under fixtures/d10/) cannot
    // pollute the assertion — mirrors the d6/d8/d9 positive fixture pattern.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d10_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d10/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // D10 is path-scoped to `crates/nmp-{core,nip17,marmot}/` — the staged
    // fixture under `target/` is opted in via `--d10-extra-scope`.
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d10-extra-scope",
        "doctrine_lint_d10_pos",
    ]);
    assert_eq!(
        code, 1,
        "d10 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D10]"),
        "d10 positive must emit a D10 finding; stdout:\n{}",
        stdout
    );
    // Every banned Auto-routing token in pos.rs must surface so a regression
    // that silently swallows one cannot pass this test.
    for token in [
        "PublishTarget::Auto",
        "publish_signed(",
        "publish_unsigned_event(",
    ] {
        assert!(
            stdout.contains(token),
            "d10 positive must name banned token `{}`; stdout:\n{}",
            token,
            stdout
        );
    }
}

#[test]
fn d10_negative_fixture_clean() {
    // Isolate neg.rs so the sibling pos.rs cannot pollute the assertion.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d10_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d10/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d10-extra-scope",
        "doctrine_lint_d10_neg",
    ]);
    assert_eq!(
        code, 0,
        "d10 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D10]"),
        "d10 negative must produce zero D10 findings; stdout:\n{}",
        stdout
    );
}

/// The current `nmp-core`, `nmp-nip17`, and `nmp-marmot` trees on master
/// MUST be D10-clean. Real protocol code touches kind:1059 today — this
/// test pins that none of those publishers regress to an Auto-routing
/// seam without explicit `doctrine-allow: D10` justification.
///
/// SCOPE — this asserts D10 cleanliness specifically. The driver
/// (`scan_one_file`) runs every applicable rule per file, so a run over
/// `nmp-marmot/src/` also surfaces unrelated rules (`nmp-marmot` is NOT
/// in `protocol_crates_are_doctrine_clean`'s scope and carries pre-existing
/// findings for rules other than D10 — out of scope for PR-K). The
/// exit-code assertion is therefore loose; only the **D10 substring** is
/// the load-bearing check.
#[test]
fn d10_scoped_crates_are_clean() {
    let scoped = ["nmp-core", "nmp-nip17", "nmp-marmot"];
    let mut args: Vec<String> = Vec::new();
    for c in &scoped {
        args.push("--path".into());
        args.push(format!("crates/{}/src", c));
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (_code, stdout, stderr) = run_lint(&arg_refs);
    assert!(
        !stdout.contains("error[D10]"),
        "scoped crates must not contain D10 findings; stdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
}

// ─── D11 (one door per publish capability) ──────────────────────────────────

#[test]
fn d11_positive_fixture_fires() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d11_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d11/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 1,
        "d11 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D11]"),
        "d11 positive must emit a D11 finding; stdout:\n{}",
        stdout
    );
    for token in [
        "ActorCommand::PublishSignedEvent",
        "ActorCommand::PublishUnsignedEvent",
    ] {
        assert!(
            stdout.contains(token),
            "d11 positive must name `{}`; stdout:\n{}",
            token,
            stdout
        );
    }
}

#[test]
fn d11_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d11_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d11/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 0,
        "d11 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D11]"),
        "d11 negative must produce zero D11 findings; stdout:\n{}",
        stdout
    );
}

// ─── D15 (host-closure invocations must be panic-guarded) ────────────────────

#[test]
fn d15_positive_fixture_fires() {
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
        "d15 positive must emit >=1 D15 finding; stdout:\n{}",
        stdout
    );
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
fn workspace_d8_skips_nmp_testing_crate() {
    // nmp-testing is test infrastructure — its harnesses/benches legitimately
    // sleep. A production-shaped sleep there must NOT be flagged.
    let root = build_fake_workspace(
        "doctrine_lint_ws_d8_skip_testing",
        &[(
            "nmp-testing",
            "harness.rs",
            "use std::thread;\nuse std::time::Duration;\n\
             pub fn settle() {\n    thread::sleep(Duration::from_millis(5));\n}\n",
        )],
    );
    let root_str = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--workspace-d8", "--workspace-d8-root", &root_str]);
    assert_eq!(
        code, 0,
        "workspace-d8 must skip the nmp-testing crate; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
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

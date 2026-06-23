//! Smoke tests for D10 (gift-wrap publish never escapes to public relays),
//! D11 (one door per publish capability), and D12 (async-completing modules
//! must record stages).
//!
//! Split out of `tests.rs` (file-size cap). Shared helpers imported from
//! parent module via `super`.

use super::{fixture_path, run_lint, workspace_root};

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
    // that silently swallows one cannot pass this test. PR-K3 added a new
    // positive fixture exercising the `publish_signed_event(` token inside
    // a marked dispatcher (see `pos.rs::dispatch_kind1059_via_empty_relays`).
    for token in [
        "PublishTarget::Auto",
        "publish_signed(",
        "publish_unsigned_event(",
        "publish_signed_event(",
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

// ─── D12 (async-completing modules must record stages) ─────────────────────

#[test]
fn d12_positive_fixture_fires() {
    // Stage `pos.rs` in isolation so `neg.rs` does not confuse the assertion.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d12_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d12/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // D12 is path-scoped to protocol/app crates — the smoke fixture staged
    // under `target/` falls outside that scope, so `--d12-extra-scope` opts
    // it in (mirrors `--d9-extra-scope`).
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d12-extra-scope",
        "doctrine_lint_d12_pos",
    ]);
    assert_eq!(
        code, 1,
        "d12 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D12]"),
        "d12 positive must emit a D12 finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("is_async_completing"),
        "d12 finding must name the offending marker; stdout:\n{}",
        stdout
    );
}

/// PR-G2 — codex MEDIUM "D12 multi-line bypass" finding. The same fixture
/// shape as `d12_positive_fixture_fires` but the declaration body spans
/// three lines. Before PR-G2 this used to slip through the rule's
/// same-line heuristic; the new scanner reads function bodies across
/// newlines and fires on the declaration line regardless of formatting.
#[test]
fn d12_multiline_positive_fixture_fires() {
    let workspace = workspace_root();
    let tmp = workspace
        .join("target")
        .join("doctrine_lint_d12_multiline_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d12/pos_multiline.rs"));
    std::fs::copy(&pos_src, tmp.join("pos_multiline.rs")).expect("copy pos_multiline fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d12-extra-scope",
        "doctrine_lint_d12_multiline_pos",
    ]);
    assert_eq!(
        code, 1,
        "d12 multi-line positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D12]"),
        "d12 multi-line positive must emit a D12 finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("is_async_completing"),
        "d12 multi-line finding must name the offending marker; stdout:\n{}",
        stdout
    );
}

#[test]
fn d12_negative_fixture_clean() {
    // The negative fixture exercises three accepted shapes (compliant
    // async, synchronous `false`, no override) — none must produce a
    // D12 finding.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d12_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d12/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d12-extra-scope",
        "doctrine_lint_d12_neg",
    ]);
    assert_eq!(
        code, 0,
        "d12 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D12]"),
        "d12 negative must produce zero D12 findings; stdout:\n{}",
        stdout
    );
}

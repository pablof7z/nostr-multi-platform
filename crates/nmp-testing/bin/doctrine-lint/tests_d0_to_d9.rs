//! Smoke tests for D0, D6, D7, D8 (no-polling), D9 (kernel-owned time), and
//! the action_namespace prefix rule, plus the authoritative
//! `workspace_is_doctrine_clean` full-workspace gate.
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
    // D6 is gated by an explicit `file_in_scope` (see `rules/d6.rs`) — a
    // fixture staged under `target/` outside any real `crates/nmp-*/src/`
    // layout must opt in via `--d6-extra-scope`, mirroring every other
    // path-scoped rule's fixture test.
    let (code, stdout, _stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d6-extra-scope",
        "doctrine_lint_d6_pos",
    ]);
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
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d6-extra-scope",
        "doctrine_lint_d6_neg",
    ]);
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

// ─── D8 — no polling (thread::sleep / tokio::time::sleep) ────────────────────
//
// The hot-path-allocation half of D8 (path-scoped to
// `crates/nmp-core/src/kernel/ingest/`, opt-in via a `// hot path` marker
// comment) was deleted (#2761 / #2769): the marker was used by zero
// functions, so the check measured nothing. See `rules/d8/mod.rs` for the
// deletion rationale. No-polling is the sole surviving D8 check.

#[test]
fn d8_sleep_positive_fixture_fires() {
    // The no-polling check is NOT path-scoped, so no `--d<N>-extra-scope`
    // flag is needed — pointing --path at the fixture dir is enough.
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

// ─── D9 (kernel-owned time) ─────────────────────────────────────────────────

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
    // D9 is path-scoped to kernel time-policy paths — the smoke fixture staged
    // under `target/` falls outside that scope, so `--d9-extra-scope` opts it in
    // (mirrors `--d6-extra-scope` / `--d14-extra-scope` for the other
    // path-scoped rules).
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
    // Every raw-time shape in the fixture must surface, including bare
    // multiline arguments and local variables whose names carry no policy
    // marker.
    for token in ["SystemTime::now", "Instant::now", "now_epoch_ms"] {
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

// ─── action_namespace (protocol-crate action namespace prefix) ──────────────

#[test]
fn action_namespace_positive_fixture_fires() {
    let workspace = workspace_root();
    let root = workspace
        .join("target")
        .join("doctrine_lint_action_namespace_pos");
    let tmp = root.join("crates").join("nmp-nip29").join("src");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("action_namespace/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 1,
        "action_namespace positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[action_namespace]"),
        "positive fixture must emit action_namespace finding; stdout:\n{}",
        stdout
    );
    for token in ["nip29.post_chat_message", "nip29.publish_group_event"] {
        assert!(
            stdout.contains(token),
            "action_namespace positive must name `{}`; stdout:\n{}",
            token,
            stdout
        );
    }
}

#[test]
fn action_namespace_negative_fixture_clean() {
    let workspace = workspace_root();
    let root = workspace
        .join("target")
        .join("doctrine_lint_action_namespace_neg");
    let tmp = root.join("crates").join("nmp-nip29").join("src");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("action_namespace/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 0,
        "action_namespace negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[action_namespace]"),
        "negative fixture must produce zero action_namespace findings; stdout:\n{}",
        stdout
    );
}

/// THE AUTHORITATIVE GUARD: the enforcement surface must equal the claimed
/// footprint (#2761). `--workspace-full` walks every `crates/*/src/` tree
/// (plus the `nmp-testing` harness binaries and every `apps/*/src/` tree) and
/// runs the full per-file ruleset; each rule's own `file_in_scope` decides
/// whether it fires on a given file, exactly as its docstring and unit tests
/// claim. This replaces the old `protocol_crates_are_doctrine_clean` test,
/// which asserted cleanliness only over a hardcoded 7-NIP-crate allowlist
/// that omitted `nmp-nip47` (the crate that shipped the #2762 `wallet_npub`
/// leak) and every non-NIP crate.
///
/// D6 (`.unwrap()`/`.expect()`/`panic!`/…) is the one rule that stays
/// deliberately bounded rather than reaching every walked file — see
/// `rules/d6.rs`'s "Scope" doc for why widening it to the whole workspace is
/// a separate, multi-PR campaign. Every other rule's scope predicate now
/// actually gets exercised against its claimed crates.
#[test]
fn workspace_is_doctrine_clean() {
    let (code, stdout, stderr) = run_lint(&["--workspace-full"]);
    assert_eq!(
        code, 0,
        "workspace must be doctrine-lint clean under --workspace-full; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    // Spell out D9 and action_namespace specifically — carried over from the
    // predecessor test. Any hit would already fail the `code == 0` check
    // above; explicit assertions make the intent obvious in the output.
    assert!(
        !stdout.contains("error[D9]"),
        "workspace must not contain D9 findings; stdout:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("error[action_namespace]"),
        "workspace must not contain action_namespace findings; stdout:\n{}",
        stdout
    );
}

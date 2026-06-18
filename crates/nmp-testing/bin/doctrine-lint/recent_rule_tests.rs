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

// ─── D21 (no ambient authority — K2 / ADR-0052 §D6 regression gate) ───────────

#[test]
fn d21_positive_fixture_fires() {
    // Stage pos.rs in isolation so neg.rs cannot pollute the assertion.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d21_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d21/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // D21 is path-scoped to the K2 blast-radius crates' `src/` trees — the
    // staged fixture under `target/` falls outside that scope, so
    // `--d21-extra-scope` opts it in (mirrors `--d20-extra-scope`).
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d21-extra-scope",
        "doctrine_lint_d21_pos",
    ]);
    assert_eq!(
        code, 1,
        "d21 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D21]"),
        "d21 positive must emit >=1 D21 finding; stdout:\n{}",
        stdout
    );
    // The fixture plants 8 banned ambient-authority statics. All must surface —
    // a regression that silently swallows one shape cannot pass this test.
    for token in [
        "ACTIVE_WALLET_RUNTIME",
        "GLOBAL_BROKER",
        "HOOK",
        "SESSIONS",
        "STORE",
        "SINK",
        "DRIVER",
        "BROKER2",
    ] {
        assert!(
            stdout.contains(token),
            "d21 positive must name banned static `{}`; stdout:\n{}",
            token,
            stdout
        );
    }
    let d21_count = stdout.matches("error[D21]").count();
    assert!(
        d21_count >= 8,
        "d21 must flag all 8 planted ambient-authority statics; got {}; stdout:\n{}",
        d21_count,
        stdout
    );
}

#[test]
fn d21_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d21_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d21/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d21-extra-scope",
        "doctrine_lint_d21_neg",
    ]);
    assert_eq!(
        code, 0,
        "d21 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D21]"),
        "d21 negative must produce zero D21 findings; stdout:\n{}",
        stdout
    );
}

/// Integration guard (K2 / ADR-0052): every K2 blast-radius crate — the crates
/// that hosted the five deleted process-global singletons (`ACTIVE_WALLET_RUNTIME`
/// in nmp-nip47, the bunker/NIP-55 `HOOK`s + `kernel_mut` in nmp-core,
/// `GLOBAL_BROKER`/`GLOBAL_DRIVER` in nmp-ffi) plus the two read-once-config
/// residuals (nmp-core wire_log, nmp-network socket_io) — must be D21-clean. If
/// a future change reintroduces an ambient-authority `static`/`OnceLock`/`Lazy`/
/// `lazy_static!` of a handle/runtime/sender/hook (outside `#[cfg(test)]`, a
/// test-only file, or an explicit `// doctrine-allow: D21 — reason`), this test
/// fails. This is the production-facing teeth of the rule that locks K2 in.
#[test]
fn k2_blast_radius_crates_are_d21_clean() {
    const K2_CRATES: &[&str] = &[
        "nmp-core",
        "nmp-ffi",
        "nmp-nip47",
        "nmp-network",
        "nmp-signer-broker",
        "nmp-signers",
        "nmp-signer-iface",
    ];
    for c in K2_CRATES {
        let path = format!("crates/{}/src", c);
        let (code, stdout, stderr) = run_lint(&["--path", &path]);
        let d21_findings: Vec<&str> = stdout
            .lines()
            .filter(|l| l.contains("error[D21]"))
            .collect();
        assert!(
            d21_findings.is_empty(),
            "{} must be D21-clean — no ambient-authority process-globals. \
             Thread per-app state through an `Arc`-slot instance field instead \
             (the K2 pattern), or gate a justified residual with \
             `// doctrine-allow: D21 — reason`. D21 findings:\n{}\nfull stdout:\n{}\nstderr:\n{}",
            c,
            d21_findings.join("\n"),
            stdout,
            stderr
        );
        // The lint may exit 1 because of OTHER unrelated rule findings in a
        // crate; we only assert D21-cleanliness here, not a clean exit code.
        let _ = code;
    }
}

// ─── no_raw_tap (raw event tap re-introduction guard) ─────────────────────────

#[test]
fn no_raw_tap_positive_fixture_fires() {
    // The no_raw_tap rule is workspace-wide but exempts `nmp-testing/` and the
    // doctrine-lint binary source.  To opt the fixture in, stage it under a
    // fake `crates/nmp-fake-crate/src/` tree so `file_in_scope` resolves to
    // `true` — mirroring the D21 approach above.
    let workspace = workspace_root();
    let crate_src = workspace
        .join("target")
        .join("doctrine_lint_no_raw_tap_pos")
        .join("crates")
        .join("nmp-fake-crate")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_no_raw_tap_pos"),
    );
    std::fs::create_dir_all(&crate_src).expect("create fake crate src dir");
    let pos_src = workspace.join(fixture_path("no_raw_tap/pos.rs"));
    std::fs::copy(&pos_src, crate_src.join("pos.rs")).expect("copy pos fixture");

    let crate_src_str = crate_src.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &crate_src_str]);
    assert_eq!(
        code, 1,
        "no_raw_tap positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[no_raw_tap]"),
        "no_raw_tap positive must emit >=1 no_raw_tap finding; stdout:\n{}",
        stdout
    );
    // Both banned tokens planted in pos.rs must surface.
    assert!(
        stdout.contains("RawEventObserver"),
        "no_raw_tap finding must name RawEventObserver; stdout:\n{}",
        stdout
    );
}

/// A bare `// doctrine-allow: no_raw_tap` (no reason) must NOT silence the rule —
/// it routes through `allow::line_allows_with_reason` (parser unit-tested in `allow.rs`).
#[test]
fn no_raw_tap_bare_allow_does_not_silence() {
    let src = workspace_root().join("target/doctrine_lint_no_raw_tap_bare/crates/fake/src");
    let _ = std::fs::remove_dir_all(workspace_root().join("target/doctrine_lint_no_raw_tap_bare"));
    std::fs::create_dir_all(&src).expect("create fake crate src");
    std::fs::write(
        src.join("bare.rs"),
        "fn w() { app.register_raw_event_observer(f, o); } // doctrine-allow: no_raw_tap\n",
    )
    .expect("write fixture");
    let (code, stdout, _) = run_lint(&["--path", &src.to_string_lossy()]);
    assert_eq!(code, 1, "bare allow must NOT silence no_raw_tap; stdout:\n{stdout}");
    assert!(stdout.contains("error[no_raw_tap]"), "finding must still fire; stdout:\n{stdout}");
}

#[test]
fn no_raw_tap_negative_fixture_clean() {
    // Stage neg.rs under a fake `crates/nmp-fake-crate/src/` tree so
    // `file_in_scope` resolves to `true` — then assert zero findings.
    let workspace = workspace_root();
    let crate_src = workspace
        .join("target")
        .join("doctrine_lint_no_raw_tap_neg")
        .join("crates")
        .join("nmp-fake-crate")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_no_raw_tap_neg"),
    );
    std::fs::create_dir_all(&crate_src).expect("create fake crate src dir");
    let neg_src = workspace.join(fixture_path("no_raw_tap/neg.rs"));
    std::fs::copy(&neg_src, crate_src.join("neg.rs")).expect("copy neg fixture");

    let crate_src_str = crate_src.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &crate_src_str]);
    assert_eq!(
        code, 0,
        "no_raw_tap negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[no_raw_tap]"),
        "no_raw_tap negative must produce zero no_raw_tap findings; stdout:\n{}",
        stdout
    );
}

#[test]
fn no_raw_tap_class_fixture_fires_without_named_token() {
    // The CLASS check must catch a RENAMED below-seam per-event ingest tap (an
    // `extern "C" fn(*mut c_void, *const c_char)` with a raw/event-tap intent
    // token) even when it uses none of the literal banned names. Staged under a
    // fake crate so it is in scope but OUTSIDE the external_event_sink module.
    let workspace = workspace_root();
    let crate_src = workspace
        .join("target")
        .join("doctrine_lint_no_raw_tap_class")
        .join("crates")
        .join("nmp-fake-crate")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_no_raw_tap_class"),
    );
    std::fs::create_dir_all(&crate_src).expect("create fake crate src dir");
    let pos_src = workspace.join(fixture_path("no_raw_tap/pos_class.rs"));
    std::fs::copy(&pos_src, crate_src.join("pos_class.rs")).expect("copy class fixture");

    let crate_src_str = crate_src.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &crate_src_str]);
    assert_eq!(
        code, 1,
        "no_raw_tap CLASS positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[no_raw_tap]"),
        "no_raw_tap CLASS positive must emit a finding for the renamed tap; stdout:\n{}",
        stdout
    );
}

#[test]
fn no_raw_tap_covers_apps_scope() {
    // This rule must cover apps/ too (an app could re-introduce a below-seam tap). Stage the named
    // positive fixture under a fake `apps/` tree and assert it fires.
    let workspace = workspace_root();
    let app_src = workspace
        .join("target")
        .join("doctrine_lint_no_raw_tap_apps")
        .join("apps")
        .join("fake-app")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_no_raw_tap_apps"),
    );
    std::fs::create_dir_all(&app_src).expect("create fake app src dir");
    let pos_src = workspace.join(fixture_path("no_raw_tap/pos.rs"));
    std::fs::copy(&pos_src, app_src.join("pos.rs")).expect("copy pos fixture");

    let app_src_str = app_src.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &app_src_str]);
    assert_eq!(
        code, 1,
        "no_raw_tap must scan apps/ and exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[no_raw_tap]"),
        "no_raw_tap must flag a banned token under apps/; stdout:\n{}",
        stdout
    );
}

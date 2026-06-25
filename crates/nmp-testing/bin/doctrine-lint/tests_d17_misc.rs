//! Smoke tests for D17 (social-timeline kind policy regression guard) and
//! miscellaneous structural invariant checks (cache-serve enqueue seal).
//!
//! Split out of `tests.rs` (file-size cap). Shared helpers imported from
//! parent module via `super`.

use super::{fixture_path, run_lint, workspace_root};

// --- D17 (social-timeline kind policy regression guard) ---------------------

#[test]
fn d17_positive_fixture_fires() {
    // Stage pos.rs in isolation so neg.rs cannot pollute the assertion.
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d17_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let pos_src = workspace.join(fixture_path("d17/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // D17 is path-scoped to `crates/nmp-core/src/` — the staged fixture
    // under `target/` falls outside that scope, so `--d17-extra-scope` opts
    // it in (mirrors `--d14-extra-scope`).
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d17-extra-scope",
        "doctrine_lint_d17_pos",
    ]);
    assert_eq!(
        code, 1,
        "d17 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D17]"),
        "d17 positive must emit >=1 D17 finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("V-68"),
        "d17 finding message must reference V-68; stdout:\n{}",
        stdout
    );
}

#[test]
fn d17_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d17_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    let neg_src = workspace.join(fixture_path("d17/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d17-extra-scope",
        "doctrine_lint_d17_neg",
    ]);
    assert_eq!(
        code, 0,
        "d17 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D17]"),
        "d17 negative must produce zero D17 findings; stdout:\n{}",
        stdout
    );
}

/// D17 must NOT fire on a test-only file (e.g. `tests.rs`) even when the
/// file path is in the nmp-core scope and the literal appears on a
/// non-comment line. This pins the `d6::file_is_test_only` exemption.
#[test]
fn d17_negative_test_only_file_exempt() {
    let workspace = workspace_root();
    let tmp = workspace
        .join("target")
        .join("doctrine_lint_d17_test_only_exempt");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    // Write a file named `tests.rs` (triggers `file_is_test_only`) whose
    // body contains the banned shape on a non-comment line.
    std::fs::write(
        tmp.join("tests.rs"),
        "fn check_kinds() {\n    \
         assert!(req.contains(\"\\\"kinds\\\":[1,6]\"));\n}\n",
    )
    .expect("write tests.rs fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d17-extra-scope",
        "doctrine_lint_d17_test_only_exempt",
    ]);
    assert_eq!(
        code, 0,
        "d17 must not fire on tests.rs; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D17]"),
        "d17 must not emit a finding for a tests.rs file; stdout:\n{}",
        stdout
    );
}

/// D17 is nmp-core-scoped: a file outside `crates/nmp-core/src/` (and not
/// opted in via `--d17-extra-scope`) must never trigger even if it contains
/// the banned shape.
#[test]
fn d17_does_not_fire_outside_nmp_core() {
    let workspace = workspace_root();
    let tmp = workspace
        .join("target")
        .join("doctrine_lint_d17_out_of_scope");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    std::fs::write(
        tmp.join("lib.rs"),
        "pub fn filter() -> &'static str {\n    \
         r#\"{\\\"kinds\\\":[1,6]}\"#\n}\n",
    )
    .expect("write lib.rs fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // NOTE: no --d17-extra-scope — the rule must self-gate away.
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 0,
        "d17 out-of-scope must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D17]"),
        "d17 must not fire on out-of-scope paths; stdout:\n{}",
        stdout
    );
}

/// N2: D17 must fire on a Rust kind-set literal `[1u32, 6u32]` (the exact
/// form that the deleted `nmp_app_open_timeline` used in nmp-ffi) when the
/// file is inside a scoped path.
#[test]
fn d17_positive_rust_kind_set_literal() {
    let workspace = workspace_root();
    let tmp = workspace
        .join("target")
        .join("doctrine_lint_d17_rust_ksl_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");
    std::fs::write(
        tmp.join("timeline.rs"),
        "use std::collections::BTreeSet;\n\
         pub fn social_kinds() -> BTreeSet<u32> {\n    \
         BTreeSet::from([1u32, 6u32])\n}\n",
    )
    .expect("write rust ksl positive fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d17-extra-scope",
        "doctrine_lint_d17_rust_ksl_pos",
    ]);
    assert_eq!(
        code, 1,
        "d17 Rust-literal positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D17]"),
        "d17 must emit a D17 finding for [1u32, 6u32]; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("V-68"),
        "d17 Rust-literal finding must reference V-68; stdout:\n{}",
        stdout
    );
}

/// N2: D17 must NOT fire on the `apps/chirp/crates/nmp-app-chirp` path even when
/// the file contains both a `"kinds":[1,6]` JSON shape and a `[1u32, 6u32]`
/// Rust literal — that is the legitimate home of the kind policy.
///
/// This is verified at the `file_in_scope` unit-test level in `d17.rs`; this
/// smoke test confirms the same invariant holds end-to-end through the binary.
/// We simulate it by staging the file in a directory that contains the
/// sentinel string `apps/chirp/crates/nmp-app-chirp` in its path (using a nested
/// subdirectory whose name carries the fragment) and passing it WITHOUT
/// `--d17-extra-scope` so the path-based gate is the sole guard.
#[test]
fn d17_does_not_fire_in_chirp_app_path() {
    let workspace = workspace_root();
    // Stage the file in target/apps/chirp/crates/nmp-app-chirp/src/ — the path
    // contains the `apps/chirp/crates/nmp-app-chirp` fragment that file_in_scope
    // exempts. The directory is NOT passed via --d17-extra-scope so the
    // file must be excluded by the path guard alone.
    let tmp = workspace
        .join("target")
        .join("apps")
        .join("chirp")
        .join("nmp-app-chirp")
        .join("src");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create chirp app temp dir");
    std::fs::write(
        tmp.join("ffi.rs"),
        "use std::collections::BTreeSet;\n\
         pub fn chirp_kinds() -> BTreeSet<u32> {\n    \
         BTreeSet::from([1u32, 6u32])\n}\n",
    )
    .expect("write chirp app fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    // Deliberately omit --d17-extra-scope — file_in_scope must self-gate.
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 0,
        "d17 must not fire in apps/chirp/crates/nmp-app-chirp; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D17]"),
        "d17 must not emit findings for the chirp app path; stdout:\n{}",
        stdout
    );
}

// ─── Cache-serve enqueue seal (ADR-0045 store-first by construction) ─────────

/// Seal guard: the two low-level enqueue helpers
/// (`enqueue_cache_serve`, `enqueue_interest_cache_serve_deferred`) MUST remain
/// PRIVATE to `crates/nmp-core/src/kernel/cache_serve/mod.rs`. Making them
/// wider (`pub`, `pub(crate)`, `pub(in crate::kernel)`, …) would re-open the
/// bypass that `Kernel::register_interest` was sealed to prevent.
///
/// The Rust compiler already enforces this once the functions are private — this
/// test is defense-in-depth: it fails immediately if anyone re-widens the
/// visibility declaration in the source, before a reviewer has to notice it.
#[test]
fn cache_serve_enqueue_helpers_are_sealed_private_to_module() {
    let root = workspace_root();
    let mod_path = root.join("crates/nmp-core/src/kernel/cache_serve/mod.rs");
    let src = std::fs::read_to_string(&mod_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", mod_path.display(), e));

    // The helpers must be plain `fn` — no visibility prefix that would expose
    // them outside `cache_serve/mod.rs`. Check every plausible wider form.
    let banned_patterns = [
        "pub fn enqueue_cache_serve(",
        "pub(crate) fn enqueue_cache_serve(",
        "pub(super) fn enqueue_cache_serve(",
        "pub(in crate::kernel) fn enqueue_cache_serve(",
        "pub fn enqueue_interest_cache_serve_deferred(",
        "pub(crate) fn enqueue_interest_cache_serve_deferred(",
        "pub(super) fn enqueue_interest_cache_serve_deferred(",
        "pub(in crate::kernel) fn enqueue_interest_cache_serve_deferred(",
        // The deleted combo helper must stay deleted.
        "pub fn enqueue_interest_cache_serve(",
        "pub(crate) fn enqueue_interest_cache_serve(",
        "pub(in crate::kernel) fn enqueue_interest_cache_serve(",
    ];

    let mut violations: Vec<String> = Vec::new();
    for (line_no, line) in src.lines().enumerate() {
        for pat in &banned_patterns {
            if line.contains(pat) {
                violations.push(format!(
                    "{}:{}: visibility widening detected — `{}` must remain private to cache_serve: {}",
                    mod_path.display(),
                    line_no + 1,
                    pat.split('(').next().unwrap_or(pat),
                    line.trim(),
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "enqueue_cache_serve and enqueue_interest_cache_serve_deferred must be \
         private to cache_serve/mod.rs (the only production enqueue path is \
         Kernel::register_interest). Violations:\n{}",
        violations.join("\n")
    );
}

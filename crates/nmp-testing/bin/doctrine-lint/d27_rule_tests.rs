//! Smoke tests for D27 — banned display helpers and precomputed `*_label` /
//! `*_display` String fields in projection / snapshot / FFI serialization.
//!
//! Split out of `tests.rs` to keep that file within the file-size hard cap.
//! Helpers (`run_lint`, `workspace_root`, `fixture_path`) are inherited via
//! `super`.

use super::{fixture_path, run_lint, workspace_root};

// ─── D27 positive fixture ─────────────────────────────────────────────────────

/// The positive fixture (`fixtures/d27/pos.rs`) plants:
///   Part A — 7 banned display-helper calls (one per helper).
///   Part B — 3 precomputed `*_label` / `*_display` String struct fields.
///
/// All 10 sites must be flagged. The fixture is staged under
/// `target/doctrine_lint_d27_pos/` and opted into D27 scope via
/// `--d27-extra-scope`.
#[test]
fn d27_positive_fixture_fires() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d27_pos");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");

    let pos_src = workspace.join(fixture_path("d27/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("pos.rs")).expect("copy pos fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d27-extra-scope",
        "doctrine_lint_d27_pos",
    ]);

    assert_eq!(
        code, 1,
        "d27 positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D27]"),
        "d27 positive must emit >=1 D27 finding; stdout:\n{}",
        stdout
    );
    // All Part-A banned-call tokens must be named in the output.
    for token in [
        "short_npub",
        "to_npub",
        "short_hex",
        "avatar_initials",
        "display_name_initials",
        "avatar_color_hex",
        "format_ago_secs",
    ] {
        assert!(
            stdout.contains(token),
            "d27 positive must flag `{token}(`; stdout:\n{}",
            stdout
        );
    }
    // Part-B precomputed field names must appear in the output.
    for field in ["signer_label", "status_label", "wallet_npub_display"] {
        assert!(
            stdout.contains(field),
            "d27 positive must flag precomputed field `{field}`; stdout:\n{}",
            stdout
        );
    }
    // The message must cite ADR-0072.
    assert!(
        stdout.contains("ADR-0072"),
        "d27 finding message must reference ADR-0072; stdout:\n{}",
        stdout
    );
}

// ─── D27 negative fixture ─────────────────────────────────────────────────────

/// The negative fixture (`fixtures/d27/neg.rs`) contains only compliant code —
/// raw projection fields, semantic tone tokens, and struct-construction values
/// that look superficially like the patterns but are NOT field declarations.
#[test]
fn d27_negative_fixture_clean() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d27_neg");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");

    let neg_src = workspace.join(fixture_path("d27/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("neg.rs")).expect("copy neg fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d27-extra-scope",
        "doctrine_lint_d27_neg",
    ]);

    assert_eq!(
        code, 0,
        "d27 negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[D27]"),
        "d27 negative must produce zero D27 findings; stdout:\n{}",
        stdout
    );
}

// ─── D27 stale-allow hardening (#1712) ─────────────────────────────────────────

/// The stale-allow fixture (`fixtures/d27/stale_allow.rs`) plants:
///   - one `// doctrine-allow: D27` marker on a clean raw field (STALE — must
///     fire a finding), and
///   - one marker on a genuine banned call (LEGIT suppression — must stay
///     silent).
///
/// Exactly one D27 finding must result, and it must name the stale marker — not
/// the legitimately-suppressed `to_npub` call.
#[test]
fn d27_stale_allow_fixture_fires() {
    let workspace = workspace_root();
    let tmp = workspace.join("target").join("doctrine_lint_d27_stale");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");

    let src = workspace.join(fixture_path("d27/stale_allow.rs"));
    std::fs::copy(&src, tmp.join("stale_allow.rs")).expect("copy stale fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&[
        "--path",
        &tmp_str,
        "--d27-extra-scope",
        "doctrine_lint_d27_stale",
    ]);

    assert_eq!(
        code, 1,
        "d27 stale-allow must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[D27]") && stdout.contains("stale"),
        "stale-allow fixture must emit a D27 `stale` finding; stdout:\n{}",
        stdout
    );
    // The legitimately-allowed `to_npub` call must remain suppressed: the only
    // finding is the stale marker, so the suggestion text must not name to_npub.
    assert_eq!(
        stdout.matches("error[D27]").count(),
        1,
        "exactly one D27 finding (the stale marker) expected; stdout:\n{}",
        stdout
    );
}

//! Smoke tests for the strengthened `no_raw_tap` rule (ADR-0072 §8 step 5).
//!
//! Covers:
//! - The #1552-deleted native push C-ABI sink (`pos_native_sink.rs` fixture
//!   must trip the rule on `nmp_app_register_event_sink`).
//! - The retained in-process `ExternalEventSinkPolicy` relay-forwarding path
//!   must NOT be flagged (it is not the deleted native push sink).
//! - The pull-path code (`after_seq`, `AdvancePullCursor`, `on_pull_wake`)
//!   must NOT be flagged.
//!
//! Split out of `tests.rs` (file-size cap). Shared helpers imported from
//! parent module via `super`.

use super::{fixture_path, run_lint, workspace_root};

// ─── pos_native_sink: named token fires ──────────────────────────────────────

/// `nmp_app_register_event_sink` — the C-ABI register function for the
/// #1552-deleted native push sink — must trip the no_raw_tap rule.
///
/// The fixture (`pos_native_sink.rs`) re-declares this symbol in a fake crate
/// to simulate a reintroduction attempt. The rule must catch it via the named
/// token list, not via the CLASS check (the function signature is different
/// from the `*mut c_void / *const c_char` raw-tap shape).
#[test]
fn no_raw_tap_native_sink_positive_fires() {
    let workspace = workspace_root();
    let crate_src = workspace
        .join("target")
        .join("doctrine_lint_no_raw_tap_native_sink_pos")
        .join("crates")
        .join("nmp-fake-crate")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_no_raw_tap_native_sink_pos"),
    );
    std::fs::create_dir_all(&crate_src).expect("create fake crate src dir");
    let pos_src = workspace.join(fixture_path("no_raw_tap/pos_native_sink.rs"));
    std::fs::copy(&pos_src, crate_src.join("pos_native_sink.rs"))
        .expect("copy pos_native_sink fixture");

    let crate_src_str = crate_src.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &crate_src_str]);
    assert_eq!(
        code, 1,
        "no_raw_tap native-sink positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[no_raw_tap]"),
        "no_raw_tap native-sink positive must emit >=1 no_raw_tap finding; stdout:\n{}",
        stdout
    );
    // The banned named token must be called out explicitly.
    assert!(
        stdout.contains("nmp_app_register_event_sink"),
        "finding must name `nmp_app_register_event_sink`; stdout:\n{}",
        stdout
    );
}

// ─── neg: ExternalEventSinkPolicy + pull path must be clean ───────────────────

/// The updated negative fixture (`neg.rs`) contains both `ExternalEventSinkPolicy`
/// (in-process relay forwarding, ALLOWED) and pull-path identifiers
/// (`after_seq`, `AdvancePullCursor`, `on_pull_wake`, `NmpApp::mirror_pull_page`).
/// None of these should trip the rule.
#[test]
fn no_raw_tap_external_event_sink_policy_and_pull_path_are_clean() {
    let workspace = workspace_root();
    let crate_src = workspace
        .join("target")
        .join("doctrine_lint_no_raw_tap_pull_neg")
        .join("crates")
        .join("nmp-fake-crate")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_no_raw_tap_pull_neg"),
    );
    std::fs::create_dir_all(&crate_src).expect("create fake crate src dir");
    let neg_src = workspace.join(fixture_path("no_raw_tap/neg.rs"));
    std::fs::copy(&neg_src, crate_src.join("neg.rs")).expect("copy neg fixture");

    let crate_src_str = crate_src.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &crate_src_str]);
    assert_eq!(
        code, 0,
        "ExternalEventSinkPolicy + pull-path negative must exit 0; \
         stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains("error[no_raw_tap]"),
        "ExternalEventSinkPolicy + pull-path negative must produce zero \
         no_raw_tap findings; stdout:\n{}",
        stdout
    );
}

// ─── inline: retain_until_ack token trips the rule ────────────────────────────

/// `retain_until_ack` — the retain-until-ack cursor pattern from the deleted
/// #1552 native push sink — must be caught by the named-token list even when
/// no other banned symbol is present.
#[test]
fn no_raw_tap_retain_until_ack_fires() {
    let workspace = workspace_root();
    let crate_src = workspace
        .join("target")
        .join("doctrine_lint_no_raw_tap_retain_ack")
        .join("crates")
        .join("nmp-fake-crate")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_no_raw_tap_retain_ack"),
    );
    std::fs::create_dir_all(&crate_src).expect("create fake crate src dir");
    // Write an inline fixture that uses the retain_until_ack pattern.
    std::fs::write(
        crate_src.join("retain_sink.rs"),
        "// A reimplemented native push sink — banned.\n\
         struct NativeSinkState {\n    \
         retain_until_ack: u64, // retain-until-ack cursor — banned\n\
         }\n",
    )
    .expect("write retain_until_ack fixture");

    let crate_src_str = crate_src.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &crate_src_str]);
    assert_eq!(
        code, 1,
        "retain_until_ack must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains("error[no_raw_tap]"),
        "retain_until_ack must emit a no_raw_tap finding; stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("retain_until_ack"),
        "finding must name `retain_until_ack`; stdout:\n{}",
        stdout
    );
}

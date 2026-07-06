//! BROWSER_RUNTIME_BOUNDARY fixture smoke tests.
//!
//! Tests the browser_runtime_boundary doctrine-lint rule against positive/negative
//! fixtures. The rule enforces that browser-runtime transport adapters remain pure
//! adapters with no routing/policy/subscription vocabulary, and extends the D8 no-polling
//! scan to browser packages.
//!
//! Run via `cargo test -p nmp-testing --test doctrine_lint_smoke`.

use std::path::PathBuf;

use super::run_lint;

#[test]
fn browser_runtime_boundary_negative_fixture_passes() {
    let args = vec!["--path", "crates/nmp-testing/bin/doctrine-lint/fixtures/browser_runtime_boundary/neg.rs"];
    let (code, stdout, stderr) = run_lint(&args);

    assert_eq!(
        code, 0,
        "BROWSER_RUNTIME_BOUNDARY negative fixture should pass (exit 0). \
         stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
}

#[test]
fn browser_runtime_boundary_positive_fixture_fails() {
    let args = vec!["--path", "crates/nmp-testing/bin/doctrine-lint/fixtures/browser_runtime_boundary/pos.rs"];
    let (code, stdout, _stderr) = run_lint(&args);

    assert_ne!(
        code, 0,
        "BROWSER_RUNTIME_BOUNDARY positive fixture should fail (exit != 0)"
    );

    // Assert that at least one finding matches the BROWSER_RUNTIME_BOUNDARY rule id.
    assert!(
        stdout.contains("BROWSER_RUNTIME_BOUNDARY"),
        "Output should contain BROWSER_RUNTIME_BOUNDARY rule violations.\nstdout:\n{}",
        stdout
    );
}

#[test]
fn browser_runtime_boundary_detects_outbox() {
    let args = vec!["--path", "crates/nmp-testing/bin/doctrine-lint/fixtures/browser_runtime_boundary/pos.rs"];
    let (code, stdout, _stderr) = run_lint(&args);

    assert_ne!(code, 0);

    // The positive fixture contains "outbox" and "outbox_resolver" mentions.
    assert!(
        stdout.contains("outbox"),
        "Output should flag outbox routing vocabulary. \
         stdout:\n{}",
        stdout
    );
}

#[test]
fn browser_runtime_boundary_detects_routing_vocab() {
    let args = vec!["--path", "crates/nmp-testing/bin/doctrine-lint/fixtures/browser_runtime_boundary/pos.rs"];
    let (code, stdout, _stderr) = run_lint(&args);

    assert_ne!(code, 0);

    // The positive fixture contains routing policy vocabulary like "route_to",
    // "Nip65", "signer_kind", "publish_target".
    let has_routing_hit = stdout.contains("Nip65")
        || stdout.contains("publish_target")
        || stdout.contains("signer_kind");

    assert!(
        has_routing_hit,
        "Output should flag at least one routing policy vocabulary term. \
         stdout:\n{}",
        stdout
    );
}

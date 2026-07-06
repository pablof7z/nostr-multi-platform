//! WASM_ABI_ONLY fixture smoke tests.
//!
//! Tests the wasm_abi_only doctrine-lint rule against positive/negative
//! fixtures. The rule enforces that nmp-wasm or browser-runtime ABI modules
//! contain only narrowly-scoped ABI glue, not domain business logic, routing,
//! signers, or NIP-specific code.
//!
//! Run via `cargo test -p nmp-testing --test doctrine_lint_smoke`.

use std::path::PathBuf;

use super::run_lint;

#[test]
fn wasm_abi_only_negative_fixture_passes() {
    let args = vec!["--path", "crates/nmp-testing/bin/doctrine-lint/fixtures/wasm_abi_only/neg.rs"];
    let (code, stdout, stderr) = run_lint(&args);

    assert_eq!(
        code, 0,
        "WASM_ABI_ONLY negative fixture should pass (exit 0). \
         stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
}

#[test]
fn wasm_abi_only_positive_fixture_fails() {
    let args = vec!["--path", "crates/nmp-testing/bin/doctrine-lint/fixtures/wasm_abi_only/pos.rs"];
    let (code, stdout, _stderr) = run_lint(&args);

    assert_ne!(
        code, 0,
        "WASM_ABI_ONLY positive fixture should fail (exit != 0)"
    );

    // Assert that at least one finding matches the WASM_ABI_ONLY rule id.
    assert!(
        stdout.contains("WASM_ABI_ONLY"),
        "Output should contain WASM_ABI_ONLY rule violations.\nstdout:\n{}",
        stdout
    );
}

#[test]
fn wasm_abi_only_detects_banned_imports() {
    let args = vec!["--path", "crates/nmp-testing/bin/doctrine-lint/fixtures/wasm_abi_only/pos.rs"];
    let (code, stdout, _stderr) = run_lint(&args);

    assert_ne!(code, 0);

    // The positive fixture contains several banned imports; at least one
    // should be caught (e.g., nmp_router, nmp_signers, nmp_nip65, apps::chirp).
    let has_banned_import_hit = stdout.contains("nmp_router")
        || stdout.contains("nmp_signers")
        || stdout.contains("nmp_nip")
        || stdout.contains("apps::");

    assert!(
        has_banned_import_hit,
        "Output should flag at least one banned import. \
         stdout:\n{}",
        stdout
    );
}

#[test]
fn wasm_abi_only_detects_banned_vocabulary() {
    let args = vec!["--path", "crates/nmp-testing/bin/doctrine-lint/fixtures/wasm_abi_only/pos.rs"];
    let (code, stdout, _stderr) = run_lint(&args);

    assert_ne!(code, 0);

    // The positive fixture contains banned vocabulary like "outbox", "route_to",
    // "Nip65", "signer_kind", "publish_target".
    let has_vocabulary_hit = stdout.contains("outbox")
        || stdout.contains("route_to")
        || stdout.contains("publish_target")
        || stdout.contains("policy vocabulary");

    assert!(
        has_vocabulary_hit,
        "Output should flag at least one banned policy vocabulary term. \
         stdout:\n{}",
        stdout
    );
}

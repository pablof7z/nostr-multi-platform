//! Doctrine-lint smoke test — runs the binary against the per-rule fixture
//! directories and asserts:
//!   - positive fixtures produce ≥1 finding tagged with the expected rule id
//!   - negative fixtures produce zero findings
//!
//! Run via `cargo test -p nmp-testing --test doctrine_lint_smoke`. The
//! GitHub Action `.github/workflows/doctrine-lint.yml` runs both this test
//! AND the binary directly against `nmp-core`.
//!
//! ## Layout
//!
//! This file is the thin entry point. Per-rule tests live in sibling files
//! grouped by doctrine rule to stay within the 500-LOC file-size ceiling:
//!
//! | sibling file              | rules covered                         |
//! |---------------------------|---------------------------------------|
//! | `tests_d0_to_d9.rs`       | D0, D6, D7, D8, D9, action_namespace |
//! | `tests_d10_d11_d12.rs`    | D10, D11, D12                         |
//! | `tests_d13_d14_d15.rs`    | D13, D14, D15                         |
//! | `tests_d16_workspace.rs`  | D16, --workspace-d8, end-to-end clean |
//! | `tests_d17_misc.rs`       | D17, cache-serve seal                 |
//! | `file_size_gate_tests.rs` | file-size baseline ratchet            |
//! | `manifest_gates.rs`       | app production dependency gates       |
//! | `authority_rule_tests.rs` | D26                                   |
//! | `d27_rule_tests.rs`       | D27                                   |
//! | `event_flow_rule_tests.rs`| D23/D24/D25                           |
//! | `no_raw_tap_rule_tests.rs`| no_raw_tap                            |
//! | `recent_rule_tests.rs`    | D19/D20/D21                           |
//! | `tests_a6.rs`             | A6                                    |

use std::path::PathBuf;
use std::process::Command;

mod authority_rule_tests; // D26 protocol-authority gate smoke tests — sibling module.
mod d27_rule_tests; // D27 projection display-helper ban smoke tests — sibling module.
mod event_flow_rule_tests; // D23/D24/D25 event-flow gate smoke tests — sibling module.
mod file_size_gate_tests; // File-size baseline ratchet smoke tests — sibling module.
mod manifest_gates; // App Cargo.toml production dependency gates — sibling module.
mod no_raw_tap_rule_tests; // no_raw_tap step-5 native-sink fixture tests — sibling module.
mod recent_rule_tests; // D19/D20/D21 fixture smoke tests — sibling module (file-size cap).
mod tests_a6; // A6 schema-less snapshot-projection lane smoke tests — sibling module.
mod tests_d0_to_d9; // D0, D6, D7, D8, D9, action_namespace — sibling module.
mod tests_d10_d11_d12; // D10, D11, D12 fixture smoke tests — sibling module.
mod tests_d13_d14_d15; // D13, D14, D15 fixture smoke tests — sibling module.
mod tests_d16_workspace; // D16, --workspace-d8, end-to-end clean — sibling module.
mod tests_d17_misc; // D17 and cache-serve seal — sibling module.

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

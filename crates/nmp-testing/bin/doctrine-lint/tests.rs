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
//! | `tests_d16_workspace.rs`  | apps/chirp tombstone, --workspace-d8, end-to-end clean |
//! | `tests_d17_misc.rs`       | D17, cache-serve seal                 |
//! | `file_size_gate_tests.rs` | file-size baseline ratchet            |
//! | `gallery_composition_gates.rs` | gallery explicit composition ratchet |
//! | `manifest_gates.rs`       | app production dependency gates       |
//! | `ownership_contract_gates.rs` | crate ownership declaration gates  |
//! | `authority_rule_tests.rs` | D26                                   |
//! | `d27_rule_tests.rs`       | D27                                   |
//! | `event_flow_rule_tests.rs`| D23/D24/D25                           |
//! | `nip29_kind_blind_tests.rs`| nip29 kind-blind transport (#2509/#2513) |
//! | `no_raw_tap_rule_tests.rs`| no_raw_tap                            |
//! | `product_raw_read_tests.rs`| product raw-read/session ratchet      |
//! | `deleted_defaults_tests.rs`| deleted nmp-defaults ratchet          |
//! | `feed_vocabulary_tests.rs`| feed-facade "session" vocabulary ratchet (#2508/#2783) |
//! | `no_deprecated_tests.rs`  | no deprecated-attribute compatibility ratchet (#2770) |
//! | `recent_rule_tests.rs`    | D19/D20/D21                           |
//! | `trellis_phase_a_gates.rs`| #2858 devtools Trellis receipt gates  |
//! | `tests_a6.rs`             | A6                                    |
//! | `browser_boundary_gates.rs` | browser-runtime/runtime-web boundary |
//! | `doc_citation_truth_gates.rs` | crate-boundaries.md §N.M / ADR-NNNN content-truth (#2768) |

use std::path::PathBuf;
use std::process::Command;

mod authority_rule_tests; // D26 protocol-authority gate smoke tests — sibling module.
mod browser_boundary_gates; // Browser runtime + runtime-web boundary smoke gates.
mod component_host_boundary_gates; // Component host package import/dependency gates.
mod concept_dependency_gates; // #2899 concept-crate dependency ratchet (binding/codegen layer).
mod concept_doorway_gates; // Concept read doorways stay in owner crates.
mod core_surface_gates; // nmp-core public surface / decoder ratchets.
mod d27_rule_tests; // D27 projection display-helper ban smoke tests — sibling module.
mod deleted_defaults_tests; // Deleted nmp-defaults production/scaffold ratchet.
mod doc_citation_truth_gates; // crate-boundaries.md §N.M / ADR-NNNN content-truth gate (#2768).
mod embed_owner_delegation_tests; // nmp-content embed projection must delegate owned protocol kinds.
mod event_flow_rule_tests; // D23/D24/D25 event-flow gate smoke tests — sibling module.
mod feed_vocabulary_tests; // Feed-facade "session" vocabulary ratchet (#2508/#2783) — sibling module.
mod file_size_gate_tests; // File-size baseline ratchet smoke tests — sibling module.
mod gallery_composition_gates; // Gallery explicit composition ratchet.
mod kind_predicate_authority_tests; // D4 nmp-kinds predicate ownership gate.
mod manifest_gates; // App Cargo.toml production dependency gates — sibling module.
mod native_runtime_boundary_gates; // Native runtime / C-ABI split boundary gates.
mod nip29_kind_blind_tests; // nip29 kind-blind transport ratchet (#2509/#2513) — sibling module.
mod no_deprecated_tests; // Deprecated compatibility attribute ratchet (#2770).
mod no_raw_tap_rule_tests; // no_raw_tap step-5 native-sink fixture tests — sibling module.
mod ownership_contract_gates; // Compiled positive ownership descriptor gates.
mod product_raw_read_tests; // Product raw-read/session ratchet smoke tests.
mod protocol_installer_shape_gates; // Protocol installer public-shape ratchet.
mod publish_route_gates; // Publish-route provenance/default deletion gates.
mod recent_rule_tests; // D19/D20/D21 fixture smoke tests — sibling module (file-size cap).
mod tests_a6; // A6 schema-less snapshot-projection lane smoke tests — sibling module.
mod tests_d0_to_d9; // D0, D6, D7, D8, D9, action_namespace — sibling module.
mod tests_d10_d11_d12; // D10, D11, D12 fixture smoke tests — sibling module.
mod tests_d13_d14_d15; // D13, D14, D15 fixture smoke tests — sibling module.
mod tests_d16_workspace; // apps/chirp tombstone, --workspace-d8, end-to-end clean — sibling module.
mod tests_d17_misc; // D17 and cache-serve seal — sibling module.
mod trellis_phase_a_fixture_support; // #2858 receipt render fixture harness.
mod trellis_phase_a_gates; // #2858 devtools Trellis diagnostic-surface gates.
mod trellis_phase_a_support; // #2858 release graph and manifest scan helpers.
mod trellis_public_api_gates; // Trellis must stay private implementation machinery.
mod trellis_public_api_support; // Shared Trellis public-surface scan helpers.
mod wasm_abi_gates; // nmp-wasm retired-crate gates (deleted #2202; must stay deleted).

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

/// Returns (exit_code, stdout, stderr) for the prebuilt doctrine-lint binary
/// invoked from the workspace root.
fn run_lint(args: &[&str]) -> (i32, String, String) {
    let root = workspace_root();
    let output = Command::new(env!("CARGO_BIN_EXE_doctrine-lint"))
        .current_dir(&root)
        .args(args)
        .output()
        .expect("doctrine-lint binary must succeed in spawning");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn fixture_path(sub: &str) -> String {
    format!("{}/{}", FIXTURE_ROOT, sub)
}

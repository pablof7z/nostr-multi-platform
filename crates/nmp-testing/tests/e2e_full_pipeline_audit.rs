//! Audit: fail CI when an e2e test is still `#[ignore]`-tagged for a milestone
//! that has already been recorded as DONE in `docs/plan.md`.
//!
//! # How it works
//!
//! 1. `DONE_MILESTONES` lists every milestone whose status is DONE per
//!    `docs/plan.md`.  Update this constant when a milestone ships.
//! 2. The test reads `e2e_full_pipeline.rs` and finds all `#[ignore = "..."]`
//!    annotations that match the pattern `blocked on M<N>+...`.
//! 3. For each gate milestone extracted from those annotations, if it appears
//!    in `DONE_MILESTONES`, the test fails with a message identifying which
//!    test function must be un-ignored and implemented.
//!
//! # When to update `DONE_MILESTONES`
//!
//! After a milestone lands on `master` and passes its exit gate (runnable
//! artifact + perf report + ADR update per `docs/plan.md`), add its label
//! here.  The label must match the `M<N>` prefix used in the ignore tags.
//!
//!   M0  — kernel substrate + non-Nostr fixture              (DONE)
//!   M1  — read-only Twitter slice on iOS                    (largely DONE — keep out until fully gated)
//!   M2  — subscription compilation + outbox + kind:3        (not yet)
//!   M3  — persistence LMDB + full insert invariants         (not yet)
//!   M4  — NIP-77 negentropy sync engine                     (not yet)
//!   M5  — NIP-42 relay auth                                 (not yet)
//!   M6  — sessions + signers + write path                   (not yet)
//!   M7  — reactions + thread + reply                        (not yet)
//!   M8  — relay manager + multi-relay sub lifecycle         (not yet)
//!
//! M0 and M1 are not referenced by any ignore tag in e2e_full_pipeline.rs,
//! so listing them here is a no-op for the audit.

use std::collections::HashSet;

/// Milestones whose exit gate is DONE on `master`.
///
/// Labels must match the `M<N>` identifiers used in
/// `#[ignore = "blocked on M<N>+..."]` tags in `e2e_full_pipeline.rs`.
///
/// IMPORTANT: Only add a milestone here once its full exit gate is met
/// (runnable artifact + perf numbers + ADR update).  Premature listing
/// causes this test to flag tests that genuinely cannot yet be implemented.
const DONE_MILESTONES: &[&str] = &[
    // M0 and M1 are done but not referenced in ignore tags — safe to list.
    "M0", "M1",
    // Add "M2", "M3", ... here as milestones land on master.
];

/// Path to the e2e test file, relative to the workspace root.
///
/// Resolved at runtime via the CARGO_MANIFEST_DIR env var so the test works
/// from any working directory the test runner uses.
const E2E_SOURCE_RELATIVE: &str = "tests/e2e_full_pipeline.rs";

#[test]
fn no_ignored_e2e_test_has_all_gates_done() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo test");
    let source_path = std::path::Path::new(&manifest_dir).join(E2E_SOURCE_RELATIVE);
    let source = std::fs::read_to_string(&source_path)
        .unwrap_or_else(|err| panic!("audit: cannot read {}: {}", source_path.display(), err));

    let done: HashSet<&str> = DONE_MILESTONES.iter().copied().collect();

    // Extract all `#[ignore = "blocked on ..."]` annotation lines.
    // Pattern: #[ignore = "blocked on M<gates>: <description>"]
    // Gates are separated by `+` (e.g. "M2+M3+M8").
    let mut failures: Vec<String> = Vec::new();

    for (line_number, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("#[ignore = \"blocked on ") {
            continue;
        }

        // Extract the gate string from the annotation.
        // Example line: #[ignore = "blocked on M2+M3+M8: subscription-planner, persistence, relay-manager"]
        let after_prefix = match trimmed.strip_prefix("#[ignore = \"blocked on ") {
            Some(rest) => rest,
            None => continue,
        };

        // Everything up to the first ':' or '"' is the gate spec.
        let gate_spec_end = after_prefix
            .find([']', ':', '"'])
            .unwrap_or(after_prefix.len());
        let gate_spec = &after_prefix[..gate_spec_end];

        // Split on '+' to get individual milestone tags.
        for gate in gate_spec.split('+') {
            let gate = gate.trim();
            if done.contains(gate) {
                // Find the function name on subsequent lines for a better error.
                let fn_name = source
                    .lines()
                    .skip(line_number + 1)
                    .find(|l| l.trim().starts_with("fn "))
                    .and_then(|l| l.trim().strip_prefix("fn "))
                    .and_then(|l| l.split('(').next())
                    .unwrap_or("<unknown>");

                failures.push(format!(
                    "  - `{}` is still #[ignore]d but its gate `{}` is DONE \
                     (line {}). Remove #[ignore] and implement the test.",
                    fn_name,
                    gate,
                    line_number + 1
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "e2e audit: the following tests must be un-ignored and implemented \
         because their blocking milestone(s) are now DONE:\n\n{}\n\n\
         Update `e2e_full_pipeline.rs` to remove the `#[ignore]` attribute \
         and provide a real implementation.",
        failures.join("\n")
    );
}

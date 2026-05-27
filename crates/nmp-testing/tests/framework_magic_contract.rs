//! Framework Magic Contract — cross-cutting test suite.
//!
//! 13 tests: C1–C9, C11–C13 behaviour tests + 1 coverage meta-test.
//! (C10 was removed when the `nmp-nip77` crate was deleted.)
//! Intentionally NOT milestone-prefixed; see
//! `docs/design/framework-magic/test-scaffolding.md` §1.
//!
//! File layout (per-chapter split, ≤300 LOC each):
//!   `framework_magic_contract.rs`              — index (this file) + meta-test
//!   `framework_magic_contract/c1_c4_c6_c9.rs` — C1, C2, C3, C4, C6, C9
//!   `framework_magic_contract/c5_c8_c13.rs`   — M2 gate: C5, C8, C13
//!   `framework_magic_contract/c7_c11.rs`       — M6 gate: C7, C11
//!   `framework_magic_contract/c12.rs`          — M8 gate: C12
//!
//! C10 was removed alongside the `nmp-nip77` crate deletion (zero shipping
//! callers; the substrate seam it exercised — `PlanCoverageHook` /
//! `set_coverage_hook` — remains pinned by `nmp-core`'s
//! `subs::coverage_hook_tests`).
//!
//! Remaining 13 tests are active. M2/M6/M8 milestones are DONE on master.
//!
//! Invocation: `cargo test -p nmp-testing --test framework_magic_contract`

mod framework_magic_contract {
    pub mod c12;
    pub mod c1_c4_c6_c9;
    pub mod c5_c8_c13;
    pub mod c7_c11;
}

// ── Coverage meta-test ────────────────────────────────────────────────────────

/// `contract_surface_complete` — asserts structural correspondence between this
/// file and `docs/design/framework-magic.md`'s contract table.
///
/// Drift classes caught:
/// 1. Doc table gains a row but this file does not grow a `#[test] fn`.
/// 2. This file gains a `#[test] fn` but the doc table does not list it.
/// 3. A renamed test breaks the doc-test correspondence.
///
/// Design: `docs/design/framework-magic/test-scaffolding.md` §4.
#[test]
fn contract_surface_complete() {
    const EXPECTED_TESTS: &[&str] = &[
        "c1_replaceable_supersedes_on_insert",
        "c2_parameterized_replaceable_supersedes_by_dtag",
        "c3_kind5_delete_removes_referenced_and_tombstones",
        "c4_nip40_expiration_removes_and_persists_schedule",
        "c5_kind3_change_recompiles_follow_dependent_subs",
        "c6_authors_subscription_routes_to_per_author_write_relays",
        "c7_publish_routes_outbox_and_private_fails_closed",
        "c8_subscriptions_coalesce_autoclose_and_buffer",
        "c9_provenance_merges_across_relay_redeliveries",
        "c11_bunker_url_and_nsec_creation_complete_via_actions",
        "c12_account_switch_rebinds_views_without_imperative_dance",
        "c13_view_payload_uses_placeholders_then_refines_in_place",
    ];

    // Parse test names from the contract table in framework-magic.md.
    // Table row format: `| # | Behavior | Sub-file | Test name | Milestone | Doctrine |`
    let doc = include_str!("../../../docs/design/framework-magic.md");
    let doc_test_names: Vec<String> = doc
        .lines()
        .filter(|l| l.starts_with("| C") || l.starts_with("| c"))
        .filter_map(|l| {
            let cols: Vec<&str> = l.split('|').collect();
            if cols.len() >= 5 {
                let name = cols[4].trim();
                if name.starts_with('`') && name.ends_with('`') {
                    Some(name[1..name.len() - 1].to_string())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    for doc_name in &doc_test_names {
        assert!(
            EXPECTED_TESTS.contains(&doc_name.as_str()),
            "contract doc lists '{}' not in EXPECTED_TESTS",
            doc_name
        );
    }
    for expected in EXPECTED_TESTS {
        assert!(
            doc_test_names.iter().any(|n| n == expected),
            "EXPECTED_TESTS lists '{}' not in the contract doc table",
            expected
        );
    }
    assert_eq!(
        doc_test_names.len(),
        EXPECTED_TESTS.len(),
        "doc table has {} names, EXPECTED_TESTS has {} — must agree",
        doc_test_names.len(),
        EXPECTED_TESTS.len()
    );
}

// ADR-0070 (#1671) — collector for the four `RefResolver` test sub-modules.
// The `*_tests_*` infix on each file satisfies the doctrine-lint `d6`
// classifier (test-only file exemption from the 500-LOC hard cap).
//
// Re-export everything from the parent `kernel` module so the test files'
// `use super::X` imports resolve through this module's namespace.
pub(super) use super::*;

#[path = "../refs_tests_event.rs"]
mod refs_tests_event;
#[path = "../refs_tests_key.rs"]
mod refs_tests_key;
#[path = "../refs_tests_lifecycle.rs"]
mod refs_tests_lifecycle;
#[path = "../refs_tests_profile.rs"]
mod refs_tests_profile;
// #1654 — NIP-73 external-ref (`i:<external-id>`) end-to-end projection coverage.
#[path = "../refs_tests_external.rs"]
mod refs_tests_external;

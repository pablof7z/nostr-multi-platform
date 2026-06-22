//! Unit tests for the `ActionLedger` and its derived projections.
//!
//! Split by concern into sibling modules:
//! - [`lifecycle_tests`] — `action_lifecycle` derived view (latest-stage
//!   collapse, first-record ordering, TTL, curated `reason_code`, cap edge).
//! - [`result_records_tests`] — `action_results` drain (S11 slice 2, #1758):
//!   `record_terminal` / `take_terminal_results` producer-order + field mapping.
//!
//! Kernel-level contract tests (driving `Kernel::record_action_stage` etc.)
//! live in `action_lifecycle_kernel_tests.rs`.

#[path = "tests/lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "tests/result_records_tests.rs"]
mod result_records_tests;

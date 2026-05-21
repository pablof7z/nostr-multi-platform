//! Positive D12 fixture — must trigger exactly one D12 finding.
//!
//! Declares `fn is_async_completing() -> bool { true }` on a single line
//! but never calls `record_action_stage` anywhere in the file. The rule
//! fires on the marker declaration line.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced from
//! a Cargo.toml `path = ...` entry). It exists solely as text for the
//! doctrine-lint smoke test to scan.

pub struct EmptyStageModule;

impl EmptyStageModule {
    // The declaration claims an async lifecycle but the module does not
    // record any stage transitions — the host stage mirror would never
    // observe this module's actions. D12 must fire here.
    pub fn is_async_completing() -> bool { true }
}

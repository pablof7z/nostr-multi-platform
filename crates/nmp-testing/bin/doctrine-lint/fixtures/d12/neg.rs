//! Negative D12 fixture — must produce zero D12 findings.
//!
//! Three shapes the rule must accept silently:
//!
//! 1. A module that declares `is_async_completing -> bool { true }` AND
//!    records `record_action_stage(` somewhere in the same file.
//! 2. A module that declares `is_async_completing -> bool { false }`
//!    (synchronous-by-declaration; no recording required).
//! 3. A module that does not override `is_async_completing` at all (uses
//!    the trait default).
//!
//! This file is NEVER compiled.

pub struct CompliantAsyncModule;

impl CompliantAsyncModule {
    pub fn is_async_completing() -> bool { true }

    pub fn drive(kernel: &mut Kernel) {
        // Sibling recording call — satisfies the rule.
        kernel.record_action_stage("corr-id", Stage::Publishing, None);
    }
}

pub struct SyncModule;

impl SyncModule {
    // Synchronous — `false` is the synchronous declaration; no recording
    // required.
    pub fn is_async_completing() -> bool { false }
}

pub struct DefaultModule;

// No override of `is_async_completing` at all — the trait default
// returns `false`. D12 must not fire.

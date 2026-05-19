//! Positive D7 fixture — file is named `capability.rs` and lives under a
//! `substrate/` path so the rule fires. Method names contain policy verbs.
//!
//! Smoke test points the lint at this dir and asserts at least one D7.

pub trait BadKeychainBridge: Send + Sync {
    /// This method's name says it "retries" — that's policy, not capability.
    fn retry_authentication(&self) -> bool;

    /// This method names a routing decision — that's the kernel's job.
    fn select_relay(&self) -> String;

    /// Reporting variant — would be fine, included here as a control.
    fn report_failure(&self) -> Option<String>;
}

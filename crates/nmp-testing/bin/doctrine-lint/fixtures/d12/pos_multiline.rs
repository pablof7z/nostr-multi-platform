//! Positive D12 fixture — multi-line declaration body — must trigger
//! exactly one D12 finding.
//!
//! Declares `fn is_async_completing() -> bool { ... true ... }` across
//! three lines but never calls `record_action_stage` anywhere in the
//! file. PR-G2 extended the rule's scanner to handle this shape after
//! codex flagged the single-line bypass: a future module formatted with
//! a multi-line body used to slip through silently. The rule now fires
//! on the declaration line regardless of whether the body is single- or
//! multi-line.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced
//! from a Cargo.toml `path = ...` entry). It exists solely as text for
//! the doctrine-lint smoke test to scan.

pub struct MultiLineAsyncModule;

impl MultiLineAsyncModule {
    // The declaration spans 3 lines and the body returns `true` —
    // identical in shape to PublishModule. Without PR-G2's multi-line
    // scan this used to slip through. D12 must now fire on the
    // declaration line.
    pub fn is_async_completing() -> bool {
        true
    }
}

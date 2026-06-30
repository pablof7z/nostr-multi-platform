//! Positive action_namespace fixture — must trigger at least one finding.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced from
//! a Cargo.toml `path = ...` entry). It exists solely as text for the
//! doctrine-lint smoke test to scan.

pub struct LegacyNip29PostAction;

impl LegacyNip29PostAction {
    // Stale naming — no `nmp.` prefix. action_namespace must fire here.
    pub const NAMESPACE: &'static str = "nip29.post_chat_message";
}

pub struct LegacyNip29PublishAction;

impl LegacyNip29PublishAction {
    // Another stale namespace — second action_namespace hit in the same file.
    pub const NAMESPACE: &'static str = "nip29.publish_group_event";
}

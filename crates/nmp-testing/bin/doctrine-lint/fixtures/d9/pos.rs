//! Positive D9 fixture — must trigger at least one D9 finding.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced from
//! a Cargo.toml `path = ...` entry). It exists solely as text for the
//! doctrine-lint smoke test to scan.

pub struct LegacyNip29PostAction;

impl LegacyNip29PostAction {
    // Stale naming — no `nmp.` prefix. D9 must fire here.
    pub const NAMESPACE: &'static str = "nip29.post_chat_message";
}

pub struct LegacyNip29ReactAction;

impl LegacyNip29ReactAction {
    // Another stale namespace — second D9 hit in the same file.
    pub const NAMESPACE: &'static str = "nip29.react_in_group";
}

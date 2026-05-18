//! Positive D0 fixture — must trigger at least one D0 finding.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced from
//! a Cargo.toml `path = ...` entry). It exists solely as text for the
//! doctrine-lint smoke test to scan.

pub struct GroupId(pub String);

pub fn join_group(group_id: &str, _pin_to: &str) -> bool {
    let _ = group_id;
    let _ = nip29::open(group_id);
    true
}

//! Positive nip29_kind_blind fixture — must trigger at least one finding.
//!
//! This file is NEVER compiled (Cargo only picks up files referenced from a
//! Cargo.toml `path = ...` entry). It exists solely as text for the
//! doctrine-lint smoke test, staged under `crates/nmp-nip29/src/` so the
//! nip29_kind_blind scope matches.

pub struct ReactInGroupAction;

impl ReactInGroupAction {
    // A reintroduced per-kind named group action — nip29_kind_blind must fire:
    // `react_in_group` is not on the kind-blind allowlist.
    pub const NAMESPACE: &'static str = "nmp.nip29.react_in_group";
}

// The deleted kind:7 authoring constant reappears — nip29_kind_blind must fire.
const REACTION_KIND: u32 = 7;

//! Integration tests for the `Grouper` algorithm. The grouper API is fully
//! public so unit-level coverage lives here rather than inline (keeps
//! `grouper.rs` under the AGENTS.md 500-LOC ceiling).
//!
//! The fake resolver in [`support`] is intentionally distinct from the
//! production NIP-10 / NIP-22 resolvers — it isolates the algorithm from
//! any convention-specific tag decoding.
//!
//! Split by behavior area under `tests/grouper/`; each submodule owns one
//! facet of the algorithm's observable behavior.

#[path = "grouper/support.rs"]
mod support;

#[path = "grouper/formation.rs"]
mod formation;
#[path = "grouper/mutation.rs"]
mod mutation;
#[path = "grouper/ordering.rs"]
mod ordering;
#[path = "grouper/roots_and_collapse.rs"]
mod roots_and_collapse;
#[path = "grouper/standalone_root_preservation.rs"]
mod standalone_root_preservation;
#[path = "grouper/supersession.rs"]
mod supersession;

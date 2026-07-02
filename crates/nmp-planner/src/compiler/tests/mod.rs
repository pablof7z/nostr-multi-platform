//! `compile_with_context` ORCHESTRATION tests — the Stage 3 per-relay merge
//! and the Stage 4 plan-id binding. Routing/lane behaviour (Stages 1+2) is
//! covered by the `partition::case_*` test modules; here we verify what
//! those tests cannot reach: how shaped relay-entries collapse into
//! `SubShape`s, how `originating_interests` accumulates and dedupes, and the
//! `compile()` vs `compile_with_context` plan-id contract.
//!
//! ## Submodules (file-size gate split)
//!
//! - [`empty_plan`] — Gap 1: an empty interest slice compiles to an empty,
//!   deterministic plan.
//! - [`filter_shape`] — Gap 2 + the mixed-shape and address-pointer cases:
//!   what a single interest's sub-shape looks like on the wire.
//! - [`relay_merge`] — Gap 3: two interests on the same relay merge when
//!   compatible and stay distinct when they fail the merge lattice.
//! - [`originating_interests`] — Gap 4 + Gap 5: how `originating_interests`
//!   and `role_tags` accumulate and dedupe across interests on one relay.
//! - [`plan_id_contract`] — Gap 6 + Gap 7: the `compile()` vs
//!   `compile_with_context()` plan-id contract, and which inputs the
//!   plan-id hash does (and does not) cover.

mod empty_plan;
mod filter_shape;
mod originating_interests;
mod plan_id_contract;
mod relay_merge;

use crate::compiler::mailbox::MailboxSnapshot;
use crate::interest::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};

/// Deterministic 64-char hex pubkey fixture from a short label.
fn pk(label: &str) -> String {
    format!("{label:0>64}").chars().take(64).collect()
}

/// A NIP-65 snapshot whose write relays are the given URLs.
fn write_snapshot(write: &[&str]) -> MailboxSnapshot {
    MailboxSnapshot {
        write_relays: write.iter().map(|s| s.to_string()).collect(),
        read_relays: vec![],
        both_relays: vec![],
    }
}

/// A tailing author+kind interest. `kinds` lets callers force a merge
/// refusal (Rule 1) by giving two interests different kind sets.
fn author_interest(
    id: u64,
    authors: &[&str],
    kinds: &[u32],
    lifecycle: InterestLifecycle,
) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: authors.iter().map(|a| pk(a)).collect(),
            kinds: kinds.iter().copied().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle,
        is_indexer_discovery: false,
    }
}

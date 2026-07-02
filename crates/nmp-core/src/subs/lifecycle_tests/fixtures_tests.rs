//! Shared interest/mailbox builders for the lifecycle test suite: a
//! deterministic 64-hex pubkey generator, a legacy registry-write helper,
//! and a single-author follow-interest constructor used across the
//! selection, dead-relay, drain-tick, and bootstrap-relay behavior modules.
use super::*;
use crate::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};

pub(crate) fn pubkey(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

pub(crate) fn push_legacy(reg: &mut InterestRegistry, interest: LogicalInterest) {
    use crate::kernel::cache_serve::{InterestWrite, RegistryWriteToken};
    let identity =
        crate::subs::test_identity_for_interest(("scoped-test-interest", interest.id.0), &interest);
    let token = RegistryWriteToken::for_test();
    let _ = reg.apply(&token, InterestWrite::Replace, identity, interest);
}

/// Single-author follow interest (kind:1 timeline).
pub(crate) fn follow(id: u64, author: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey(author)].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

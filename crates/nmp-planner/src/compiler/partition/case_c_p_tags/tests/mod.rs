//! PD-033-C planner extension — Case C bootstrap-content inbox fallback.
//!
//! Mirrors the matrix in `case_d_no_author.rs::pd033c_*` (Stage 1
//! precedent): positive route (`Global` and, #2942, `ActiveAccount`),
//! scope=Account counterpoint, lifecycle=OneShot counterpoint,
//! p_tag_routing=Nip17DmRelays counterpoint (fail-closed preserved), partial
//! inbox cache counterpoint (gate refuses), empty bootstrap counterpoint
//! (fall through to fail-closed), and plan_id stability under bootstrap
//! toggle.
//!
//! The headline contract: a `Tailing + #p (Nip65ReadRelays)` interest scoped
//! to the viewer's own account (`Global` or `ActiveAccount`) whose tagged
//! pubkey has no cached NIP-65 inbox AND `bootstrap_content_relays` is
//! non-empty routes to the bootstrap content lane, lane =
//! `UserConfigured(Bootstrap)`. This is the silent-loss regression Stage 2 of
//! PD-033-C exposes for the kernel's self-zap-receipts subscription
//! (`kind:9735 #p=[self_pk]` on `RelayRole::Content`) — and, before
//! `ActiveAccount` joined the gate (#2942), for any `ActiveAccount`-scoped
//! `#p` interest such as nmp-wallet's NIP-61 nutzap receipts.
//!
//! ## Submodules (file-size gate split)
//!
//! - [`gate`] — routing-decision matrix (positive route + counterpoints +
//!   plan-id stability).
//! - [`probe`] — Defect 1 regression: the bootstrap-inbox path fires
//!   `request_probe` for every tagged pubkey (uses a probe-recording cache).

mod gate;
mod probe;

use crate::interest::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, PTagRouting,
};
use std::collections::{BTreeMap, BTreeSet};

/// Deterministic 64-char hex pubkey fixture from a short label.
pub(super) fn pk(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

/// Build a `#p`-only interest with the given `p_tag_routing` mode.
/// Defaults to kind:9735 (the self-zap-receipts shape) and the canonical
/// `Tailing + Global` lifecycle/scope that the dispatcher gate keys on.
pub(super) fn p_tag_interest(
    id: u64,
    tagged: &[&str],
    routing: PTagRouting,
    lifecycle: InterestLifecycle,
    scope: InterestScope,
) -> LogicalInterest {
    let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let values: BTreeSet<String> = tagged.iter().map(|p| pk(p)).collect();
    tags.insert("p".to_string(), values);
    LogicalInterest {
        id: InterestId(id),
        scope,
        shape: InterestShape {
            kinds: [9735u32].into_iter().collect(),
            tags,
            limit: Some(50),
            p_tag_routing: routing,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle,
        is_indexer_discovery: false,
    }
}

pub(super) fn self_zap_receipts_interest() -> LogicalInterest {
    p_tag_interest(
        1,
        &["self"],
        PTagRouting::Nip65ReadRelays,
        InterestLifecycle::Tailing,
        InterestScope::Global,
    )
}

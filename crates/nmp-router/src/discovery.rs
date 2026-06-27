//! Discovery-kind helpers for lane 6 (Indexer) — shared by the router so the
//! `router.rs` hand-authored file stays under its LOC ceiling.
//!
//! Spec §3.1 lane 6 discovery kinds are the regular replaceable event kinds.
//! The indexer lane is ALWAYS-ON for these kinds — it stacks on top of the
//! per-author NIP-65 set so newer versions of these replaceable events
//! published to relays NOT in the cached set can still be discovered (defeating
//! the kind:10002 self-sealing loop).

use std::collections::BTreeSet;

/// True for kinds the indexer lane serves (regular replaceable kinds).
#[inline]
#[must_use]
pub fn is_discovery_kind(kind: u32) -> bool {
    nmp_kinds::is_replaceable(kind)
}

/// Compute the per-relay kind scope to attach to indexer relays for a
/// subscription interest carrying `kinds`.
///
/// Returns:
/// - `None` when lane 6 should NOT fire (no discovery kind present) OR when
///   every interest kind is already a discovery kind (no scope override
///   needed — the relay receives the full, all-discovery kind set).
/// - `Some(subset)` when the interest MIXES discovery and content kinds: the
///   indexer relay must be scoped to only `subset` (the discovery kinds) so
///   content kinds (e.g. kind:1 notes) do not leak onto the indexer.
///
/// The caller distinguishes "lane fires unscoped" from "lane does not fire"
/// by separately testing [`is_discovery_kind`] on the interest kinds; this
/// helper only answers "what scope override, if any, does the indexer need?".
#[must_use]
pub(crate) fn indexer_kind_scope(kinds: &BTreeSet<u32>) -> Option<BTreeSet<u32>> {
    let any_discovery = kinds.iter().any(|k| is_discovery_kind(*k));
    if !any_discovery {
        return None;
    }
    let any_content = kinds.iter().any(|k| !is_discovery_kind(*k));
    if !any_content {
        // All-discovery interest — no override; use the full kind set.
        return None;
    }
    Some(
        kinds
            .iter()
            .copied()
            .filter(|k| is_discovery_kind(*k))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_kinds_recognised() {
        assert!(is_discovery_kind(0));
        assert!(is_discovery_kind(3));
        assert!(is_discovery_kind(41));
        assert!(is_discovery_kind(10_002));
        assert!(!is_discovery_kind(1));
        assert!(!is_discovery_kind(6));
        assert!(!is_discovery_kind(20_000));
    }

    #[test]
    fn mixed_interest_scopes_to_discovery_subset() {
        let kinds = BTreeSet::from([1u32, 3]);
        assert_eq!(indexer_kind_scope(&kinds), Some(BTreeSet::from([3u32])));
    }

    #[test]
    fn all_discovery_interest_needs_no_scope() {
        let kinds = BTreeSet::from([0u32, 3]);
        assert_eq!(indexer_kind_scope(&kinds), None);
    }

    #[test]
    fn content_only_interest_needs_no_scope() {
        let kinds = BTreeSet::from([1u32, 6]);
        assert_eq!(indexer_kind_scope(&kinds), None);
    }
}

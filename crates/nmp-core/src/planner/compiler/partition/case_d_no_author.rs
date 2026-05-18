//! Case D: no authors, addresses, or `#p` → active-account read relays ∪
//! app relays.
//!
//! Used for hashtag firehose queries and global search — interests that are
//! not scoped to any specific author or recipient. Per the routing-rules
//! clarification:
//!
//! - The hashtag firehose REQ goes to the UNION of the active account's
//!   `read_relays` and the kernel-configured `app_relays`. Both lanes
//!   (`UserConfigured(AccountRead)` and `UserConfigured(AppRelay)`) are
//!   recorded so diagnostics show why each URL was selected.
//! - When BOTH sets are empty, we fall through to the indexer set as a
//!   last-resort cold-start landing pad. This is the only remaining content
//!   path that touches the indexer set and exists purely so kernel-driven
//!   bootstrap traffic still lands somewhere before the user has configured
//!   anything; it is not a substitute for app_relays in normal operation.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::{
    interest::{InterestShape, LogicalInterest, RelayUrl},
    plan::{RoutingSource, UserConfiguredCategory},
};
use super::RelayEntry;

/// Route a no-author/no-address/no-p interest to active-account ∪ app_relays.
pub(super) fn route(
    interest: &LogicalInterest,
    base_shape: &InterestShape,
    active_account_read_relays: &[RelayUrl],
    app_relays: &[RelayUrl],
    indexer_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    // Per-URL source accumulator so a relay that appears in BOTH
    // `active_account_read_relays` and `app_relays` records both lanes
    // (`AccountRead` ∪ `AppRelay`) rather than collapsing to whichever set
    // was iterated last.
    let mut per_relay: BTreeMap<RelayUrl, BTreeSet<RoutingSource>> = BTreeMap::new();

    for relay in active_account_read_relays {
        per_relay
            .entry(relay.clone())
            .or_default()
            .insert(RoutingSource::UserConfigured(UserConfiguredCategory::AccountRead));
    }

    for relay in app_relays {
        per_relay
            .entry(relay.clone())
            .or_default()
            .insert(RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay));
    }

    // Cold-start indexer fallback: ONLY when both user-configured sources
    // produced zero URLs do we fall through to the indexer. This preserves
    // bootstrap behaviour for kernel-driven discovery REQs (kind:0/3/10002)
    // that legitimately fire before any account configuration is loaded.
    if per_relay.is_empty() {
        for relay in indexer_relays {
            per_relay
                .entry(relay.clone())
                .or_default()
                .insert(RoutingSource::UserConfigured(UserConfiguredCategory::Indexer));
        }
    }

    for (relay_url, sources) in per_relay {
        relay_entries.entry(relay_url).or_default().push(RelayEntry {
            base_shape: base_shape.clone(),
            authors_for_relay: BTreeSet::new(),
            addresses_for_relay: BTreeSet::new(),
            lifecycle: interest.lifecycle.clone(),
            sources,
            interest_id: interest.id.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::planner::{
        compiler::{InMemoryMailboxCache, SubscriptionCompiler},
        interest::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest},
        plan::{RoutingSource, UserConfiguredCategory},
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn hashtag_interest(id: u64, tag: &str) -> LogicalInterest {
        let mut tags: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut vals = BTreeSet::new();
        vals.insert(tag.to_string());
        tags.insert("t".to_string(), vals);
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape {
                kinds: [1u32].into_iter().collect(),
                tags,
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
        }
    }

    /// active_account ∪ app_relays — both lanes recorded on the union URL.
    #[test]
    fn case_d_unions_active_account_with_app_relays() {
        let cache = InMemoryMailboxCache::new();
        let aar = vec!["wss://read-1".to_string(), "wss://shared".to_string()];
        let app = vec!["wss://app".to_string(), "wss://shared".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &[], &aar, &app);

        let plan = compiler.compile(&[hashtag_interest(1, "nostr")]).expect("compile");

        // AccountRead-only URL.
        let read1 = plan.per_relay.get("wss://read-1").expect("read-1");
        assert!(read1
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AccountRead)));
        assert!(!read1
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));

        // AppRelay-only URL.
        let app_p = plan.per_relay.get("wss://app").expect("app");
        assert!(app_p
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));

        // Both lanes on shared URL.
        let shared = plan.per_relay.get("wss://shared").expect("shared");
        assert!(shared
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AccountRead)));
        assert!(shared
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));
    }

    /// Cold-start: both active_account and app_relays empty → fall through
    /// to indexer as a last-resort landing pad (kernel discovery REQs).
    #[test]
    fn case_d_cold_start_falls_through_to_indexer() {
        let cache = InMemoryMailboxCache::new();
        let indexer = vec!["wss://purplepag.es".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &[]);

        let plan = compiler.compile(&[hashtag_interest(1, "nostr")]).expect("compile");

        let ix = plan.per_relay.get("wss://purplepag.es").expect("indexer fallback");
        assert!(ix
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::Indexer)));
    }

    /// app_relays alone (no active_account) → routes to app_relays without
    /// touching the indexer set.
    #[test]
    fn case_d_app_relays_alone_skips_indexer() {
        let cache = InMemoryMailboxCache::new();
        let indexer = vec!["wss://purplepag.es".to_string()];
        let app = vec!["wss://app".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &app);

        let plan = compiler.compile(&[hashtag_interest(1, "nostr")]).expect("compile");

        assert!(plan.per_relay.get("wss://app").is_some());
        assert!(
            plan.per_relay.get("wss://purplepag.es").is_none(),
            "indexer must NOT be touched when app_relays carry the firehose"
        );
    }
}

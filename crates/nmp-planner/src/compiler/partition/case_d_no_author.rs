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
//!   anything; it is not a substitute for `app_relays` in normal operation.
//!
//! ## PD-033-C planner extension
//!
//! The sibling `route_bootstrap_content` helper handles the kernel-driven
//! discovery-oneshot case for referenced event ids. Callers (the partition
//! dispatcher in `partition::mod`) gate on `OneShot + Global + event_ids` and
//! invoke this helper BEFORE the normal Case D body, so a discovery REQ for
//! known event-id batches lands on a content relay
//! (`bootstrap_content_relays`) rather than the indexer set. Non-discovery
//! Case D interests (`Tailing` firehose, `Account`-scoped reads, anything
//! without concrete `event_ids`) still flow through `route` unchanged.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2,
//!          `docs/architecture-audit/pd033c-plan.md` §4.3
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use super::RelayEntry;
use crate::{
    interest::{InterestShape, LogicalInterest, RelayUrl},
    plan::{RoutingSource, UserConfiguredCategory},
};

/// Route no-author hints. Stacks with the normal Case D sources.
pub(super) fn route_hints(
    interest: &LogicalInterest,
    base_shape: &InterestShape,
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    let mut per_relay: BTreeMap<RelayUrl, BTreeSet<RoutingSource>> = BTreeMap::new();
    for hint in &interest.hints {
        let Some((relay_url, source)) = super::hint_helper::route_for_hint(hint) else {
            continue;
        };
        per_relay.entry(relay_url).or_default().insert(source);
    }
    for (relay_url, sources) in per_relay {
        relay_entries
            .entry(relay_url)
            .or_default()
            .push(RelayEntry {
                base_shape: base_shape.clone(),
                authors_for_relay: BTreeSet::new(),
                addresses_for_relay: BTreeSet::new(),
                lifecycle: interest.lifecycle.clone(),
                sources,
                interest_id: interest.id.clone(),
            });
    }
}

/// Route a no-author/no-address/no-p interest to active-account ∪ `app_relays`.
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
            .insert(RoutingSource::UserConfigured(
                UserConfiguredCategory::AccountRead,
            ));
    }

    for relay in app_relays {
        per_relay
            .entry(relay.clone())
            .or_default()
            .insert(RoutingSource::UserConfigured(
                UserConfiguredCategory::AppRelay,
            ));
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
                .insert(RoutingSource::UserConfigured(
                    UserConfiguredCategory::Indexer,
                ));
        }
    }

    for (relay_url, sources) in per_relay {
        relay_entries
            .entry(relay_url)
            .or_default()
            .push(RelayEntry {
                base_shape: base_shape.clone(),
                authors_for_relay: BTreeSet::new(),
                addresses_for_relay: BTreeSet::new(),
                lifecycle: interest.lifecycle.clone(),
                sources,
                interest_id: interest.id.clone(),
            });
    }
}

/// PD-033-C planner extension: route a `OneShot + Global + event_ids` discovery
/// interest to `bootstrap_content_relays`.
///
/// All emitted entries are tagged
/// `RoutingSource::UserConfigured(UserConfiguredCategory::Bootstrap)` — a
/// distinct lane sub-category so diagnostics can tell "cold-start discovery
/// fetch landed here" apart from "user-configured app relay carried this
/// content" (`AppRelay`) or "indexer carried this fallback firehose"
/// (`Indexer`).
pub(super) fn route_bootstrap_content(
    interest: &LogicalInterest,
    base_shape: &InterestShape,
    bootstrap_content_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
) {
    let mut per_relay: BTreeMap<RelayUrl, BTreeSet<RoutingSource>> = BTreeMap::new();
    for relay in bootstrap_content_relays {
        per_relay
            .entry(relay.clone())
            .or_default()
            .insert(RoutingSource::UserConfigured(
                UserConfiguredCategory::Bootstrap,
            ));
    }
    for (relay_url, sources) in per_relay {
        relay_entries
            .entry(relay_url)
            .or_default()
            .push(RelayEntry {
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
    use crate::{
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
            is_indexer_discovery: false,
        }
    }

    /// active_account ∪ app_relays — both lanes recorded on the union URL.
    #[test]
    fn case_d_unions_active_account_with_app_relays() {
        let cache = InMemoryMailboxCache::new();
        let aar = vec!["wss://read-1".to_string(), "wss://shared".to_string()];
        let app = vec!["wss://app".to_string(), "wss://shared".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &[], &aar, &app);

        let plan = compiler
            .compile(&[hashtag_interest(1, "nostr")])
            .expect("compile");

        // AccountRead-only URL.
        let read1 = plan.per_relay.get("wss://read-1").expect("read-1");
        assert!(read1.role_tags.contains(&RoutingSource::UserConfigured(
            UserConfiguredCategory::AccountRead
        )));
        assert!(!read1.role_tags.contains(&RoutingSource::UserConfigured(
            UserConfiguredCategory::AppRelay
        )));

        // AppRelay-only URL.
        let app_p = plan.per_relay.get("wss://app").expect("app");
        assert!(app_p.role_tags.contains(&RoutingSource::UserConfigured(
            UserConfiguredCategory::AppRelay
        )));

        // Both lanes on shared URL.
        let shared = plan.per_relay.get("wss://shared").expect("shared");
        assert!(shared.role_tags.contains(&RoutingSource::UserConfigured(
            UserConfiguredCategory::AccountRead
        )));
        assert!(shared.role_tags.contains(&RoutingSource::UserConfigured(
            UserConfiguredCategory::AppRelay
        )));
    }

    /// Cold-start: both active_account and app_relays empty → fall through
    /// to indexer as a last-resort landing pad (kernel discovery REQs).
    #[test]
    fn case_d_cold_start_falls_through_to_indexer() {
        let cache = InMemoryMailboxCache::new();
        let indexer = vec!["wss://purplepag.es".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &[]);

        let plan = compiler
            .compile(&[hashtag_interest(1, "nostr")])
            .expect("compile");

        let ix = plan
            .per_relay
            .get("wss://purplepag.es")
            .expect("indexer fallback");
        assert!(ix.role_tags.contains(&RoutingSource::UserConfigured(
            UserConfiguredCategory::Indexer
        )));
    }

    /// app_relays alone (no active_account) → routes to app_relays without
    /// touching the indexer set.
    #[test]
    fn case_d_app_relays_alone_skips_indexer() {
        let cache = InMemoryMailboxCache::new();
        let indexer = vec!["wss://purplepag.es".to_string()];
        let app = vec!["wss://app".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &app);

        let plan = compiler
            .compile(&[hashtag_interest(1, "nostr")])
            .expect("compile");

        assert!(plan.per_relay.get("wss://app").is_some());
        assert!(
            plan.per_relay.get("wss://purplepag.es").is_none(),
            "indexer must NOT be touched when app_relays carry the firehose"
        );
    }

    fn hex(byte: &str) -> String {
        byte.repeat(32)
    }

    fn discovery_oneshot_ids(id: u64, event_ids: &[&str]) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape {
                event_ids: event_ids.iter().map(|s| hex(s)).collect(),
                limit: Some(event_ids.len() as u32),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
            is_indexer_discovery: false,
        }
    }

    #[test]
    fn pd033c_event_ids_oneshot_global_routes_to_bootstrap_content() {
        let cache = InMemoryMailboxCache::new();
        let indexer = vec!["wss://purplepag.es".to_string()];
        let bootstrap = vec!["wss://relay.primal.net".to_string()];
        let aar = vec!["wss://user-read.example".to_string()];
        let app = vec!["wss://user-app.example".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &indexer,
            &aar,
            &app,
            &bootstrap,
            /* bootstrap_indexer = */ &[],
        );

        let plan = compiler
            .compile(&[discovery_oneshot_ids(1, &["aa", "bb"])])
            .expect("compile");

        let landed = plan
            .per_relay
            .get("wss://relay.primal.net")
            .expect("bootstrap content relay must carry the discovery REQ");
        assert!(landed.role_tags.contains(&RoutingSource::UserConfigured(
            UserConfiguredCategory::Bootstrap,
        )));
        assert!(
            plan.per_relay.get("wss://purplepag.es").is_none(),
            "event_ids discovery must NOT land on the indexer lane"
        );
        assert!(plan.per_relay.get("wss://user-read.example").is_none());
        assert!(plan.per_relay.get("wss://user-app.example").is_none());
        assert_eq!(plan.per_relay.len(), 1);
    }

    #[test]
    fn pd033c_event_ids_oneshot_with_empty_bootstrap_falls_through() {
        let cache = InMemoryMailboxCache::new();
        let indexer = vec!["wss://purplepag.es".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &indexer,
            &[],
            &[],
            /* bootstrap_content = */ &[],
            /* bootstrap_indexer = */ &[],
        );

        let plan = compiler
            .compile(&[discovery_oneshot_ids(1, &["aa"])])
            .expect("compile");

        let ix = plan
            .per_relay
            .get("wss://purplepag.es")
            .expect("indexer fallback still applies when bootstrap is empty");
        assert!(ix.role_tags.contains(&RoutingSource::UserConfigured(
            UserConfiguredCategory::Indexer
        )));
    }

    #[test]
    fn pd033c_tailing_event_ids_does_not_trigger_bootstrap_gate() {
        let cache = InMemoryMailboxCache::new();
        let indexer = vec!["wss://purplepag.es".to_string()];
        let bootstrap = vec!["wss://relay.primal.net".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &indexer,
            &[],
            &[],
            &bootstrap,
            /* bootstrap_indexer = */ &[],
        );

        let mut interest = discovery_oneshot_ids(1, &["aa"]);
        interest.lifecycle = InterestLifecycle::Tailing;
        let plan = compiler.compile(&[interest]).expect("compile");

        assert!(
            plan.per_relay.get("wss://relay.primal.net").is_none(),
            "Tailing event_ids must NOT route to bootstrap content relays"
        );
        assert!(plan.per_relay.get("wss://purplepag.es").is_some());
    }

    #[test]
    fn pd033c_account_scoped_event_ids_does_not_trigger_bootstrap_gate() {
        let cache = InMemoryMailboxCache::new();
        let bootstrap = vec!["wss://relay.primal.net".to_string()];
        let indexer = vec!["wss://purplepag.es".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &indexer,
            &[],
            &[],
            &bootstrap,
            /* bootstrap_indexer = */ &[],
        );

        let mut interest = discovery_oneshot_ids(1, &["aa"]);
        interest.scope = InterestScope::Account(hex("cc"));
        let plan = compiler.compile(&[interest]).expect("compile");

        assert!(
            plan.per_relay.get("wss://relay.primal.net").is_none(),
            "Account-scoped event_ids must NOT route to bootstrap content relays"
        );
    }

    #[test]
    fn pd033c_oneshot_global_without_event_ids_does_not_trigger_bootstrap_gate() {
        let cache = InMemoryMailboxCache::new();
        let bootstrap = vec!["wss://relay.primal.net".to_string()];
        let indexer = vec!["wss://purplepag.es".to_string()];
        let compiler = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &indexer,
            &[],
            &[],
            &bootstrap,
            /* bootstrap_indexer = */ &[],
        );

        let mut interest = hashtag_interest(1, "nostr");
        interest.lifecycle = InterestLifecycle::OneShot;
        let plan = compiler.compile(&[interest]).expect("compile");

        assert!(
            plan.per_relay.get("wss://relay.primal.net").is_none(),
            "OneShot+Global without event_ids must NOT route to bootstrap content"
        );
    }

    #[test]
    fn pd033c_bootstrap_toggle_does_not_change_plan_id() {
        let cache = InMemoryMailboxCache::new();
        let interests = [discovery_oneshot_ids(1, &["aa"])];

        let bootstrap_set = vec!["wss://relay.primal.net".to_string()];
        let no_bootstrap = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            /* bootstrap_content = */ &[],
            /* bootstrap_indexer = */ &[],
        );
        let with_bootstrap = SubscriptionCompiler::with_relays_and_bootstrap(
            &cache,
            &[],
            &[],
            &[],
            &bootstrap_set,
            /* bootstrap_indexer = */ &[],
        );

        let plan_without = no_bootstrap.compile(&interests).expect("compile");
        let plan_with = with_bootstrap.compile(&interests).expect("compile");
        assert!(plan_without.per_relay.is_empty());
        assert!(plan_with.per_relay.contains_key("wss://relay.primal.net"));
        assert_eq!(
            plan_without.plan_id, plan_with.plan_id,
            "bootstrap_content_relays must be excluded from compute_plan_id \
             (matches app_relays treatment — see compile_with_context Stage 4)"
        );
    }
}

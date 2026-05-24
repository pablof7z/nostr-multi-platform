//! Case B: no explicit authors, but `addresses` → Outbox (write relays).
//!
//! Routes address-pointer coordinates to the coordinate pubkey's outbox
//! relays. Per the routing-rules clarification, indexer relays are NOT
//! a content fallback; `app_relays` substitute when NIP-65 is unknown.
//!
//! - Coord pubkey with NIP-65 → REQ to `outbox_relays()` ∪ `app_relays`.
//! - Coord pubkey without NIP-65 → REQ to `app_relays` ONLY; we still
//!   `request_probe` so the next recompile routes via NIP-65.
//! - Coord pubkey without NIP-65 AND no `app_relays` → coord.pubkey is
//!   pushed into `unroutable` so the kernel can surface a UI diagnostic.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
//! Doctrine: D3 (outbox routing automatic).

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    interest::{InterestShape, LogicalInterest, NaddrCoord, Pubkey, RelayUrl},
    plan::{RoutingSource, UserConfiguredCategory},
};
use super::{MailboxCache, RelayEntry};

/// Route an interest with address-pointer pubkeys to their outbox relays.
pub(super) fn route(
    interest: &LogicalInterest,
    base_shape: &InterestShape,
    mailbox_cache: &dyn MailboxCache,
    app_relays: &[RelayUrl],
    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
    unroutable: &mut BTreeSet<Pubkey>,
) {
    let mut per_relay: BTreeMap<RelayUrl, (BTreeSet<NaddrCoord>, BTreeSet<RoutingSource>)> =
        BTreeMap::new();

    for coord in &interest.shape.addresses {
        let mut landed = false;

        match mailbox_cache.get(&coord.pubkey) {
            Some(snapshot) => {
                for relay in snapshot.outbox_relays() {
                    let entry = per_relay
                        .entry(relay.clone())
                        .or_insert_with(|| (BTreeSet::new(), BTreeSet::new()));
                    entry.0.insert(coord.clone());
                    entry.1.insert(RoutingSource::Nip65);
                    landed = true;
                }
            }
            None => {
                // Probe so the cache can route via NIP-65 on next recompile.
                mailbox_cache.request_probe(&coord.pubkey);
            }
        }

        for relay in app_relays {
            let entry = per_relay
                .entry(relay.clone())
                .or_insert_with(|| (BTreeSet::new(), BTreeSet::new()));
            entry.0.insert(coord.clone());
            entry.1.insert(RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay));
            landed = true;
        }

        if !landed {
            unroutable.insert(coord.pubkey.clone());
        }
    }

    for (relay_url, (addrs, sources)) in per_relay {
        relay_entries.entry(relay_url).or_default().push(RelayEntry {
            base_shape: base_shape.clone(),
            authors_for_relay: BTreeSet::new(),
            addresses_for_relay: addrs,
            lifecycle: interest.lifecycle.clone(),
            sources,
            interest_id: interest.id.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        compiler::{InMemoryMailboxCache, MailboxSnapshot, SubscriptionCompiler},
        interest::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest},
        plan::{RoutingSource, UserConfiguredCategory},
    };

    fn pk(s: &str) -> String {
        format!("{s:0>64}").chars().take(64).collect()
    }

    fn addr_interest(id: u64, coords: Vec<NaddrCoord>) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape {
                addresses: coords.into_iter().collect(),
                kinds: [30023u32].into_iter().collect(),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::OneShot,
        }
    }

    /// Coord pubkey with NIP-65 → outbox ∪ app_relays; AppRelay is additive.
    #[test]
    fn case_b_nip65_known_unions_with_app_relays() {
        let mut cache = InMemoryMailboxCache::new();
        cache.put(
            pk("alice"),
            MailboxSnapshot {
                write_relays: vec!["wss://alice-write".to_string()],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
        let app = vec!["wss://app".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &app);

        let coord = NaddrCoord { pubkey: pk("alice"), kind: 30023, d_tag: "post-1".to_string() };
        let plan = compiler.compile(&[addr_interest(1, vec![coord])]).expect("compile");

        assert!(plan
            .per_relay
            .get("wss://alice-write")
            .unwrap()
            .role_tags
            .contains(&RoutingSource::Nip65));
        assert!(plan
            .per_relay
            .get("wss://app")
            .unwrap()
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));
        assert!(plan.unroutable_authors.is_empty());
    }

    /// Coord pubkey without NIP-65 + no app_relays → coord.pubkey unroutable;
    /// the indexer is NOT a fallback.
    #[test]
    fn case_b_no_nip65_no_app_relays_marks_pubkey_unroutable() {
        let cache = InMemoryMailboxCache::new();
        let indexer = vec!["wss://purplepag.es".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &indexer, &[], &[]);

        let coord = NaddrCoord { pubkey: pk("ghost"), kind: 30023, d_tag: "post-1".to_string() };
        let plan = compiler.compile(&[addr_interest(1, vec![coord])]).expect("compile");

        assert!(plan.per_relay.is_empty(), "indexer must not carry content");
        assert!(plan.unroutable_authors.contains(&pk("ghost")));
    }

    /// Coord pubkey without NIP-65 + app_relays configured → AppRelay only.
    #[test]
    fn case_b_no_nip65_with_app_relays_routes_to_app_only() {
        let cache = InMemoryMailboxCache::new();
        let app = vec!["wss://app".to_string()];
        let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &app);

        let coord = NaddrCoord { pubkey: pk("ghost"), kind: 30023, d_tag: "x".to_string() };
        let plan = compiler.compile(&[addr_interest(1, vec![coord])]).expect("compile");

        let app_plan = plan.per_relay.get("wss://app").expect("app");
        assert!(app_plan
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::AppRelay)));
        assert!(!app_plan
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::Indexer)));
        assert!(plan.unroutable_authors.is_empty());
    }
}

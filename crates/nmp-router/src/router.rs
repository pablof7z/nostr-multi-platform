//! `GenericOutboxRouter` — the single
//! [`nmp_core::substrate::OutboxRouter`] impl
//! (`docs/architecture/crate-boundaries.md` §3.2).
//!
//! Step 2 ships:
//!
//! - The `explicit_targets` override path, fully correct (§3.4).
//! - Lane 1 (NIP-65 mailbox): `route_publish` consults
//!   [`MailboxCache::write_relays`] for the author; `route_subscription`
//!   consults `read_relays` for each author in the interest shape.
//! - Lane 7 (AppRelay) fallback when lane 1 is empty.
//! - Blocked-relay (kind:10006) subtractive post-filter via
//!   [`RoutedRelaySet::from_explicit`] / [`RoutedRelaySet::add`].
//!
//! Step 3 (the kernel cut-over PR) extends the algorithm with lanes 2/3/4/5/6
//! (hints, provenance, user-configured, NIP-51 class routing, indexer
//! eligibility for discovery kinds), keyed by the TODO insertion points
//! below. Step 2 is structurally complete enough for the
//! `explicit_targets` paths the NIP-17/NIP-29/Marmot migrations (steps 5,
//! 6) need.

use nmp_core::planner::LogicalInterest;
use nmp_core::substrate::{
    AppRelayMode, Direction, OutboxRouter, RoutedRelaySet, RoutingContext, RoutingError,
    RoutingSource, UnsignedEvent,
};

pub struct GenericOutboxRouter;

impl GenericOutboxRouter {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for GenericOutboxRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl OutboxRouter for GenericOutboxRouter {
    fn route_publish(
        &self,
        evt: &UnsignedEvent,
        ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        if let Some(explicit) = ctx.explicit_targets {
            // §3.4 — the override seam. Skip the generic algorithm.
            return Ok(RoutedRelaySet::from_explicit(explicit, ctx.blocked_relays));
        }

        let mut out = RoutedRelaySet::new();

        // Lane 1 — author's NIP-65 write set.
        if let Some(writes) = ctx.mailbox_cache.write_relays(&evt.pubkey) {
            for url in writes {
                if ctx.blocked_relays.contains(&url) {
                    continue;
                }
                out.add(url, RoutingSource::Nip65 { direction: Direction::Write });
            }
        }

        // Lane 7 — AppRelay fallback when lane 1 returned nothing.
        if out.is_empty() {
            for url in ctx.session_keys.app_relays.iter() {
                if ctx.blocked_relays.contains(url) {
                    continue;
                }
                out.add(url.clone(), RoutingSource::AppRelay {
                    mode: AppRelayMode::Fallback,
                });
            }
        }

        // TODO §3.1 lane 2 — relay-hint tags on `evt`.
        // TODO §3.1 lane 3 — provenance (kind/event-id seen at relay X).
        // TODO §3.1 lane 4 — UserConfigured (active-account write).
        // TODO §3.1 lane 5 — NIP-51 ClassRouted (search/draft/wiki).
        // TODO §3.1 lane 6 — Indexer eligibility for kind:0 / kind:3 / 10000–19999.

        if out.is_empty() {
            return Err(RoutingError::Unroutable(evt.pubkey.clone()));
        }
        Ok(out)
    }

    fn route_subscription(
        &self,
        interest: &LogicalInterest,
        ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        if let Some(explicit) = ctx.explicit_targets {
            return Ok(RoutedRelaySet::from_explicit(explicit, ctx.blocked_relays));
        }

        let mut out = RoutedRelaySet::new();

        // Lane 1 — each author's NIP-65 read set.
        for author in &interest.shape.authors {
            if let Some(reads) = ctx.mailbox_cache.read_relays(author) {
                for url in reads {
                    if ctx.blocked_relays.contains(&url) {
                        continue;
                    }
                    out.add(url, RoutingSource::Nip65 { direction: Direction::Read });
                }
            }
        }

        // Lane 7 — AppRelay fallback when lane 1 returned nothing.
        if out.is_empty() {
            for url in ctx.session_keys.app_relays.iter() {
                if ctx.blocked_relays.contains(url) {
                    continue;
                }
                out.add(url.clone(), RoutingSource::AppRelay {
                    mode: AppRelayMode::Fallback,
                });
            }
        }

        // TODO §3.1 lane 6 — Indexer eligibility for discovery kinds in
        // `interest.shape.kinds` (kind:0 / kind:3 / 10000–19999).

        if out.is_empty() {
            // No author resolved and no AppRelay configured — surface as
            // Unroutable for the first author so the kernel toast points
            // at a concrete pubkey. Empty author set is a different shape
            // (wildcard) that the generic algorithm can't currently route
            // — also Unroutable, attributed to the empty string author.
            let pk = interest
                .shape
                .authors
                .iter()
                .next()
                .cloned()
                .unwrap_or_default();
            return Err(RoutingError::Unroutable(pk));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use nmp_core::planner::{
        InterestId, InterestLifecycle, InterestScope, InterestShape,
    };
    use nmp_core::substrate::{
        BlockedRelaySet, MailboxCache, ParsedRelayList, SessionKeySet,
    };

    use crate::InMemoryMailboxCache;

    fn pubkey() -> String {
        "alice".into()
    }

    fn unsigned() -> UnsignedEvent {
        UnsignedEvent {
            pubkey: pubkey(),
            kind: 1,
            tags: vec![],
            content: String::new(),
            created_at: 0,
        }
    }

    fn interest_for(authors: &[&str]) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(0),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: authors.iter().map(|s| (*s).into()).collect(),
                ..InterestShape::default()
            },
            hints: vec![],
            lifecycle: InterestLifecycle::OneShot,
        }
    }

    fn ctx<'a>(
        cache: &'a dyn MailboxCache,
        blocked: &'a BlockedRelaySet,
        explicit: Option<&'a [String]>,
        app_relays: &'a [String],
    ) -> RoutingContext<'a> {
        RoutingContext {
            active_account: None,
            session_keys: SessionKeySet {
                app_relays,
                ..SessionKeySet::default()
            },
            mailbox_cache: cache,
            blocked_relays: blocked,
            explicit_targets: explicit,
        }
    }

    #[test]
    fn publish_explicit_targets_skips_generic_algorithm() {
        let cache = InMemoryMailboxCache::new();
        let blocked = BlockedRelaySet::new();
        let explicit = vec!["wss://forced.example".to_string()];
        let app = vec!["wss://app.example".to_string()];
        let c = ctx(&cache, &blocked, Some(&explicit), &app);

        let router = GenericOutboxRouter::new();
        let r = router.route_publish(&unsigned(), &c).unwrap();
        let urls: Vec<&String> = r.urls().collect();

        assert_eq!(urls, vec![&"wss://forced.example".to_string()]);
        // AppRelay was configured but explicit_targets shortcut it.
        for sources in r.relays.values() {
            for s in sources {
                assert!(matches!(s, RoutingSource::ClassRouted { .. }));
            }
        }
    }

    #[test]
    fn publish_uses_nip65_write_set() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        cache.upsert(pubkey(), ParsedRelayList {
            write: vec!["wss://w.example".into()],
            both: vec!["wss://b.example".into()],
            ..ParsedRelayList::default()
        });
        let blocked = BlockedRelaySet::new();
        let app: Vec<String> = vec![];
        let c = ctx(&*cache, &blocked, None, &app);

        let router = GenericOutboxRouter::new();
        let r = router.route_publish(&unsigned(), &c).unwrap();

        let urls: std::collections::HashSet<&String> = r.urls().collect();
        let w = "wss://w.example".to_string();
        let b = "wss://b.example".to_string();
        assert!(urls.contains(&w));
        assert!(urls.contains(&b));
        for sources in r.relays.values() {
            assert!(sources.iter().any(|s| matches!(
                s,
                RoutingSource::Nip65 { direction: Direction::Write }
            )));
        }
    }

    #[test]
    fn publish_app_relay_fallback_when_no_nip65() {
        let cache = InMemoryMailboxCache::new();
        let blocked = BlockedRelaySet::new();
        let app = vec!["wss://app.example".to_string()];
        let c = ctx(&cache, &blocked, None, &app);

        let router = GenericOutboxRouter::new();
        let r = router.route_publish(&unsigned(), &c).unwrap();
        let urls: Vec<&String> = r.urls().collect();
        assert_eq!(urls, vec![&"wss://app.example".to_string()]);
        for sources in r.relays.values() {
            assert!(sources.iter().any(|s| matches!(
                s,
                RoutingSource::AppRelay { mode: AppRelayMode::Fallback }
            )));
        }
    }

    #[test]
    fn publish_unroutable_when_no_nip65_and_no_app_relay() {
        let cache = InMemoryMailboxCache::new();
        let blocked = BlockedRelaySet::new();
        let app: Vec<String> = vec![];
        let c = ctx(&cache, &blocked, None, &app);

        let router = GenericOutboxRouter::new();
        let err = router.route_publish(&unsigned(), &c).unwrap_err();
        assert_eq!(err, RoutingError::Unroutable(pubkey()));
    }

    #[test]
    fn publish_drops_blocked_relays() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        cache.upsert(pubkey(), ParsedRelayList {
            write: vec!["wss://blocked.example".into(), "wss://ok.example".into()],
            ..ParsedRelayList::default()
        });
        let mut blocked = BlockedRelaySet::new();
        blocked.insert("wss://blocked.example".into());
        let app: Vec<String> = vec![];
        let c = ctx(&*cache, &blocked, None, &app);

        let router = GenericOutboxRouter::new();
        let r = router.route_publish(&unsigned(), &c).unwrap();
        let urls: Vec<&String> = r.urls().collect();
        assert_eq!(urls, vec![&"wss://ok.example".to_string()]);
    }

    #[test]
    fn subscribe_uses_nip65_read_set_for_each_author() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        cache.upsert("alice".into(), ParsedRelayList {
            read: vec!["wss://alice-r.example".into()],
            ..ParsedRelayList::default()
        });
        cache.upsert("bob".into(), ParsedRelayList {
            both: vec!["wss://bob-b.example".into()],
            ..ParsedRelayList::default()
        });
        let blocked = BlockedRelaySet::new();
        let app: Vec<String> = vec![];
        let c = ctx(&*cache, &blocked, None, &app);

        let router = GenericOutboxRouter::new();
        let r = router
            .route_subscription(&interest_for(&["alice", "bob"]), &c)
            .unwrap();
        let urls: std::collections::HashSet<&String> = r.urls().collect();
        assert!(urls.contains(&"wss://alice-r.example".to_string()));
        assert!(urls.contains(&"wss://bob-b.example".to_string()));
    }

    #[test]
    fn subscribe_explicit_targets_shortcuts() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        cache.upsert("alice".into(), ParsedRelayList {
            read: vec!["wss://from-cache.example".into()],
            ..ParsedRelayList::default()
        });
        let blocked = BlockedRelaySet::new();
        let explicit = vec!["wss://override.example".to_string()];
        let app: Vec<String> = vec![];
        let c = ctx(&*cache, &blocked, Some(&explicit), &app);

        let router = GenericOutboxRouter::new();
        let r = router
            .route_subscription(&interest_for(&["alice"]), &c)
            .unwrap();
        let urls: Vec<&String> = r.urls().collect();
        assert_eq!(urls, vec![&"wss://override.example".to_string()]);
    }

    #[test]
    fn subscribe_unroutable_when_no_lane_resolves() {
        let cache = InMemoryMailboxCache::new();
        let blocked = BlockedRelaySet::new();
        let app: Vec<String> = vec![];
        let c = ctx(&cache, &blocked, None, &app);

        let router = GenericOutboxRouter::new();
        let err = router
            .route_subscription(&interest_for(&["alice"]), &c)
            .unwrap_err();
        assert_eq!(err, RoutingError::Unroutable("alice".into()));
    }

    #[test]
    fn router_satisfies_dyn_outbox_router_bound() {
        // Compile-test: confirm the impl is object-safe and the kernel can
        // hold it as Arc<dyn OutboxRouter>.
        let _: Box<dyn OutboxRouter> = Box::new(GenericOutboxRouter::new());
    }
}

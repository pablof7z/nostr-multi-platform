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

use std::sync::Arc;

use nmp_core::planner::LogicalInterest;
use nmp_core::substrate::{
    truncate_event_id, AppRelayMode, Direction, OutboxRouter, PublishTrace, RoutedRelaySet,
    RoutingContext, RoutingError, RoutingSource, RoutingTraceObserver, SubscriptionTrace,
    UnsignedEvent,
};

#[derive(Default)]
pub struct GenericOutboxRouter {
    /// V-51 phase 1 — optional trace observer fired after every successful
    /// `route_publish` / `route_subscription`. `None` by default; production
    /// composition binds the kernel's `RoutingTraceProjection` clone via
    /// [`Self::with_trace_observer`]. D8: the `Option::is_some` gate keeps
    /// the no-observer path zero-alloc.
    trace_observer: Option<Arc<dyn RoutingTraceObserver>>,
}

impl GenericOutboxRouter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install a [`RoutingTraceObserver`] (V-51 phase 1). The router fires
    /// `on_publish` / `on_subscription` after every successful resolution;
    /// `Err(RoutingError::*)` returns are NOT observed.
    #[must_use]
    pub fn with_trace_observer(mut self, obs: Arc<dyn RoutingTraceObserver>) -> Self {
        self.trace_observer = Some(obs);
        self
    }
}

impl OutboxRouter for GenericOutboxRouter {
    fn route_publish(
        &self,
        evt: &UnsignedEvent,
        ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        let explicit_targets_set = ctx.explicit_targets.is_some();
        let out = if let Some(explicit) = ctx.explicit_targets {
            // §3.4 — the override seam. Skip the generic algorithm.
            RoutedRelaySet::from_explicit(explicit, ctx.blocked_relays)
        } else {
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
            out
        };

        // V-51 — fire trace observer if installed (D8 gate).
        if let Some(obs) = self.trace_observer.as_ref() {
            obs.on_publish(
                PublishTrace {
                    kind: evt.kind,
                    author: evt.pubkey.clone(),
                    event_id_short: truncate_event_id(None),
                    explicit_targets_set,
                },
                &out,
            );
        }

        Ok(out)
    }

    fn route_subscription(
        &self,
        interest: &LogicalInterest,
        ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        let explicit_targets_set = ctx.explicit_targets.is_some();
        let out = if let Some(explicit) = ctx.explicit_targets {
            RoutedRelaySet::from_explicit(explicit, ctx.blocked_relays)
        } else {
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
            out
        };

        if let Some(obs) = self.trace_observer.as_ref() {
            obs.on_subscription(
                SubscriptionTrace {
                    interest_id: interest.id.0,
                    kinds: interest.shape.kinds.iter().copied().collect(),
                    authors_count: interest.shape.authors.len(),
                    explicit_targets_set,
                },
                &out,
            );
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

    #[derive(Default)]
    struct TestObserver {
        publishes: std::sync::Mutex<Vec<(PublishTrace, usize)>>,
        subscriptions: std::sync::Mutex<Vec<SubscriptionTrace>>,
    }

    impl RoutingTraceObserver for TestObserver {
        fn on_publish(&self, summary: PublishTrace, routed: &RoutedRelaySet) {
            self.publishes
                .lock()
                .unwrap()
                .push((summary, routed.relays.len()));
        }
        fn on_subscription(&self, summary: SubscriptionTrace, _routed: &RoutedRelaySet) {
            self.subscriptions.lock().unwrap().push(summary);
        }
    }

    #[test]
    fn trace_observer_fires_on_success_and_skips_unroutable() {
        // Two route_publish calls and one route_subscription against a single
        // router+observer instance. Asserts the observer fires once per
        // successful call (with the right trace payload), and NOT at all when
        // the router returns Unroutable.
        let cache = Arc::new(InMemoryMailboxCache::new());
        cache.upsert(pubkey(), ParsedRelayList {
            write: vec!["wss://w.example".into()],
            read: vec!["wss://r.example".into()],
            ..ParsedRelayList::default()
        });
        let blocked = BlockedRelaySet::new();
        let app: Vec<String> = vec![];
        let obs = Arc::new(TestObserver::default());
        let router = GenericOutboxRouter::new()
            .with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);

        // 1. Successful publish — explicit_targets unset.
        let c = ctx(&*cache, &blocked, None, &app);
        let _ = router.route_publish(&unsigned(), &c).unwrap();
        // 2. Successful publish — explicit_targets set.
        let explicit = vec!["wss://forced.example".to_string()];
        let c = ctx(&*cache, &blocked, Some(&explicit), &app);
        let _ = router.route_publish(&unsigned(), &c).unwrap();
        // 3. Successful subscription with a non-default interest id.
        let c = ctx(&*cache, &blocked, None, &app);
        let mut interest = interest_for(&["alice"]);
        interest.id = nmp_core::planner::InterestId(42);
        let _ = router.route_subscription(&interest, &c).unwrap();
        // 4. Unroutable publish (no cache, no app-relay) — observer MUST NOT fire.
        let empty_cache = InMemoryMailboxCache::new();
        let c = ctx(&empty_cache, &blocked, None, &app);
        let _ = router
            .route_publish(
                &UnsignedEvent {
                    pubkey: "ghost".into(),
                    ..unsigned()
                },
                &c,
            )
            .unwrap_err();

        let pubs = obs.publishes.lock().unwrap();
        assert_eq!(pubs.len(), 2, "two publish successes only");
        assert_eq!(pubs[0].0.kind, 1);
        assert_eq!(pubs[0].0.author, pubkey());
        assert!(!pubs[0].0.explicit_targets_set);
        assert!(pubs[1].0.explicit_targets_set);

        let subs = obs.subscriptions.lock().unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].interest_id, 42);
        assert_eq!(subs[0].authors_count, 1);
        assert!(!subs[0].explicit_targets_set);
    }
}

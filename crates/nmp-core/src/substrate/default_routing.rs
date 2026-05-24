//! Default substrate impls of [`MailboxCache`] and [`OutboxRouter`].
//!
//! Step 3 of the crate-boundary migration (`docs/architecture/crate-boundaries.md` §5)
//! cuts the kernel over to `Arc<dyn OutboxRouter>` + `Arc<dyn MailboxCache>`.
//! Production composition (`nmp-app-chirp`, `nmp-app-template`, etc.) is
//! expected to inject `nmp_router::GenericOutboxRouter` +
//! `nmp_router::InMemoryMailboxCache` because routing is a Layer-2
//! concern (the kernel is Layer 3 and cannot depend on Layer 2).
//!
//! But the kernel's default constructor (`Kernel::new`, used by every test
//! site inside `nmp-core` itself and by the bare-actor cold-start path
//! before any app-level wiring runs) needs *something* for the
//! `Arc<dyn …>` fields to point at — `nmp-core` can't depend on
//! `nmp-router`, so the default cannot be `GenericOutboxRouter`. These
//! impls are the fallback:
//!
//! - [`InMemoryMailboxCache`]: a `RwLock<HashMap<Pubkey, ParsedRelayList>>`.
//!   Identical data layout to `nmp_router::InMemoryMailboxCache` so the
//!   two are behaviourally indistinguishable; production composition
//!   injects the routing-crate version because that is where the type
//!   architecturally lives.
//! - [`Nip65WriteSetRouter`]: a minimal `OutboxRouter` that consults
//!   `MailboxCache` for the author's NIP-65 write set on publish and
//!   each author's read set on subscribe, plus AppRelay fallback. It is
//!   the same algorithm `nmp_router::GenericOutboxRouter` ships
//!   (because both impls implement the same trait against the same
//!   substrate types). When production composition swaps in
//!   `GenericOutboxRouter` the only difference is that the production
//!   impl is allowed to grow lanes 2/3/4/5/6 (hints, provenance,
//!   user-configured, class-routed, indexer); the default impl here
//!   stays minimal.
//!
//! # Lock-poisoning policy (D15)
//!
//! The cache below mirrors `nmp_router::cache`'s policy: every
//! `RwLock::read`/`write` `Err` (poisoned lock) degrades to "no data" /
//! silent no-op rather than panicking on the actor thread. See the module
//! docs of `nmp_router::cache` for the rationale.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::identity::UnsignedEvent;
use super::routing::{
    AppRelayMode, Direction, MailboxCache, OutboxRouter, ParsedRelayList, Pubkey, RelayUrl,
    RoutedRelaySet, RoutingContext, RoutingError, RoutingSource,
};
use super::routing_trace::{
    truncate_event_id, PublishTrace, RoutingTraceObserver, SubscriptionTrace,
};
use crate::planner::LogicalInterest;

// ─── InMemoryMailboxCache (default) ──────────────────────────────────────────

/// Default `MailboxCache` impl used by `Kernel::new`. Production
/// composition is expected to inject `nmp_router::InMemoryMailboxCache`
/// instead; that crate is where the kind:10002 ingest parser writes from
/// and where the type architecturally lives.
#[derive(Default)]
pub struct InMemoryMailboxCache {
    inner: RwLock<HashMap<Pubkey, ParsedRelayList>>,
}

impl InMemoryMailboxCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Diagnostic: number of authors currently cached. Returns `0` on a
    /// poisoned lock (degrade-gracefully policy — see module docs).
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().map(|g| g.len()).unwrap_or(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl MailboxCache for InMemoryMailboxCache {
    fn read_relays(&self, author: &Pubkey) -> Option<Vec<RelayUrl>> {
        self.inner
            .read()
            .ok()
            .and_then(|g| g.get(author).map(ParsedRelayList::read_set))
    }

    fn write_relays(&self, author: &Pubkey) -> Option<Vec<RelayUrl>> {
        self.inner
            .read()
            .ok()
            .and_then(|g| g.get(author).map(ParsedRelayList::write_set))
    }

    fn snapshot(&self, author: &Pubkey) -> Option<ParsedRelayList> {
        self.inner.read().ok().and_then(|g| g.get(author).cloned())
    }

    fn snapshot_all(&self) -> Vec<(Pubkey, ParsedRelayList)> {
        self.inner
            .read()
            .map(|g| g.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    fn remove(&self, author: &Pubkey) {
        if let Ok(mut g) = self.inner.write() {
            g.remove(author);
        }
    }

    fn upsert(&self, author: Pubkey, list: ParsedRelayList) {
        if let Ok(mut g) = self.inner.write() {
            g.insert(author, list);
        }
    }
}

// ─── Nip65WriteSetRouter (default) ───────────────────────────────────────────

/// Default `OutboxRouter` impl used by `Kernel::new`. Mirrors
/// `nmp_router::GenericOutboxRouter`'s algorithm: `explicit_targets`
/// override, lane 1 (NIP-65), lane 7 (AppRelay fallback), blocked-relay
/// subtractive post-filter. Lanes 2/3/4/5/6 are not implemented here —
/// production composition is expected to inject
/// `nmp_router::GenericOutboxRouter` (the architectural home for the
/// generic router) which is allowed to grow those lanes.
#[derive(Default)]
pub struct Nip65WriteSetRouter {
    /// V-51 phase 1 — optional observer fired after every successful
    /// `route_publish` / `route_subscription` call. `None` by default; the
    /// kernel binds an `Arc<RoutingTraceProjection>` clone via
    /// [`Self::with_trace_observer`] at composition time. D8: the
    /// `Option::is_some` gate keeps the no-observer path zero-alloc.
    trace_observer: Option<Arc<dyn RoutingTraceObserver>>,
}

impl Nip65WriteSetRouter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install a [`RoutingTraceObserver`] (V-51 phase 1). The router fires
    /// `on_publish` / `on_subscription` after every successful resolution;
    /// `Err(RoutingError::*)` returns are NOT observed (the unroutable case
    /// is already surfaced via `CompiledPlan::unroutable_authors`).
    #[must_use]
    pub fn with_trace_observer(mut self, obs: Arc<dyn RoutingTraceObserver>) -> Self {
        self.trace_observer = Some(obs);
        self
    }
}

impl OutboxRouter for Nip65WriteSetRouter {
    fn route_publish(
        &self,
        evt: &UnsignedEvent,
        ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        let explicit_targets_set = ctx.explicit_targets.is_some();
        let out = if let Some(explicit) = ctx.explicit_targets {
            RoutedRelaySet::from_explicit(explicit, ctx.blocked_relays)
        } else {
            let mut out = RoutedRelaySet::new();

            if let Some(writes) = ctx.mailbox_cache.write_relays(&evt.pubkey) {
                for url in writes {
                    if ctx.blocked_relays.contains(&url) {
                        continue;
                    }
                    out.add(
                        url,
                        RoutingSource::Nip65 {
                            direction: Direction::Write,
                        },
                    );
                }
            }

            if out.is_empty() {
                for url in ctx.session_keys.app_relays.iter() {
                    if ctx.blocked_relays.contains(url) {
                        continue;
                    }
                    out.add(
                        url.clone(),
                        RoutingSource::AppRelay {
                            mode: AppRelayMode::Fallback,
                        },
                    );
                }
            }

            if out.is_empty() {
                return Err(RoutingError::Unroutable(evt.pubkey.clone()));
            }
            out
        };

        // V-51 — fire trace observer if installed. `Option::is_some` gate is
        // the D8 zero-alloc no-observer path.
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

            for author in &interest.shape.authors {
                if let Some(reads) = ctx.mailbox_cache.read_relays(author) {
                    for url in reads {
                        if ctx.blocked_relays.contains(&url) {
                            continue;
                        }
                        out.add(
                            url,
                            RoutingSource::Nip65 {
                                direction: Direction::Read,
                            },
                        );
                    }
                }
            }

            if out.is_empty() {
                for url in ctx.session_keys.app_relays.iter() {
                    if ctx.blocked_relays.contains(url) {
                        continue;
                    }
                    out.add(
                        url.clone(),
                        RoutingSource::AppRelay {
                            mode: AppRelayMode::Fallback,
                        },
                    );
                }
            }

            if out.is_empty() {
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
    use std::sync::Mutex;

    use crate::planner::interest::{
        InterestId, InterestLifecycle, InterestScope, InterestShape,
    };
    use crate::substrate::BlockedRelaySet;

    #[test]
    fn cache_round_trips_via_substrate_trait() {
        let cache = InMemoryMailboxCache::new();
        let alice: Pubkey = "alice".into();
        assert!(!cache.known(&alice));
        cache.upsert(
            alice.clone(),
            ParsedRelayList {
                read: vec!["wss://r.example".into()],
                write: vec!["wss://w.example".into()],
                both: vec!["wss://b.example".into()],
            },
        );
        assert!(cache.known(&alice));
        let snap = cache.snapshot(&alice).expect("snapshot present");
        assert_eq!(snap.read, vec!["wss://r.example"]);
        assert_eq!(snap.write, vec!["wss://w.example"]);
        assert_eq!(snap.both, vec!["wss://b.example"]);
    }

    #[derive(Default)]
    struct CountingObserver {
        publishes: Mutex<Vec<(PublishTrace, RoutedRelaySet)>>,
        subscriptions: Mutex<Vec<(SubscriptionTrace, RoutedRelaySet)>>,
    }

    impl RoutingTraceObserver for CountingObserver {
        fn on_publish(&self, summary: PublishTrace, routed: &RoutedRelaySet) {
            self.publishes
                .lock()
                .unwrap()
                .push((summary, routed.clone()));
        }
        fn on_subscription(&self, summary: SubscriptionTrace, routed: &RoutedRelaySet) {
            self.subscriptions
                .lock()
                .unwrap()
                .push((summary, routed.clone()));
        }
    }

    fn unsigned(pubkey: &str, kind: u32) -> UnsignedEvent {
        UnsignedEvent {
            pubkey: pubkey.into(),
            kind,
            tags: vec![],
            content: String::new(),
            created_at: 0,
        }
    }

    fn interest_for(id: u64, authors: &[&str], kinds: &[u32]) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: authors.iter().map(|s| (*s).into()).collect(),
                kinds: kinds.iter().copied().collect(),
                ..InterestShape::default()
            },
            hints: vec![],
            lifecycle: InterestLifecycle::OneShot,
        }
    }

    #[test]
    fn trace_observer_fires_on_publish_with_lane_attribution() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        cache.upsert(
            "alice".into(),
            ParsedRelayList {
                write: vec!["wss://w.example".into()],
                ..ParsedRelayList::default()
            },
        );
        let blocked = BlockedRelaySet::new();
        let app: Vec<String> = vec![];
        let ctx = RoutingContext {
            active_account: None,
            session_keys: crate::substrate::SessionKeySet {
                app_relays: &app,
                ..crate::substrate::SessionKeySet::default()
            },
            mailbox_cache: &*cache,
            blocked_relays: &blocked,
            explicit_targets: None,
        };

        let obs = Arc::new(CountingObserver::default());
        let router =
            Nip65WriteSetRouter::new().with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
        let _ = router.route_publish(&unsigned("alice", 1), &ctx).unwrap();

        let pubs = obs.publishes.lock().unwrap();
        assert_eq!(pubs.len(), 1);
        let (summary, routed) = &pubs[0];
        assert_eq!(summary.kind, 1);
        assert_eq!(summary.author, "alice");
        assert!(!summary.explicit_targets_set);
        // Lane attribution survives observation.
        let sources = &routed.relays[&"wss://w.example".to_string()];
        assert!(sources.contains(&RoutingSource::Nip65 {
            direction: Direction::Write
        }));
    }

    #[test]
    fn trace_observer_fires_on_subscription_with_interest_id() {
        let cache = Arc::new(InMemoryMailboxCache::new());
        cache.upsert(
            "alice".into(),
            ParsedRelayList {
                read: vec!["wss://r.example".into()],
                ..ParsedRelayList::default()
            },
        );
        let blocked = BlockedRelaySet::new();
        let app: Vec<String> = vec![];
        let ctx = RoutingContext {
            active_account: None,
            session_keys: crate::substrate::SessionKeySet {
                app_relays: &app,
                ..crate::substrate::SessionKeySet::default()
            },
            mailbox_cache: &*cache,
            blocked_relays: &blocked,
            explicit_targets: None,
        };

        let obs = Arc::new(CountingObserver::default());
        let router =
            Nip65WriteSetRouter::new().with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
        let interest = interest_for(7, &["alice"], &[1, 6]);
        let _ = router.route_subscription(&interest, &ctx).unwrap();

        let subs = obs.subscriptions.lock().unwrap();
        assert_eq!(subs.len(), 1);
        let (summary, _) = &subs[0];
        assert_eq!(summary.interest_id, 7);
        assert_eq!(summary.authors_count, 1);
        assert!(summary.kinds.contains(&1));
        assert!(summary.kinds.contains(&6));
        assert!(!summary.explicit_targets_set);
    }

    #[test]
    fn trace_observer_not_fired_on_unroutable_error() {
        let cache = InMemoryMailboxCache::new();
        let blocked = BlockedRelaySet::new();
        let app: Vec<String> = vec![];
        let ctx = RoutingContext {
            active_account: None,
            session_keys: crate::substrate::SessionKeySet {
                app_relays: &app,
                ..crate::substrate::SessionKeySet::default()
            },
            mailbox_cache: &cache,
            blocked_relays: &blocked,
            explicit_targets: None,
        };

        let obs = Arc::new(CountingObserver::default());
        let router =
            Nip65WriteSetRouter::new().with_trace_observer(obs.clone() as Arc<dyn RoutingTraceObserver>);
        let _ = router.route_publish(&unsigned("ghost", 1), &ctx).unwrap_err();

        assert!(obs.publishes.lock().unwrap().is_empty());
    }

    #[test]
    fn remove_clears_entry() {
        let cache = InMemoryMailboxCache::new();
        let alice: Pubkey = "alice".into();
        cache.upsert(
            alice.clone(),
            ParsedRelayList {
                write: vec!["wss://w.example".into()],
                ..ParsedRelayList::default()
            },
        );
        assert!(cache.known(&alice));
        cache.remove(&alice);
        assert!(!cache.known(&alice));
    }
}

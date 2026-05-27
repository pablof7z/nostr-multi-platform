//! Trait-shape-only [`OutboxRouter`] / [`MailboxCache`] impls.
//!
//! Substrate-honest debt B (2026-05-24): the previous default-routing
//! module shipped 484 LOC that re-implemented `nmp_router::GenericOutboxRouter` +
//! `nmp_router::InMemoryMailboxCache` byte-for-byte inside `nmp-core` to
//! satisfy the kernel's `Arc<dyn …>` slots before the production factory
//! swap. That was duplicate algorithm: `nmp-core` (Layer 3) cannot depend
//! on `nmp-router` (Layer 2), so the only way to keep a default that *also*
//! routed was to copy the algorithm — which is exactly what made it debt.
//!
//! This module replaces the duplicate with two trivial trait-shape-only
//! stubs:
//!
//! - [`EmptyOutboxRouter`]: every `route_publish` / `route_subscription`
//!   call returns `RoutingError::Unroutable`. Production composition
//!   immediately swaps this for `nmp_router::GenericOutboxRouter` via
//!   [`crate::NmpApp::set_routing_substrate`]; tests that exercise real
//!   routing call `Kernel::set_routing` with a real router.
//! - [`EmptyMailboxCache`]: every read returns `None`; `upsert` / `remove`
//!   are silent no-ops. Production composition swaps in
//!   `nmp_router::InMemoryMailboxCache`. Tests that need a real backing
//!   cache (the kind:10002 ingest tests, the planner-side compiler tests
//!   in this crate) use [`TestInMemoryMailboxCache`] under
//!   `cfg(any(test, feature = "test-support"))`.
//!
//! ## Why an Empty default at all?
//!
//! `Kernel::new` / `Kernel::with_storage_path` must produce a kernel whose
//! `outbox_router` / `mailbox_cache` slots are populated before the actor
//! reads the per-app `RoutingSubstrateSlot` and applies the factory's
//! `(router, cache)` pair via `Kernel::set_routing`. The Empty defaults
//! fill that window. They are NEVER consulted in a production session
//! that registers a routing factory (chirp does; the template does;
//! nmp-repl does).

#[cfg(any(test, feature = "test-support"))]
use std::sync::Arc;

use super::identity::UnsignedEvent;
use super::routing::{
    MailboxCache, OutboxRouter, ParsedRelayList, Pubkey, RelayUrl, RoutedRelaySet, RoutingContext,
    RoutingError,
};
use crate::planner::LogicalInterest;

// ─── EmptyOutboxRouter ───────────────────────────────────────────────────────

/// Trait-shape-only [`OutboxRouter`]. Returns `Unroutable` for every call.
/// Production composition replaces this via
/// [`crate::NmpApp::set_routing_substrate`] before the kernel issues any
/// routing decision; tests that exercise routing call `Kernel::set_routing`
/// with `nmp_router::GenericOutboxRouter` directly.
#[derive(Default)]
pub struct EmptyOutboxRouter;

impl EmptyOutboxRouter {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl OutboxRouter for EmptyOutboxRouter {
    fn route_publish(
        &self,
        evt: &UnsignedEvent,
        _ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        Err(RoutingError::Unroutable(evt.pubkey.clone()))
    }

    fn route_subscription(
        &self,
        interest: &LogicalInterest,
        _ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        let pk = interest
            .shape
            .authors
            .iter()
            .next()
            .cloned()
            .unwrap_or_default();
        Err(RoutingError::Unroutable(pk))
    }
}

// ─── EmptyMailboxCache ───────────────────────────────────────────────────────

/// Trait-shape-only [`MailboxCache`]. Every read returns `None`; writes are
/// silent no-ops. Production composition swaps in
/// `nmp_router::InMemoryMailboxCache`.
#[derive(Default)]
pub struct EmptyMailboxCache;

impl EmptyMailboxCache {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl MailboxCache for EmptyMailboxCache {
    fn read_relays(&self, _author: &Pubkey) -> Option<Vec<RelayUrl>> {
        None
    }

    fn write_relays(&self, _author: &Pubkey) -> Option<Vec<RelayUrl>> {
        None
    }

    fn snapshot(&self, _author: &Pubkey) -> Option<ParsedRelayList> {
        None
    }

    fn snapshot_all(&self) -> Vec<(Pubkey, ParsedRelayList)> {
        Vec::new()
    }

    fn remove(&self, _author: &Pubkey) {}

    fn upsert(&self, _author: Pubkey, _list: ParsedRelayList) {}
}

// ─── TestInMemoryMailboxCache (test/test-support only) ───────────────────────

/// Real in-memory [`MailboxCache`] backing for tests. Exists ONLY so the
/// dozens of `cargo test -p nmp-core` cases that ingest a kind:10002 and
/// then assert via `kernel.mailbox_cache().known()` keep working without
/// every one of them having to inject `nmp_router::InMemoryMailboxCache`
/// from a downstream crate (which `nmp-core` cannot depend on — layering).
///
/// Production paths NEVER construct this: the actor's
/// `routing_substrate_slot` factory installs `nmp_router::InMemoryMailboxCache`
/// before any kind:10002 is ingested.
///
/// Lock-poisoning policy: every `RwLock::read`/`write` `Err` degrades to
/// "no data" / silent no-op rather than panicking on the actor thread —
/// same policy `nmp_router::cache::InMemoryMailboxCache` follows (D15).
#[cfg(any(test, feature = "test-support"))]
pub struct TestInMemoryMailboxCache {
    inner: std::sync::RwLock<std::collections::HashMap<Pubkey, ParsedRelayList>>,
}

#[cfg(any(test, feature = "test-support"))]
impl Default for TestInMemoryMailboxCache {
    fn default() -> Self {
        Self {
            inner: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl TestInMemoryMailboxCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct as `Arc<dyn MailboxCache>` for direct injection via
    /// [`crate::kernel::Kernel::set_routing`].
    #[must_use]
    pub fn new_arc() -> Arc<dyn MailboxCache> {
        Arc::new(Self::default())
    }
}

#[cfg(any(test, feature = "test-support"))]
impl MailboxCache for TestInMemoryMailboxCache {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::interest::{InterestId, InterestLifecycle, InterestScope, InterestShape};
    use crate::substrate::BlockedRelaySet;

    fn unsigned(pubkey: &str, kind: u32) -> UnsignedEvent {
        UnsignedEvent {
            pubkey: pubkey.into(),
            kind,
            tags: vec![],
            content: String::new(),
            created_at: 0,
        }
    }

    fn interest_for(id: u64, authors: &[&str]) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::Global,
            shape: InterestShape {
                authors: authors.iter().map(|s| (*s).into()).collect(),
                ..InterestShape::default()
            },
            hints: vec![],
            lifecycle: InterestLifecycle::OneShot,
            is_indexer_discovery: false,
        }
    }

    #[test]
    fn empty_router_returns_unroutable_on_publish() {
        let cache = EmptyMailboxCache::new();
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
        let router = EmptyOutboxRouter::new();
        let err = router
            .route_publish(&unsigned("alice", 1), &ctx)
            .unwrap_err();
        assert_eq!(err, RoutingError::Unroutable("alice".into()));
    }

    #[test]
    fn empty_router_returns_unroutable_on_subscription() {
        let cache = EmptyMailboxCache::new();
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
        let router = EmptyOutboxRouter::new();
        let err = router
            .route_subscription(&interest_for(1, &["alice"]), &ctx)
            .unwrap_err();
        assert_eq!(err, RoutingError::Unroutable("alice".into()));
    }

    #[test]
    fn empty_cache_reads_return_none() {
        let cache = EmptyMailboxCache::new();
        let alice: Pubkey = "alice".into();
        assert!(cache.read_relays(&alice).is_none());
        assert!(cache.write_relays(&alice).is_none());
        assert!(cache.snapshot(&alice).is_none());
        assert!(cache.snapshot_all().is_empty());
        assert!(!cache.known(&alice));
        // upsert + remove are silent no-ops on the empty cache.
        cache.upsert(
            alice.clone(),
            ParsedRelayList {
                read: vec!["wss://r.example".into()],
                ..ParsedRelayList::default()
            },
        );
        assert!(!cache.known(&alice));
        cache.remove(&alice);
    }

    #[test]
    fn test_in_memory_cache_round_trips() {
        let cache = TestInMemoryMailboxCache::new();
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
        let snap = cache.snapshot(&alice).expect("present");
        assert_eq!(snap.read, vec!["wss://r.example"]);
        assert_eq!(snap.write, vec!["wss://w.example"]);
        assert_eq!(snap.both, vec!["wss://b.example"]);
        cache.remove(&alice);
        assert!(!cache.known(&alice));
    }
}

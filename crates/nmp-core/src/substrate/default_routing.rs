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

use std::collections::HashMap;
use std::sync::RwLock;

use super::identity::UnsignedEvent;
use super::routing::{
    AppRelayMode, Direction, MailboxCache, OutboxRouter, ParsedRelayList, Pubkey, RelayUrl,
    RoutedRelaySet, RoutingContext, RoutingError, RoutingSource,
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

    /// Diagnostic: number of authors currently cached.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().expect("RwLock poisoned").len()
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
            .expect("RwLock poisoned")
            .get(author)
            .map(ParsedRelayList::read_set)
    }

    fn write_relays(&self, author: &Pubkey) -> Option<Vec<RelayUrl>> {
        self.inner
            .read()
            .expect("RwLock poisoned")
            .get(author)
            .map(ParsedRelayList::write_set)
    }

    fn snapshot(&self, author: &Pubkey) -> Option<ParsedRelayList> {
        self.inner
            .read()
            .expect("RwLock poisoned")
            .get(author)
            .cloned()
    }

    fn snapshot_all(&self) -> Vec<(Pubkey, ParsedRelayList)> {
        self.inner
            .read()
            .expect("RwLock poisoned")
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn remove(&self, author: &Pubkey) {
        self.inner.write().expect("RwLock poisoned").remove(author);
    }

    fn upsert(&self, author: Pubkey, list: ParsedRelayList) {
        self.inner
            .write()
            .expect("RwLock poisoned")
            .insert(author, list);
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
pub struct Nip65WriteSetRouter;

impl Nip65WriteSetRouter {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for Nip65WriteSetRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl OutboxRouter for Nip65WriteSetRouter {
    fn route_publish(
        &self,
        evt: &UnsignedEvent,
        ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        if let Some(explicit) = ctx.explicit_targets {
            return Ok(RoutedRelaySet::from_explicit(explicit, ctx.blocked_relays));
        }

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
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

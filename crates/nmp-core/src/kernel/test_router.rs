//! Test-only [`OutboxRouter`] + `Kernel::new_for_test` constructor.
//!
//! Substrate Debt B fallout: `Kernel::new` installs
//! [`crate::substrate::EmptyOutboxRouter`] (every `route_publish` /
//! `route_subscription` returns `RoutingError::Unroutable`), so kernel tests
//! that exercise real routing (open_author REQ fan-out, open_thread, profile
//! claims, hashtag firehose, AUTH-paused partitioning, view-close eviction,
//! thread-id hydration, etc.) must immediately swap in a router that
//! actually consults [`crate::substrate::MailboxCache`] and the session-key
//! indexer/app-relay sets.
//!
//! `nmp-core` (Layer 3) cannot link `nmp-router` (Layer 2) in production —
//! that would invert the §3 crate-boundary arrow — and adding `nmp-router`
//! as a dev-dep of `nmp-core` triggers rustc's "multiple different versions
//! of crate nmp_core in the dependency graph" trait-coherence failure (the
//! test binary's view of the `OutboxRouter` trait diverges from the view
//! `nmp-router` was compiled against because the dev-dep cycle yields two
//! different rmeta compilations of nmp-core). So the test-only router is
//! kept in-crate.
//!
//! The lanes implemented below mirror lanes 1, 6 (discovery indexer), and 7
//! (AppRelay fallback) of `nmp_router::GenericOutboxRouter` — the exact
//! lanes the kernel unit tests assert on. This is an acknowledged minor
//! algorithm duplication; Debt B's full elimination still holds for
//! production code, where composition installs
//! `nmp_router::GenericOutboxRouter` via
//! `NmpApp::set_routing_substrate` -> `Kernel::set_routing`. Both routers
//! flow through the same `OutboxRouter` trait seam, so the kernel hot-path
//! is identical across test and production.

use std::sync::Arc;

use super::Kernel;
use crate::planner::LogicalInterest;
use crate::substrate::{
    AppRelayMode, Direction, OutboxRouter, RoutedRelaySet, RoutingContext, RoutingError,
    RoutingSource, UnsignedEvent,
};

/// Spec §3.1 lane 6 discovery kinds: kind:0 (profile metadata), kind:3
/// (contacts), kind:10000–19999 (NIP-51 lists, INCLUDING kind:10002).
#[inline]
fn is_discovery_kind(kind: u32) -> bool {
    kind == 0 || kind == 3 || (10_000..20_000).contains(&kind)
}

/// Test-only [`OutboxRouter`] mirroring the subset of
/// `nmp_router::GenericOutboxRouter` the kernel unit tests assert on
/// (lanes 1, 6, 7). See module docs.
pub(crate) struct TestOutboxRouter;

impl TestOutboxRouter {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl OutboxRouter for TestOutboxRouter {
    fn route_publish(
        &self,
        evt: &UnsignedEvent,
        ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError> {
        if let Some(explicit) = ctx.explicit_targets {
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
        // Lane 6 — Indexer ALWAYS-ON for discovery kinds.
        if is_discovery_kind(evt.kind) {
            for url in ctx.session_keys.indexer_relays.iter() {
                if ctx.blocked_relays.contains(url) {
                    continue;
                }
                out.add(url.clone(), RoutingSource::Indexer);
            }
        }
        // Lane 7 — AppRelay fallback when no earlier lane resolved anything.
        if out.is_empty() {
            for url in ctx.session_keys.app_relays.iter() {
                if ctx.blocked_relays.contains(url) {
                    continue;
                }
                out.add(
                    url.clone(),
                    RoutingSource::AppRelay { mode: AppRelayMode::Fallback },
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
        // Lane 6 — Indexer for any discovery kind in the interest shape.
        if interest.shape.kinds.iter().any(|k| is_discovery_kind(*k)) {
            for url in ctx.session_keys.indexer_relays.iter() {
                if ctx.blocked_relays.contains(url) {
                    continue;
                }
                out.add(url.clone(), RoutingSource::Indexer);
            }
        }
        // Lane 7 — AppRelay fallback.
        if out.is_empty() {
            for url in ctx.session_keys.app_relays.iter() {
                if ctx.blocked_relays.contains(url) {
                    continue;
                }
                out.add(
                    url.clone(),
                    RoutingSource::AppRelay { mode: AppRelayMode::Fallback },
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

impl Kernel {
    /// Construct a fresh [`Kernel`] with `visible_limit` and immediately
    /// install [`TestOutboxRouter`] via [`Kernel::set_routing`] so the
    /// substrate trait wiring resolves routing decisions like production.
    /// Production composition installs `nmp_router::GenericOutboxRouter`
    /// through the same `set_routing` seam; the existing
    /// `TestInMemoryMailboxCache` default already covers the cache side.
    ///
    /// Use this in tests that exercise the routing seam (`open_author`,
    /// `open_thread`, `open_firehose_tag`, profile claims, AUTH gating,
    /// etc.) instead of the bare [`Kernel::new`].
    pub(crate) fn new_for_test(visible_limit: usize) -> Self {
        let mut kernel = Self::new(visible_limit);
        kernel.set_routing(
            Arc::new(TestOutboxRouter::new()),
            kernel.mailbox_cache_arc(),
        );
        kernel
    }
}

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

/// Spec §3.1 lane 6 discovery kinds: kind:0 (profile metadata), kind:3
/// (contacts), kind:10000–19999 (NIP-51 lists, INCLUDING kind:10002
/// relay-list). The indexer lane is ALWAYS-ON for these kinds — it
/// stacks on top of the per-author NIP-65 set so that newer versions of
/// these replaceable events published to relays NOT in the cached set
/// can still be discovered (defeating the kind:10002 self-sealing
/// loop).
#[inline]
fn is_discovery_kind(kind: u32) -> bool {
    kind == 0 || kind == 3 || (10_000..20_000).contains(&kind)
}

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

            // Lane 6 — Indexer (ALWAYS-ON for discovery kinds): kind:0
            // profile, kind:3 contacts, kind:10000–19999 NIP-51 lists
            // (INCLUDING kind:10002 relay-list itself). R+W symmetric per
            // router spec §3.1: discovery kinds publish to indexers, not
            // just consume from them. This lane STACKS on top of lane 1;
            // it is precisely what defeats the "self-sealing loop" where
            // a cached stale kind:10002 keeps routing kind:10002 refreshes
            // only to the stale relays — by always also asking the
            // operator's indexers we let a newer kind:10002 published on
            // a different relay still arrive.
            if is_discovery_kind(evt.kind) {
                for url in ctx.session_keys.indexer_relays.iter() {
                    if ctx.blocked_relays.contains(url) {
                        continue;
                    }
                    out.add(url.clone(), RoutingSource::Indexer);
                }
            }

            // Lane 7 — AppRelay fallback when no earlier lane resolved
            // anything (lane 1 empty AND lane 6 didn't fire / had no
            // indexer URLs configured).
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

            // Lane 6 — Indexer (ALWAYS-ON for any discovery kind in the
            // interest shape): kind:0 profile, kind:3 contacts, kind:
            // 10000–19999 NIP-51 lists, INCLUDING kind:10002 relay-list
            // itself. Per router spec §3.1 lane 6 the indexer set STACKS
            // on top of lane 1 — it is the structural defeat of the
            // kind:10002 self-sealing loop (a cached stale kind:10002
            // would otherwise keep refreshing only against the stale
            // relays; asking the operator's indexers in parallel lets a
            // newer kind:10002 published elsewhere still arrive).
            if interest.shape.kinds.iter().any(|k| is_discovery_kind(*k)) {
                for url in ctx.session_keys.indexer_relays.iter() {
                    if ctx.blocked_relays.contains(url) {
                        continue;
                    }
                    out.add(url.clone(), RoutingSource::Indexer);
                }
            }

            // Lane 7 — AppRelay fallback when no earlier lane resolved
            // anything.
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
#[path = "router/tests.rs"]
mod tests;

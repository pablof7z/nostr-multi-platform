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
//! The lanes implemented below mirror lanes 1, 2 (hint), 4
//! (UserConfigured), 5 (ClassRouted attribution refinement on
//! `explicit_targets`), 6 (discovery indexer), and 7 (AppRelay fallback)
//! of `nmp_router::GenericOutboxRouter` — the same coverage as
//! production routing. Lane 3 (Provenance) is subscribe-only and is
//! also covered. This is an acknowledged minor algorithm duplication;
//! Debt B's full elimination still holds for production code, where
//! composition installs `nmp_router::GenericOutboxRouter` via
//! `NmpApp::set_routing_substrate` -> `Kernel::set_routing`. Both routers
//! flow through the same `OutboxRouter` trait seam, so the kernel hot-path
//! is identical across test and production.

use std::sync::Arc;

use super::Kernel;
use crate::planner::{HintSource, LogicalInterest};
use crate::substrate::{
    AppRelayMode, BlockedRelaySet, ClassRoutingPath, Direction, EventClass, OutboxRouter,
    RoutedRelaySet, RoutingContext, RoutingError, RoutingSource, UnsignedEvent,
    UserConfiguredCategory,
};

/// Spec §3.1 lane 6 discovery kinds: kind:0 (profile metadata), kind:3
/// (contacts), kind:10000–19999 (NIP-51 lists, INCLUDING kind:10002).
#[inline]
fn is_discovery_kind(kind: u32) -> bool {
    kind == 0 || kind == 3 || (10_000..20_000).contains(&kind)
}

/// Tag keys whose third column carries a lane-2 relay hint (mirrors
/// `nmp_router::router::HINT_TAG_KEYS`).
const HINT_TAG_KEYS: &[&str] = &["e", "p", "a", "q"];

/// Lift relay-hint URLs from `tags` for lane 2 attribution. Returns
/// deduped owned URLs in tag-document order; empty hint slots and
/// non-hint tag keys are skipped.
fn relay_hints_from_tags(tags: &[Vec<String>]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let Some(key) = tag.first() else { continue };
        if !HINT_TAG_KEYS.contains(&key.as_str()) {
            continue;
        }
        let Some(hint) = tag.get(2) else { continue };
        if hint.is_empty() {
            continue;
        }
        if !out.iter().any(|u| u == hint) {
            out.push(hint.clone());
        }
    }
    out
}

/// Map an event kind to its [`EventClass`] for lane-5 attribution
/// (mirrors `nmp_router::router::classify_kind`).
fn classify_kind(kind: u32) -> EventClass {
    match kind {
        818 | 30_818 | 30_819 => EventClass::Wiki,
        1234 | 31_234 => EventClass::Draft,
        _ => EventClass::Other(String::from("explicit")),
    }
}

/// Build the lane-5 explicit-set with the right `EventClass` for `kind`.
fn explicit_set_for_kind(urls: &[String], blocked: &BlockedRelaySet, kind: u32) -> RoutedRelaySet {
    let class = classify_kind(kind);
    let mut out = RoutedRelaySet::new();
    for url in urls {
        if blocked.contains(url) {
            continue;
        }
        out.add(
            url.clone(),
            RoutingSource::ClassRouted {
                class: class.clone(),
                via: ClassRoutingPath::Explicit,
            },
        );
    }
    out
}

/// Test-only [`OutboxRouter`] mirroring
/// `nmp_router::GenericOutboxRouter` lane-by-lane. See module docs.
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
            // Lane 5 — refine the EventClass for the publish kind.
            return Ok(explicit_set_for_kind(
                explicit,
                ctx.blocked_relays,
                evt.kind,
            ));
        }
        let mut out = RoutedRelaySet::new();
        // Lane 1 — author's NIP-65 write set.
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
        // Lane 2 — relay-hint tags on `evt` (e/p/a/q position 2).
        for url in relay_hints_from_tags(&evt.tags) {
            if ctx.blocked_relays.contains(&url) {
                continue;
            }
            out.add(url, RoutingSource::Hint);
        }
        // Lane 4 — UserConfigured active-account write (only when
        // publishing as the active account).
        if let Some(active) = ctx.active_account {
            if active == &evt.pubkey {
                for url in ctx.session_keys.active_write.iter() {
                    if ctx.blocked_relays.contains(url) {
                        continue;
                    }
                    out.add(
                        url.clone(),
                        RoutingSource::UserConfigured(UserConfiguredCategory::ActiveAccountWrite),
                    );
                }
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
            // Lane 5 — no kind available on subscriptions; fall through to
            // the substrate's generic explicit-set (attributes via
            // `EventClass::Other("explicit")`). The generic router does the
            // same for `route_subscription`.
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
                    out.add(
                        url,
                        RoutingSource::Nip65 {
                            direction: Direction::Read,
                        },
                    );
                }
            }
        }
        // Lanes 2 + 3 — relay hints on the interest.
        for hint in &interest.hints {
            if ctx.blocked_relays.contains(&hint.url) {
                continue;
            }
            let lane = match hint.source {
                HintSource::EventTag { .. } => RoutingSource::Hint,
                HintSource::Provenance { .. } => RoutingSource::Provenance,
                HintSource::UserConfigured => {
                    RoutingSource::UserConfigured(UserConfiguredCategory::Debug)
                }
            };
            out.add(hint.url.clone(), lane);
        }
        // Lane 4 — UserConfigured active-account read (active in scope).
        if let Some(active) = ctx.active_account {
            let active_in_scope =
                interest.shape.authors.is_empty() || interest.shape.authors.contains(active);
            if active_in_scope {
                for url in ctx.session_keys.active_read.iter() {
                    if ctx.blocked_relays.contains(url) {
                        continue;
                    }
                    out.add(
                        url.clone(),
                        RoutingSource::UserConfigured(UserConfiguredCategory::ActiveAccountRead),
                    );
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
        // Seed the well-known test relays explicitly. Production no longer
        // hardcodes a relay fallback (the app declares its relay set through
        // `NmpAppBuilder` / `ActorCommand::Start { initial_relays }`); the
        // routing-seam tests still need a deterministic cold-start relay set,
        // so the test constructor seeds it the same way an app would — via the
        // public `set_configured_relays` reducer — rather than relying on an
        // implicit kernel default. Content lane = `both` (read+write), indexer
        // lane = `indexer`; matches the prior `bootstrap_urls_for_role`
        // fallback so the routing assertions keep their expected frame counts.
        kernel.set_configured_relays(vec![
            crate::kernel::AppRelay::new(
                crate::relay::FALLBACK_CONTENT_RELAY.to_string(),
                "both".to_string(),
            ),
            crate::kernel::AppRelay::new(
                crate::relay::FALLBACK_INDEXER_RELAY.to_string(),
                "indexer".to_string(),
            ),
        ]);
        kernel
    }
}

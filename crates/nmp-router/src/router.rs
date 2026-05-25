//! `GenericOutboxRouter` — the single
//! [`nmp_core::substrate::OutboxRouter`] impl
//! (`docs/architecture/crate-boundaries.md` §3.2).
//!
//! Lanes implemented (spec §3.1):
//!
//! - **Lane 1 — NIP-65 mailbox.** `route_publish` consults
//!   [`MailboxCache::write_relays`] for `evt.pubkey`; `route_subscription`
//!   consults `read_relays` for each author in the interest shape.
//! - **Lane 2 — Hint.** Relay-hint URLs lifted from `evt.tags`
//!   (e/p/a/q tag position 2) on publish; lifted from `interest.hints`
//!   carrying [`HintSource::EventTag`] on subscribe. Stacks on top of
//!   lane 1 — never substitutes.
//! - **Lane 3 — Provenance.** Subscribe-only: lifted from
//!   `interest.hints` carrying [`HintSource::Provenance`] (the relay we
//!   last saw a referenced event id at, so a re-fetch goes back there).
//! - **Lane 4 — UserConfigured.** When `evt.pubkey == ctx.active_account`
//!   on publish, `session_keys.active_write` is attributed to
//!   [`UserConfiguredCategory::ActiveAccountWrite`]; when an author in the
//!   interest shape (or the active account itself for an authorless
//!   wildcard) matches `ctx.active_account` on subscribe,
//!   `session_keys.active_read` is attributed to
//!   [`UserConfiguredCategory::ActiveAccountRead`].
//! - **Lane 5 — ClassRouted.** On the [`RoutingContext::explicit_targets`]
//!   shortcut, the router classifies `evt.kind` into the right
//!   [`EventClass`] (Search/Draft/Wiki/Other) before attributing the
//!   forced URLs to `ClassRouted{class, via: Explicit}`. The generic
//!   algorithm itself has NO NIP-51 cache to consult — the substrate
//!   carries no class-relays accessor — so the only ClassRouted path in
//!   the router is the explicit-targets shortcut (which the NIP-17 /
//!   NIP-29 / Marmot / NIP-50 search actions populate). Class refinement
//!   in attribution lets the V-51 observer trace render the correct
//!   class label.
//! - **Lane 6 — Indexer.** ALWAYS-ON for discovery kinds (kind:0,
//!   kind:3, kind:10000–19999) — both publish and subscribe (R+W
//!   symmetric per spec §3.1). Stacks on top of lane 1; defeats the
//!   kind:10002 self-sealing loop.
//! - **Lane 7 — AppRelay.** Fallback when no earlier lane resolved
//!   anything.
//!
//! Blocked-relay (kind:10006) is a subtractive post-filter applied via
//! [`RoutedRelaySet::from_explicit`] and the per-lane `blocked_relays.contains`
//! guards.

use std::sync::Arc;

use nmp_core::planner::{HintSource, LogicalInterest};
use nmp_core::substrate::{
    truncate_event_id, AppRelayMode, ClassRoutingPath, Direction, EventClass, OutboxRouter,
    PublishTrace, RoutedRelaySet, RoutingContext, RoutingError, RoutingSource,
    RoutingTraceObserver, SubscriptionTrace, UnsignedEvent, UserConfiguredCategory,
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

/// Tag keys whose third column carries a relay-hint URL: `e` (event ref),
/// `p` (pubkey ref), `a` (NIP-33 address ref), `q` (NIP-18 quote ref).
/// Matches `nmp_core::tags::{e_tag, p_tag, a_tag, q_tag}` — the same set
/// of relay-hint-carrying tags.
const HINT_TAG_KEYS: &[&str] = &["e", "p", "a", "q"];

/// Lift relay-hint URLs from `tags` — the third column of any e/p/a/q
/// tag (spec §3.1 lane 2). Returns deduped owned strings in tag-document
/// order. Empty hint slots (the NIP-10 four-column form with empty
/// relay) are skipped.
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

/// Map an `UnsignedEvent.kind` to its [`EventClass`] for lane-5
/// attribution. Mirrors ADR-0020 §2's built-in kind→class table for the
/// three classes the enum enumerates:
///
/// - kind:818 / 30818 / 30819 → [`EventClass::Wiki`]
/// - kind:1234 / 31234       → [`EventClass::Draft`]
/// - everything else         → [`EventClass::Other("explicit")`]
///
/// `EventClass::Search` has no canonical publish kind (search is a
/// REQ-only operation), so it never appears here. The fallback string
/// `"explicit"` matches [`RoutedRelaySet::from_explicit`]'s pre-existing
/// label so the on-the-wire JSON projection
/// (`nmp_core::kernel::routing_trace_dto::event_class_to_json`) is
/// stable for callers that already relied on the prior label.
fn classify_kind(kind: u32) -> EventClass {
    match kind {
        818 | 30_818 | 30_819 => EventClass::Wiki,
        1234 | 31_234 => EventClass::Draft,
        _ => EventClass::Other(String::from("explicit")),
    }
}

/// Lane-5 specialisation of [`RoutedRelaySet::from_explicit`]: same
/// blocked-relay post-filter, but the attributed `EventClass` is the
/// classification of `kind` (not the placeholder `Other("explicit")`).
/// Used by the publish path so a kind:30818 wiki forced through
/// `explicit_targets` traces as `ClassRouted{Wiki, Explicit}` rather
/// than `ClassRouted{Other("explicit"), Explicit}`.
fn explicit_set_for_kind(
    urls: &[String],
    blocked: &nmp_core::substrate::BlockedRelaySet,
    kind: u32,
) -> RoutedRelaySet {
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
            // Lane 5: classify `evt.kind` so the ClassRouted attribution
            // carries the right EventClass (Wiki/Draft/Other), not the
            // generic "explicit" placeholder.
            explicit_set_for_kind(explicit, ctx.blocked_relays, evt.kind)
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

            // Lane 2 — relay-hint tags on `evt` (e/p/a/q position 2).
            // Stacks on top of lane 1; never substitutes. A relay
            // appearing as a hint AND in the NIP-65 write set will carry
            // both sources in its `BTreeSet<RoutingSource>` (additive via
            // `RoutedRelaySet::add`).
            for url in relay_hints_from_tags(&evt.tags) {
                if ctx.blocked_relays.contains(&url) {
                    continue;
                }
                out.add(url, RoutingSource::Hint);
            }

            // Lane 4 — UserConfigured (active-account write). Only fires
            // when the publishing key IS the active account; this is the
            // "publish from my own keypair" path. For relay-pinned or
            // delegated publishes (`evt.pubkey != active_account`) the
            // session's active-write set MUST NOT be added — that would
            // leak the operator's account-keyed relays to events the
            // active account did not author.
            if let Some(active) = ctx.active_account {
                if active == &evt.pubkey {
                    for url in ctx.session_keys.active_write.iter() {
                        if ctx.blocked_relays.contains(url) {
                            continue;
                        }
                        out.add(
                            url.clone(),
                            RoutingSource::UserConfigured(
                                UserConfiguredCategory::ActiveAccountWrite,
                            ),
                        );
                    }
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
            // anything (every prior lane empty / didn't fire).
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

            // Lane 3 (Provenance) is subscribe-only: an event being
            // published has no prior-observation relay. The lane is
            // populated for `route_subscription` below.

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

            // Lanes 2 + 3 — relay hints carried on the interest. The
            // planner attaches `RelayHint`s when an `e`/`p`/`a`/`q` tag's
            // third column gave us a hint (lane 2, `HintSource::EventTag`)
            // or when a prior event id's provenance relay is the right
            // place to re-fetch (lane 3, `HintSource::Provenance`). Both
            // stack on top of lane 1 — never substitute. `UserConfigured`
            // hints (user typed a relay in app settings) attribute to
            // lane 4 below for symmetry with the publish path.
            for hint in &interest.hints {
                if ctx.blocked_relays.contains(&hint.url) {
                    continue;
                }
                let lane = match hint.source {
                    HintSource::EventTag { .. } => RoutingSource::Hint,
                    HintSource::Provenance { .. } => RoutingSource::Provenance,
                    HintSource::UserConfigured => RoutingSource::UserConfigured(
                        UserConfiguredCategory::Debug,
                    ),
                };
                out.add(hint.url.clone(), lane);
            }

            // Lane 4 — UserConfigured (active-account read). Fires when
            // the active account is one of the interest's authors OR
            // when the interest is authorless (a wildcard subscription
            // implicitly includes the active user's view of the wire).
            // For multi-author interests that DON'T include the active
            // account, the active-read set is silent — we're reading
            // about other people, not from our own read mailbox.
            if let Some(active) = ctx.active_account {
                let active_in_scope = interest.shape.authors.is_empty()
                    || interest.shape.authors.contains(active);
                if active_in_scope {
                    for url in ctx.session_keys.active_read.iter() {
                        if ctx.blocked_relays.contains(url) {
                            continue;
                        }
                        out.add(
                            url.clone(),
                            RoutingSource::UserConfigured(
                                UserConfiguredCategory::ActiveAccountRead,
                            ),
                        );
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

#[cfg(test)]
#[path = "router/tests_lanes.rs"]
mod tests_lanes;

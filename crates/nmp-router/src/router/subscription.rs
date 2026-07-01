use nmp_core::substrate::{
    AppRelayMode, Direction, LaneOutcome, RouteAttempt, RoutedRelaySet, RoutingContext,
    RoutingError, RoutingLane, RoutingSource, SubscriptionTrace, UserConfiguredCategory,
};
use nmp_planner::{HintSource, LogicalInterest};

use crate::discovery::{indexer_kind_scope, is_discovery_kind};

use super::GenericOutboxRouter;

pub(super) fn route(
    router: &GenericOutboxRouter,
    interest: &LogicalInterest,
    ctx: &RoutingContext<'_>,
) -> Result<RoutedRelaySet, RoutingError> {
    // D8: gate attempt accumulation on observer presence.
    let tracing_active = router.trace_observer.is_some();
    let mut out = RoutedRelaySet::new();
    let mut attempts: Vec<RouteAttempt> = Vec::new();

    // Lane 1 — each author's NIP-65 read set.
    // Count admissible URLs so that a URL that also appeared in an
    // earlier lane still reports Matched here.
    {
        let mut lane_count = 0usize;
        for author in &interest.shape.authors {
            if let Some(reads) = ctx.mailbox_cache.read_relays(author) {
                for url in reads {
                    if ctx.blocked_relays.contains(&url) {
                        continue;
                    }
                    if !router.admission.is_admissible(&url) {
                        continue;
                    }
                    out.add(
                        url,
                        RoutingSource::Nip65 {
                            direction: Direction::Read,
                        },
                    );
                    if tracing_active {
                        lane_count += 1;
                    }
                }
            }
        }
        if tracing_active {
            attempts.push(RouteAttempt {
                lane: RoutingLane::Nip65,
                outcome: if lane_count > 0 {
                    LaneOutcome::Matched { count: lane_count }
                } else {
                    LaneOutcome::Empty
                },
            });
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
    //
    // Track Hint and Provenance separately for per-lane granularity.
    // Count admissible passes (not net-new keys) for accuracy when
    // a hint relay was already added by lane 1.
    {
        let mut hint_count = 0usize;
        let mut prov_count = 0usize;
        for hint in &interest.hints {
            if ctx.blocked_relays.contains(&hint.url) {
                continue;
            }
            if !router.admission.is_admissible(&hint.url) {
                continue;
            }
            let lane_src = match hint.source {
                HintSource::EventTag { .. } => RoutingSource::Hint,
                HintSource::Provenance { .. } => RoutingSource::Provenance,
                HintSource::UserConfigured => {
                    RoutingSource::UserConfigured(UserConfiguredCategory::Debug)
                }
            };
            out.add(hint.url.clone(), lane_src.clone());
            if tracing_active {
                match &lane_src {
                    RoutingSource::Hint => hint_count += 1,
                    RoutingSource::Provenance => prov_count += 1,
                    _ => {}
                }
            }
        }
        if tracing_active {
            attempts.push(RouteAttempt {
                lane: RoutingLane::Hint,
                outcome: if hint_count > 0 {
                    LaneOutcome::Matched { count: hint_count }
                } else {
                    LaneOutcome::Empty
                },
            });
            attempts.push(RouteAttempt {
                lane: RoutingLane::Provenance,
                outcome: if prov_count > 0 {
                    LaneOutcome::Matched { count: prov_count }
                } else {
                    LaneOutcome::Empty
                },
            });
        }
    }

    // Lane 4 — UserConfigured (active-account read). Fires when
    // the active account is one of the interest's authors OR
    // when the interest is authorless (a wildcard subscription
    // implicitly includes the active user's view of the wire).
    // For multi-author interests that DON'T include the active
    // account, the active-read set is silent — we're reading
    // about other people, not from our own read mailbox.
    //
    // An attempt is only emitted when the lane is applicable (active
    // account is in scope), symmetric with Lane 6 only emitting for
    // discovery kinds.
    if let Some(active) = ctx.active_account {
        let active_in_scope =
            interest.shape.authors.is_empty() || interest.shape.authors.contains(active);
        if active_in_scope {
            let mut lane_count = 0usize;
            for url in ctx.session_keys.active_read.iter() {
                if ctx.blocked_relays.contains(url) {
                    continue;
                }
                out.add(
                    url.clone(),
                    RoutingSource::UserConfigured(UserConfiguredCategory::ActiveAccountRead),
                );
                if tracing_active {
                    lane_count += 1;
                }
            }
            if tracing_active {
                attempts.push(RouteAttempt {
                    lane: RoutingLane::UserConfigured,
                    outcome: if lane_count > 0 {
                        LaneOutcome::Matched { count: lane_count }
                    } else {
                        LaneOutcome::Empty
                    },
                });
            }
        }
    }

    // Lane 6 — Indexer (ALWAYS-ON for any discovery kind in the
    // interest shape, spec §3.1): stacks on top of lane 1 and defeats
    // the kind:10002 self-sealing loop (a stale cached kind:10002 would
    // otherwise refresh only against its own stale relays). An attempt
    // is emitted only when the lane applies (a discovery kind present).
    //
    // Indexer-leak fix: a mixed discovery/content interest (e.g.
    // `[1, 3]`) fires on kind:3 but must NOT send kind:1 notes to the
    // indexer. `indexer_kind_scope` returns `Some(subset)` to scope it
    // to the discovery kinds, `None` for an all-discovery interest.
    if interest.shape.kinds.iter().any(|k| is_discovery_kind(*k)) {
        let scope = indexer_kind_scope(&interest.shape.kinds);
        let mut lane_count = 0usize;
        for url in ctx.session_keys.indexer_relays.iter() {
            if ctx.blocked_relays.contains(url) {
                continue;
            }
            match &scope {
                Some(kinds) => {
                    out.add_with_kind_scope(url.clone(), RoutingSource::Indexer, kinds.clone())
                }
                None => out.add(url.clone(), RoutingSource::Indexer),
            }
            if tracing_active {
                lane_count += 1;
            }
        }
        if tracing_active {
            attempts.push(RouteAttempt {
                lane: RoutingLane::Indexer,
                outcome: if lane_count > 0 {
                    LaneOutcome::Matched { count: lane_count }
                } else {
                    LaneOutcome::Empty
                },
            });
        }
    }

    // Lane 7 — AppRelay fallback when no earlier lane resolved
    // anything.
    // Since `out.is_empty()` gate ensures no overlap with earlier
    // lanes, lane_count equals net-new URLs.
    if out.is_empty() {
        let mut lane_count = 0usize;
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
            if tracing_active {
                lane_count += 1;
            }
        }
        if tracing_active {
            attempts.push(RouteAttempt {
                lane: RoutingLane::AppRelayFallback,
                outcome: if lane_count > 0 {
                    LaneOutcome::Matched { count: lane_count }
                } else {
                    LaneOutcome::Empty
                },
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

    if let Some(obs) = router.trace_observer.as_ref() {
        obs.on_subscription(
            SubscriptionTrace {
                interest_id: interest.id.0,
                kinds: interest.shape.kinds.iter().copied().collect(),
                authors_count: interest.shape.authors.len(),
                attempts,
            },
            &out,
        );
    }

    Ok(out)
}

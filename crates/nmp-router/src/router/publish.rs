use nmp_core::substrate::{
    truncate_event_id, AppRelayMode, Direction, LaneOutcome, PublishTrace, RouteAttempt,
    RoutedRelaySet, RoutingContext, RoutingError, RoutingLane, RoutingSource,
    UserConfiguredCategory,
};
use nmp_signer_iface::UnsignedEvent;

use crate::discovery::is_discovery_kind;

use super::GenericOutboxRouter;

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

pub(super) fn route(
    router: &GenericOutboxRouter,
    evt: &UnsignedEvent,
    ctx: &RoutingContext<'_>,
) -> Result<RoutedRelaySet, RoutingError> {
    // D8: gate attempt accumulation on observer presence — Vec::new()
    // is zero-alloc, but .push() allocates; skip it when nobody reads.
    let tracing_active = router.trace_observer.is_some();
    let mut out = RoutedRelaySet::new();
    let mut attempts: Vec<RouteAttempt> = Vec::new();

    // Lane 1 — author's NIP-65 write set.
    // Count admissible URLs (not net-new keys) so that a URL that
    // also appeared in an earlier lane still reports Matched here.
    {
        let mut lane_count = 0usize;
        if let Some(writes) = ctx.mailbox_cache.write_relays(&evt.pubkey) {
            for url in writes {
                if ctx.blocked_relays.contains(&url) {
                    continue;
                }
                if !router.admission.is_admissible(&url) {
                    continue;
                }
                out.add(
                    url,
                    RoutingSource::Nip65 {
                        direction: Direction::Write,
                    },
                );
                if tracing_active {
                    lane_count += 1;
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

    // Lane 2 — relay-hint tags on `evt` (e/p/a/q position 2).
    // Stacks on top of lane 1; never substitutes. A relay
    // appearing as a hint AND in the NIP-65 write set will carry
    // both sources in its `BTreeSet<RoutingSource>` (additive via
    // `RoutedRelaySet::add`).
    {
        let mut lane_count = 0usize;
        for url in relay_hints_from_tags(&evt.tags) {
            if ctx.blocked_relays.contains(&url) {
                continue;
            }
            if !router.admission.is_admissible(&url) {
                continue;
            }
            out.add(url, RoutingSource::Hint);
            if tracing_active {
                lane_count += 1;
            }
        }
        if tracing_active {
            attempts.push(RouteAttempt {
                lane: RoutingLane::Hint,
                outcome: if lane_count > 0 {
                    LaneOutcome::Matched { count: lane_count }
                } else {
                    LaneOutcome::Empty
                },
            });
        }
    }

    // Lane 4 — UserConfigured (active-account write). Only fires
    // when the publishing key IS the active account; this is the
    // "publish from my own keypair" path. For relay-pinned or
    // delegated publishes (`evt.pubkey != active_account`) the
    // session's active-write set MUST NOT be added — that would
    // leak the operator's account-keyed relays to events the
    // active account did not author.
    //
    // An attempt is only emitted when the lane is applicable (active
    // account is present and matches the event pubkey). No attempt
    // means "lane did not apply to this call", symmetrical with Lane 6
    // not emitting an attempt for non-discovery kinds.
    if let Some(active) = ctx.active_account {
        if active == &evt.pubkey {
            let mut lane_count = 0usize;
            for url in ctx.session_keys.active_write.iter() {
                if ctx.blocked_relays.contains(url) {
                    continue;
                }
                out.add(
                    url.clone(),
                    RoutingSource::UserConfigured(UserConfiguredCategory::ActiveAccountWrite),
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
    //
    // An attempt is emitted only for discovery kinds (lane applicable).
    if is_discovery_kind(evt.kind) {
        let mut lane_count = 0usize;
        for url in ctx.session_keys.indexer_relays.iter() {
            if ctx.blocked_relays.contains(url) {
                continue;
            }
            out.add(url.clone(), RoutingSource::Indexer);
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
    // anything (every prior lane empty / didn't fire).
    // Lane 7 fires only when `out.is_empty()`, so lane_count equals
    // net-new URLs — no overlap with earlier lanes possible.
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

    // Lane 3 (Provenance) is subscribe-only: an event being
    // published has no prior-observation relay. The lane is
    // populated for `route_subscription`.

    if out.is_empty() {
        return Err(RoutingError::Unroutable(evt.pubkey.clone()));
    }

    // Stash the attempts into the out set's trace slot via a
    // thread-local trick would be awkward; instead return them
    // out-of-band through a temporary struct. The trace observer
    // gets them via the `PublishTrace` summary below.
    //
    // Note: `attempts` was built while tracing_active; if the
    // observer is None, `attempts` is empty (no pushes occurred).
    if let Some(obs) = router.trace_observer.as_ref() {
        obs.on_publish(
            PublishTrace {
                kind: evt.kind,
                author: evt.pubkey.clone(),
                event_id_short: truncate_event_id(None),
                attempts,
            },
            &out,
        );
    }

    Ok(out)
}
